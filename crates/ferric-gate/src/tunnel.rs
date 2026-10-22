//! A self-hosted WireGuard tunnel plan for reaching Ferric remotely.
//!
//! Ferric does not ship its own VPN and does not depend on a coordination
//! service. It writes a two-peer WireGuard plan: the Ferric host and one
//! client, with a preshared key on top of the X25519 handshake, and the
//! client's `AllowedIPs` set to the host's single address, so the tunnel
//! reaches Ferric and nothing else. The data plane is Mullvad's GotaTun
//! (userspace WireGuard in Rust), configured over the standard cross-platform
//! UAPI socket. Any WireGuard implementation (kernel `wg-quick`, the official
//! apps) accepts the same files.
//!
//! The plan holds secrets, so every file is written mode 0600 and existing
//! files are never overwritten.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Value, json};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::GateError;
use crate::token::{check_private, state_dir, write_private};

pub const DEFAULT_INTERFACE: &str = "ferric0";
pub const DEFAULT_LISTEN_PORT: u16 = 51820;
pub const HOST_ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 1);
pub const CLIENT_ADDRESS: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 2);
pub const PREFIX: u8 = 24;

/// The default plan directory: `<state dir>/tunnel`.
pub fn default_tunnel_dir() -> Option<PathBuf> {
    state_dir().map(|dir| dir.join("tunnel"))
}

/// The UAPI socket a userspace WireGuard (GotaTun, wireguard-go, BoringTun)
/// creates for `interface`.
pub fn uapi_socket_path(interface: &str) -> PathBuf {
    PathBuf::from(format!("/var/run/wireguard/{interface}.sock"))
}

/// An X25519 key pair in WireGuard's encoding.
pub struct KeyPair {
    secret: [u8; 32],
    public: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Result<Self, GateError> {
        let mut secret = random32()?;
        // WireGuard's clamping, as `wg genkey` stores it.
        secret[0] &= 248;
        secret[31] &= 127;
        secret[31] |= 64;
        Ok(Self::from_secret(secret))
    }

    pub fn from_secret(secret: [u8; 32]) -> Self {
        let public = PublicKey::from(&StaticSecret::from(secret)).to_bytes();
        KeyPair { secret, public }
    }

    pub fn public_base64(&self) -> String {
        B64.encode(self.public)
    }

    fn secret_base64(&self) -> String {
        B64.encode(self.secret)
    }
}

fn random32() -> Result<[u8; 32], GateError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| GateError::Random(error.to_string()))?;
    Ok(bytes)
}

/// A complete two-peer plan.
pub struct TunnelPlan {
    pub interface: String,
    pub listen_port: u16,
    /// `host:port` the client dials; `None` leaves a placeholder to fill in.
    pub endpoint: Option<String>,
    host: KeyPair,
    client: KeyPair,
    psk: [u8; 32],
}

impl TunnelPlan {
    pub fn generate(endpoint: Option<String>) -> Result<Self, GateError> {
        Ok(TunnelPlan {
            interface: DEFAULT_INTERFACE.to_string(),
            listen_port: DEFAULT_LISTEN_PORT,
            endpoint,
            host: KeyPair::generate()?,
            client: KeyPair::generate()?,
            psk: random32()?,
        })
    }

    /// The Ferric host's side, in `wg-quick` format.
    pub fn host_conf(&self) -> String {
        format!(
            "# Ferric tunnel: the Ferric host. Secret; mode 0600.\n\
             [Interface]\n\
             PrivateKey = {}\n\
             Address = {HOST_ADDRESS}/{PREFIX}\n\
             ListenPort = {}\n\
             \n\
             [Peer]\n\
             # client-1\n\
             PublicKey = {}\n\
             PresharedKey = {}\n\
             AllowedIPs = {CLIENT_ADDRESS}/32\n",
            self.host.secret_base64(),
            self.listen_port,
            self.client.public_base64(),
            B64.encode(self.psk),
        )
    }

    /// The client's side. `AllowedIPs` is the host's one address: the
    /// tunnel carries Ferric traffic only.
    pub fn client_conf(&self) -> String {
        let endpoint = self
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("<ferric-host>:{}", self.listen_port));
        format!(
            "# Ferric tunnel: client-1. Secret; mode 0600. Import into any WireGuard client.\n\
             [Interface]\n\
             PrivateKey = {}\n\
             Address = {CLIENT_ADDRESS}/32\n\
             \n\
             [Peer]\n\
             # the Ferric host\n\
             PublicKey = {}\n\
             PresharedKey = {}\n\
             AllowedIPs = {HOST_ADDRESS}/32\n\
             Endpoint = {endpoint}\n\
             PersistentKeepalive = 25\n",
            self.client.secret_base64(),
            self.host.public_base64(),
            B64.encode(self.psk),
        )
    }

    /// Non-secret facts other commands read (bind address, interface).
    pub fn info(&self) -> Value {
        json!({
            "interface": self.interface,
            "host_address": HOST_ADDRESS.to_string(),
            "client_address": CLIENT_ADDRESS.to_string(),
            "prefix": PREFIX,
            "listen_port": self.listen_port,
            "host_public_key": self.host.public_base64(),
            "client_public_key": self.client.public_base64(),
            "endpoint": self.endpoint,
        })
    }

    /// Write `host.conf`, `client-1.conf` (both 0600) and `tunnel.json` into
    /// `dir`. Refuses if a plan already exists there.
    pub fn write(&self, dir: &Path) -> Result<TunnelFiles, GateError> {
        let files = TunnelFiles::in_dir(dir);
        for path in [&files.host_conf, &files.client_conf, &files.info] {
            if path.exists() {
                return Err(GateError::Tunnel(format!(
                    "{} already exists; a tunnel plan is never overwritten (move the directory aside to start over)",
                    path.display()
                )));
            }
        }
        write_private(&files.host_conf, &self.host_conf())?;
        write_private(&files.client_conf, &self.client_conf())?;
        let info = serde_json::to_string_pretty(&self.info()).unwrap_or_default();
        write_private(&files.info, &info)?;
        Ok(files)
    }
}

pub struct TunnelFiles {
    pub host_conf: PathBuf,
    pub client_conf: PathBuf,
    pub info: PathBuf,
}

impl TunnelFiles {
    pub fn in_dir(dir: &Path) -> Self {
        TunnelFiles {
            host_conf: dir.join("host.conf"),
            client_conf: dir.join("client-1.conf"),
            info: dir.join("tunnel.json"),
        }
    }
}

/// The non-secret facts of an existing plan.
#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub interface: String,
    pub host_address: IpAddr,
    pub prefix: u8,
    pub listen_port: u16,
}

impl TunnelInfo {
    pub fn load(dir: &Path) -> Result<Self, GateError> {
        let path = TunnelFiles::in_dir(dir).info;
        let text = std::fs::read_to_string(&path).map_err(|e| GateError::io(&path, e))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| GateError::Tunnel(format!("{}: {e}", path.display())))?;
        let field = |name: &str| {
            value
                .get(name)
                .ok_or_else(|| GateError::Tunnel(format!("{}: missing `{name}`", path.display())))
        };
        let interface = field("interface")?.as_str().unwrap_or_default().to_string();
        let host_address = field("host_address")?
            .as_str()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| GateError::Tunnel(format!("{}: bad host_address", path.display())))?;
        let prefix = field("prefix")?.as_u64().unwrap_or(32) as u8;
        let listen_port = field("listen_port")?.as_u64().unwrap_or(0) as u16;
        if interface.is_empty() || listen_port == 0 {
            return Err(GateError::Tunnel(format!(
                "{}: incomplete plan",
                path.display()
            )));
        }
        Ok(TunnelInfo {
            interface,
            host_address,
            prefix,
            listen_port,
        })
    }
}

/// Turn a `wg-quick` host config into a UAPI `set` request (keys in hex, as
/// the UAPI protocol requires). Only the keys Ferric writes are understood.
pub fn uapi_set_request(host_conf: &str) -> Result<String, GateError> {
    let mut out = String::from("set=1\nreplace_peers=true\n");
    let mut section = "";
    let mut saw_private = false;
    for raw in host_conf.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            section = if line.eq_ignore_ascii_case("[Interface]") {
                "interface"
            } else if line.eq_ignore_ascii_case("[Peer]") {
                "peer"
            } else {
                return Err(GateError::Tunnel(format!("unknown section {line}")));
            };
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .map(|(k, v)| (k.trim(), v.trim()))
            .ok_or_else(|| GateError::Tunnel(format!("cannot parse `{line}`")))?;
        match (section, key) {
            ("interface", "PrivateKey") => {
                out.push_str(&format!("private_key={}\n", b64_to_hex(value)?));
                saw_private = true;
            }
            ("interface", "ListenPort") => out.push_str(&format!("listen_port={value}\n")),
            ("interface", "Address") => {}
            ("peer", "PublicKey") => {
                out.push_str(&format!("public_key={}\n", b64_to_hex(value)?));
                out.push_str("replace_allowed_ips=true\n");
            }
            ("peer", "PresharedKey") => {
                out.push_str(&format!("preshared_key={}\n", b64_to_hex(value)?));
            }
            ("peer", "AllowedIPs") => {
                for ip in value.split(',') {
                    out.push_str(&format!("allowed_ip={}\n", ip.trim()));
                }
            }
            ("peer", "Endpoint") => out.push_str(&format!("endpoint={value}\n")),
            ("peer", "PersistentKeepalive") => {
                out.push_str(&format!("persistent_keepalive_interval={value}\n"));
            }
            _ => return Err(GateError::Tunnel(format!("unsupported key `{key}`"))),
        }
    }
    if !saw_private {
        return Err(GateError::Tunnel("host config has no PrivateKey".into()));
    }
    out.push('\n');
    Ok(out)
}

/// Read a plan's host config, refusing one that others can read.
pub fn read_host_conf(dir: &Path) -> Result<String, GateError> {
    let path = TunnelFiles::in_dir(dir).host_conf;
    check_private(&path)?;
    std::fs::read_to_string(&path).map_err(|e| GateError::io(&path, e))
}

fn b64_to_hex(value: &str) -> Result<String, GateError> {
    let bytes = B64
        .decode(value)
        .map_err(|_| GateError::Tunnel("key is not base64".into()))?;
    if bytes.len() != 32 {
        return Err(GateError::Tunnel("key is not 32 bytes".into()));
    }
    Ok(hex::encode(bytes))
}

/// One peer's state from a UAPI `get` response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerStatus {
    pub public_key: String,
    pub endpoint: Option<String>,
    pub last_handshake_sec: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Parse the peers out of a UAPI `get=1` response. Public keys come back in
/// base64 for display.
pub fn parse_status(response: &str) -> Vec<PeerStatus> {
    let mut peers: Vec<PeerStatus> = Vec::new();
    for line in response.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "public_key" => peers.push(PeerStatus {
                public_key: hex::decode(value)
                    .map(|bytes| B64.encode(bytes))
                    .unwrap_or_else(|_| value.to_string()),
                ..PeerStatus::default()
            }),
            "endpoint" => {
                if let Some(peer) = peers.last_mut() {
                    peer.endpoint = Some(value.to_string());
                }
            }
            "last_handshake_time_sec" => {
                if let Some(peer) = peers.last_mut() {
                    peer.last_handshake_sec = value.parse().unwrap_or(0);
                }
            }
            "rx_bytes" => {
                if let Some(peer) = peers.last_mut() {
                    peer.rx_bytes = value.parse().unwrap_or(0);
                }
            }
            "tx_bytes" => {
                if let Some(peer) = peers.last_mut() {
                    peer.tx_bytes = value.parse().unwrap_or(0);
                }
            }
            _ => {}
        }
    }
    peers
}

/// Send one UAPI request and return the response (up to the blank line).
/// Fails unless the response ends with `errno=0`.
#[cfg(unix)]
pub fn uapi_exchange(socket: &Path, request: &str) -> Result<String, GateError> {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(socket).map_err(|e| GateError::io(socket, e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| GateError::io(socket, e))?;
    stream
        .write_all(request.as_bytes())
        .map_err(|e| GateError::io(socket, e))?;
    let mut response = String::new();
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| GateError::io(socket, e))?;
        if read == 0 || line == "\n" {
            break;
        }
        response.push_str(&line);
    }
    match response.lines().rev().find(|l| l.starts_with("errno=")) {
        Some("errno=0") => Ok(response),
        Some(errno) => Err(GateError::Tunnel(format!(
            "WireGuard UAPI refused: {errno}"
        ))),
        None => Err(GateError::Tunnel("WireGuard UAPI gave no errno".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_matches_the_rfc7748_vector() {
        // RFC 7748 §6.1, Alice.
        let secret: [u8; 32] =
            hex::decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
                .unwrap()
                .try_into()
                .unwrap();
        let pair = KeyPair::from_secret(secret);
        assert_eq!(
            hex::encode(B64.decode(pair.public_base64()).unwrap()),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
    }

    #[test]
    fn generated_keys_are_clamped_and_distinct() {
        let a = KeyPair::generate().unwrap();
        let b = KeyPair::generate().unwrap();
        assert_ne!(a.public, b.public);
        assert_eq!(a.secret[0] & 7, 0);
        assert_eq!(a.secret[31] & 0x80, 0);
        assert_eq!(a.secret[31] & 0x40, 0x40);
    }

    #[test]
    fn the_client_reaches_only_the_ferric_host() {
        let plan = TunnelPlan::generate(Some("gpu-box.example:51820".into())).unwrap();
        let client = plan.client_conf();
        assert!(client.contains("AllowedIPs = 10.77.0.1/32"));
        assert!(client.contains("Endpoint = gpu-box.example:51820"));
        assert!(client.contains("PresharedKey = "));
        let host = plan.host_conf();
        assert!(host.contains("AllowedIPs = 10.77.0.2/32"));
        // Each side carries the other's public key, never its private key.
        assert!(host.contains(&plan.client.public_base64()));
        assert!(!host.contains(&plan.client.secret_base64()));
        assert!(!client.contains(&plan.host.secret_base64()));
    }

    #[test]
    fn host_conf_round_trips_into_a_uapi_request() {
        let plan = TunnelPlan::generate(None).unwrap();
        let request = uapi_set_request(&plan.host_conf()).unwrap();
        assert!(request.starts_with("set=1\nreplace_peers=true\n"));
        assert!(request.contains(&format!("private_key={}\n", hex::encode(plan.host.secret))));
        assert!(request.contains(&format!("public_key={}\n", hex::encode(plan.client.public))));
        assert!(request.contains(&format!("preshared_key={}\n", hex::encode(plan.psk))));
        assert!(request.contains("listen_port=51820\n"));
        assert!(request.contains("allowed_ip=10.77.0.2/32\n"));
        assert!(request.ends_with("\n\n"));
        assert!(uapi_set_request("[Interface]\nListenPort = 1\n").is_err());
    }

    #[test]
    fn a_plan_is_written_private_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let plan = TunnelPlan::generate(None).unwrap();
        let files = plan.write(dir.path()).unwrap();
        let info = TunnelInfo::load(dir.path()).unwrap();
        assert_eq!(info.host_address, IpAddr::V4(HOST_ADDRESS));
        assert_eq!(info.interface, "ferric0");
        assert_eq!(
            read_host_conf(dir.path()).unwrap(),
            format!("{}\n", plan.host_conf())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for path in [&files.host_conf, &files.client_conf, &files.info] {
                let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{}", path.display());
            }
        }
        let again = TunnelPlan::generate(None).unwrap();
        assert!(again.write(dir.path()).is_err());
    }

    #[test]
    fn status_parses_peers_from_a_uapi_get() {
        let key = [7u8; 32];
        let response = format!(
            "private_key=00\nlisten_port=51820\npublic_key={}\nendpoint=192.0.2.4:40000\n\
             last_handshake_time_sec=1700000000\nrx_bytes=10\ntx_bytes=20\nerrno=0\n",
            hex::encode(key)
        );
        let peers = parse_status(&response);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].public_key, B64.encode(key));
        assert_eq!(peers[0].endpoint.as_deref(), Some("192.0.2.4:40000"));
        assert_eq!(peers[0].last_handshake_sec, 1_700_000_000);
        assert_eq!((peers[0].rx_bytes, peers[0].tx_bytes), (10, 20));
    }
}
