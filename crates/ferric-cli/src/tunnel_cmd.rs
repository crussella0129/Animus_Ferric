//! `ferric tunnel` — self-hosted WireGuard access to Ferric, with Mullvad's
//! GotaTun as the data plane (sprint 127, INT-0015).
//!
//! No coordination server, no account, no third party in the key path: the
//! keys are generated here and travel only as the client file you hand to
//! your own device. Ferric itself never needs root. Only `up` and `down`,
//! which create a network interface, do.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Args, Subcommand};
use ferric_gate::tunnel::{TunnelInfo, TunnelPlan, default_tunnel_dir};

#[derive(Args)]
pub struct TunnelArgs {
    #[command(subcommand)]
    command: TunnelCommand,
}

#[derive(Subcommand)]
enum TunnelCommand {
    /// Generate host and client keys and WireGuard configs (no root needed)
    Init {
        /// `host:port` your client dials (this machine's LAN or public address)
        #[arg(long)]
        endpoint: Option<String>,
        /// Plan directory. Default: `<state dir>/ferric/tunnel`.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Bring the tunnel up with GotaTun (Linux, needs root)
    Up {
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Bring up this config instead of the plan's host side (for
        /// example `client-1.conf` on a Linux client).
        #[arg(long)]
        conf: Option<PathBuf>,
        /// Interface name. Default: the plan's (`ferric0`).
        #[arg(long)]
        interface: Option<String>,
        /// The GotaTun binary (`cargo install gotatun-cli`).
        #[arg(long, default_value = "gotatun")]
        gotatun: String,
    },
    /// Show peers and their last handshake
    Status {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        interface: Option<String>,
    },
    /// Remove the tunnel interface (Linux, needs root)
    Down {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        interface: Option<String>,
    },
}

pub fn run_tunnel(args: TunnelArgs) -> ExitCode {
    let result = match args.command {
        TunnelCommand::Init { endpoint, dir } => init(endpoint, dir),
        TunnelCommand::Up {
            dir,
            conf,
            interface,
            gotatun,
        } => up(dir, conf, interface, &gotatun),
        TunnelCommand::Status { dir, interface } => status(dir, interface),
        TunnelCommand::Down { dir, interface } => down(dir, interface),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("tunnel: {message}");
            ExitCode::FAILURE
        }
    }
}

fn plan_dir(dir: Option<PathBuf>) -> Result<PathBuf, String> {
    dir.or_else(default_tunnel_dir)
        .ok_or_else(|| "no state directory; pass --dir".to_string())
}

fn init(endpoint: Option<String>, dir: Option<PathBuf>) -> Result<(), String> {
    let dir = plan_dir(dir)?;
    let plan = TunnelPlan::generate(endpoint.clone()).map_err(|e| e.to_string())?;
    let files = plan.write(&dir).map_err(|e| e.to_string())?;
    println!("Ferric tunnel plan written (every file mode 0600):");
    println!("  host side    {}", files.host_conf.display());
    println!("  client side  {}", files.client_conf.display());
    println!("  facts        {}", files.info.display());
    println!();
    println!("The client reaches 10.77.0.1 (this machine's Ferric) and nothing else.");
    if endpoint.is_none() {
        println!(
            "Edit `Endpoint` in the client file to this machine's address before importing it."
        );
    }
    println!();
    println!("Next:");
    println!("  1. cargo install gotatun-cli          # Mullvad's userspace WireGuard");
    println!("  2. sudo ferric tunnel up              # interface ferric0 at 10.77.0.1");
    println!(
        "  3. ferric api --tunnel                # API on the tunnel only, token still required"
    );
    println!("  4. move client-1.conf to your device (any WireGuard client), then delete it here");
    Ok(())
}

/// Read the config to bring up and its interface address.
#[cfg(target_os = "linux")]
fn load_conf(dir: &Path, conf: Option<PathBuf>) -> Result<(String, String), String> {
    let text = match conf {
        Some(path) => {
            ferric_gate::token_file_private(&path).map_err(|e| e.to_string())?;
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        None => ferric_gate::tunnel::read_host_conf(dir).map_err(|e| e.to_string())?,
    };
    let address = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "Address")
        .map(|(_, value)| value.trim().to_string())
        .ok_or_else(|| "config has no Address".to_string())?;
    Ok((text, address))
}

/// Every `AllowedIPs` entry in a config.
#[cfg(target_os = "linux")]
fn allowed_ips(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split('#').next()?.split_once('='))
        .filter(|(key, _)| key.trim() == "AllowedIPs")
        .flat_map(|(_, value)| value.split(',').map(|ip| ip.trim().to_string()))
        .filter(|ip| !ip.is_empty())
        .collect()
}

fn interface_for(dir: &Path, interface: Option<String>) -> Result<String, String> {
    match interface {
        Some(name) => Ok(name),
        None => TunnelInfo::load(dir)
            .map(|info| info.interface)
            .map_err(|e| format!("{e} (run `ferric tunnel init`)")),
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
fn up(
    dir: Option<PathBuf>,
    conf: Option<PathBuf>,
    interface: Option<String>,
    gotatun: &str,
) -> Result<(), String> {
    let dir = plan_dir(dir)?;
    let (text, address) = load_conf(&dir, conf)?;
    use ferric_gate::tunnel::{uapi_set_request, uapi_socket_path};
    use std::time::{Duration, Instant};
    let request = uapi_set_request(&text).map_err(|e| e.to_string())?;
    let interface = interface_for(&dir, interface)?;
    let socket = uapi_socket_path(&interface);
    if !socket.exists() {
        // GotaTun creates the TUN device and its UAPI socket, then
        // daemonizes once the device is ready.
        let mut command = Command::new(gotatun);
        // Under `sudo`, GotaTun drops back to the invoking user once the
        // device exists. As plain root there is no user to drop to, and
        // GotaTun's lookup of one fails without a terminal.
        if std::env::var_os("SUDO_UID").is_none() {
            command.arg("--disable-drop-privileges");
        }
        let status = command.arg(&interface).status().map_err(|e| {
            format!("cannot start {gotatun}: {e} (install it with `cargo install gotatun-cli`)")
        })?;
        if !status.success() {
            return Err(format!(
                "{gotatun} {interface} exited with {status} (creating a TUN device needs root)"
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            if Instant::now() > deadline {
                return Err(format!("{} did not appear", socket.display()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    ferric_gate::tunnel::uapi_exchange(&socket, &request).map_err(|e| e.to_string())?;
    run("ip", &["address", "replace", &address, "dev", &interface])?;
    run("ip", &["link", "set", &interface, "up"])?;
    // Route each peer's AllowedIPs into the tunnel, as wg-quick does.
    for route in allowed_ips(&text) {
        run("ip", &["route", "replace", &route, "dev", &interface])?;
    }
    println!(
        "Tunnel up: {interface} at {address} (GotaTun, configured over {})",
        socket.display()
    );
    println!(
        "Serve Ferric on it with `ferric api --tunnel`; check peers with `ferric tunnel status`."
    );
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn allowed_ips_are_collected_from_every_peer() {
        let conf = "[Interface]\nAddress = 10.77.0.2/32\n[Peer]\nAllowedIPs = 10.77.0.1/32, fd77::1/128 # host\n";
        assert_eq!(
            super::allowed_ips(conf),
            vec!["10.77.0.1/32", "fd77::1/128"]
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn up(
    dir: Option<PathBuf>,
    _conf: Option<PathBuf>,
    _interface: Option<String>,
    _gotatun: &str,
) -> Result<(), String> {
    let dir = plan_dir(dir)?;
    Err(format!(
        "`ferric tunnel up` drives GotaTun on Linux. On this system import {} into the \
         official WireGuard app (or `wg-quick up` it); the files are standard WireGuard.",
        dir.join("host.conf").display()
    ))
}

#[cfg(unix)]
fn status(dir: Option<PathBuf>, interface: Option<String>) -> Result<(), String> {
    use ferric_gate::tunnel::{parse_status, uapi_socket_path};
    use std::time::{SystemTime, UNIX_EPOCH};
    let dir = plan_dir(dir)?;
    let interface = interface_for(&dir, interface)?;
    let socket = uapi_socket_path(&interface);
    let response = ferric_gate::tunnel::uapi_exchange(&socket, "get=1\n\n")
        .map_err(|e| format!("{e} (is the tunnel up?)"))?;
    let peers = parse_status(&response);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    println!("{interface}: {} peer(s)", peers.len());
    for peer in peers {
        let handshake = if peer.last_handshake_sec == 0 {
            "never".to_string()
        } else {
            format!("{}s ago", now.saturating_sub(peer.last_handshake_sec))
        };
        println!(
            "  peer {}  endpoint {}  handshake {}  rx {} B  tx {} B",
            peer.public_key,
            peer.endpoint.as_deref().unwrap_or("-"),
            handshake,
            peer.rx_bytes,
            peer.tx_bytes
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn status(_dir: Option<PathBuf>, _interface: Option<String>) -> Result<(), String> {
    Err("use the WireGuard app's status view on this system".to_string())
}

fn down(dir: Option<PathBuf>, interface: Option<String>) -> Result<(), String> {
    let dir = plan_dir(dir)?;
    let interface = interface_for(&dir, interface)?;
    run("ip", &["link", "delete", &interface])?;
    println!("Removed {interface}.");
    Ok(())
}
