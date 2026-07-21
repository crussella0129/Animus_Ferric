//! `ferric server` — launcher for the OpenAI-compatible inference server (the
//! ADR-001 HTTP valve), so the constrained path is one command instead of a
//! manually-started server. Default engine: llama.cpp `llama-server`; Ollama
//! pluggable via `--engine`. The host is pinned to loopback (ADR-005) — the
//! launcher never binds a public interface and never execs an arbitrary binary
//! (the engine is a closed enum).
//!
//! Lifecycle (`up`/`status`/`down`) uses a TCP-connect readiness probe and a
//! platform kill, so the whole command is std-only and lives in the default
//! build. The deeper *constrained* capability check is `ferric toolbench
//! --protocol grammar` against the launched server.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

/// The inference engine the launcher manages. A closed set — the launcher never
/// execs an arbitrary binary (ADR-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    /// llama.cpp `llama-server` (default): GBNF/json_schema constraints +
    /// libmtmd multimodal (image/audio/video).
    LlamaServer,
    /// Ollama (`ollama serve`).
    Ollama,
}

impl Engine {
    pub fn program(self) -> &'static str {
        match self {
            Engine::LlamaServer => "llama-server",
            Engine::Ollama => "ollama",
        }
    }
}

#[derive(Subcommand)]
pub enum ServerCommand {
    /// Launch the server and register it (writes `.ferric/server.json`).
    Up(Box<ServerUpArgs>),
    /// Health-check the registered server and print its base URL.
    Status,
    /// Stop the registered server and remove the runfile.
    Down,
    /// Check engine-binary + model presence (and reachability if up).
    Doctor(Box<ServerUpArgs>),
}

#[derive(Args, Clone)]
pub struct ServerUpArgs {
    /// Engine to launch.
    #[arg(long, value_enum, default_value = "llama-server")]
    pub engine: Engine,
    /// GGUF model path (llama-server) or model name (Ollama).
    #[arg(long)]
    pub model: Option<String>,
    /// Multimodal projector GGUF (llama-server, for image/audio/video).
    #[arg(long)]
    pub mmproj: Option<PathBuf>,
    /// Context window in tokens.
    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,
    /// Port to bind on 127.0.0.1.
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
    /// CPU threads (llama-server only; ignored for Ollama). Edge-tuning knob —
    /// the primary latency lever on constrained CPU targets (Jetson/RPi).
    #[arg(long)]
    pub threads: Option<u32>,
    /// GPU layers to offload (llama-server only; ignored for Ollama).
    #[arg(long)]
    pub gpu_layers: Option<u32>,
    /// Batch size (llama-server only; ignored for Ollama).
    #[arg(long)]
    pub batch_size: Option<u32>,
    /// Securely expose the engine port over Tailscale (requires `tailscale` CLI).
    #[arg(long)]
    pub tailscale: bool,
}

/// What `command()` needs. Built from `ServerUpArgs` with the host fixed.
pub struct ServerConfig {
    pub engine: Engine,
    pub model: Option<String>,
    pub mmproj: Option<PathBuf>,
    pub ctx: u32,
    pub host: String,
    pub port: u16,
    /// Edge-tuning knobs (sprint 35). Only consumed by `Engine::LlamaServer`'s
    /// argv builder — Ollama doesn't take these as CLI flags.
    pub threads: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub batch_size: Option<u32>,
    pub tailscale: bool,
}

impl ServerConfig {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}/v1", self.host, self.port)
    }
}

/// A resolved launch command: program + args + extra env.
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Build the engine launch command. Pure (no spawn). Host is whatever
/// `cfg.host` is — callers pin it to `127.0.0.1` (ADR-005).
pub fn command(cfg: &ServerConfig) -> LaunchCommand {
    match cfg.engine {
        Engine::LlamaServer => {
            let mut args: Vec<String> = Vec::new();
            if let Some(model) = &cfg.model {
                args.push("-m".to_string());
                args.push(model.clone());
            }
            if let Some(mmproj) = &cfg.mmproj {
                args.push("--mmproj".to_string());
                args.push(mmproj.display().to_string());
            }
            args.push("-c".to_string());
            args.push(cfg.ctx.to_string());
            if let Some(threads) = cfg.threads {
                args.push("-t".to_string());
                args.push(threads.to_string());
            }
            if let Some(gpu_layers) = cfg.gpu_layers {
                args.push("-ngl".to_string());
                args.push(gpu_layers.to_string());
            }
            if let Some(batch_size) = cfg.batch_size {
                args.push("-b".to_string());
                args.push(batch_size.to_string());
            }
            args.push("--host".to_string());
            args.push(cfg.host.clone());
            args.push("--port".to_string());
            args.push(cfg.port.to_string());
            LaunchCommand {
                program: cfg.engine.program().to_string(),
                args,
                env: Vec::new(),
            }
        }
        Engine::Ollama => LaunchCommand {
            program: cfg.engine.program().to_string(),
            args: vec!["serve".to_string()],
            env: vec![(
                "OLLAMA_HOST".to_string(),
                format!("{}:{}", cfg.host, cfg.port),
            )],
        },
    }
}

/// The readiness/health endpoint to poll. llama-server exposes `/health`; both
/// expose `/v1/models`.
pub fn health_url(engine: Engine, base_url: &str) -> String {
    let root = base_url.trim_end_matches("/v1").trim_end_matches('/');
    match engine {
        Engine::LlamaServer => format!("{root}/health"),
        Engine::Ollama => format!("{root}/v1/models"),
    }
}

/// The registered-server record. Lets `query`/`toolbench` auto-discover the
/// base URL (T-805) and `down` find the PID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRunfile {
    pub engine: Engine,
    pub pid: u32,
    pub port: u16,
    pub base_url: String,
    #[serde(default)]
    pub tailscale: bool,
}

/// Runfile location: `<workspace>/.ferric/server.json` (the `.ferric/` dir is
/// already write-denied to the LLM — ADR-005).
pub fn runfile_path(workspace: &Path) -> PathBuf {
    workspace.join(".ferric").join("server.json")
}

pub fn global_runfile_path_from(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    crate::config::user_config_path_from(env).map(|p| p.with_file_name("server.json"))
}

pub fn global_runfile_path() -> Option<PathBuf> {
    global_runfile_path_from(&|k| std::env::var(k).ok())
}

pub fn read_runfile_from(
    workspace: &Path,
    env: &impl Fn(&str) -> Option<String>,
) -> Option<ServerRunfile> {
    let local = runfile_path(workspace);
    if let Ok(text) = std::fs::read_to_string(&local)
        && let Ok(rf) = serde_json::from_str(&text)
    {
        return Some(rf);
    }
    if let Some(global) = global_runfile_path_from(env)
        && let Ok(text) = std::fs::read_to_string(&global)
        && let Ok(rf) = serde_json::from_str(&text)
    {
        return Some(rf);
    }
    None
}

/// Read the registered server, if any (local first, global fallback).
pub fn read_runfile(workspace: &Path) -> Option<ServerRunfile> {
    read_runfile_from(workspace, &|k| std::env::var(k).ok())
}

/// Poll a TCP connect to `(host, port)` until it succeeds or `timeout` elapses.
fn wait_listening(host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let addr = format!("{host}:{port}");
    while Instant::now() < deadline {
        if let Ok(mut addrs) = addr.to_socket_addrs()
            && let Some(sa) = addrs.next()
            && TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn is_listening(host: &str, port: u16) -> bool {
    format!("{host}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .map(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok())
        .unwrap_or(false)
}

/// Kill a process by PID, portably (TerminateProcess on Windows, SIGKILL on
/// Unix), since `up` does not retain the `Child` handle across invocations.
fn kill_pid(pid: u32) -> bool {
    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(not(windows))]
    let status = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(status, Ok(s) if s.success())
}

/// Is the engine binary runnable? Tries `<program> --version`.
fn binary_present(engine: Engine) -> bool {
    Command::new(engine.program())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn run_server(workspace: &Path, cmd: ServerCommand) -> ExitCode {
    match cmd {
        ServerCommand::Up(args) => up(workspace, &args),
        ServerCommand::Status => status(workspace),
        ServerCommand::Down => down(workspace),
        ServerCommand::Doctor(args) => doctor(workspace, &args),
    }
}

fn config_from(args: &ServerUpArgs) -> ServerConfig {
    ServerConfig {
        engine: args.engine,
        model: args.model.clone(),
        mmproj: args.mmproj.clone(),
        ctx: args.ctx,
        host: "127.0.0.1".to_string(),
        port: args.port,
        threads: args.threads,
        gpu_layers: args.gpu_layers,
        batch_size: args.batch_size,
        tailscale: args.tailscale,
    }
}

fn up(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    let cfg = config_from(args);
    let launch = command(&cfg);

    let mut proc = Command::new(&launch.program);
    proc.args(&launch.args);
    for (k, v) in &launch.env {
        proc.env(k, v);
    }
    println!("Launching {} on {} ...", launch.program, cfg.base_url());
    let child = match proc.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!(
                "could not start `{}`: {e}\n(is it installed and on PATH?)",
                launch.program
            );
            return ExitCode::FAILURE;
        }
    };
    let pid = child.id();
    // Drop the handle — the server keeps running past this command.
    drop(child);

    if !wait_listening(&cfg.host, cfg.port, Duration::from_secs(300)) {
        eprintln!(
            "server did not become reachable on {} within 300s",
            cfg.base_url()
        );
        let _ = kill_pid(pid);
        return ExitCode::FAILURE;
    }

    let mut base_url = cfg.base_url();
    if cfg.tailscale {
        println!(
            "Exposing {} to Tailnet (tailscale serve {})...",
            base_url, cfg.port
        );
        match Command::new("tailscale")
            .args(["serve", "--bg", &cfg.port.to_string()])
            .status()
        {
            Ok(status) if status.success() => {
                println!("Tailscale proxy active.");
                if let Ok(status_out) = Command::new("tailscale")
                    .args(["status", "--json"])
                    .output()
                    && let Ok(json) =
                        serde_json::from_slice::<serde_json::Value>(&status_out.stdout)
                    && let Some(dns_name) = json.get("DNSName").and_then(|v| v.as_str())
                {
                    let clean_dns = dns_name.trim_end_matches('.');
                    base_url = format!("https://{}/v1", clean_dns);
                    println!("Tailscale FQDN discovered: {}", clean_dns);
                }
            }
            Ok(status) => {
                eprintln!("Warning: `tailscale serve` failed with status: {}", status);
            }
            Err(e) => {
                eprintln!("Warning: could not execute `tailscale serve`: {e}");
            }
        }
    }

    let runfile = ServerRunfile {
        engine: cfg.engine,
        pid,
        port: cfg.port,
        base_url: base_url.clone(),
        tailscale: cfg.tailscale,
    };

    let path = runfile_path(workspace);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let local_res = serde_json::to_string_pretty(&runfile)
        .map_err(|_| ())
        .and_then(|s| std::fs::write(&path, &s).map_err(|_| ()));

    let global_res = if let Some(global) = global_runfile_path() {
        if let Some(parent) = global.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        serde_json::to_string_pretty(&runfile)
            .map_err(|_| ())
            .and_then(|s| std::fs::write(&global, &s).map_err(|_| ()))
            .map(|_| global)
    } else {
        Err(())
    };

    if local_res.is_ok() || global_res.is_ok() {
        println!("server ready: {} (pid {pid})", base_url);
        if local_res.is_ok() {
            println!("registered locally at {}", path.display());
        }
        if let Ok(global) = global_res {
            println!("registered globally at {}", global.display());
        }
        ExitCode::SUCCESS
    } else {
        eprintln!("server is up (pid {pid}) but the runfile could not be written");
        let _ = kill_pid(pid);
        ExitCode::FAILURE
    }
}

fn status(workspace: &Path) -> ExitCode {
    match read_runfile(workspace) {
        Some(rf) => {
            let live = is_listening("127.0.0.1", rf.port);
            println!(
                "engine={:?} pid={} base_url={} ({})",
                rf.engine,
                rf.pid,
                rf.base_url,
                if live { "reachable" } else { "NOT reachable" }
            );
            if live {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        None => {
            println!("no server registered (no .ferric/server.json)");
            ExitCode::FAILURE
        }
    }
}

fn down(workspace: &Path) -> ExitCode {
    match read_runfile(workspace) {
        Some(rf) => {
            let killed = kill_pid(rf.pid);
            let _ = std::fs::remove_file(runfile_path(workspace));
            if let Some(global) = global_runfile_path() {
                let _ = std::fs::remove_file(global);
            }
            if killed {
                println!("stopped server pid {}", rf.pid);
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "could not kill pid {} (already gone?); runfile removed",
                    rf.pid
                );
                ExitCode::FAILURE
            }
        }
        None => {
            println!("no server registered");
            ExitCode::SUCCESS
        }
    }
}

fn doctor(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    let mut ok = true;

    let bin = binary_present(args.engine);
    println!(
        "[{}] engine binary `{}`",
        if bin { "ok" } else { "MISSING" },
        args.engine.program()
    );
    ok &= bin;

    if let Some(model) = &args.model {
        // Only path-like models can be checked locally (Ollama names can't).
        if args.engine == Engine::LlamaServer {
            let present = Path::new(model).exists();
            println!(
                "[{}] model `{}`",
                if present { "ok" } else { "MISSING" },
                model
            );
            ok &= present;
        }
    } else if args.engine == Engine::LlamaServer {
        println!("[warn] no --model given (llama-server needs one)");
    }

    match read_runfile(workspace) {
        Some(rf) if is_listening("127.0.0.1", rf.port) => {
            println!("[ok] server reachable at {}", rf.base_url);
            println!("     health: {}", health_url(rf.engine, &rf.base_url));
            println!(
                "     verify the constrained path: `ferric toolbench --backend openai --protocol grammar`"
            );
        }
        Some(rf) => println!("[warn] registered server {} is not reachable", rf.base_url),
        None => println!("[info] no server running — `ferric server up` to start one"),
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(engine: Engine) -> ServerConfig {
        ServerConfig {
            engine,
            model: Some("model.gguf".to_string()),
            mmproj: None,
            ctx: 4096,
            host: "127.0.0.1".to_string(),
            port: 8080,
            threads: None,
            gpu_layers: None,
            batch_size: None,
            tailscale: false,
        }
    }

    #[test]
    fn llama_server_argv() {
        let c = command(&cfg(Engine::LlamaServer));
        assert_eq!(c.program, "llama-server");
        assert_eq!(
            c.args,
            vec![
                "-m",
                "model.gguf",
                "-c",
                "4096",
                "--host",
                "127.0.0.1",
                "--port",
                "8080"
            ]
        );
        assert!(c.env.is_empty());
    }

    #[test]
    fn llama_server_mmproj() {
        let mut config = cfg(Engine::LlamaServer);
        config.mmproj = Some(PathBuf::from("proj.gguf"));
        let c = command(&config);
        assert!(c.args.windows(2).any(|w| w == ["--mmproj", "proj.gguf"]));
    }

    #[test]
    fn ollama_argv_and_env() {
        let c = command(&cfg(Engine::Ollama));
        assert_eq!(c.program, "ollama");
        assert_eq!(c.args, vec!["serve"]);
        assert_eq!(
            c.env,
            vec![("OLLAMA_HOST".to_string(), "127.0.0.1:8080".to_string())]
        );
    }

    #[test]
    fn llama_server_edge_tuning_flags() {
        // WHEN threads/gpu_layers/batch_size are set with llama-server THEN
        // argv SHALL include the matching flags (sprint 35).
        let mut config = cfg(Engine::LlamaServer);
        config.threads = Some(4);
        config.gpu_layers = Some(20);
        config.batch_size = Some(512);
        let c = command(&config);
        assert!(c.args.windows(2).any(|w| w == ["-t", "4"]));
        assert!(c.args.windows(2).any(|w| w == ["-ngl", "20"]));
        assert!(c.args.windows(2).any(|w| w == ["-b", "512"]));
    }

    #[test]
    fn ollama_ignores_edge_tuning_flags() {
        // Ollama doesn't take these as CLI flags — set-but-unused, argv unchanged.
        let mut config = cfg(Engine::Ollama);
        config.threads = Some(4);
        config.gpu_layers = Some(20);
        config.batch_size = Some(512);
        let c = command(&config);
        assert_eq!(
            c.args,
            vec!["serve"],
            "edge-tuning flags must not leak into Ollama argv"
        );
    }

    #[test]
    fn host_is_loopback() {
        // ADR-005: the launcher binds loopback only.
        for engine in [Engine::LlamaServer, Engine::Ollama] {
            let c = command(&cfg(engine));
            let joined = format!("{} {:?}", c.args.join(" "), c.env);
            assert!(joined.contains("127.0.0.1"));
            assert!(!joined.contains("0.0.0.0"));
        }
    }

    #[test]
    fn health_url_per_engine() {
        assert_eq!(
            health_url(Engine::LlamaServer, "http://127.0.0.1:8080/v1"),
            "http://127.0.0.1:8080/health"
        );
        assert_eq!(
            health_url(Engine::Ollama, "http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/v1/models"
        );
    }

    #[test]
    fn runfile_serde_roundtrip() {
        let rf = ServerRunfile {
            engine: Engine::LlamaServer,
            pid: 4321,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
        };
        let s = serde_json::to_string(&rf).unwrap();
        let back: ServerRunfile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.pid, 4321);
        assert_eq!(back.engine, Engine::LlamaServer);
        assert_eq!(back.base_url, rf.base_url);
    }

    #[test]
    fn read_runfile_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_runfile_from(dir.path(), &|_| None).is_none());
    }
}
