//! `ferric server` — launcher for the OpenAI-compatible inference server (the
//! ADR-001 HTTP valve), so the constrained path is one command instead of a
//! manually-started server. Default engine: llama.cpp `llama-server`; Ollama
//! pluggable via `--engine`. The host is pinned to loopback (ADR-005) — the
//! launcher never binds a public interface and never execs an arbitrary binary
//! (the engine is a closed enum).
//!
//! Lifecycle (`up`/`status`/`down`) uses an engine-specific HTTP health probe,
//! retained Windows process HANDLEs or Linux pidfds, and exact listener-owner
//! inspection. The deeper *constrained* capability check is `ferric toolbench
//! --protocol grammar` against the launched server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::server_process::{
    ListenerState, LiveProcess, NativeProcessRuntime, ProcessError, ProcessFacts, ProcessIdentity,
    ProcessRuntime, RetainedProcess as RetainedProcessHandle, acquire_matching_process,
    loopback_listener_state,
};
use crate::server_registration::{
    CapturedRegistration, PublishError, RegistrationCoordinate, RegistrationInventory,
    RegistrationScope, RegistrationSlot, RemovalOutcome, ReplacementOutcome, inventory_runfiles,
    publish_mirrored, remove_if_unchanged, replace_if_unchanged, validate_runfile,
};
use crate::server_resolution::{
    Candidate, CandidateState, HealthState, Resolution, ResolutionIssue, ResolutionIssueKind,
    resolve,
};

pub(crate) const RUNFILE_SCHEMA_V2: u8 = 2;

const fn legacy_runfile_schema() -> u8 {
    1
}

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
    /// Non-destructively bind one live legacy registration to its exact process identity.
    Adopt(ServerAdoptArgs),
    /// Stop the registered server and remove the runfile.
    Down,
    /// Check engine-binary + model presence (and reachability if up).
    Doctor(Box<ServerUpArgs>),
}

#[derive(Args, Clone)]
pub struct ServerAdoptArgs {
    /// PID named by the live schema-v1 registration being adopted.
    #[arg(long)]
    pub pid: u32,
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
    /// Sampling seed (llama-server only). Use a non-negative value for a
    /// reproducible non-greedy run; llama.cpp reserves -1 for a random seed.
    #[arg(long, allow_hyphen_values = true)]
    pub seed: Option<i64>,
    /// Number of concurrent llama-server request slots. The Sprint 113 causal
    /// comparison uses one slot to avoid cross-request scheduling effects.
    #[arg(long)]
    pub parallel: Option<u32>,
    /// Reserved; currently refused until exact Tailscale Serve rollback exists.
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
    pub seed: Option<i64>,
    pub parallel: Option<u32>,
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
            if let Some(seed) = cfg.seed {
                args.push("--seed".to_string());
                args.push(seed.to_string());
            }
            if let Some(parallel) = cfg.parallel {
                args.push("--parallel".to_string());
                args.push(parallel.to_string());
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
    format!("{root}{}", health_path(engine))
}

fn health_path(engine: Engine) -> &'static str {
    match engine {
        Engine::LlamaServer => "/health",
        Engine::Ollama => "/v1/models",
    }
}

/// The registered-server record. Lets `query`/`toolbench` auto-discover the
/// base URL (T-805) and `down` find the PID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRunfile {
    /// Schema 1 is the historical PID-only record. Schema 2 binds the record
    /// to a process creation instance, executable, argv, and its originating
    /// local alias. Missing schema metadata therefore remains readable without
    /// being mistaken for teardown authority.
    #[serde(default = "legacy_runfile_schema")]
    pub schema_version: u8,
    pub engine: Engine,
    pub pid: u32,
    pub port: u16,
    pub base_url: String,
    #[serde(default)]
    pub tailscale: bool,
    /// Additive launch provenance. Old runfiles deserialize these fields as
    /// unknown rather than silently claiming a reproducible setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling_seed: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_slots: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_identity: Option<ProcessIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_local_runfile: Option<PathBuf>,
}

/// Runfile location: `<workspace>/.ferric/server.json` (the `.ferric/` dir is
/// already write-denied to the LLM — ADR-005).
pub fn runfile_path(workspace: &Path) -> PathBuf {
    workspace.join(".ferric").join("server.json")
}

pub fn global_runfile_path() -> Option<PathBuf> {
    crate::config::user_config_path().map(|p| p.with_file_name("server.json"))
}

#[cfg(test)]
fn read_runfile_result_impl(
    workspace: &Path,
    global: Option<PathBuf>,
) -> Result<Option<ServerRunfile>, String> {
    let scope = ManagedDiscoveryScope {
        workspace: workspace.to_path_buf(),
        global,
    };
    let discovery = discover_managed_server_in(&scope);
    match discovery.state {
        ManagedServerState::Empty => Ok(None),
        ManagedServerState::StaleOnly { stale } => {
            let details = stale
                .iter()
                .map(|coordinate| registration_label(coordinate.scope, &coordinate.path))
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!(
                "only stale server registrations remain ({details}); run `ferric server down` to clean them after reviewing the reported listener state"
            ))
        }
        ManagedServerState::Conflict { issues } | ManagedServerState::Unverifiable { issues } => {
            Err(format!(
                "server registration resolution is blocked: {}",
                issues
                    .iter()
                    .map(|issue| issue.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        }
        ManagedServerState::Degraded { server, .. } => match server.listener {
            ListenerState::Absent => Err(format!(
                "managed server PID {} has no listener on its registered port {}",
                server.runfile.pid, server.runfile.port
            )),
            ListenerState::OwnedByTargetWildcard => Err(format!(
                "managed server PID {} exposes registered port {} through a wildcard/public listener",
                server.runfile.pid, server.runfile.port
            )),
            ListenerState::OwnedByTarget if server.health != HealthState::Healthy => Err(format!(
                "managed server PID {} owns the registered loopback listener, but its engine health endpoint is not healthy",
                server.runfile.pid
            )),
            other => Err(format!(
                "managed server PID {} is degraded by listener state {other:?}",
                server.runfile.pid
            )),
        },
        ManagedServerState::Ready(server) => Ok(Some(server.runfile)),
    }
}

fn is_listening(host: &str, port: u16) -> bool {
    format!("{host}:{port}")
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .map(|sa| TcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok())
        .unwrap_or(false)
}

/// Issue a bounded HTTP/1.1 GET to the engine's local health endpoint.
///
/// A TCP handshake alone is not server readiness: an unrelated process can own
/// the port, and llama-server opens its socket before every HTTP route is ready.
/// Keep this std-only so `ferric server` remains available in the default build.
fn http_status_ok(host: &str, port: u16, path: &str) -> bool {
    let addr = format!("{host}:{port}");
    let Some(socket_addr) = addr
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
    else {
        return false;
    };

    let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, Duration::from_millis(500))
    else {
        return false;
    };
    let timeout = Some(Duration::from_millis(500));
    if stream.set_read_timeout(timeout).is_err() || stream.set_write_timeout(timeout).is_err() {
        return false;
    }

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    // The status line is tiny. Bounding the reader prevents an uncooperative
    // loopback service from making the readiness probe allocate without limit.
    let mut first_line = String::new();
    let mut reader = BufReader::new(stream).take(256);
    if reader.read_line(&mut first_line).is_err() {
        return false;
    }
    let mut fields = first_line.split_whitespace();
    matches!(fields.next(), Some("HTTP/1.0") | Some("HTTP/1.1")) && fields.next() == Some("200")
}

/// Live process/listener facts used by strict autonomy evidence.  The runfile
/// is only a registration hint; callers must bind it back to the process and
/// socket which exist now before treating it as provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegisteredServerSnapshot {
    pub pid: u32,
    pub executable: PathBuf,
    pub argv: Vec<String>,
    pub listener_owner_pid: u32,
}

/// Fail-closed live validation for a registered managed server.
///
/// This deliberately does not infer process identity from HTTP health alone:
/// an unrelated service can answer on the registered port.  Platform process
/// inspection must prove the executable/argv and that the registered PID owns
/// the listening socket.  Unsupported or unavailable inspection is an error.
pub(crate) fn inspect_registered_server(
    runfile: &ServerRunfile,
) -> Result<RegisteredServerSnapshot, String> {
    with_registered_server_effect(runfile, || {
        if http_status_ok("127.0.0.1", runfile.port, health_path(runfile.engine)) {
            Ok(())
        } else {
            Err(format!(
                "registered server PID {} does not have a healthy engine endpoint on loopback port {}",
                runfile.pid, runfile.port
            ))
        }
    })
    .map(|(snapshot, ())| snapshot)
}

/// Execute one synchronous consumer effect while retaining and revalidating
/// the exact registered process generation on both sides of the effect.
pub(crate) fn with_registered_server_effect<T, F>(
    runfile: &ServerRunfile,
    effect: F,
) -> Result<(RegisteredServerSnapshot, T), String>
where
    F: FnOnce() -> Result<T, String>,
{
    bracket_registered_effect_with(&NativeProcessRuntime, runfile, effect)
}

fn bracket_registered_effect_with<R, T, F>(
    runtime: &R,
    runfile: &ServerRunfile,
    effect: F,
) -> Result<(RegisteredServerSnapshot, T), String>
where
    R: ProcessRuntime,
    F: FnOnce() -> Result<T, String>,
{
    let (retained_process, facts) = if let Some(expected) = &runfile.process_identity {
        let inspection = acquire_matching_process(runtime, runfile.pid, runfile.port, expected)
            .map_err(|error| format!("bind registered process identity: {error}"))?;
        (inspection.process, inspection.facts)
    } else {
        let process = runtime
            .acquire(runfile.pid)
            .map_err(|error| format!("acquire registered process: {error}"))?;
        let facts = process
            .inspect(runfile.port)
            .map_err(|error| format!("inspect registered process: {error}"))?;
        (process, facts)
    };
    require_exclusive_registered_listener(runfile, &facts.listener)?;
    let effect_result = effect();

    // Consumer I/O creates a scheduling window. Reinspect even when the
    // effect fails so a replacement listener cannot hide the lost authority.
    let post_probe = retained_process
        .inspect(runfile.port)
        .map_err(|error| format!("revalidate registered process after consumer effect: {error}"))?;
    if post_probe.identity != facts.identity {
        return Err(format!(
            "registered server PID {} changed process identity during consumer effect",
            runfile.pid
        ));
    }
    require_exclusive_registered_listener(runfile, &post_probe.listener)?;
    let effect_value = effect_result?;

    Ok((
        RegisteredServerSnapshot {
            pid: runfile.pid,
            executable: post_probe.identity.executable,
            argv: post_probe.identity.argv,
            listener_owner_pid: runfile.pid,
        },
        effect_value,
    ))
}

fn require_exclusive_registered_listener(
    runfile: &ServerRunfile,
    listener: &ListenerState,
) -> Result<(), String> {
    match listener {
        ListenerState::OwnedByTarget => {}
        ListenerState::OwnedByTargetWildcard => {
            return Err(format!(
                "registered server PID {} owns a wildcard/public listener on port {}; only an exclusive loopback listener is healthy managed state",
                runfile.pid, runfile.port
            ));
        }
        ListenerState::Absent => {
            return Err(format!(
                "registered server PID {} owns no loopback listener on port {}",
                runfile.pid, runfile.port
            ));
        }
        ListenerState::OwnedByOther(owners) => {
            return Err(format!(
                "loopback port {} is owned by other PIDs {owners:?}, not registered PID {}",
                runfile.port, runfile.pid
            ));
        }
        ListenerState::Uninspectable(error) => return Err(error.clone()),
    }
    Ok(())
}

/// Narrow lifecycle interfaces keep the spawn/bind/readiness windows
/// deterministic in tests. Production still delegates to `Child`, the native
/// retained HANDLE/pidfd adapter, and the real listener/HTTP/clock functions.
trait SpawnedChild {
    type ExitStatus: std::fmt::Display;

    fn pid(&self) -> u32;
    fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String>;
    fn wait(&mut self) -> Result<Self::ExitStatus, String>;
    fn kill(&mut self) -> Result<(), String>;
}

impl SpawnedChild for Child {
    type ExitStatus = std::process::ExitStatus;

    fn pid(&self) -> u32 {
        Child::id(self)
    }

    fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String> {
        Child::try_wait(self).map_err(|error| error.to_string())
    }

    fn wait(&mut self) -> Result<Self::ExitStatus, String> {
        Child::wait(self).map_err(|error| error.to_string())
    }

    fn kill(&mut self) -> Result<(), String> {
        Child::kill(self).map_err(|error| error.to_string())
    }
}

trait SpawnedProcessRuntime<C: SpawnedChild> {
    type Process: RetainedProcessHandle;

    fn acquire_child(&self, child: &C) -> Result<Self::Process, String>;
}

struct NativeSpawnedProcessRuntime;

impl SpawnedProcessRuntime<Child> for NativeSpawnedProcessRuntime {
    type Process = LiveProcess;

    fn acquire_child(&self, child: &Child) -> Result<Self::Process, String> {
        LiveProcess::acquire_child(child).map_err(|error| error.to_string())
    }
}

trait ListenerInspector {
    fn listener_state(&self, pid: u32, port: u16) -> ListenerState;
}

struct NativeListenerInspector;

impl ListenerInspector for NativeListenerInspector {
    fn listener_state(&self, pid: u32, port: u16) -> ListenerState {
        loopback_listener_state(pid, port)
    }
}

trait HealthProbe {
    fn status_ok(&mut self, host: &str, port: u16, path: &str) -> bool;
}

struct NativeHealthProbe;

impl HealthProbe for NativeHealthProbe {
    fn status_ok(&mut self, host: &str, port: u16, path: &str) -> bool {
        http_status_ok(host, port, path)
    }
}

trait LifecycleClock {
    fn now(&mut self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

struct SystemLifecycleClock;

impl LifecycleClock for SystemLifecycleClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Retain the spawned child while polling HTTP readiness. This ties a healthy
/// endpoint to a process that has not already exited before any runfile is
/// written. Port-availability preflight closes the ordinary conflicting-listener
/// case; the post-probe `try_wait` closes the child-exited-during-probe race.
fn wait_healthy_with<C: SpawnedChild, H: HealthProbe, K: LifecycleClock>(
    child: &mut C,
    engine: Engine,
    host: &str,
    port: u16,
    timeout: Duration,
    health: &mut H,
    clock: &mut K,
) -> Result<(), String> {
    let deadline = clock.now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!("engine process exited before readiness ({status})"));
            }
            Ok(None) => {}
            Err(error) => return Err(format!("could not inspect engine process: {error}")),
        }

        if health.status_ok(host, port, health_path(engine)) {
            return match child.try_wait() {
                Ok(Some(status)) => Err(format!(
                    "engine process exited while readiness was checked ({status})"
                )),
                Ok(None) => Ok(()),
                Err(error) => Err(format!("could not inspect engine process: {error}")),
            };
        }

        if clock.now() >= deadline {
            return Err(format!(
                "HTTP health endpoint {} did not return 200 within {}s",
                health_path(engine),
                timeout.as_secs()
            ));
        }
        clock.sleep(Duration::from_millis(500));
    }
}

fn wait_healthy(
    child: &mut Child,
    engine: Engine,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), String> {
    wait_healthy_with(
        child,
        engine,
        host,
        port,
        timeout,
        &mut NativeHealthProbe,
        &mut SystemLifecycleClock,
    )
}

fn stop_child<C: SpawnedChild>(child: &mut C) -> Result<(), String> {
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => {
            return Err(format!(
                "could not prove spawned child PID {} is still the unreaped child before fallback shutdown: {error}",
                child.pid()
            ));
        }
    }

    if let Err(kill_error) = child.kill() {
        return match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!(
                "could not terminate owned child PID {}: {kill_error}",
                child.pid()
            )),
            Err(recheck_error) => Err(format!(
                "could not terminate owned child PID {} ({kill_error}) or recheck it ({recheck_error})",
                child.pid()
            )),
        };
    }
    child.wait().map(|_| ()).map_err(|error| {
        format!(
            "could not reap terminated child PID {}: {error}",
            child.pid()
        )
    })
}

fn require_listener_released_with<L: ListenerInspector>(
    listener: &L,
    pid: u32,
    port: u16,
) -> Result<(), String> {
    match listener.listener_state(pid, port) {
        ListenerState::Absent => Ok(()),
        ListenerState::OwnedByTarget | ListenerState::OwnedByTargetWildcard => Err(format!(
            "numeric PID {pid} still owns registered port {port} after retained-process exit"
        )),
        ListenerState::OwnedByOther(owners) => Err(format!(
            "registered port {port} remains owned by PIDs {owners:?} after retained-process exit"
        )),
        ListenerState::Uninspectable(error) => Err(format!(
            "registered port {port} ownership is uninspectable after retained-process exit: {error}"
        )),
    }
}

fn require_listener_released(pid: u32, port: u16) -> Result<(), String> {
    require_listener_released_with(&NativeListenerInspector, pid, port)
}

/// Stop the exact process object retained before readiness and publication.
/// Registration rollback is authorized only after this returns `Ok(())`.
fn stop_managed_child_with<C, P, L>(
    child: &mut C,
    process: &P,
    port: u16,
    listener: &L,
) -> Result<(), String>
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
{
    process
        .terminate()
        .map_err(|error| format!("terminate retained process object: {error}"))?;
    match process.wait(Duration::from_secs(10)) {
        Ok(true) => {}
        Ok(false) => {
            return Err("retained process object did not exit within 10s".to_string());
        }
        Err(error) => return Err(format!("wait for retained process object: {error}")),
    }

    // Once retained-handle exit is proven, reaping the original Child is
    // unconditional. A residual or uninspectable listener is a failed cleanup
    // postcondition, but must not leak the known-exited child.
    let reap_error = child
        .wait()
        .map(|_| ())
        .map_err(|error| format!("reap exited child PID {}: {error}", child.pid()))
        .err();
    let listener_error = require_listener_released_with(listener, process.pid(), port).err();
    match (reap_error, listener_error) {
        (None, None) => Ok(()),
        (Some(reap), None) => Err(reap),
        (None, Some(listener)) => Err(listener),
        (Some(reap), Some(listener)) => Err(format!("{reap}; {listener}")),
    }
}

fn stop_managed_child(child: &mut Child, process: &LiveProcess, port: u16) -> Result<(), String> {
    stop_managed_child_with(child, process, port, &NativeListenerInspector)
}

/// Bind the spawned child to its durable OS process object before any
/// readiness operation. Before binding, the original `Child` remains the
/// authority: Windows owns its process HANDLE, while Unix cannot reuse a live,
/// unreaped child's PID. Failures after binding clean up only through the
/// retained object and name an unproved retained generation as recovery state.
fn bind_spawned_child<C, R, L>(
    child: &mut C,
    runtime: &R,
    port: u16,
    listener: &L,
) -> Result<R::Process, String>
where
    C: SpawnedChild,
    R: SpawnedProcessRuntime<C>,
    L: ListenerInspector,
{
    let pid = child.pid();
    let process = match runtime.acquire_child(child) {
        Ok(process) => process,
        Err(error) => {
            return match stop_child(child) {
                Ok(()) => Err(format!(
                    "could not bind spawned child PID {pid} to an exact process object ({error}); the original child was stopped"
                )),
                Err(cleanup) => Err(format!(
                    "could not bind spawned child PID {pid} to an exact process object ({error}); recovery required because original-child cleanup was not proven: {cleanup}"
                )),
            };
        }
    };

    if process.pid() != pid {
        return match stop_child(child) {
            Ok(()) => Err(format!(
                "retained process object reported PID {} for spawned child PID {pid}; the original child was stopped without signalling the mismatched object",
                process.pid()
            )),
            Err(cleanup) => Err(format!(
                "retained process object reported PID {} for spawned child PID {pid}; recovery required because original-child cleanup was not proven: {cleanup}",
                process.pid()
            )),
        };
    }

    match child.try_wait() {
        Ok(None) => Ok(process),
        Ok(Some(status)) => Err(format!(
            "spawned engine PID {pid} exited before retained-process binding could be confirmed ({status}); no replacement process was signalled"
        )),
        Err(error) => match stop_managed_child_with(child, &process, port, listener) {
            Ok(()) => Err(format!(
                "could not confirm spawned engine PID {pid} after retained-process binding ({error}); the exact retained child was stopped"
            )),
            Err(cleanup) => Err(format!(
                "could not confirm spawned engine PID {pid} after retained-process binding ({error}); recovery failure for retained PID {pid}: cleanup was not proven: {cleanup}"
            )),
        },
    }
}

/// Identity/listener inspection is the final publication gate. Any
/// non-exclusive result stops and reaps the retained child before returning an
/// error, so callers cannot publish a registration for that result.
fn inspect_bound_child_for_publication<C, P, L>(
    child: &mut C,
    process: &P,
    port: u16,
    listener: &L,
) -> Result<ProcessFacts, String>
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
{
    let facts = match process.inspect(port) {
        Ok(facts) => facts,
        Err(error) => {
            let cleanup = stop_managed_child_with(child, process, port, listener);
            return Err(match cleanup {
                Ok(()) => format!(
                    "server became healthy but retained process/listener identity inspection failed ({error}); exact child exit was proved"
                ),
                Err(cleanup) => format!(
                    "server became healthy but retained process/listener identity inspection failed ({error}); recovery failure for retained PID {}: {cleanup}",
                    process.pid()
                ),
            });
        }
    };
    if facts.listener == ListenerState::OwnedByTarget {
        return Ok(facts);
    }

    let ownership = format!("{:?}", facts.listener);
    let cleanup = stop_managed_child_with(child, process, port, listener);
    Err(match cleanup {
        Ok(()) => format!(
            "server child PID {} does not exclusively own the expected loopback listener ({ownership}); exact child exit was proved and no registration may be published",
            process.pid()
        ),
        Err(cleanup) => format!(
            "server child PID {} does not exclusively own the expected loopback listener ({ownership}); no registration may be published; recovery failure: {cleanup}",
            process.pid()
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedDiscoveryScope {
    pub workspace: PathBuf,
    pub global: Option<PathBuf>,
}

impl ManagedDiscoveryScope {
    pub(crate) fn for_workspace(workspace: &Path) -> Result<Self, String> {
        Ok(Self {
            workspace: std::path::absolute(workspace)
                .map_err(|error| format!("resolve managed discovery workspace: {error}"))?,
            global: global_runfile_path(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservationId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromisedOriginProvenance {
    pub source: RegistrationCoordinate,
    pub expected_runfile: ServerRunfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeObservation {
    NotInspected,
    Verified {
        identity: ProcessIdentity,
        listener: ListenerState,
        health: HealthState,
    },
    Stale {
        reason: String,
        observed_identity: Option<ProcessIdentity>,
        listener: ListenerState,
    },
    LegacyLive {
        pid: u32,
    },
    Unverifiable {
        reason: String,
        observed_identity: Option<ProcessIdentity>,
        listener: Option<ListenerState>,
        health: HealthState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedRegistrationState {
    Absent,
    Blocked {
        reason: String,
    },
    Captured {
        runfile: Box<ServerRunfile>,
        raw_sha256: String,
        runtime: RuntimeObservation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedRegistrationObservation {
    pub id: ObservationId,
    pub coordinate: RegistrationCoordinate,
    pub promised: Option<PromisedOriginProvenance>,
    pub state: ManagedRegistrationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationRevisionState {
    Absent,
    Blocked(String),
    Captured(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationRevision {
    pub coordinate: RegistrationCoordinate,
    pub promised: Option<PromisedOriginProvenance>,
    pub state: RegistrationRevisionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveryFingerprint {
    pub pid: u32,
    pub identity: ProcessIdentity,
    pub runfile: ServerRunfile,
    pub revisions: Vec<RegistrationRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedServer {
    pub registration: RegistrationCoordinate,
    pub runfile: ServerRunfile,
    pub identity: ProcessIdentity,
    pub listener: ListenerState,
    pub health: HealthState,
    pub aliases: Vec<RegistrationCoordinate>,
    pub stale: Vec<RegistrationCoordinate>,
    pub fingerprint: DiscoveryFingerprint,
}

impl ManagedServer {
    pub(crate) fn ready_snapshot(&self) -> Result<RegisteredServerSnapshot, String> {
        if self.listener != ListenerState::OwnedByTarget || self.health != HealthState::Healthy {
            return Err(
                "managed process snapshot requires exclusive loopback ownership and healthy HTTP"
                    .to_string(),
            );
        }
        Ok(RegisteredServerSnapshot {
            pid: self.runfile.pid,
            executable: self.identity.executable.clone(),
            argv: self.identity.argv.clone(),
            listener_owner_pid: self.runfile.pid,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedServerState {
    Empty,
    Ready(ManagedServer),
    Degraded {
        server: ManagedServer,
        issues: Vec<ResolutionIssue>,
    },
    StaleOnly {
        stale: Vec<RegistrationCoordinate>,
    },
    Conflict {
        issues: Vec<ResolutionIssue>,
    },
    Unverifiable {
        issues: Vec<ResolutionIssue>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedServerDiscovery {
    pub inventory: RegistrationInventory,
    pub observations: Vec<ManagedRegistrationObservation>,
    pub state: ManagedServerState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StatusNextAction {
    StartServer,
    ContinueManaged {
        base_url: String,
    },
    StopManaged {
        pid: u32,
    },
    CleanStale,
    AdoptLegacy {
        pid: u32,
    },
    InspectWildcard {
        port: u16,
    },
    InspectPromisedOrigin {
        path: PathBuf,
    },
    InspectTailscale {
        port: u16,
    },
    ResolveConflict {
        coordinates: Vec<RegistrationCoordinate>,
    },
    RepairUnverifiable {
        coordinates: Vec<RegistrationCoordinate>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerStatusReport {
    pub registrations: Vec<ManagedRegistrationObservation>,
    pub state: ManagedServerState,
    pub next_action: StatusNextAction,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedServerStatus {
    stdout: Vec<String>,
    stderr: Vec<String>,
    success: bool,
}

struct LifecycleDiscovery {
    managed: ManagedServerDiscovery,
    observations: Vec<LifecycleObservation>,
    resolution: Resolution,
}

struct LifecycleObservation {
    candidate: Candidate,
    label: String,
    capture: Option<CapturedRegistration>,
    process: Option<LiveProcess>,
}

fn registration_label(scope: RegistrationScope, path: &Path) -> String {
    format!("{scope} registration {}", path.display())
}

fn flatten_inventory(inventory: &RegistrationInventory) -> Vec<ManagedRegistrationObservation> {
    fn flatten_slot(
        observations: &mut Vec<ManagedRegistrationObservation>,
        slot: &RegistrationSlot,
        promised: Option<PromisedOriginProvenance>,
    ) {
        let (coordinate, state) = match slot {
            RegistrationSlot::Absent { scope, path } => (
                RegistrationCoordinate {
                    scope: *scope,
                    path: path.clone(),
                },
                ManagedRegistrationState::Absent,
            ),
            RegistrationSlot::Blocked {
                scope,
                path,
                reason,
            } => (
                RegistrationCoordinate {
                    scope: *scope,
                    path: path.clone(),
                },
                ManagedRegistrationState::Blocked {
                    reason: reason.to_string(),
                },
            ),
            RegistrationSlot::Captured(capture) => (
                RegistrationCoordinate {
                    scope: capture.scope,
                    path: capture.path.clone(),
                },
                ManagedRegistrationState::Captured {
                    runfile: Box::new(capture.runfile.clone()),
                    raw_sha256: ferric_bench::sha256_bytes(&capture.raw),
                    runtime: RuntimeObservation::NotInspected,
                },
            ),
        };
        observations.push(ManagedRegistrationObservation {
            id: ObservationId(observations.len()),
            coordinate,
            promised,
            state,
        });
    }

    let mut observations = Vec::new();
    flatten_slot(&mut observations, &inventory.local, None);
    if let Some(global) = &inventory.global {
        flatten_slot(&mut observations, global, None);
    }
    for origin in &inventory.promised_origins {
        flatten_slot(
            &mut observations,
            &origin.slot,
            Some(PromisedOriginProvenance {
                source: origin.source.clone(),
                expected_runfile: origin.expected_runfile.clone(),
            }),
        );
    }
    observations
}

fn static_inventory_issues(
    observations: &[ManagedRegistrationObservation],
) -> Vec<ResolutionIssue> {
    let mut issues = Vec::new();
    for observation in observations {
        match &observation.state {
            ManagedRegistrationState::Blocked { reason } => issues.push(ResolutionIssue {
                coordinates: vec![observation.coordinate.clone()],
                kind: ResolutionIssueKind::Unverifiable,
                detail: format!(
                    "{}: {reason}",
                    registration_label(observation.coordinate.scope, &observation.coordinate.path)
                ),
            }),
            ManagedRegistrationState::Absent if observation.promised.is_some() => {
                issues.push(ResolutionIssue {
                    coordinates: vec![observation.coordinate.clone()],
                    kind: ResolutionIssueKind::Unverifiable,
                    detail: format!(
                        "promised origin registration {} is absent",
                        observation.coordinate.path.display()
                    ),
                });
            }
            ManagedRegistrationState::Captured { runfile, .. } => {
                if runfile.tailscale {
                    issues.push(ResolutionIssue {
                        coordinates: vec![observation.coordinate.clone()],
                        kind: ResolutionIssueKind::Unverifiable,
                        detail: "registration owns durable Tailscale Serve state".to_string(),
                    });
                }
                if let Some(promised) = &observation.promised
                    && runfile.as_ref() != &promised.expected_runfile
                {
                    issues.push(ResolutionIssue {
                        coordinates: vec![promised.source.clone(), observation.coordinate.clone()],
                        kind: ResolutionIssueKind::Conflict,
                        detail: format!(
                            "promised origin registration {} changed from the source registration metadata",
                            observation.coordinate.path.display()
                        ),
                    });
                }
            }
            ManagedRegistrationState::Absent => {}
        }
    }

    let captured = observations
        .iter()
        .filter_map(|observation| match &observation.state {
            ManagedRegistrationState::Captured { runfile, .. } => Some((observation, runfile)),
            ManagedRegistrationState::Absent | ManagedRegistrationState::Blocked { .. } => None,
        })
        .collect::<Vec<_>>();
    for (left_index, (left_observation, left)) in captured.iter().enumerate() {
        for (right_observation, right) in &captured[left_index + 1..] {
            let same_process_key = left.pid == right.pid
                && left
                    .process_identity
                    .as_ref()
                    .zip(right.process_identity.as_ref())
                    .is_some_and(|(left, right)| left.start_token == right.start_token);
            if same_process_key && left != right {
                issues.push(ResolutionIssue {
                    coordinates: vec![
                        left_observation.coordinate.clone(),
                        right_observation.coordinate.clone(),
                    ],
                    kind: ResolutionIssueKind::Conflict,
                    detail: "the same persisted process key has conflicting registration metadata"
                        .to_string(),
                });
            }
        }
    }
    issues
}

fn push_inventory_slot(
    slot: RegistrationSlot,
    captures: &mut Vec<CapturedRegistration>,
    observations: &mut Vec<LifecycleObservation>,
) {
    match slot {
        RegistrationSlot::Absent { .. } => {}
        RegistrationSlot::Captured(captured) => captures.push(*captured),
        RegistrationSlot::Blocked {
            scope,
            path,
            reason,
        } => {
            let label = registration_label(scope, &path);
            observations.push(LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate { scope, path },
                    runfile: None,
                    state: CandidateState::Unverifiable {
                        reason: reason.to_string(),
                        observed_identity: None,
                        listener: None,
                        health: HealthState::NotProbed,
                    },
                },
                label,
                capture: None,
                process: None,
            });
        }
    }
}

fn expand_registration_captures(
    inventory: RegistrationInventory,
) -> (Vec<CapturedRegistration>, Vec<LifecycleObservation>) {
    let mut captures = Vec::new();
    let mut observations = Vec::new();
    push_inventory_slot(inventory.local, &mut captures, &mut observations);
    if let Some(global) = inventory.global {
        push_inventory_slot(global, &mut captures, &mut observations);
    }

    // The store captured each global-v2 promised origin independently. Consume
    // those exact observations rather than re-reading or collapsing a
    // same-path local/origin pair. A changed but valid origin remains a
    // candidate with its own raw-byte cleanup token; lifecycle resolution, not
    // the inventory adapter, decides whether it is stale, an alias, or a live
    // conflict.
    for promised in inventory.promised_origins {
        match promised.slot {
            RegistrationSlot::Absent { .. } => {}
            RegistrationSlot::Captured(origin_capture) => captures.push(*origin_capture),
            RegistrationSlot::Blocked {
                scope,
                path,
                reason,
            } => {
                let label = registration_label(scope, &path);
                observations.push(LifecycleObservation {
                    candidate: Candidate {
                        coordinate: RegistrationCoordinate { scope, path },
                        runfile: None,
                        state: CandidateState::Unverifiable {
                            reason: format!(
                                "{reason}; promised by {} registration {}",
                                promised.source.scope,
                                promised.source.path.display()
                            ),
                            observed_identity: None,
                            listener: None,
                            health: HealthState::NotProbed,
                        },
                    },
                    label,
                    capture: None,
                    process: None,
                });
            }
        }
    }
    (captures, observations)
}

fn stale_observation(
    capture: CapturedRegistration,
    reason: String,
    observed_identity: Option<ProcessIdentity>,
    listener: ListenerState,
) -> LifecycleObservation {
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            runfile: Some(capture.runfile.clone()),
            state: CandidateState::Stale {
                reason,
                observed_identity,
                listener,
            },
        },
        label,
        capture: Some(capture),
        process: None,
    }
}

fn stale_observation_from_listener(
    capture: CapturedRegistration,
    reason: String,
    observed_identity: Option<ProcessIdentity>,
    listener: ListenerState,
) -> LifecycleObservation {
    let port = capture.runfile.port;
    match listener {
        ListenerState::Uninspectable(error) => blocked_observation_with_facts(
            capture,
            format!(
                "{reason}; listener ownership on registered loopback port {port} is uninspectable: {error}"
            ),
            observed_identity,
            Some(ListenerState::Uninspectable(error)),
            HealthState::NotProbed,
        ),
        listener => stale_observation(capture, reason, observed_identity, listener),
    }
}

fn blocked_observation(capture: CapturedRegistration, reason: String) -> LifecycleObservation {
    blocked_observation_with_facts(capture, reason, None, None, HealthState::NotProbed)
}

fn blocked_observation_with_facts(
    capture: CapturedRegistration,
    reason: String,
    observed_identity: Option<ProcessIdentity>,
    listener: Option<ListenerState>,
    health: HealthState,
) -> LifecycleObservation {
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            runfile: Some(capture.runfile.clone()),
            state: CandidateState::Unverifiable {
                reason,
                observed_identity,
                listener,
                health,
            },
        },
        label,
        capture: Some(capture),
        process: None,
    }
}

fn observe_registration(capture: CapturedRegistration) -> LifecycleObservation {
    let pid = capture.runfile.pid;
    let port = capture.runfile.port;
    if capture.runfile.tailscale {
        return blocked_observation(
            capture,
            "this registration owns durable Tailscale Serve state that this build cannot yet compare-and-remove safely; stop the engine and remove that exact Serve endpoint with Tailscale tooling before removing the registration"
                .to_string(),
        );
    }
    let process = match LiveProcess::acquire(pid) {
        Ok(process) => process,
        Err(ProcessError::NotFound(_)) => {
            let listener = loopback_listener_state(pid, port);
            return stale_observation_from_listener(
                capture,
                format!("PID {pid} is absent"),
                None,
                listener,
            );
        }
        Err(error) => {
            return blocked_observation(
                capture,
                format!("could not acquire an exact process handle for PID {pid}: {error}"),
            );
        }
    };

    if capture.runfile.schema_version == 1 {
        return match process.wait(Duration::ZERO) {
            Ok(true) | Err(ProcessError::NotFound(_)) => stale_observation_from_listener(
                capture,
                format!("legacy PID {pid} is absent"),
                None,
                loopback_listener_state(pid, port),
            ),
            Ok(false) => {
                let label = registration_label(capture.scope, &capture.path);
                LifecycleObservation {
                    candidate: Candidate {
                        coordinate: RegistrationCoordinate {
                            scope: capture.scope,
                            path: capture.path.clone(),
                        },
                        runfile: Some(capture.runfile.clone()),
                        state: CandidateState::Unverifiable {
                            reason: format!(
                                "live schema-1 PID {pid} has no creation identity and cannot authorize teardown"
                            ),
                            observed_identity: None,
                            listener: None,
                            health: HealthState::NotProbed,
                        },
                    },
                    label,
                    capture: Some(capture),
                    process: Some(process),
                }
            }
            Err(error) => blocked_observation(
                capture,
                format!("could not inspect legacy PID {pid}: {error}"),
            ),
        };
    }

    let expected = capture
        .runfile
        .process_identity
        .as_ref()
        .expect("schema-v2 inventory validation requires process identity");
    let facts = match process.inspect(capture.runfile.port) {
        Ok(facts) => facts,
        Err(ProcessError::NotFound(_)) => {
            return stale_observation_from_listener(
                capture,
                format!("PID {pid} exited during inspection"),
                None,
                loopback_listener_state(pid, port),
            );
        }
        Err(error) => {
            return blocked_observation(
                capture,
                format!("could not bind PID {pid} to its process/listener facts: {error}"),
            );
        }
    };
    if facts.identity.start_token != expected.start_token {
        return stale_observation_from_listener(
            capture,
            format!("PID {pid} belongs to a different process creation instance"),
            Some(facts.identity.clone()),
            facts.listener,
        );
    }
    if facts.identity.executable != expected.executable || facts.identity.argv != expected.argv {
        return blocked_observation_with_facts(
            capture,
            format!(
                "live process creation instance {pid} has executable/argv facts that differ from its registration"
            ),
            Some(facts.identity),
            Some(facts.listener),
            HealthState::NotProbed,
        );
    }

    let listener = match facts.listener {
        ListenerState::OwnedByTarget => ListenerState::OwnedByTarget,
        ListenerState::OwnedByTargetWildcard => ListenerState::OwnedByTargetWildcard,
        ListenerState::Absent => ListenerState::Absent,
        ListenerState::OwnedByOther(owners) => ListenerState::OwnedByOther(owners),
        ListenerState::Uninspectable(error) => {
            return blocked_observation_with_facts(
                capture,
                format!("loopback port {} ownership is uninspectable: {error}", port),
                Some(facts.identity),
                Some(ListenerState::Uninspectable(error)),
                HealthState::NotProbed,
            );
        }
    };
    // HTTP is deliberately deferred until all process/listener observations
    // resolve to one exclusive target. Ambiguity must never trigger a probe.
    let health = HealthState::NotProbed;
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            runfile: Some(capture.runfile.clone()),
            state: CandidateState::Verified {
                identity: facts.identity,
                listener,
                health,
            },
        },
        label,
        capture: Some(capture),
        process: Some(process),
    }
}

fn revalidate_registration_after_health(
    observation: &mut LifecycleObservation,
) -> Result<(), String> {
    let CandidateState::Verified {
        identity, listener, ..
    } = &observation.candidate.state
    else {
        return Err("post-health revalidation requires a verified observation".to_string());
    };
    let expected_identity = identity.clone();
    let expected_listener = listener.clone();
    let capture = observation
        .capture
        .as_ref()
        .ok_or_else(|| "verified observation has no exact registration capture".to_string())?;
    let process = observation.process.as_ref().ok_or_else(|| {
        "verified observation did not retain its exact process object across HTTP health"
            .to_string()
    })?;
    let facts = process.inspect(capture.runfile.port).map_err(|error| {
        format!("retained process reinspection after HTTP health failed: {error}")
    })?;
    if facts.identity != expected_identity {
        return Err(
            "retained process identity changed while HTTP health was being checked".to_string(),
        );
    }
    if facts.listener != expected_listener {
        return Err(format!(
            "listener ownership changed while HTTP health was being checked: before={expected_listener:?}, after={:?}",
            facts.listener
        ));
    }
    Ok(())
}

fn lifecycle_resolution(observations: &[LifecycleObservation]) -> Resolution {
    let candidates = observations
        .iter()
        .map(|observation| observation.candidate.clone())
        .collect::<Vec<_>>();
    resolve(&candidates)
}

fn discovery_revisions(
    observations: &[ManagedRegistrationObservation],
) -> Vec<RegistrationRevision> {
    observations
        .iter()
        .map(|observation| RegistrationRevision {
            coordinate: observation.coordinate.clone(),
            promised: observation.promised.clone(),
            state: match &observation.state {
                ManagedRegistrationState::Absent => RegistrationRevisionState::Absent,
                ManagedRegistrationState::Blocked { reason } => {
                    RegistrationRevisionState::Blocked(reason.clone())
                }
                ManagedRegistrationState::Captured { raw_sha256, .. } => {
                    RegistrationRevisionState::Captured(raw_sha256.clone())
                }
            },
        })
        .collect()
}

fn update_managed_runtime_observations(
    managed: &mut [ManagedRegistrationObservation],
    lifecycle: &[LifecycleObservation],
) {
    for observation in lifecycle {
        let Some(capture) = &observation.capture else {
            continue;
        };
        let Some(target) = managed.iter_mut().find(|candidate| {
            candidate.coordinate.scope == capture.scope
                && candidate.coordinate.path == capture.path
                && matches!(candidate.state, ManagedRegistrationState::Captured { .. })
        }) else {
            continue;
        };
        let runtime = match &observation.candidate.state {
            CandidateState::Verified {
                identity,
                listener,
                health,
            } => RuntimeObservation::Verified {
                identity: identity.clone(),
                listener: listener.clone(),
                health: *health,
            },
            CandidateState::Stale {
                reason,
                observed_identity,
                listener,
            } => RuntimeObservation::Stale {
                reason: reason.clone(),
                observed_identity: observed_identity.clone(),
                listener: listener.clone(),
            },
            CandidateState::Unverifiable { .. } if capture.runfile.tailscale => {
                RuntimeObservation::NotInspected
            }
            CandidateState::Unverifiable { .. }
                if capture.runfile.schema_version == 1 && observation.process.is_some() =>
            {
                RuntimeObservation::LegacyLive {
                    pid: capture.runfile.pid,
                }
            }
            CandidateState::Unverifiable {
                reason,
                observed_identity,
                listener,
                health,
            } => RuntimeObservation::Unverifiable {
                reason: reason.clone(),
                observed_identity: observed_identity.clone(),
                listener: listener.clone(),
                health: *health,
            },
        };
        let ManagedRegistrationState::Captured {
            runtime: target_runtime,
            ..
        } = &mut target.state
        else {
            unreachable!("captured runtime target changed state");
        };
        *target_runtime = runtime;
    }
}

fn managed_server_from_resolution(
    observations: &[LifecycleObservation],
    managed_observations: &[ManagedRegistrationObservation],
    target: usize,
    aliases: &[usize],
    stale: &[usize],
) -> ManagedServer {
    let target_observation = &observations[target];
    let capture = target_observation
        .capture
        .as_ref()
        .expect("resolved managed target has a capture");
    let CandidateState::Verified {
        identity,
        listener,
        health,
    } = &target_observation.candidate.state
    else {
        unreachable!("resolved managed target is verified");
    };
    let alias_coordinates = aliases
        .iter()
        .map(|index| observations[*index].candidate.coordinate.clone())
        .collect();
    let stale_coordinates = stale
        .iter()
        .map(|index| observations[*index].candidate.coordinate.clone())
        .collect::<Vec<_>>();
    ManagedServer {
        registration: target_observation.candidate.coordinate.clone(),
        runfile: capture.runfile.clone(),
        identity: identity.clone(),
        listener: listener.clone(),
        health: *health,
        aliases: alias_coordinates,
        stale: stale_coordinates,
        fingerprint: DiscoveryFingerprint {
            pid: capture.runfile.pid,
            identity: identity.clone(),
            runfile: capture.runfile.clone(),
            revisions: discovery_revisions(managed_observations),
        },
    }
}

fn managed_state_from_resolution(
    observations: &[LifecycleObservation],
    managed_observations: &[ManagedRegistrationObservation],
    resolution: &Resolution,
) -> ManagedServerState {
    match resolution {
        Resolution::Empty => ManagedServerState::Empty,
        Resolution::Ready {
            target,
            aliases,
            stale,
        } => ManagedServerState::Ready(managed_server_from_resolution(
            observations,
            managed_observations,
            *target,
            aliases,
            stale,
        )),
        Resolution::Degraded {
            target,
            aliases,
            stale,
            issues,
            ..
        } => ManagedServerState::Degraded {
            server: managed_server_from_resolution(
                observations,
                managed_observations,
                *target,
                aliases,
                stale,
            ),
            issues: issues.clone(),
        },
        Resolution::StaleOnly { stale } => ManagedServerState::StaleOnly {
            stale: stale
                .iter()
                .map(|index| observations[*index].candidate.coordinate.clone())
                .collect(),
        },
        Resolution::Conflict { issues } => ManagedServerState::Conflict {
            issues: issues.clone(),
        },
        Resolution::Unverifiable { issues } => ManagedServerState::Unverifiable {
            issues: issues.clone(),
        },
    }
}

fn discover_inventory_before_health_with<O>(
    inventory: RegistrationInventory,
    mut observe: O,
) -> LifecycleDiscovery
where
    O: FnMut(CapturedRegistration) -> LifecycleObservation,
{
    let mut managed_observations = flatten_inventory(&inventory);
    let static_issues = static_inventory_issues(&managed_observations);
    if !static_issues.is_empty() {
        let (captures, mut observations) = expand_registration_captures(inventory.clone());
        let reason = static_issues
            .iter()
            .map(|issue| issue.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        observations.extend(
            captures
                .into_iter()
                .map(|capture| blocked_observation(capture, reason.clone())),
        );
        let resolution = if static_issues
            .iter()
            .any(|issue| issue.kind == ResolutionIssueKind::Unverifiable)
        {
            Resolution::Unverifiable {
                issues: static_issues,
            }
        } else {
            Resolution::Conflict {
                issues: static_issues,
            }
        };
        let state =
            managed_state_from_resolution(&observations, &managed_observations, &resolution);
        return LifecycleDiscovery {
            managed: ManagedServerDiscovery {
                inventory,
                observations: managed_observations,
                state,
            },
            observations,
            resolution,
        };
    }

    let (captures, mut observations) = expand_registration_captures(inventory.clone());
    observations.extend(captures.into_iter().map(&mut observe));
    let resolution = lifecycle_resolution(&observations);
    update_managed_runtime_observations(&mut managed_observations, &observations);
    let state = managed_state_from_resolution(&observations, &managed_observations, &resolution);
    LifecycleDiscovery {
        managed: ManagedServerDiscovery {
            inventory,
            observations: managed_observations,
            state,
        },
        observations,
        resolution,
    }
}

fn complete_lifecycle_health_with<H, R>(
    mut discovery: LifecycleDiscovery,
    health: &mut H,
    mut revalidate_after_health: R,
) -> LifecycleDiscovery
where
    H: HealthProbe,
    R: FnMut(&mut LifecycleObservation) -> Result<(), String>,
{
    let mut resolution = discovery.resolution.clone();

    // A unique exact loopback owner is the only state that warrants an HTTP
    // effect. Probe once, then apply that health result to every exact alias.
    if let Resolution::Degraded {
        target,
        aliases,
        listener: ListenerState::OwnedByTarget,
        health: HealthState::NotProbed,
        ..
    } = &resolution
    {
        let target = *target;
        let capture = discovery.observations[target]
            .capture
            .as_ref()
            .expect("unique target has a capture");
        let health_state = if health.status_ok(
            "127.0.0.1",
            capture.runfile.port,
            health_path(capture.runfile.engine),
        ) {
            HealthState::Healthy
        } else {
            HealthState::Unhealthy
        };
        let mut health_indices = Vec::with_capacity(aliases.len() + 1);
        health_indices.push(target);
        health_indices.extend(aliases.iter().copied());
        for index in health_indices {
            let CandidateState::Verified {
                health: candidate_health,
                ..
            } = &mut discovery.observations[index].candidate.state
            else {
                unreachable!("resolved alias is verified");
            };
            *candidate_health = health_state;
            if let Err(error) = revalidate_after_health(&mut discovery.observations[index]) {
                let (observed_identity, listener, health) =
                    match &discovery.observations[index].candidate.state {
                        CandidateState::Verified {
                            identity,
                            listener,
                            health,
                        } => (Some(identity.clone()), Some(listener.clone()), *health),
                        _ => unreachable!("resolved alias is verified"),
                    };
                discovery.observations[index].candidate.state = CandidateState::Unverifiable {
                    reason: format!("post-health authority revalidation failed: {error}"),
                    observed_identity,
                    listener,
                    health,
                };
            }
        }
        resolution = lifecycle_resolution(&discovery.observations);
    }

    update_managed_runtime_observations(
        &mut discovery.managed.observations,
        &discovery.observations,
    );
    discovery.managed.state = managed_state_from_resolution(
        &discovery.observations,
        &discovery.managed.observations,
        &resolution,
    );
    discovery.resolution = resolution;
    discovery
}

#[cfg(test)]
fn discover_inventory_with<O, H, R>(
    inventory: RegistrationInventory,
    observe: O,
    health: &mut H,
    revalidate_after_health: R,
) -> LifecycleDiscovery
where
    O: FnMut(CapturedRegistration) -> LifecycleObservation,
    H: HealthProbe,
    R: FnMut(&mut LifecycleObservation) -> Result<(), String>,
{
    let discovery = discover_inventory_before_health_with(inventory, observe);
    complete_lifecycle_health_with(discovery, health, revalidate_after_health)
}

fn discover_lifecycle_before_health_in(scope: &ManagedDiscoveryScope) -> LifecycleDiscovery {
    let inventory = inventory_runfiles(&scope.workspace, scope.global.clone());
    discover_inventory_before_health_with(inventory, observe_registration)
}

fn discover_lifecycle_in(scope: &ManagedDiscoveryScope) -> LifecycleDiscovery {
    let discovery = discover_lifecycle_before_health_in(scope);
    let mut health = NativeHealthProbe;
    complete_lifecycle_health_with(discovery, &mut health, revalidate_registration_after_health)
}

pub(crate) struct PendingManagedDiscovery {
    lifecycle: LifecycleDiscovery,
}

impl PendingManagedDiscovery {
    pub(crate) fn discovery(&self) -> &ManagedServerDiscovery {
        &self.lifecycle.managed
    }

    pub(crate) fn finish(self) -> ManagedServerDiscovery {
        let mut health = NativeHealthProbe;
        complete_lifecycle_health_with(
            self.lifecycle,
            &mut health,
            revalidate_registration_after_health,
        )
        .managed
    }
}

pub(crate) fn begin_managed_server_discovery_in(
    scope: &ManagedDiscoveryScope,
) -> PendingManagedDiscovery {
    PendingManagedDiscovery {
        lifecycle: discover_lifecycle_before_health_in(scope),
    }
}

pub(crate) fn discover_managed_server_in(scope: &ManagedDiscoveryScope) -> ManagedServerDiscovery {
    begin_managed_server_discovery_in(scope).finish()
}

trait DoctorProbeEffects {
    fn binary_present(&mut self, engine: Engine) -> bool;
    fn regular_file(&mut self, path: &Path) -> bool;
}

struct NativeDoctorProbeEffects;

impl DoctorProbeEffects for NativeDoctorProbeEffects {
    fn binary_present(&mut self, engine: Engine) -> bool {
        matches!(
            Command::new(engine.program())
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
            Ok(status) if status.success()
        )
    }

    fn regular_file(&mut self, path: &Path) -> bool {
        path.is_file()
    }
}

pub fn run_server(workspace: &Path, cmd: ServerCommand) -> ExitCode {
    match cmd {
        ServerCommand::Up(args) => up(workspace, &args),
        ServerCommand::Status => status(workspace),
        ServerCommand::Adopt(args) => adopt(workspace, &args),
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
        seed: args.seed,
        parallel: args.parallel,
        tailscale: args.tailscale,
    }
}

fn require_registration_absent(path: &Path, scope: &str) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(_) => Err(format!(
            "{scope} server registration already exists at {}; inspect it and stop the registered server before launching another",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not inspect {scope} server registration {}: {error}",
            path.display()
        )),
    }
}

fn validate_launch_preconditions(
    workspace: &Path,
    args: &ServerUpArgs,
    global_runfile: Option<&Path>,
) -> Result<(), String> {
    if args.port == 0 {
        return Err("--port must be greater than zero".to_string());
    }
    if args.tailscale {
        return Err(
            "--tailscale is temporarily fail-closed: Ferric cannot yet prove ownership of durable Tailscale Serve state during rollback and teardown"
                .to_string(),
        );
    }

    if args.engine == Engine::LlamaServer {
        if args.ctx == 0 {
            return Err("--ctx must be greater than zero for llama-server".to_string());
        }

        let model = args
            .model
            .as_deref()
            .ok_or_else(|| "--model is required for llama-server".to_string())?;
        if !Path::new(model).is_file() {
            return Err(format!(
                "llama-server model must be a regular file: {model}"
            ));
        }

        if let Some(mmproj) = &args.mmproj
            && !mmproj.is_file()
        {
            return Err(format!(
                "llama-server multimodal projector must be a regular file: {}",
                mmproj.display()
            ));
        }
        if args.parallel == Some(0) {
            return Err("--parallel must be greater than zero for llama-server".to_string());
        }
    } else if args.seed.is_some() || args.parallel.is_some() {
        return Err("--seed and --parallel are supported only by llama-server".to_string());
    }

    require_registration_absent(&runfile_path(workspace), "local")?;
    if let Some(global) = global_runfile {
        require_registration_absent(global, "global")?;
    }

    if is_listening("127.0.0.1", args.port) {
        return Err(format!(
            "refusing to launch: 127.0.0.1:{} is already listening",
            args.port
        ));
    }
    Ok(())
}

fn up(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    let global_path = global_runfile_path();
    if let Err(error) = validate_launch_preconditions(workspace, args, global_path.as_deref()) {
        eprintln!("server launch preflight failed: {error}");
        return ExitCode::FAILURE;
    }

    let cfg = config_from(args);
    let launch = command(&cfg);

    let mut proc = Command::new(&launch.program);
    proc.args(&launch.args);
    for (k, v) in &launch.env {
        proc.env(k, v);
    }
    println!("Launching {} on {} ...", launch.program, cfg.base_url());
    let mut child = match proc.spawn() {
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

    // Retain the exact OS process object before any readiness polling or
    // publication. Every later failure path can then terminate this creation
    // instance without converting the numeric PID back into authority.
    let managed_process = match bind_spawned_child(
        &mut child,
        &NativeSpawnedProcessRuntime,
        cfg.port,
        &NativeListenerInspector,
    ) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("could not establish exact lifecycle control for spawned PID {pid}: {error}");
            return ExitCode::FAILURE;
        }
    };
    debug_assert_eq!(managed_process.pid(), pid);

    if let Err(error) = wait_healthy(
        &mut child,
        cfg.engine,
        &cfg.host,
        cfg.port,
        Duration::from_secs(300),
    ) {
        eprintln!(
            "server did not become HTTP-healthy at {}: {error}",
            cfg.base_url()
        );
        if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
            eprintln!("could not confirm exact child shutdown: {stop_error}");
        }
        return ExitCode::FAILURE;
    }

    let base_url = cfg.base_url();
    let process_facts = match inspect_bound_child_for_publication(
        &mut child,
        &managed_process,
        cfg.port,
        &NativeListenerInspector,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            eprintln!("server launch was rejected before publication: {error}");
            return ExitCode::FAILURE;
        }
    };
    let local_path = match std::path::absolute(runfile_path(workspace)) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("could not resolve the local registration path: {error}");
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!("could not confirm exact child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
    };
    let runfile = ServerRunfile {
        schema_version: RUNFILE_SCHEMA_V2,
        engine: cfg.engine,
        pid,
        port: cfg.port,
        base_url: base_url.clone(),
        tailscale: cfg.tailscale,
        model: cfg.model.clone(),
        context_size: (cfg.engine == Engine::LlamaServer).then_some(cfg.ctx),
        sampling_seed: cfg.seed,
        parallel_slots: cfg.parallel,
        process_identity: Some(process_facts.identity),
        origin_local_runfile: Some(local_path),
    };
    let published = match publish_mirrored(workspace, global_path.as_deref(), &runfile) {
        Ok(published) => published,
        Err(PublishError::Durability {
            path,
            detail,
            published,
        }) => {
            eprintln!(
                "registration committed at {} but its directory durability check failed: {detail}; stopping the owned child before exact-byte rollback",
                path.display()
            );
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!(
                    "could not prove exact child exit ({stop_error}); published registrations are kept for recovery"
                );
                return ExitCode::FAILURE;
            }
            clean_captured_registration(&published.local);
            if let Some(global) = &published.global {
                clean_captured_registration(global);
            }
            return ExitCode::FAILURE;
        }
        Err(PublishError::Mirror {
            path,
            detail,
            local,
        }) => {
            eprintln!(
                "global registration publication failed at {}: {detail}; stopping the owned child before local rollback",
                path.display()
            );
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!(
                    "could not prove exact child exit ({stop_error}); local registration at {} is kept for recovery",
                    local.path.display()
                );
                return ExitCode::FAILURE;
            }
            if !clean_captured_registration(&local) {
                eprintln!(
                    "the child is stopped, but local publication rollback was partial at {}",
                    local.path.display()
                );
            }
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("server registration publication failed: {error}");
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!("could not confirm exact child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
    };

    match child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            eprintln!(
                "engine process exited during registration publication ({status}); rolling back unchanged registrations"
            );
            if let Err(error) = require_listener_released(pid, cfg.port) {
                eprintln!(
                    "published registrations are kept because endpoint release is not proven: {error}"
                );
                return ExitCode::FAILURE;
            }
            clean_captured_registration(&published.local);
            if let Some(global) = &published.global {
                clean_captured_registration(global);
            }
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("could not confirm the engine child after publication: {error}");
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!(
                    "could not prove exact child exit ({stop_error}); published registrations are kept for recovery"
                );
                return ExitCode::FAILURE;
            }
            clean_captured_registration(&published.local);
            if let Some(global) = &published.global {
                clean_captured_registration(global);
            }
            return ExitCode::FAILURE;
        }
    }

    println!("server ready: {} (pid {pid})", base_url);
    println!("registered locally at {}", published.local.path.display());
    if let Some(global) = published.global {
        println!("registered globally at {}", global.path.display());
    }
    ExitCode::SUCCESS
}

fn executable_matches_engine(engine: Engine, executable: &Path) -> bool {
    let Some(file_name) = executable.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let expected = engine.program();
    #[cfg(windows)]
    {
        file_name.eq_ignore_ascii_case(expected)
            || file_name.eq_ignore_ascii_case(&format!("{expected}.exe"))
    }
    #[cfg(not(windows))]
    {
        file_name == expected
    }
}

fn argv_has_pair(argv: &[String], flag: &str, value: &str) -> bool {
    argv.windows(2)
        .any(|pair| pair[0] == flag && pair[1] == value)
}

fn validate_legacy_process_coordinates(
    runfile: &ServerRunfile,
    identity: &ProcessIdentity,
) -> Result<(), String> {
    if !executable_matches_engine(runfile.engine, &identity.executable) {
        return Err(format!(
            "observed executable {} is not the closed {:?} engine `{}`",
            identity.executable.display(),
            runfile.engine,
            runfile.engine.program()
        ));
    }
    match runfile.engine {
        Engine::LlamaServer => {
            for (flag, value, coordinate) in [
                ("--host", "127.0.0.1".to_string(), "loopback host"),
                ("--port", runfile.port.to_string(), "registered port"),
            ] {
                if !argv_has_pair(&identity.argv, flag, &value) {
                    return Err(format!(
                        "observed argv does not contain the expected {coordinate} pair `{flag} {value}`"
                    ));
                }
            }
            if let Some(model) = &runfile.model
                && !argv_has_pair(&identity.argv, "-m", model)
                && !argv_has_pair(&identity.argv, "--model", model)
            {
                return Err(format!(
                    "observed argv does not contain the recorded model `{model}`"
                ));
            }
            if let Some(context) = runfile.context_size
                && !argv_has_pair(&identity.argv, "-c", &context.to_string())
                && !argv_has_pair(&identity.argv, "--ctx-size", &context.to_string())
            {
                return Err(format!(
                    "observed argv does not contain the recorded context size {context}"
                ));
            }
            if let Some(seed) = runfile.sampling_seed
                && !argv_has_pair(&identity.argv, "--seed", &seed.to_string())
            {
                return Err(format!(
                    "observed argv does not contain the recorded sampling seed {seed}"
                ));
            }
            if let Some(parallel) = runfile.parallel_slots
                && !argv_has_pair(&identity.argv, "--parallel", &parallel.to_string())
            {
                return Err(format!(
                    "observed argv does not contain the recorded parallel slot count {parallel}"
                ));
            }
        }
        Engine::Ollama => {
            if !identity.argv.iter().any(|argument| argument == "serve") {
                return Err("observed Ollama argv does not contain `serve`".to_string());
            }
        }
    }
    Ok(())
}

fn rollback_adoption(replacements: &[(CapturedRegistration, CapturedRegistration)]) -> bool {
    let mut complete = true;
    for (legacy, adopted) in replacements.iter().rev() {
        match replace_if_unchanged(adopted, &legacy.raw) {
            Ok(ReplacementOutcome::Replaced) => {}
            Ok(ReplacementOutcome::Absent) => {
                complete = false;
                eprintln!(
                    "adoption rollback could not restore absent registration {}",
                    legacy.path.display()
                );
            }
            Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => {
                complete = false;
                eprintln!(
                    "adoption rollback preserved a concurrent replacement for {} at {}: {detail}",
                    legacy.path.display(),
                    path.display()
                );
            }
            Err(error) => {
                complete = false;
                eprintln!("adoption rollback incomplete: {error}");
            }
        }
    }
    complete
}

fn adopt(workspace: &Path, args: &ServerAdoptArgs) -> ExitCode {
    adopt_impl(workspace, global_runfile_path(), args)
}

fn adopt_impl(workspace: &Path, global_path: Option<PathBuf>, args: &ServerAdoptArgs) -> ExitCode {
    if args.pid == 0 {
        eprintln!("adoption requires a nonzero --pid");
        return ExitCode::FAILURE;
    }
    let inventory = inventory_runfiles(workspace, global_path);
    let (captures, blocked) = expand_registration_captures(inventory);
    if !blocked.is_empty() {
        eprintln!("refusing adoption: registration inventory is blocked");
        for observation in blocked {
            let reason = match observation.candidate.state {
                CandidateState::Unverifiable { reason, .. }
                | CandidateState::Stale { reason, .. } => reason,
                CandidateState::Verified { .. } => "unexpected verified observation".to_string(),
            };
            eprintln!("  - {}: {reason}", observation.label);
        }
        return ExitCode::FAILURE;
    }
    let Some(reference) = captures.first() else {
        eprintln!("refusing adoption: no server registration exists");
        return ExitCode::FAILURE;
    };
    let Some(origin) = captures
        .iter()
        .find(|capture| capture.scope == RegistrationScope::Local)
        .map(|capture| capture.path.clone())
    else {
        eprintln!(
            "refusing adoption: the originating local schema-1 registration is not present in this workspace"
        );
        return ExitCode::FAILURE;
    };
    if captures
        .iter()
        .any(|capture| capture.runfile.schema_version != 1)
    {
        eprintln!("refusing adoption: every selected registration must use legacy schema 1");
        return ExitCode::FAILURE;
    }
    if captures
        .iter()
        .any(|capture| capture.runfile != reference.runfile)
    {
        eprintln!("refusing adoption: local/global legacy registrations disagree");
        return ExitCode::FAILURE;
    }
    if reference.runfile.pid != args.pid {
        eprintln!(
            "refusing adoption: --pid {} does not match registered PID {}",
            args.pid, reference.runfile.pid
        );
        return ExitCode::FAILURE;
    }
    if reference.runfile.tailscale {
        eprintln!(
            "refusing adoption: tailscale=true owns external Serve state that Ferric cannot yet compare-and-replace safely"
        );
        return ExitCode::FAILURE;
    }
    let expected_base_url = format!("http://127.0.0.1:{}/v1", reference.runfile.port);
    if reference.runfile.port == 0 || reference.runfile.base_url != expected_base_url {
        eprintln!(
            "refusing adoption: legacy endpoint must be exactly {expected_base_url} with a nonzero port"
        );
        return ExitCode::FAILURE;
    }

    let process = match LiveProcess::acquire(args.pid) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("refusing adoption: could not acquire exact process handle: {error}");
            return ExitCode::FAILURE;
        }
    };
    let facts = match process.inspect(reference.runfile.port) {
        Ok(facts) => facts,
        Err(error) => {
            eprintln!("refusing adoption: could not inspect exact process/listener facts: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = validate_legacy_process_coordinates(&reference.runfile, &facts.identity) {
        eprintln!("refusing adoption: {error}");
        return ExitCode::FAILURE;
    }
    if facts.listener != ListenerState::OwnedByTarget {
        eprintln!(
            "refusing adoption: registered endpoint is not exclusively owned on IPv4 loopback by PID {}: {:?}",
            args.pid, facts.listener
        );
        return ExitCode::FAILURE;
    }
    match process.wait(Duration::ZERO) {
        Ok(false) => {}
        Ok(true) => {
            eprintln!("refusing adoption: registered process exited during validation");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("refusing adoption: could not confirm retained process liveness: {error}");
            return ExitCode::FAILURE;
        }
    }

    let mut adopted_runfile = reference.runfile.clone();
    adopted_runfile.schema_version = RUNFILE_SCHEMA_V2;
    adopted_runfile.process_identity = Some(facts.identity.clone());
    adopted_runfile.origin_local_runfile = Some(origin);
    for capture in &captures {
        if let Err(error) = validate_runfile(capture.scope, &capture.path, &adopted_runfile) {
            eprintln!(
                "refusing adoption: schema-v2 replacement for {} is invalid: {error}",
                capture.path.display()
            );
            return ExitCode::FAILURE;
        }
    }
    let replacement_raw = match serde_json::to_vec_pretty(&adopted_runfile) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("refusing adoption: could not serialize schema-v2 registration: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut replacements = Vec::new();
    for legacy in &captures {
        let adopted_capture = CapturedRegistration {
            scope: legacy.scope,
            path: legacy.path.clone(),
            raw: replacement_raw.clone(),
            runfile: adopted_runfile.clone(),
        };
        match replace_if_unchanged(legacy, &replacement_raw) {
            Ok(ReplacementOutcome::Replaced) => {
                replacements.push((legacy.clone(), adopted_capture));
            }
            Ok(ReplacementOutcome::Absent) => {
                eprintln!(
                    "adoption stopped because {} disappeared; rolling back earlier replacements",
                    legacy.path.display()
                );
                let rolled_back = rollback_adoption(&replacements);
                eprintln!("adoption rollback completed={rolled_back}");
                return ExitCode::FAILURE;
            }
            Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => {
                eprintln!(
                    "adoption stopped because {} changed; replacement preserved at {}: {detail}",
                    legacy.path.display(),
                    path.display()
                );
                let rolled_back = rollback_adoption(&replacements);
                eprintln!("adoption rollback completed={rolled_back}");
                return ExitCode::FAILURE;
            }
            Err(error) => {
                if error.replacement_committed {
                    replacements.push((legacy.clone(), adopted_capture));
                }
                eprintln!("adoption replacement failed: {error}");
                let rolled_back = rollback_adoption(&replacements);
                eprintln!("adoption rollback completed={rolled_back}");
                return ExitCode::FAILURE;
            }
        }
    }

    let still_exact = process
        .inspect(reference.runfile.port)
        .map(|current| {
            current.identity == facts.identity && current.listener == ListenerState::OwnedByTarget
        })
        .unwrap_or(false);
    if !still_exact {
        eprintln!(
            "adoption validation changed before completion; restoring legacy records where still unchanged"
        );
        let rolled_back = rollback_adoption(&replacements);
        eprintln!("adoption rollback completed={rolled_back}");
        return ExitCode::FAILURE;
    }

    println!(
        "adopted live schema-1 server PID {} into schema 2 without signalling it (registrations={})",
        args.pid,
        replacements.len()
    );
    ExitCode::SUCCESS
}

fn status(workspace: &Path) -> ExitCode {
    status_impl(workspace, global_runfile_path())
}

fn issue_coordinates(issues: &[ResolutionIssue]) -> Vec<RegistrationCoordinate> {
    let mut coordinates = issues
        .iter()
        .flat_map(|issue| issue.coordinates.iter().cloned())
        .collect::<Vec<_>>();
    coordinates.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.scope.to_string().cmp(&right.scope.to_string()))
    });
    coordinates.dedup();
    coordinates
}

fn status_next_action(discovery: &ManagedServerDiscovery) -> StatusNextAction {
    match &discovery.state {
        ManagedServerState::Empty => StatusNextAction::StartServer,
        ManagedServerState::Ready(server) => StatusNextAction::ContinueManaged {
            base_url: server.runfile.base_url.clone(),
        },
        ManagedServerState::Degraded { server, .. } => match server.listener {
            ListenerState::OwnedByTargetWildcard => StatusNextAction::InspectWildcard {
                port: server.runfile.port,
            },
            ListenerState::OwnedByTarget | ListenerState::Absent => StatusNextAction::StopManaged {
                pid: server.runfile.pid,
            },
            ListenerState::OwnedByOther(_) | ListenerState::Uninspectable(_) => {
                unreachable!("conflicting or uninspectable listeners cannot resolve degraded")
            }
        },
        ManagedServerState::StaleOnly { .. } => StatusNextAction::CleanStale,
        ManagedServerState::Conflict { issues } => StatusNextAction::ResolveConflict {
            coordinates: issue_coordinates(issues),
        },
        ManagedServerState::Unverifiable { issues } => {
            if let Some(port) =
                discovery
                    .observations
                    .iter()
                    .find_map(|observation| match &observation.state {
                        ManagedRegistrationState::Captured { runfile, .. } if runfile.tailscale => {
                            Some(runfile.port)
                        }
                        ManagedRegistrationState::Absent
                        | ManagedRegistrationState::Blocked { .. }
                        | ManagedRegistrationState::Captured { .. } => None,
                    })
            {
                return StatusNextAction::InspectTailscale { port };
            }
            if let Some((path, _source)) = discovery.observations.iter().find_map(|observation| {
                matches!(observation.state, ManagedRegistrationState::Absent)
                    .then(|| {
                        observation.promised.as_ref().map(|promised| {
                            (observation.coordinate.path.clone(), promised.source.clone())
                        })
                    })
                    .flatten()
            }) {
                return StatusNextAction::InspectPromisedOrigin { path };
            }
            let legacy = discovery
                .observations
                .iter()
                .filter_map(|observation| match &observation.state {
                    ManagedRegistrationState::Captured {
                        runfile,
                        runtime: RuntimeObservation::LegacyLive { pid },
                        ..
                    } => Some((*pid, runfile.as_ref())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let compatible_legacy_aliases = legacy.first().is_some_and(|(pid, runfile)| {
                legacy
                    .iter()
                    .all(|(alias_pid, alias)| alias_pid == pid && alias == runfile)
                    && discovery
                        .observations
                        .iter()
                        .all(|observation| match &observation.state {
                            ManagedRegistrationState::Absent => observation.promised.is_none(),
                            ManagedRegistrationState::Captured {
                                runfile: candidate,
                                runtime: RuntimeObservation::LegacyLive { pid: candidate_pid },
                                ..
                            } => candidate_pid == pid && candidate.as_ref() == *runfile,
                            ManagedRegistrationState::Blocked { .. }
                            | ManagedRegistrationState::Captured { .. } => false,
                        })
            });
            if compatible_legacy_aliases {
                return StatusNextAction::AdoptLegacy { pid: legacy[0].0 };
            }
            StatusNextAction::RepairUnverifiable {
                coordinates: issue_coordinates(issues),
            }
        }
    }
}

fn status_report(discovery: &ManagedServerDiscovery) -> ServerStatusReport {
    ServerStatusReport {
        registrations: discovery.observations.clone(),
        state: discovery.state.clone(),
        next_action: status_next_action(discovery),
        success: matches!(discovery.state, ManagedServerState::Ready(_)),
    }
}

fn listener_status(listener: &ListenerState) -> String {
    match listener {
        ListenerState::OwnedByTarget => "owned-loopback".to_string(),
        ListenerState::OwnedByTargetWildcard => "wildcard-public".to_string(),
        ListenerState::Absent => "absent".to_string(),
        ListenerState::OwnedByOther(owners) => format!("foreign-or-shared:{owners:?}"),
        ListenerState::Uninspectable(detail) => format!("uninspectable:{detail}"),
    }
}

fn health_status(health: HealthState) -> &'static str {
    match health {
        HealthState::NotProbed => "not-probed",
        HealthState::Healthy => "healthy",
        HealthState::Unhealthy => "unhealthy",
    }
}

fn render_registration_status(observation: &ManagedRegistrationObservation) -> String {
    let promised = observation
        .promised
        .as_ref()
        .map_or_else(String::new, |promised| {
            format!(
                " promised-by={} registration {}",
                promised.source.scope,
                promised.source.path.display()
            )
        });
    let prefix = format!(
        "{} registration {}{promised}",
        observation.coordinate.scope,
        observation.coordinate.path.display()
    );
    match &observation.state {
        ManagedRegistrationState::Absent => format!(
            "[absent] {prefix}: recorded-identity=none observed-identity=none listener=none health=none"
        ),
        ManagedRegistrationState::Blocked { reason } => format!(
            "[blocked] {prefix}: {reason}; recorded-identity=unavailable observed-identity=not-inspected listener=not-inspected health=not-probed"
        ),
        ManagedRegistrationState::Captured {
            runfile, runtime, ..
        } => {
            let recorded = runfile.process_identity.as_ref().map_or_else(
                || "legacy-none".to_string(),
                |identity| {
                    format!(
                        "token={} executable={} argv={:?}",
                        identity.start_token,
                        identity.executable.display(),
                        identity.argv
                    )
                },
            );
            let observed = match runtime {
                RuntimeObservation::NotInspected => {
                    "observed-identity=not-inspected listener=not-inspected health=not-probed"
                        .to_string()
                }
                RuntimeObservation::Verified {
                    identity,
                    listener,
                    health,
                } => format!(
                    "observed-identity=token={} executable={} argv={:?} listener={} health={}",
                    identity.start_token,
                    identity.executable.display(),
                    identity.argv,
                    listener_status(listener),
                    health_status(*health)
                ),
                RuntimeObservation::Stale {
                    reason,
                    observed_identity,
                    listener,
                } => {
                    let identity = observed_identity.as_ref().map_or_else(
                        || "stale-unavailable".to_string(),
                        |identity| {
                            format!(
                                "token={} executable={} argv={:?}",
                                identity.start_token,
                                identity.executable.display(),
                                identity.argv
                            )
                        },
                    );
                    format!(
                        "observed-identity={identity} stale-reason={reason} listener={} health=not-probed",
                        listener_status(listener)
                    )
                }
                RuntimeObservation::LegacyLive { pid } => format!(
                    "observed-identity=legacy-live-pid:{pid} listener=unverified health=not-probed"
                ),
                RuntimeObservation::Unverifiable {
                    reason,
                    observed_identity,
                    listener,
                    health,
                } => {
                    let identity = observed_identity.as_ref().map_or_else(
                        || "unavailable".to_string(),
                        |identity| {
                            format!(
                                "token={} executable={} argv={:?}",
                                identity.start_token,
                                identity.executable.display(),
                                identity.argv
                            )
                        },
                    );
                    let listener = listener
                        .as_ref()
                        .map_or_else(|| "unverifiable".to_string(), listener_status);
                    format!(
                        "observed-identity={identity} unverifiable-reason={reason} listener={listener} health={}",
                        health_status(*health)
                    )
                }
            };
            format!(
                "[captured] {prefix}: schema={} engine={:?} pid={} base-url={} recorded-identity={recorded} {observed}",
                runfile.schema_version, runfile.engine, runfile.pid, runfile.base_url
            )
        }
    }
}

fn next_action_text(action: &StatusNextAction) -> String {
    match action {
        StatusNextAction::StartServer => {
            "no registration is active; inspect required launch options with `ferric server up --help`"
                .to_string()
        }
        StatusNextAction::ContinueManaged { base_url } => format!(
            "managed server is ready at {base_url}; continue with the intended Ferric command and omit `--api-base` to use it"
        ),
        StatusNextAction::StopManaged { pid } => format!(
            "managed PID {pid} is identity-authorized for recovery; run `ferric server down`"
        ),
        StatusNextAction::CleanStale => {
            "only cleanup-safe stale registrations remain; run `ferric server down`".to_string()
        }
        StatusNextAction::AdoptLegacy { pid } => format!(
            "verify and record the live legacy process without signalling it: `ferric server adopt --pid {pid}`"
        ),
        StatusNextAction::InspectWildcard { port } => format!(
            "port {port} is wildcard/public; reconfigure it to bind only 127.0.0.1, then rerun `ferric server status` (teardown is not authorized)"
        ),
        StatusNextAction::InspectPromisedOrigin { path } => format!(
            "the promised origin {} is missing or changed; restore or reconcile that exact registration, then rerun `ferric server status`",
            path.display()
        ),
        StatusNextAction::InspectTailscale { port } => format!(
            "registration port {port} claims durable Tailscale Serve state; inspect and remove only that exact Serve endpoint with Tailscale tooling (Ferric cannot safely reset it)"
        ),
        StatusNextAction::ResolveConflict { coordinates } => format!(
            "resolve the {} conflicting registration coordinate(s) without signalling a process, then rerun `ferric server status`",
            coordinates.len()
        ),
        StatusNextAction::RepairUnverifiable { coordinates } => format!(
            "repair or make readable the {} unverifiable registration coordinate(s), then rerun `ferric server status` (no process action is authorized)",
            coordinates.len()
        ),
    }
}

fn render_status(report: &ServerStatusReport) -> RenderedServerStatus {
    let mut stdout = report
        .registrations
        .iter()
        .map(render_registration_status)
        .collect::<Vec<_>>();
    let mut stderr = Vec::new();
    match &report.state {
        ManagedServerState::Empty => stdout.push("[state] empty".to_string()),
        ManagedServerState::Ready(server) => stdout.push(format!(
            "[state] ready pid={} aliases={} stale={} listener={} health={}",
            server.runfile.pid,
            server.aliases.len(),
            server.stale.len(),
            listener_status(&server.listener),
            health_status(server.health)
        )),
        ManagedServerState::Degraded { server, issues } => {
            stdout.push(format!(
                "[state] degraded pid={} listener={} health={}",
                server.runfile.pid,
                listener_status(&server.listener),
                health_status(server.health)
            ));
            stderr.extend(
                issues
                    .iter()
                    .map(|issue| format!("[diagnostic] {}", issue.detail)),
            );
        }
        ManagedServerState::StaleOnly { stale } => {
            stdout.push(format!("[state] stale-only registrations={}", stale.len()));
        }
        ManagedServerState::Conflict { issues } => {
            stdout.push("[state] conflict".to_string());
            stderr.extend(
                issues
                    .iter()
                    .map(|issue| format!("[diagnostic] {}", issue.detail)),
            );
        }
        ManagedServerState::Unverifiable { issues } => {
            stdout.push("[state] unverifiable".to_string());
            stderr.extend(
                issues
                    .iter()
                    .map(|issue| format!("[diagnostic] {}", issue.detail)),
            );
        }
    }
    stdout.push(format!("[next] {}", next_action_text(&report.next_action)));
    RenderedServerStatus {
        stdout,
        stderr,
        success: report.success,
    }
}

fn status_impl(workspace: &Path, global_path: Option<PathBuf>) -> ExitCode {
    let scope = ManagedDiscoveryScope {
        workspace: workspace.to_path_buf(),
        global: global_path,
    };
    let discovery = discover_managed_server_in(&scope);
    let rendered = render_status(&status_report(&discovery));
    for line in rendered.stdout {
        println!("{line}");
    }
    for line in rendered.stderr {
        eprintln!("{line}");
    }
    if rendered.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn clean_captured_registration(captured: &CapturedRegistration) -> bool {
    match remove_if_unchanged(captured) {
        Ok(RemovalOutcome::Removed) => {
            println!(
                "removed unchanged {} registration at {}",
                captured.scope,
                captured.path.display()
            );
            true
        }
        Ok(RemovalOutcome::Absent) => {
            println!(
                "{} registration already absent at {}",
                captured.scope,
                captured.path.display()
            );
            true
        }
        Ok(RemovalOutcome::ReplacementPreserved { path, detail }) => {
            eprintln!(
                "kept replacement for {} registration at {}: {detail}",
                captured.scope,
                path.display()
            );
            false
        }
        Err(error) => {
            eprintln!("registration cleanup incomplete: {error}");
            false
        }
    }
}

fn clean_registration_indices(
    observations: &[LifecycleObservation],
    indices: impl IntoIterator<Item = usize>,
) -> bool {
    let mut complete = true;
    for index in indices {
        let Some(captured) = observations[index].capture.as_ref() else {
            complete = false;
            eprintln!(
                "could not clean {} because no exact-byte capture exists",
                observations[index].label
            );
            continue;
        };
        complete &= clean_captured_registration(captured);
    }
    complete
}

fn confirm_cleanup_ports_quiescent(
    observations: &[LifecycleObservation],
    indices: &[usize],
) -> bool {
    let mut quiescent = true;
    for index in indices {
        let Some(captured) = observations[*index].capture.as_ref() else {
            quiescent = false;
            eprintln!(
                "could not clean {} because no exact-byte capture exists",
                observations[*index].label
            );
            continue;
        };
        match loopback_listener_state(captured.runfile.pid, captured.runfile.port) {
            ListenerState::Absent => {}
            ListenerState::OwnedByTarget | ListenerState::OwnedByTargetWildcard => {
                quiescent = false;
                eprintln!(
                    "kept registrations because PID {} still owns registered port {} named by {}",
                    captured.runfile.pid,
                    captured.runfile.port,
                    captured.path.display()
                );
            }
            ListenerState::OwnedByOther(owners) => {
                quiescent = false;
                eprintln!(
                    "kept registrations because port {} named by {} remains owned by PIDs {owners:?}",
                    captured.runfile.port,
                    captured.path.display()
                );
            }
            ListenerState::Uninspectable(error) => {
                quiescent = false;
                eprintln!(
                    "kept registrations because port {} ownership named by {} is uninspectable: {error}",
                    captured.runfile.port,
                    captured.path.display()
                );
            }
        }
    }
    quiescent
}

fn down(workspace: &Path) -> ExitCode {
    down_impl(workspace, global_runfile_path())
}

fn down_mutation_blocker(state: &ManagedServerState) -> Option<&[ResolutionIssue]> {
    match state {
        ManagedServerState::Conflict { issues } | ManagedServerState::Unverifiable { issues } => {
            Some(issues)
        }
        ManagedServerState::Degraded { server, issues } if !server.listener.permits_teardown() => {
            Some(issues)
        }
        ManagedServerState::Empty
        | ManagedServerState::Ready(_)
        | ManagedServerState::Degraded { .. }
        | ManagedServerState::StaleOnly { .. } => None,
    }
}

fn down_impl(workspace: &Path, global_path: Option<PathBuf>) -> ExitCode {
    let scope = ManagedDiscoveryScope {
        workspace: workspace.to_path_buf(),
        global: global_path,
    };
    let LifecycleDiscovery {
        managed,
        mut observations,
        resolution,
    } = discover_lifecycle_in(&scope);
    if let Some(issues) = down_mutation_blocker(&managed.state) {
        eprintln!("refusing teardown: server registration state does not authorize mutation");
        for issue in issues {
            eprintln!("  - {}", issue.detail);
        }
        return ExitCode::FAILURE;
    }
    match resolution {
        Resolution::Empty => {
            println!("no server registered");
            ExitCode::SUCCESS
        }
        Resolution::Conflict { .. } | Resolution::Unverifiable { .. } => {
            unreachable!("typed down blocker returned before mutation")
        }
        Resolution::StaleOnly { stale } => {
            if !confirm_cleanup_ports_quiescent(&observations, &stale) {
                println!("no process was stopped; stale registrations were kept");
                return ExitCode::FAILURE;
            }
            let cleaned = clean_registration_indices(&observations, stale);
            println!("no process was stopped; stale registration cleanup completed={cleaned}");
            if cleaned {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Resolution::Degraded { ref listener, .. } if !listener.permits_teardown() => {
            unreachable!("typed down listener blocker returned before mutation")
        }
        Resolution::Ready {
            target,
            aliases,
            stale,
        }
        | Resolution::Degraded {
            target,
            aliases,
            stale,
            ..
        } => {
            let target_capture = observations[target]
                .capture
                .as_ref()
                .expect("resolved target has a capture")
                .clone();
            let expected = target_capture
                .runfile
                .process_identity
                .clone()
                .expect("resolved v2 target has process identity");
            let pid = target_capture.runfile.pid;
            let port = target_capture.runfile.port;
            let process = observations[target]
                .process
                .take()
                .expect("resolved target retains its exact process handle");

            let mut already_exited = match process.wait(Duration::ZERO) {
                Ok(exited) => exited,
                Err(error) => {
                    eprintln!(
                        "refusing teardown: could not query retained process handle: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            if !already_exited {
                match process.inspect(port) {
                    Ok(facts) => {
                        if facts.identity != expected {
                            eprintln!(
                                "refusing teardown: process creation/executable/argv identity changed after resolution"
                            );
                            return ExitCode::FAILURE;
                        }
                        if !facts.listener.permits_teardown() {
                            match facts.listener {
                                ListenerState::OwnedByTargetWildcard => {
                                    eprintln!(
                                        "refusing teardown: registered port {port} is bound through a wildcard/public listener; registration kept for recovery"
                                    );
                                }
                                ListenerState::OwnedByOther(owners) => {
                                    eprintln!(
                                        "refusing teardown: loopback port {port} is owned by other PIDs {owners:?}"
                                    );
                                }
                                ListenerState::Uninspectable(error) => {
                                    eprintln!(
                                        "refusing teardown: loopback ownership is uninspectable: {error}"
                                    );
                                }
                                ListenerState::OwnedByTarget | ListenerState::Absent => {
                                    unreachable!(
                                        "authorizing listener state passed the fail-closed branch"
                                    )
                                }
                            }
                            return ExitCode::FAILURE;
                        }
                    }
                    Err(ProcessError::NotFound(_)) => match process.wait(Duration::ZERO) {
                        Ok(true) => already_exited = true,
                        Ok(false) => {
                            eprintln!(
                                "refusing teardown: process identity vanished without an exit proof"
                            );
                            return ExitCode::FAILURE;
                        }
                        Err(error) => {
                            eprintln!(
                                "refusing teardown: process identity vanished and retained-handle exit inspection failed: {error}"
                            );
                            return ExitCode::FAILURE;
                        }
                    },
                    Err(error) => {
                        eprintln!("refusing teardown: exact process revalidation failed: {error}");
                        return ExitCode::FAILURE;
                    }
                }
            }

            let signalled = if already_exited {
                false
            } else {
                match process.terminate() {
                    Ok(signalled) => signalled,
                    Err(error) => {
                        eprintln!(
                            "could not terminate retained process handle: {error}; registrations kept"
                        );
                        return ExitCode::FAILURE;
                    }
                }
            };
            match process.wait(Duration::from_secs(10)) {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!(
                        "retained process handle did not exit within 10s; registrations kept"
                    );
                    return ExitCode::FAILURE;
                }
                Err(error) => {
                    eprintln!(
                        "could not confirm retained process exit: {error}; registrations kept"
                    );
                    return ExitCode::FAILURE;
                }
            }

            match loopback_listener_state(pid, port) {
                ListenerState::Absent => {}
                ListenerState::OwnedByOther(owners) => {
                    eprintln!(
                        "managed process exited, but loopback port {port} remains owned by PIDs {owners:?}; registrations kept"
                    );
                    return ExitCode::FAILURE;
                }
                ListenerState::OwnedByTarget | ListenerState::OwnedByTargetWildcard => {
                    eprintln!(
                        "managed process exited, but numeric PID {pid} still owns loopback port {port}; registrations kept"
                    );
                    return ExitCode::FAILURE;
                }
                ListenerState::Uninspectable(error) => {
                    eprintln!(
                        "could not verify listener release after exit: {error}; registrations kept"
                    );
                    return ExitCode::FAILURE;
                }
            }

            if signalled {
                println!("stopped managed server pid {pid} through its retained process handle");
            } else {
                println!("managed server pid {pid} had already exited; no process was signalled");
            }
            let mut cleanup = vec![target];
            cleanup.extend(aliases);
            cleanup.extend(stale);
            cleanup.sort_unstable();
            cleanup.dedup();
            if !confirm_cleanup_ports_quiescent(&observations, &cleanup) {
                eprintln!(
                    "managed process exit is confirmed, but at least one registered endpoint remains active or uninspectable; registrations kept"
                );
                return ExitCode::FAILURE;
            }
            let cleaned = clean_registration_indices(&observations, cleanup);
            if cleaned {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "managed process exit is confirmed, but registration cleanup was partial"
                );
                ExitCode::FAILURE
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    lines: Vec<String>,
    success: bool,
}

fn static_doctor_blocker(args: &ServerUpArgs) -> Option<DoctorReport> {
    let mut lines = Vec::new();
    if args.tailscale {
        lines.push(
            "[BLOCKED] --tailscale is fail-closed before registration, PID, engine, model, or network probes because Ferric cannot yet compare-and-remove only the Serve endpoint it owns"
                .to_string(),
        );
        lines.push(
            "[next] leave the registration untouched and inspect exact Serve ownership with Tailscale tooling; do not run a blind node-wide reset"
                .to_string(),
        );
    }
    if args.port == 0 {
        lines.push("[INVALID] --port must be greater than zero".to_string());
    }
    if args.engine == Engine::LlamaServer {
        if args.ctx == 0 {
            lines.push("[INVALID] --ctx must be greater than zero for llama-server".to_string());
        }
        if args.model.is_none() {
            lines.push("[MISSING] --model is required for llama-server".to_string());
        }
        if args.parallel == Some(0) {
            lines.push(
                "[INVALID] --parallel must be greater than zero for llama-server".to_string(),
            );
        }
    } else if args.seed.is_some() || args.parallel.is_some() {
        lines
            .push("[INVALID] --seed and --parallel are supported only by llama-server".to_string());
    }
    (!lines.is_empty()).then_some(DoctorReport {
        lines,
        success: false,
    })
}

fn registration_doctor_blocker(discovery: &ManagedServerDiscovery) -> Option<DoctorReport> {
    if matches!(
        discovery.state,
        ManagedServerState::Empty | ManagedServerState::Ready(_)
    ) {
        return None;
    }
    let status = status_report(discovery);
    let rendered = render_status(&status);
    let mut lines = vec![format!(
        "[BLOCKED] managed registration state is {} before engine/model probes",
        match discovery.state {
            ManagedServerState::Degraded { .. } => "degraded",
            ManagedServerState::StaleOnly { .. } => "stale-only",
            ManagedServerState::Conflict { .. } => "conflicting",
            ManagedServerState::Unverifiable { .. } => "unverifiable",
            ManagedServerState::Empty | ManagedServerState::Ready(_) => unreachable!(),
        }
    )];
    lines.extend(rendered.stderr);
    lines.push(format!("[next] {}", next_action_text(&status.next_action)));
    Some(DoctorReport {
        lines,
        success: false,
    })
}

fn execute_doctor_probes<E: DoctorProbeEffects>(
    args: &ServerUpArgs,
    discovery: &ManagedServerDiscovery,
    effects: &mut E,
) -> DoctorReport {
    let mut lines = Vec::new();
    let mut ok = true;
    let bin = effects.binary_present(args.engine);
    lines.push(format!(
        "[{}] engine binary `{}`",
        if bin { "ok" } else { "MISSING" },
        args.engine.program()
    ));
    ok &= bin;

    if args.engine == Engine::LlamaServer {
        let model = args
            .model
            .as_deref()
            .expect("static doctor validation requires a llama-server model");
        let present = effects.regular_file(Path::new(model));
        lines.push(format!(
            "[{}] model `{model}`",
            if present { "ok" } else { "MISSING" }
        ));
        ok &= present;

        if let Some(mmproj) = &args.mmproj {
            let present = effects.regular_file(mmproj);
            lines.push(format!(
                "[{}] multimodal projector `{}`",
                if present { "ok" } else { "MISSING" },
                mmproj.display()
            ));
            ok &= present;
        }
    }

    match &discovery.state {
        ManagedServerState::Ready(server) => {
            lines.push(format!(
                "[ok] exact managed process/listener identity and HTTP health at {}",
                server.runfile.base_url
            ));
            lines.push(format!(
                "     health: {}",
                health_url(server.runfile.engine, &server.runfile.base_url)
            ));
            lines.push(
                "     verify the constrained path: `ferric bench ltd --protocol grammar`"
                    .to_string(),
            );
        }
        ManagedServerState::Empty => {
            lines.push("[info] no server running — `ferric server up` to start one".to_string());
        }
        ManagedServerState::Degraded { .. }
        | ManagedServerState::StaleOnly { .. }
        | ManagedServerState::Conflict { .. }
        | ManagedServerState::Unverifiable { .. } => {
            unreachable!("blocked discovery must not reach doctor effects")
        }
    }
    DoctorReport { lines, success: ok }
}

fn doctor_report_after_discovery<E: DoctorProbeEffects>(
    args: &ServerUpArgs,
    discovery: &ManagedServerDiscovery,
    effects: &mut E,
) -> DoctorReport {
    if let Some(report) = static_doctor_blocker(args) {
        return report;
    }
    if let Some(report) = registration_doctor_blocker(discovery) {
        return report;
    }
    execute_doctor_probes(args, discovery, effects)
}

fn emit_doctor_report(report: DoctorReport) -> ExitCode {
    for line in report.lines {
        println!("{line}");
    }
    if report.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn doctor(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    if let Some(report) = static_doctor_blocker(args) {
        return emit_doctor_report(report);
    }
    let scope = match ManagedDiscoveryScope::for_workspace(workspace) {
        Ok(scope) => scope,
        Err(error) => {
            return emit_doctor_report(DoctorReport {
                lines: vec![format!(
                    "[BLOCKED] resolve managed discovery scope: {error}"
                )],
                success: false,
            });
        }
    };
    let discovery = discover_managed_server_in(&scope);
    let mut effects = NativeDoctorProbeEffects;
    emit_doctor_report(doctor_report_after_discovery(
        args,
        &discovery,
        &mut effects,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_process::canonical_test_start_token;
    use crate::server_registration::PromisedOriginRegistration;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::net::TcpListener;
    use std::rc::Rc;
    use std::thread;

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
            seed: None,
            parallel: None,
            tailscale: false,
        }
    }

    fn discovery_fixture_path(name: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!(r"C:\fixture\{name}\.ferric\server.json"))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(format!("/fixture/{name}/.ferric/server.json"))
        }
    }

    fn discovery_fixture_executable() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(r"C:\fixture\llama-server.exe")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/fixture/llama-server")
        }
    }

    fn discovery_fixture_identity(seed: u64) -> ProcessIdentity {
        ProcessIdentity {
            start_token: canonical_test_start_token(seed),
            executable: discovery_fixture_executable(),
            argv: vec!["llama-server".to_string(), "--serve".to_string()],
        }
    }

    fn discovery_fixture_runfile(pid: u32, name: &str) -> ServerRunfile {
        let port = u16::try_from(7000 + pid % 1000).unwrap();
        ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("model.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(discovery_fixture_identity(u64::from(pid))),
            origin_local_runfile: Some(discovery_fixture_path(name)),
        }
    }

    fn discovery_fixture_coordinate(
        scope: RegistrationScope,
        name: &str,
    ) -> RegistrationCoordinate {
        RegistrationCoordinate {
            scope,
            path: discovery_fixture_path(name),
        }
    }

    fn discovery_fixture_observation(
        coordinate: RegistrationCoordinate,
        runfile: ServerRunfile,
        runtime: RuntimeObservation,
    ) -> ManagedRegistrationObservation {
        ManagedRegistrationObservation {
            id: ObservationId(0),
            coordinate,
            promised: None,
            state: ManagedRegistrationState::Captured {
                runfile: Box::new(runfile),
                raw_sha256: "fixture-revision".to_string(),
                runtime,
            },
        }
    }

    fn discovery_fixture_empty() -> ManagedServerDiscovery {
        let local = discovery_fixture_coordinate(RegistrationScope::Local, "local");
        let global = discovery_fixture_coordinate(RegistrationScope::Global, "global");
        ManagedServerDiscovery {
            inventory: RegistrationInventory {
                local: RegistrationSlot::Absent {
                    scope: local.scope,
                    path: local.path.clone(),
                },
                global: Some(RegistrationSlot::Absent {
                    scope: global.scope,
                    path: global.path.clone(),
                }),
                promised_origins: Vec::new(),
            },
            observations: vec![
                ManagedRegistrationObservation {
                    id: ObservationId(0),
                    coordinate: local,
                    promised: None,
                    state: ManagedRegistrationState::Absent,
                },
                ManagedRegistrationObservation {
                    id: ObservationId(1),
                    coordinate: global,
                    promised: None,
                    state: ManagedRegistrationState::Absent,
                },
            ],
            state: ManagedServerState::Empty,
        }
    }

    fn discovery_fixture_ready() -> ManagedServerDiscovery {
        let coordinate = discovery_fixture_coordinate(RegistrationScope::Local, "local");
        let runfile = discovery_fixture_runfile(4101, "local");
        let identity = runfile.process_identity.clone().unwrap();
        let raw = serde_json::to_vec(&runfile).unwrap();
        let observation = ManagedRegistrationObservation {
            id: ObservationId(0),
            coordinate: coordinate.clone(),
            promised: None,
            state: ManagedRegistrationState::Captured {
                runfile: Box::new(runfile.clone()),
                raw_sha256: ferric_bench::sha256_bytes(&raw),
                runtime: RuntimeObservation::Verified {
                    identity: identity.clone(),
                    listener: ListenerState::OwnedByTarget,
                    health: HealthState::Healthy,
                },
            },
        };
        let revision = RegistrationRevision {
            coordinate: coordinate.clone(),
            promised: None,
            state: RegistrationRevisionState::Captured(ferric_bench::sha256_bytes(&raw)),
        };
        let server = ManagedServer {
            registration: coordinate.clone(),
            runfile: runfile.clone(),
            identity: identity.clone(),
            listener: ListenerState::OwnedByTarget,
            health: HealthState::Healthy,
            aliases: Vec::new(),
            stale: Vec::new(),
            fingerprint: DiscoveryFingerprint {
                pid: runfile.pid,
                identity,
                runfile: runfile.clone(),
                revisions: vec![revision],
            },
        };
        ManagedServerDiscovery {
            inventory: RegistrationInventory {
                local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                    scope: RegistrationScope::Local,
                    path: coordinate.path.clone(),
                    raw,
                    runfile,
                })),
                global: None,
                promised_origins: Vec::new(),
            },
            observations: vec![observation],
            state: ManagedServerState::Ready(server),
        }
    }

    fn discovery_fixture_degraded(
        listener: ListenerState,
        health: HealthState,
    ) -> ManagedServerDiscovery {
        let mut discovery = discovery_fixture_ready();
        let mut server = match &discovery.state {
            ManagedServerState::Ready(server) => server.clone(),
            _ => unreachable!(),
        };
        server.listener = listener.clone();
        server.health = health;
        let coordinate = server.registration.clone();
        if let ManagedRegistrationState::Captured { runtime, .. } =
            &mut discovery.observations[0].state
        {
            *runtime = RuntimeObservation::Verified {
                identity: server.identity.clone(),
                listener: listener.clone(),
                health,
            };
        }
        discovery.state = ManagedServerState::Degraded {
            server,
            issues: vec![ResolutionIssue {
                coordinates: vec![coordinate],
                kind: ResolutionIssueKind::Degraded,
                detail: "fixture degraded state".to_string(),
            }],
        };
        discovery
    }

    fn discovery_fixture_stale_only() -> ManagedServerDiscovery {
        let mut discovery = discovery_fixture_ready();
        let coordinate = discovery.observations[0].coordinate.clone();
        if let ManagedRegistrationState::Captured { runtime, .. } =
            &mut discovery.observations[0].state
        {
            *runtime = RuntimeObservation::Stale {
                reason: "PID is absent".to_string(),
                observed_identity: None,
                listener: ListenerState::Absent,
            };
        }
        discovery.state = ManagedServerState::StaleOnly {
            stale: vec![coordinate],
        };
        discovery
    }

    fn discovery_fixture_blocked(conflict: bool) -> ManagedServerDiscovery {
        let mut discovery = discovery_fixture_ready();
        let coordinate = discovery.observations[0].coordinate.clone();
        let issue = ResolutionIssue {
            coordinates: vec![coordinate],
            kind: if conflict {
                ResolutionIssueKind::Conflict
            } else {
                ResolutionIssueKind::Unverifiable
            },
            detail: if conflict {
                "fixture registration conflict"
            } else {
                "fixture registration is unverifiable"
            }
            .to_string(),
        };
        if conflict {
            discovery.state = ManagedServerState::Conflict {
                issues: vec![issue],
            };
        } else {
            if let ManagedRegistrationState::Captured { runtime, .. } =
                &mut discovery.observations[0].state
            {
                *runtime = RuntimeObservation::Unverifiable {
                    reason: "fixture process inspection failed".to_string(),
                    observed_identity: None,
                    listener: None,
                    health: HealthState::NotProbed,
                };
            }
            discovery.state = ManagedServerState::Unverifiable {
                issues: vec![issue],
            };
        }
        discovery
    }

    struct PanicHealth;

    impl HealthProbe for PanicHealth {
        fn status_ok(&mut self, _host: &str, _port: u16, _path: &str) -> bool {
            panic!("static registration blockers must precede HTTP health")
        }
    }

    #[test]
    fn tailscale_registration_blocks_before_process_inspection() {
        let coordinate = discovery_fixture_coordinate(RegistrationScope::Local, "tailscale");
        let mut runfile = discovery_fixture_runfile(4201, "tailscale");
        runfile.tailscale = true;
        let raw = serde_json::to_vec(&runfile).unwrap();
        let inventory = RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: coordinate.scope,
                path: coordinate.path,
                raw,
                runfile,
            })),
            global: None,
            promised_origins: Vec::new(),
        };
        let observer_calls = Rc::new(RefCell::new(0_usize));
        let calls = Rc::clone(&observer_calls);
        let discovery = discover_inventory_with(
            inventory,
            move |_capture| {
                *calls.borrow_mut() += 1;
                panic!("Tailscale blocker must precede process acquisition")
            },
            &mut PanicHealth,
            |_observation| panic!("Tailscale blocker must precede retained reinspection"),
        );
        assert_eq!(*observer_calls.borrow(), 0);
        assert!(matches!(
            discovery.managed.state,
            ManagedServerState::Unverifiable { .. }
        ));
        assert!(matches!(
            discovery.managed.observations[0].state,
            ManagedRegistrationState::Captured {
                runtime: RuntimeObservation::NotInspected,
                ..
            }
        ));
    }

    #[test]
    fn promised_origin_static_matrix_precedes_process_inspection() {
        for changed in [false, true] {
            let local = discovery_fixture_coordinate(RegistrationScope::Local, "local");
            let global = discovery_fixture_coordinate(RegistrationScope::Global, "global");
            let origin = discovery_fixture_coordinate(RegistrationScope::Origin, "origin");
            let expected = discovery_fixture_runfile(4202, "origin");
            let global_capture = CapturedRegistration {
                scope: global.scope,
                path: global.path.clone(),
                raw: serde_json::to_vec(&expected).unwrap(),
                runfile: expected.clone(),
            };
            let origin_slot = if changed {
                let mut replacement = expected.clone();
                replacement.context_size = Some(4096);
                RegistrationSlot::Captured(Box::new(CapturedRegistration {
                    scope: origin.scope,
                    path: origin.path.clone(),
                    raw: serde_json::to_vec(&replacement).unwrap(),
                    runfile: replacement,
                }))
            } else {
                RegistrationSlot::Absent {
                    scope: origin.scope,
                    path: origin.path.clone(),
                }
            };
            let inventory = RegistrationInventory {
                local: RegistrationSlot::Absent {
                    scope: local.scope,
                    path: local.path,
                },
                global: Some(RegistrationSlot::Captured(Box::new(global_capture))),
                promised_origins: vec![PromisedOriginRegistration {
                    source: global,
                    expected_runfile: expected,
                    slot: origin_slot,
                }],
            };
            let result = discover_inventory_with(
                inventory,
                |_capture| panic!("origin blocker must precede process acquisition"),
                &mut PanicHealth,
                |_observation| panic!("origin blocker must precede retained reinspection"),
            );
            if changed {
                assert!(matches!(
                    result.managed.state,
                    ManagedServerState::Conflict { .. }
                ));
            } else {
                assert!(matches!(
                    result.managed.state,
                    ManagedServerState::Unverifiable { .. }
                ));
            }
            assert_eq!(result.managed.observations.len(), 3);
            assert!(
                result
                    .managed
                    .observations
                    .iter()
                    .any(|observation| observation.coordinate.scope == RegistrationScope::Origin)
            );
        }
    }

    fn doctor_fixture_args() -> ServerUpArgs {
        ServerUpArgs {
            engine: Engine::LlamaServer,
            model: Some("model.gguf".to_string()),
            mmproj: None,
            ctx: 8192,
            port: 8080,
            threads: None,
            gpu_layers: None,
            batch_size: None,
            seed: Some(42),
            parallel: Some(1),
            tailscale: false,
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DoctorEvent {
        Binary,
        File,
    }

    #[derive(Default)]
    struct RecordingDoctorEffects {
        events: Vec<DoctorEvent>,
    }

    impl DoctorProbeEffects for RecordingDoctorEffects {
        fn binary_present(&mut self, _engine: Engine) -> bool {
            self.events.push(DoctorEvent::Binary);
            true
        }

        fn regular_file(&mut self, _path: &Path) -> bool {
            self.events.push(DoctorEvent::File);
            true
        }
    }

    #[test]
    fn status_reports_scope_identity_health_and_next_action() {
        let empty = discovery_fixture_empty();
        let empty_report = status_report(&empty);
        let empty_rendered = render_status(&empty_report);
        assert_eq!(empty_report.state, ManagedServerState::Empty);
        assert_eq!(empty_report.next_action, StatusNextAction::StartServer);
        assert_eq!(empty_rendered.stderr, Vec::<String>::new());
        assert_eq!(
            empty_rendered.stdout,
            vec![
                format!(
                    "[absent] local registration {}: recorded-identity=none observed-identity=none listener=none health=none",
                    discovery_fixture_path("local").display()
                ),
                format!(
                    "[absent] global registration {}: recorded-identity=none observed-identity=none listener=none health=none",
                    discovery_fixture_path("global").display()
                ),
                "[state] empty".to_string(),
                "[next] no registration is active; inspect required launch options with `ferric server up --help`".to_string(),
            ]
        );

        let ready = discovery_fixture_ready();
        let ready_report = status_report(&ready);
        let ready_rendered = render_status(&ready_report);
        assert!(matches!(
            ready_report.next_action,
            StatusNextAction::ContinueManaged { .. }
        ));
        assert!(ready_rendered.success);
        assert!(ready_rendered.stdout[0].contains("recorded-identity=token="));
        assert!(ready_rendered.stdout[0].contains("observed-identity=token="));
        assert!(ready_rendered.stdout[0].contains("listener=owned-loopback"));
        assert!(ready_rendered.stdout[0].contains("health=healthy"));

        let unhealthy =
            discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy);
        assert!(matches!(
            status_report(&unhealthy).next_action,
            StatusNextAction::StopManaged { .. }
        ));
        let absent = discovery_fixture_degraded(ListenerState::Absent, HealthState::NotProbed);
        assert!(matches!(
            status_report(&absent).next_action,
            StatusNextAction::StopManaged { .. }
        ));
        let wildcard =
            discovery_fixture_degraded(ListenerState::OwnedByTargetWildcard, HealthState::Healthy);
        assert!(matches!(
            status_report(&wildcard).next_action,
            StatusNextAction::InspectWildcard { .. }
        ));
        assert!(
            render_status(&status_report(&wildcard))
                .stdout
                .last()
                .unwrap()
                .contains("teardown is not authorized")
        );

        let stale = discovery_fixture_stale_only();
        assert_eq!(
            status_report(&stale).next_action,
            StatusNextAction::CleanStale
        );

        let mut split = discovery_fixture_ready();
        let stale_coordinate =
            discovery_fixture_coordinate(RegistrationScope::Global, "stale-global");
        let stale_record = discovery_fixture_runfile(4102, "stale-global");
        let stale_raw = serde_json::to_vec(&stale_record).unwrap();
        split.inventory.global = Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
            scope: stale_coordinate.scope,
            path: stale_coordinate.path.clone(),
            raw: stale_raw,
            runfile: stale_record.clone(),
        })));
        split.observations.push(discovery_fixture_observation(
            stale_coordinate.clone(),
            stale_record,
            RuntimeObservation::Stale {
                reason: "generation changed".to_string(),
                observed_identity: Some(discovery_fixture_identity(4102)),
                listener: ListenerState::Absent,
            },
        ));
        if let ManagedServerState::Ready(server) = &mut split.state {
            server.stale.push(stale_coordinate);
        }
        let split_rendered = render_status(&status_report(&split));
        assert_eq!(split_rendered.stdout.len(), 4);
        assert!(split_rendered.stdout[1].contains("observed-identity=token="));
        assert!(split_rendered.stdout[1].contains("stale-reason=generation changed"));

        let conflict = discovery_fixture_blocked(true);
        assert!(matches!(
            status_report(&conflict).next_action,
            StatusNextAction::ResolveConflict { .. }
        ));
        assert_eq!(render_status(&status_report(&conflict)).stderr.len(), 1);

        let unverifiable = discovery_fixture_blocked(false);
        assert!(matches!(
            status_report(&unverifiable).next_action,
            StatusNextAction::RepairUnverifiable { .. }
        ));

        let mut missing_origin = discovery_fixture_empty();
        let origin = discovery_fixture_coordinate(RegistrationScope::Origin, "missing-origin");
        let source = discovery_fixture_coordinate(RegistrationScope::Global, "global");
        let expected = discovery_fixture_runfile(4103, "missing-origin");
        let promised = PromisedOriginProvenance {
            source: source.clone(),
            expected_runfile: expected.clone(),
        };
        missing_origin
            .observations
            .push(ManagedRegistrationObservation {
                id: ObservationId(2),
                coordinate: origin.clone(),
                promised: Some(promised),
                state: ManagedRegistrationState::Absent,
            });
        missing_origin.inventory.promised_origins = vec![PromisedOriginRegistration {
            source,
            expected_runfile: expected,
            slot: RegistrationSlot::Absent {
                scope: origin.scope,
                path: origin.path.clone(),
            },
        }];
        missing_origin.state = ManagedServerState::Unverifiable {
            issues: vec![ResolutionIssue {
                coordinates: vec![origin.clone()],
                kind: ResolutionIssueKind::Unverifiable,
                detail: "promised origin is absent".to_string(),
            }],
        };
        assert_eq!(
            status_report(&missing_origin).next_action,
            StatusNextAction::InspectPromisedOrigin {
                path: origin.path.clone()
            }
        );
        let missing_rendered = render_status(&status_report(&missing_origin));
        assert!(missing_rendered.stdout[2].contains("promised-by=global registration"));

        let mut legacy = discovery_fixture_ready();
        let legacy_pid = 4101;
        if let ManagedRegistrationState::Captured {
            runfile, runtime, ..
        } = &mut legacy.observations[0].state
        {
            runfile.schema_version = 1;
            runfile.process_identity = None;
            *runtime = RuntimeObservation::LegacyLive { pid: legacy_pid };
        }
        let legacy_runfile = match &legacy.observations[0].state {
            ManagedRegistrationState::Captured { runfile, .. } => runfile.as_ref().clone(),
            _ => unreachable!(),
        };
        let legacy_raw = serde_json::to_vec(&legacy_runfile).unwrap();
        if let RegistrationSlot::Captured(local) = &mut legacy.inventory.local {
            local.runfile = legacy_runfile.clone();
            local.raw = legacy_raw.clone();
        }
        let legacy_global =
            discovery_fixture_coordinate(RegistrationScope::Global, "legacy-global");
        legacy.inventory.global =
            Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: legacy_global.scope,
                path: legacy_global.path.clone(),
                raw: legacy_raw,
                runfile: legacy_runfile.clone(),
            })));
        let mut legacy_alias = discovery_fixture_observation(
            legacy_global.clone(),
            legacy_runfile,
            RuntimeObservation::LegacyLive { pid: legacy_pid },
        );
        legacy_alias.id = ObservationId(1);
        legacy.observations.push(legacy_alias);
        legacy.state = ManagedServerState::Unverifiable {
            issues: vec![ResolutionIssue {
                coordinates: vec![
                    legacy.observations[0].coordinate.clone(),
                    legacy_global.clone(),
                ],
                kind: ResolutionIssueKind::Unverifiable,
                detail: "live legacy registration".to_string(),
            }],
        };
        assert_eq!(
            status_report(&legacy).next_action,
            StatusNextAction::AdoptLegacy { pid: legacy_pid }
        );
        assert!(
            render_status(&status_report(&legacy))
                .stdout
                .last()
                .unwrap()
                .contains("`ferric server adopt --pid 4101`")
        );
        let mut incompatible_legacy = legacy.clone();
        if let ManagedRegistrationState::Captured { runfile, .. } =
            &mut incompatible_legacy.observations[1].state
        {
            runfile.context_size = Some(4096);
        }
        assert!(matches!(
            status_report(&incompatible_legacy).next_action,
            StatusNextAction::RepairUnverifiable { .. }
        ));

        let mut tailscale = discovery_fixture_ready();
        if let ManagedRegistrationState::Captured {
            runfile, runtime, ..
        } = &mut tailscale.observations[0].state
        {
            runfile.tailscale = true;
            *runtime = RuntimeObservation::NotInspected;
        }
        tailscale.state = ManagedServerState::Unverifiable {
            issues: vec![ResolutionIssue {
                coordinates: vec![tailscale.observations[0].coordinate.clone()],
                kind: ResolutionIssueKind::Unverifiable,
                detail: "durable Tailscale Serve state".to_string(),
            }],
        };
        assert!(matches!(
            status_report(&tailscale).next_action,
            StatusNextAction::InspectTailscale { .. }
        ));

        for discovery in [
            ready,
            unhealthy,
            absent,
            wildcard,
            stale,
            split,
            conflict,
            unverifiable,
            missing_origin,
            incompatible_legacy,
            legacy,
            tailscale,
        ] {
            let rendered = render_status(&status_report(&discovery));
            assert_eq!(
                rendered
                    .stdout
                    .iter()
                    .filter(|line| line.starts_with("[next] "))
                    .count(),
                1,
                "every state must render exactly one safe next action"
            );
            for registration in rendered.stdout.iter().take(discovery.observations.len()) {
                assert!(registration.contains("recorded-identity="));
                assert!(registration.contains("observed-identity="));
                assert!(registration.contains("listener="));
                assert!(registration.contains("health="));
            }
        }
    }

    #[test]
    fn doctor_blocks_before_external_probes() {
        let mut tailscale_args = doctor_fixture_args();
        tailscale_args.tailscale = true;
        let mut effects = RecordingDoctorEffects::default();
        let report = static_doctor_blocker(&tailscale_args).expect("Tailscale must block");
        assert!(!report.success);
        assert!(report.lines[0].contains("before registration, PID, engine, model, or network"));
        assert!(effects.events.is_empty());

        for discovery in [
            discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy),
            discovery_fixture_stale_only(),
            discovery_fixture_blocked(true),
            discovery_fixture_blocked(false),
        ] {
            effects.events.clear();
            let report =
                doctor_report_after_discovery(&doctor_fixture_args(), &discovery, &mut effects);
            assert!(!report.success);
            assert!(report.lines[0].starts_with("[BLOCKED]"));
            assert!(effects.events.is_empty());
        }

        effects.events.clear();
        let ready = doctor_report_after_discovery(
            &doctor_fixture_args(),
            &discovery_fixture_ready(),
            &mut effects,
        );
        assert!(ready.success);
        assert_eq!(effects.events, vec![DoctorEvent::Binary, DoctorEvent::File]);
    }

    #[test]
    fn registration_consumers_propagate_typed_ambiguity() {
        let scope = ManagedDiscoveryScope {
            workspace: discovery_fixture_path("workspace")
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf(),
            global: Some(discovery_fixture_path("global")),
        };
        let explicit =
            crate::backend::select_endpoint_with(Some("http://explicit.example/v1"), || {
                panic!("explicit endpoint selection must not run managed discovery")
            })
            .unwrap();
        assert!(matches!(
            explicit,
            crate::backend::EndpointSelection::Explicit { .. }
        ));
        assert!(
            down_mutation_blocker(
                &discovery_fixture_degraded(
                    ListenerState::OwnedByTargetWildcard,
                    HealthState::NotProbed,
                )
                .state
            )
            .is_some()
        );
        let fixtures = [
            discovery_fixture_empty(),
            discovery_fixture_ready(),
            discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy),
            discovery_fixture_stale_only(),
            discovery_fixture_blocked(true),
            discovery_fixture_blocked(false),
        ];
        for discovery in fixtures {
            let status = status_report(&discovery);
            assert_eq!(status.state, discovery.state);

            let mut effects = RecordingDoctorEffects::default();
            let doctor =
                doctor_report_after_discovery(&doctor_fixture_args(), &discovery, &mut effects);
            let automatic =
                crate::backend::automatic_endpoint_from_discovery(scope.clone(), discovery.clone());
            let strict =
                crate::backend::require_managed_endpoint(scope.clone(), discovery.clone(), None);
            let down_blocker = down_mutation_blocker(&discovery.state);
            match &discovery.state {
                ManagedServerState::Empty => {
                    assert!(matches!(
                        automatic,
                        Ok(crate::backend::EndpointSelection::Default { .. })
                    ));
                    assert!(strict.is_err());
                    assert!(doctor.success);
                    assert!(!effects.events.is_empty());
                    assert!(down_blocker.is_none());
                }
                ManagedServerState::Ready(server) => {
                    assert!(matches!(
                        automatic,
                        Ok(crate::backend::EndpointSelection::Managed { .. })
                    ));
                    assert!(strict.is_ok());
                    let explicit_match = format!("{}/", server.runfile.base_url);
                    let matching = crate::backend::require_managed_endpoint(
                        scope.clone(),
                        discovery.clone(),
                        Some(&explicit_match),
                    )
                    .unwrap();
                    assert!(matches!(
                        matching,
                        crate::backend::EndpointSelection::Managed {
                            explicit_base_url: Some(_),
                            ..
                        }
                    ));
                    assert!(matches!(
                        crate::backend::require_managed_endpoint(
                            scope.clone(),
                            discovery.clone(),
                            Some("http://127.0.0.1:65535/v1"),
                        ),
                        Err(crate::backend::EndpointSelectionError::ExplicitManagedMismatch { .. })
                    ));
                    assert!(doctor.success);
                    assert!(!effects.events.is_empty());
                    assert!(down_blocker.is_none());
                }
                ManagedServerState::Degraded { .. } => {
                    assert!(matches!(
                        automatic,
                        Err(crate::backend::EndpointSelectionError::Degraded(_))
                    ));
                    assert!(matches!(
                        strict,
                        Err(crate::backend::EndpointSelectionError::Degraded(_))
                    ));
                    assert!(!doctor.success);
                    assert!(effects.events.is_empty());
                    assert!(down_blocker.is_none());
                }
                ManagedServerState::StaleOnly { .. } => {
                    assert!(matches!(
                        automatic,
                        Err(crate::backend::EndpointSelectionError::StaleOnly(_))
                    ));
                    assert!(matches!(
                        strict,
                        Err(crate::backend::EndpointSelectionError::StaleOnly(_))
                    ));
                    assert!(!doctor.success);
                    assert!(effects.events.is_empty());
                    assert!(down_blocker.is_none());
                }
                ManagedServerState::Conflict { .. } => {
                    assert!(matches!(
                        automatic,
                        Err(crate::backend::EndpointSelectionError::Conflict(_))
                    ));
                    assert!(matches!(
                        strict,
                        Err(crate::backend::EndpointSelectionError::Conflict(_))
                    ));
                    assert!(!doctor.success);
                    assert!(effects.events.is_empty());
                    assert!(down_blocker.is_some());
                }
                ManagedServerState::Unverifiable { .. } => {
                    assert!(matches!(
                        automatic,
                        Err(crate::backend::EndpointSelectionError::Unverifiable(_))
                    ));
                    assert!(matches!(
                        strict,
                        Err(crate::backend::EndpointSelectionError::Unverifiable(_))
                    ));
                    assert!(!doctor.success);
                    assert!(effects.events.is_empty());
                    assert!(down_blocker.is_some());
                }
            }
        }
    }

    #[test]
    fn strict_autonomy_requires_fresh_managed_discovery_before_http() {
        fn mirrored_inventory() -> RegistrationInventory {
            let local = discovery_fixture_coordinate(RegistrationScope::Local, "strict-local");
            let global = discovery_fixture_coordinate(RegistrationScope::Global, "strict-global");
            let runfile = discovery_fixture_runfile(4101, "strict-local");
            let raw = serde_json::to_vec(&runfile).unwrap();
            RegistrationInventory {
                local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                    scope: local.scope,
                    path: local.path,
                    raw: raw.clone(),
                    runfile: runfile.clone(),
                })),
                global: Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
                    scope: global.scope,
                    path: global.path,
                    raw,
                    runfile,
                }))),
                promised_origins: Vec::new(),
            }
        }

        fn observe_exact(capture: CapturedRegistration) -> LifecycleObservation {
            let identity = capture.runfile.process_identity.clone().unwrap();
            let label = registration_label(capture.scope, &capture.path);
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate {
                        scope: capture.scope,
                        path: capture.path.clone(),
                    },
                    runfile: Some(capture.runfile.clone()),
                    state: CandidateState::Verified {
                        identity,
                        listener: ListenerState::OwnedByTarget,
                        health: HealthState::NotProbed,
                    },
                },
                label,
                capture: Some(capture),
                process: None,
            }
        }

        let inventory = mirrored_inventory();
        let initial = discover_inventory_before_health_with(inventory.clone(), observe_exact);
        let expected = match &initial.managed.state {
            ManagedServerState::Degraded { server, .. } => server.fingerprint.clone(),
            state => panic!("pre-health exact owner must be typed Degraded, got {state:?}"),
        };
        crate::autonomy_cmd::require_matching_pre_health_discovery(&initial.managed, &expected)
            .unwrap();
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: ledger.clone(),
        };
        let ready = complete_lifecycle_health_with(initial, &mut health, |_| Ok(()));
        assert!(matches!(ready.managed.state, ManagedServerState::Ready(_)));
        assert_eq!(ledger.borrow().as_slice(), &[LifecycleEvent::Health(7101)]);
        let ready_server = match &ready.managed.state {
            ManagedServerState::Ready(server) => server,
            _ => unreachable!(),
        };
        let facts = ProcessFacts {
            identity: ready_server.identity.clone(),
            listener: ListenerState::OwnedByTarget,
        };
        ledger.borrow_mut().clear();
        let process = ScriptedProcess::new(
            ready_server.runfile.pid,
            "strict-generation",
            ledger.clone(),
        )
        .with_inspection(Ok(facts.clone()))
        .with_inspection(Ok(facts));
        let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
        bracket_registered_effect_with(&runtime, &ready_server.runfile, || {
            ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            ledger.borrow().as_slice(),
            &[
                LifecycleEvent::Acquire(4101),
                LifecycleEvent::Inspect("strict-generation", 7101),
                LifecycleEvent::ConsumerHttp,
                LifecycleEvent::Inspect("strict-generation", 7101),
            ]
        );

        let mut changed_revision = inventory.clone();
        let Some(RegistrationSlot::Captured(global)) = &mut changed_revision.global else {
            unreachable!()
        };
        global.raw.push(b' ');

        let mut missing_alias = inventory.clone();
        let global_path = match missing_alias.global.take().unwrap() {
            RegistrationSlot::Captured(global) => global.path,
            _ => unreachable!(),
        };
        missing_alias.global = Some(RegistrationSlot::Absent {
            scope: RegistrationScope::Global,
            path: global_path,
        });

        let mut conflicting_peer = inventory;
        let Some(RegistrationSlot::Captured(global)) = &mut conflicting_peer.global else {
            unreachable!()
        };
        global.runfile.pid = 4102;
        global.runfile.process_identity = Some(discovery_fixture_identity(4102));
        global.raw = serde_json::to_vec(&global.runfile).unwrap();

        for changed in [changed_revision, missing_alias, conflicting_peer] {
            ledger.borrow_mut().clear();
            let before_health = discover_inventory_before_health_with(changed, observe_exact);
            assert!(
                crate::autonomy_cmd::require_matching_pre_health_discovery(
                    &before_health.managed,
                    &expected,
                )
                .is_err()
            );
            assert!(
                ledger.borrow().is_empty(),
                "fingerprint or conflict rejection must precede HTTP health"
            );
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum LifecycleEvent {
        Acquire(u32),
        ChildTryWait(u32),
        ChildKill(u32),
        ChildWait(u32),
        Inspect(&'static str, u16),
        Terminate(&'static str),
        RetainedWait(&'static str),
        Listener(u32, u16),
        Health(u16),
        ConsumerHttp,
        ClockNow,
        Sleep,
        Publish,
    }

    type EventLedger = Rc<RefCell<Vec<LifecycleEvent>>>;

    #[derive(Debug, Clone)]
    struct ScriptedExit(&'static str);

    impl std::fmt::Display for ScriptedExit {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    struct ScriptedChild {
        pid: u32,
        try_wait: VecDeque<Result<Option<ScriptedExit>, String>>,
        wait: VecDeque<Result<ScriptedExit, String>>,
        kill: VecDeque<Result<(), String>>,
        ledger: EventLedger,
    }

    impl ScriptedChild {
        fn new(
            pid: u32,
            try_wait: impl IntoIterator<Item = Result<Option<ScriptedExit>, String>>,
            ledger: EventLedger,
        ) -> Self {
            Self {
                pid,
                try_wait: try_wait.into_iter().collect(),
                wait: VecDeque::from([Ok(ScriptedExit("exited"))]),
                kill: VecDeque::from([Ok(())]),
                ledger,
            }
        }
    }

    impl SpawnedChild for ScriptedChild {
        type ExitStatus = ScriptedExit;

        fn pid(&self) -> u32 {
            self.pid
        }

        fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::ChildTryWait(self.pid));
            self.try_wait
                .pop_front()
                .expect("scripted child try_wait result")
        }

        fn wait(&mut self) -> Result<Self::ExitStatus, String> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::ChildWait(self.pid));
            self.wait.pop_front().expect("scripted child wait result")
        }

        fn kill(&mut self) -> Result<(), String> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::ChildKill(self.pid));
            self.kill.pop_front().expect("scripted child kill result")
        }
    }

    #[derive(Debug, Clone)]
    struct ScriptedProcess {
        pid: u32,
        generation: &'static str,
        inspect: Rc<RefCell<VecDeque<Result<ProcessFacts, ProcessError>>>>,
        terminate: Rc<RefCell<VecDeque<Result<bool, ProcessError>>>>,
        wait: Rc<RefCell<VecDeque<Result<bool, ProcessError>>>>,
        ledger: EventLedger,
    }

    impl ScriptedProcess {
        fn new(pid: u32, generation: &'static str, ledger: EventLedger) -> Self {
            Self {
                pid,
                generation,
                inspect: Rc::new(RefCell::new(VecDeque::new())),
                terminate: Rc::new(RefCell::new(VecDeque::from([Ok(true)]))),
                wait: Rc::new(RefCell::new(VecDeque::from([Ok(true)]))),
                ledger,
            }
        }

        fn with_inspection(self, result: Result<ProcessFacts, ProcessError>) -> Self {
            self.inspect.borrow_mut().push_back(result);
            self
        }

        fn with_terminate(self, result: Result<bool, ProcessError>) -> Self {
            *self.terminate.borrow_mut() = VecDeque::from([result]);
            self
        }

        fn with_wait(self, result: Result<bool, ProcessError>) -> Self {
            *self.wait.borrow_mut() = VecDeque::from([result]);
            self
        }
    }

    impl RetainedProcessHandle for ScriptedProcess {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Inspect(self.generation, port));
            self.inspect
                .borrow_mut()
                .pop_front()
                .expect("scripted retained-process inspection")
        }

        fn terminate(&self) -> Result<bool, ProcessError> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Terminate(self.generation));
            self.terminate
                .borrow_mut()
                .pop_front()
                .expect("scripted retained-process terminate result")
        }

        fn wait(&self, _timeout: Duration) -> Result<bool, ProcessError> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::RetainedWait(self.generation));
            self.wait
                .borrow_mut()
                .pop_front()
                .expect("scripted retained-process wait result")
        }
    }

    struct ScriptedRuntime {
        acquisitions: RefCell<VecDeque<Result<ScriptedProcess, String>>>,
        ledger: EventLedger,
    }

    impl ScriptedRuntime {
        fn new(result: Result<ScriptedProcess, String>, ledger: EventLedger) -> Self {
            Self {
                acquisitions: RefCell::new(VecDeque::from([result])),
                ledger,
            }
        }
    }

    impl SpawnedProcessRuntime<ScriptedChild> for ScriptedRuntime {
        type Process = ScriptedProcess;

        fn acquire_child(&self, child: &ScriptedChild) -> Result<Self::Process, String> {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Acquire(child.pid));
            self.acquisitions
                .borrow_mut()
                .pop_front()
                .expect("scripted process acquisition")
        }
    }

    impl ProcessRuntime for ScriptedRuntime {
        type Process = ScriptedProcess;

        fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
            self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
            self.acquisitions
                .borrow_mut()
                .pop_front()
                .expect("scripted process acquisition")
                .map_err(ProcessError::Operation)
        }
    }

    #[test]
    fn registered_consumer_effect_revalidates_retained_generation_on_every_outcome() {
        let runfile = discovery_fixture_runfile(4101, "consumer-effect");
        let before = ProcessFacts {
            identity: runfile.process_identity.clone().unwrap(),
            listener: ListenerState::OwnedByTarget,
        };

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(4101, "effect-error", ledger.clone())
            .with_inspection(Ok(before.clone()))
            .with_inspection(Ok(before.clone()));
        let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
        let error = bracket_registered_effect_with(&runtime, &runfile, || {
            ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
            Err::<(), _>("scripted HTTP failure".to_string())
        })
        .unwrap_err();
        assert_eq!(error, "scripted HTTP failure");
        assert_eq!(
            ledger.borrow().as_slice(),
            &[
                LifecycleEvent::Acquire(4101),
                LifecycleEvent::Inspect("effect-error", 7101),
                LifecycleEvent::ConsumerHttp,
                LifecycleEvent::Inspect("effect-error", 7101),
            ]
        );

        for (label, after, expected_error) in [
            (
                "identity-change",
                ProcessFacts {
                    identity: discovery_fixture_identity(4102),
                    listener: ListenerState::OwnedByTarget,
                },
                "changed process identity",
            ),
            (
                "listener-change",
                ProcessFacts {
                    identity: before.identity.clone(),
                    listener: ListenerState::OwnedByTargetWildcard,
                },
                "wildcard/public listener",
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(4101, label, ledger.clone())
                .with_inspection(Ok(before.clone()))
                .with_inspection(Ok(after));
            let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
            let error = bracket_registered_effect_with(&runtime, &runfile, || {
                ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
                Ok(())
            })
            .unwrap_err();
            assert!(error.contains(expected_error), "{label}: {error}");
            assert_eq!(ledger.borrow()[2], LifecycleEvent::ConsumerHttp);
            assert!(matches!(
                ledger.borrow()[3],
                LifecycleEvent::Inspect(_, 7101)
            ));
        }
    }

    struct ScriptedListener {
        states: RefCell<VecDeque<ListenerState>>,
        ledger: EventLedger,
    }

    impl ScriptedListener {
        fn new(state: ListenerState, ledger: EventLedger) -> Self {
            Self {
                states: RefCell::new(VecDeque::from([state])),
                ledger,
            }
        }
    }

    impl ListenerInspector for ScriptedListener {
        fn listener_state(&self, pid: u32, port: u16) -> ListenerState {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Listener(pid, port));
            self.states
                .borrow_mut()
                .pop_front()
                .expect("scripted listener state")
        }
    }

    struct ScriptedHealth {
        results: VecDeque<bool>,
        ledger: EventLedger,
    }

    impl HealthProbe for ScriptedHealth {
        fn status_ok(&mut self, _host: &str, port: u16, _path: &str) -> bool {
            self.ledger.borrow_mut().push(LifecycleEvent::Health(port));
            self.results.pop_front().expect("scripted health result")
        }
    }

    struct ScriptedClock {
        now: Instant,
        ledger: EventLedger,
    }

    impl LifecycleClock for ScriptedClock {
        fn now(&mut self) -> Instant {
            self.ledger.borrow_mut().push(LifecycleEvent::ClockNow);
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.ledger.borrow_mut().push(LifecycleEvent::Sleep);
            self.now += duration;
        }
    }

    fn scripted_facts(listener: ListenerState) -> ProcessFacts {
        ProcessFacts {
            identity: ProcessIdentity {
                start_token: "scripted-generation".to_string(),
                executable: PathBuf::from("scripted-engine"),
                argv: vec!["scripted-engine".to_string()],
            },
            listener,
        }
    }

    #[test]
    fn bound_child_try_wait_error_uses_retained_cleanup_or_preserves_recovery() {
        let pid = 4101;
        let port = 9411;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-a", Rc::clone(&ledger));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(
            pid,
            [Err("post-bind try_wait failed".to_string())],
            Rc::clone(&ledger),
        );

        let error = bind_spawned_child(&mut child, &runtime, port, &listener)
            .expect_err("a post-bind child inspection error must fail launch");
        assert!(error.contains("exact retained child was stopped"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Terminate("generation-a"),
                LifecycleEvent::RetainedWait("generation-a"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
            ]
        );

        let recovery_ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-b", Rc::clone(&recovery_ledger))
            .with_terminate(Err(ProcessError::Operation("access denied".to_string())));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&recovery_ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&recovery_ledger));
        let mut child = ScriptedChild::new(
            pid,
            [Err("post-bind try_wait failed".to_string())],
            Rc::clone(&recovery_ledger),
        );

        let error = bind_spawned_child(&mut child, &runtime, port, &listener)
            .expect_err("unproved retained cleanup must preserve a recovery clue");
        assert!(error.contains("recovery failure for retained PID 4101"));
        assert!(error.contains("access denied"));
        assert_eq!(
            *recovery_ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Terminate("generation-b"),
            ]
        );
    }

    #[test]
    fn stop_managed_child_reaps_proven_exit_before_listener_failure() {
        let pid = 4102;
        let port = 9412;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-a", Rc::clone(&ledger));
        let listener =
            ScriptedListener::new(ListenerState::OwnedByOther(vec![9999]), Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));

        let error = stop_managed_child_with(&mut child, &process, port, &listener)
            .expect_err("a residual listener must fail the cleanup postcondition");
        assert!(error.contains("remains owned by PIDs"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Terminate("generation-a"),
                LifecycleEvent::RetainedWait("generation-a"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
            ],
            "known-exited child must be reaped before listener postcondition reporting"
        );
    }

    #[test]
    fn up_nonexclusive_listener_stops_retained_child_and_publishes_nothing() {
        for coordinate in 0..5 {
            let pid = 4201 + u32::try_from(coordinate).unwrap();
            let port = 9421 + u16::try_from(coordinate).unwrap();
            let state = match coordinate {
                0 => ListenerState::OwnedByTargetWildcard,
                1 => ListenerState::OwnedByOther(vec![5101]),
                2 => ListenerState::OwnedByOther(vec![5102, 5103]),
                // Listener inspection includes the target PID when the
                // target and a peer share ownership of the selected port.
                3 => ListenerState::OwnedByOther(vec![pid, 5104]),
                4 => ListenerState::Uninspectable("listener table denied".to_string()),
                _ => unreachable!(),
            };
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, "bound-generation", Rc::clone(&ledger))
                .with_inspection(Ok(scripted_facts(state.clone())));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut child =
                ScriptedChild::new(pid, [Ok(None), Ok(None), Ok(None)], Rc::clone(&ledger));
            let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
            let mut health = ScriptedHealth {
                results: VecDeque::from([true]),
                ledger: Rc::clone(&ledger),
            };
            let mut clock = ScriptedClock {
                now: Instant::now(),
                ledger: Rc::clone(&ledger),
            };
            wait_healthy_with(
                &mut child,
                Engine::LlamaServer,
                "127.0.0.1",
                port,
                Duration::from_secs(1),
                &mut health,
                &mut clock,
            )
            .unwrap();

            let publication =
                inspect_bound_child_for_publication(&mut child, &process, port, &listener);
            if publication.is_ok() {
                ledger.borrow_mut().push(LifecycleEvent::Publish);
            }
            let error = publication.expect_err("non-exclusive ownership must block publication");
            assert!(error.contains("no registration may be published"));
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Acquire(pid),
                    LifecycleEvent::ChildTryWait(pid),
                    LifecycleEvent::ClockNow,
                    LifecycleEvent::ChildTryWait(pid),
                    LifecycleEvent::Health(port),
                    LifecycleEvent::ChildTryWait(pid),
                    LifecycleEvent::Inspect("bound-generation", port),
                    LifecycleEvent::Terminate("bound-generation"),
                    LifecycleEvent::RetainedWait("bound-generation"),
                    LifecycleEvent::ChildWait(pid),
                    LifecycleEvent::Listener(pid, port),
                ],
                "case {state:?} must clean only the retained generation and publish nothing"
            );
        }
    }

    #[test]
    fn spawned_child_binding_window_matrix() {
        let pid = 4301;
        let port = 9431;

        // Exit/reuse before a retained object can be acquired: inspecting the
        // original Child proves exit, so no numeric-PID replacement is killed.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let runtime = ScriptedRuntime::new(
            Err("PID now maps to replacement".to_string()),
            Rc::clone(&ledger),
        );
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child =
            ScriptedChild::new(pid, [Ok(Some(ScriptedExit("exited")))], Rc::clone(&ledger));
        bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid)
            ]
        );

        // The retained object can be acquired just before the original Child
        // reports exit. That proves this generation ended and must not signal
        // either the retained object or a numeric-PID replacement.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-exited-at-bind", Rc::clone(&ledger));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(
            pid,
            [Ok(Some(ScriptedExit("exited immediately after bind")))],
            Rc::clone(&ledger),
        );
        let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
        assert!(error.contains("exited before retained-process binding could be confirmed"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid)
            ]
        );

        // Binding failure while the original Child is live stops and reaps it
        // through that still-authoritative Child object.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let runtime =
            ScriptedRuntime::new(Err("pidfd open failed".to_string()), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [Ok(None)], Rc::clone(&ledger));
        let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
        assert!(error.contains("the original child was stopped"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::ChildKill(pid),
                LifecycleEvent::ChildWait(pid),
            ]
        );

        // An inspection error immediately after binding cannot fall back to a
        // PID. Cleanup calls only the retained generation; an unproved wait is
        // returned as a recovery clue.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-at-bind", Rc::clone(&ledger))
            .with_wait(Err(ProcessError::Operation(
                "retained wait unavailable".to_string(),
            )));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(
            pid,
            [Err("child inspection unavailable".to_string())],
            Rc::clone(&ledger),
        );
        let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
        assert!(error.contains("recovery failure for retained PID 4301"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Terminate("generation-at-bind"),
                LifecycleEvent::RetainedWait("generation-at-bind"),
            ]
        );

        // Exit during readiness is cleaned and reaped through the object that
        // was retained before polling began.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "generation-before-poll", Rc::clone(&ledger));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(
            pid,
            [Ok(None), Ok(Some(ScriptedExit("exit during readiness")))],
            Rc::clone(&ledger),
        );
        let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
        let mut health = ScriptedHealth {
            results: VecDeque::new(),
            ledger: Rc::clone(&ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&ledger),
        };
        wait_healthy_with(
            &mut child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            Duration::from_secs(1),
            &mut health,
            &mut clock,
        )
        .unwrap_err();
        stop_managed_child_with(&mut child, &process, port, &listener).unwrap();
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::ClockNow,
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Terminate("generation-before-poll"),
                LifecycleEvent::RetainedWait("generation-before-poll"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
            ]
        );

        // A healthy child becomes publishable only after bind, both liveness
        // checks, HTTP readiness, and exact listener inspection.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "healthy-generation", Rc::clone(&ledger))
            .with_inspection(Ok(scripted_facts(ListenerState::OwnedByTarget)));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [Ok(None), Ok(None), Ok(None)], Rc::clone(&ledger));
        let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&ledger),
        };
        wait_healthy_with(
            &mut child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            Duration::from_secs(1),
            &mut health,
            &mut clock,
        )
        .unwrap();
        inspect_bound_child_for_publication(&mut child, &process, port, &listener).unwrap();
        ledger.borrow_mut().push(LifecycleEvent::Publish);
        assert_eq!(
            ledger.borrow().last(),
            Some(&LifecycleEvent::Publish),
            "publication is the final event after retained-generation validation"
        );
        assert!(!ledger.borrow().iter().any(|event| matches!(
            event,
            LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
        )));
    }

    fn unused_port() -> u16 {
        TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    const LIFECYCLE_HELPER_ENV: &str = "FERRIC_TEST_LIFECYCLE_HELPER";

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    const LIFECYCLE_HELPER_WILDCARD_ENV: &str = "FERRIC_TEST_LIFECYCLE_WILDCARD";

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    static LIFECYCLE_PARENT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    const LIFECYCLE_HELPER_READY_TIMEOUT: Duration = Duration::from_secs(30);

    /// Child-scoped test process used by the cross-workspace lifecycle
    /// regression below. A normal test-harness invocation returns immediately;
    /// only the explicitly spawned child enters the serving loop.
    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn lifecycle_helper_process() {
        if std::env::var(LIFECYCLE_HELPER_ENV).ok().as_deref() != Some("1") {
            return;
        }
        let port = std::env::var("FERRIC_TEST_LIFECYCLE_PORT")
            .expect("helper port")
            .parse::<u16>()
            .expect("numeric helper port");
        let bind_host = if std::env::var(LIFECYCLE_HELPER_WILDCARD_ENV).ok().as_deref() == Some("1")
        {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let listener = TcpListener::bind((bind_host, port)).expect("bind helper listener");
        loop {
            let (mut stream, _) = listener.accept().expect("accept helper request");
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .expect("write helper response");
        }
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    fn spawn_lifecycle_helper(port: u16) -> Child {
        spawn_lifecycle_helper_with_binding(port, false)
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    fn spawn_lifecycle_helper_with_binding(port: u16, wildcard: bool) -> Child {
        Command::new(std::env::current_exe().expect("current test executable"))
            .args([
                "--exact",
                "server::tests::lifecycle_helper_process",
                "--nocapture",
            ])
            .env(LIFECYCLE_HELPER_ENV, "1")
            .env(
                LIFECYCLE_HELPER_WILDCARD_ENV,
                if wildcard { "1" } else { "0" },
            )
            .env("FERRIC_TEST_LIFECYCLE_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn lifecycle helper")
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    fn lifecycle_parent_test_guard() -> std::sync::MutexGuard<'static, ()> {
        LIFECYCLE_PARENT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    fn wait_for_lifecycle_helper(child: &mut Child, port: u16) {
        if let Err(error) = wait_healthy(
            child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            LIFECYCLE_HELPER_READY_TIMEOUT,
        ) {
            let cleanup = stop_child(child);
            panic!(
                "lifecycle helper did not become HTTP-ready: {error}; cleanup result: {cleanup:?}"
            );
        }
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    fn native_listener_matches_or_has_documented_visibility_limit(
        actual: &ListenerState,
        expected: ListenerState,
    ) -> bool {
        if actual == &expected {
            return true;
        }
        #[cfg(target_os = "linux")]
        if let ListenerState::Uninspectable(error) = actual {
            assert!(
                error.contains("listener owner enumeration is incomplete")
                    || error.contains("whose owners are not inspectable"),
                "unexpected Linux listener-inspection failure: {error}"
            );
            return false;
        }
        panic!("expected native listener state {expected:?}, got {actual:?}");
    }

    #[cfg(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    fn is_documented_linux_listener_visibility_error(error: &str) -> bool {
        error.contains("listener owner enumeration is incomplete")
            || error.contains("whose owners are not inspectable")
    }

    fn write_test_runfile(path: &Path, runfile: &ServerRunfile) {
        std::fs::create_dir_all(path.parent().expect("runfile parent")).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(runfile).unwrap()).unwrap();
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn cross_workspace_stale_local_selects_and_stops_only_verified_global() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace_a = root.path().join("workspace-a");
        let workspace_b = root.path().join("workspace-b");
        std::fs::create_dir_all(&workspace_a).unwrap();
        std::fs::create_dir_all(&workspace_b).unwrap();
        let local_a = std::path::absolute(runfile_path(&workspace_a)).unwrap();
        let local_b = std::path::absolute(runfile_path(&workspace_b)).unwrap();
        let global = root.path().join("config").join("server.json");
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(&mut helper, port);

        let helper_pid = helper.id();
        let helper_facts = LiveProcess::acquire_child(&helper)
            .unwrap()
            .inspect(port)
            .unwrap();
        if !native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTarget,
        ) {
            stop_child(&mut helper).expect("clean up visibility-limited helper");
            return;
        }
        let live = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper_pid,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local_b.clone()),
        };
        write_test_runfile(&local_b, &live);
        write_test_runfile(&global, &live);

        // The stale local record names the test runner itself with a deliberately
        // wrong creation token. Old local-first/PID-only teardown would signal
        // this PID. Identity resolution must classify it stale and target only
        // the exact helper retained by the global/origin mirrors.
        let mut stale_identity = LiveProcess::acquire(std::process::id())
            .unwrap()
            .inspect(port)
            .unwrap()
            .identity;
        let alternative = canonical_test_start_token(1);
        stale_identity.start_token = if stale_identity.start_token == alternative {
            canonical_test_start_token(2)
        } else {
            alternative
        };
        let mut stale = live.clone();
        stale.pid = std::process::id();
        stale.process_identity = Some(stale_identity);
        stale.origin_local_runfile = Some(local_a.clone());
        write_test_runfile(&local_a, &stale);

        let discovered = read_runfile_result_impl(&workspace_a, Some(global.clone()))
            .unwrap()
            .expect("read-only consumers resolve the verified global server");
        assert_eq!(discovered.pid, helper_pid);
        assert_eq!(
            status_impl(&workspace_a, Some(global.clone())),
            ExitCode::SUCCESS
        );
        assert_eq!(
            down_impl(&workspace_a, Some(global.clone())),
            ExitCode::SUCCESS
        );
        let _ = helper.wait().expect("reap terminated helper");
        assert!(!local_a.exists(), "stale current-workspace alias cleaned");
        assert!(!local_b.exists(), "selected global origin alias cleaned");
        assert!(!global.exists(), "selected global alias cleaned");
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn wildcard_listener_blocks_teardown_and_preserves_registration() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper_with_binding(port, true);
        wait_for_lifecycle_helper(&mut helper, port);

        let helper_pid = helper.id();
        let helper_facts = LiveProcess::acquire_child(&helper)
            .unwrap()
            .inspect(port)
            .unwrap();
        native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTargetWildcard,
        );
        let record = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper_pid,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &record);
        let original_registration = std::fs::read(&local).unwrap();

        assert_eq!(status_impl(&workspace, None), ExitCode::FAILURE);
        let result = down_impl(&workspace, None);
        let registration_after_down = std::fs::read(&local).unwrap();
        let helper_remained_live = helper.try_wait().unwrap().is_none();
        let _ = helper.kill();
        let _ = helper.wait();

        assert_eq!(result, ExitCode::FAILURE);
        assert_eq!(
            registration_after_down, original_registration,
            "wildcard teardown must have an empty registration-delete ledger"
        );
        assert!(
            helper_remained_live,
            "wildcard teardown must have an empty process-signal ledger"
        );
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn stale_registration_keeps_live_foreign_listener_and_recovery_record() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(&mut helper, port);

        let helper_facts = LiveProcess::acquire_child(&helper)
            .unwrap()
            .inspect(port)
            .unwrap();
        native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTarget,
        );
        let stale = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: u32::MAX,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &stale);

        let result = down_impl(&workspace, None);
        let registration_remained = local.exists();
        let helper_remained_live = helper.try_wait().unwrap().is_none();
        let _ = helper.kill();
        let _ = helper.wait();

        assert_eq!(result, ExitCode::FAILURE);
        assert!(
            registration_remained,
            "an active endpoint must keep its recovery registration"
        );
        assert!(
            helper_remained_live,
            "a listener owned by a foreign PID must never be signalled"
        );
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn live_legacy_registration_cannot_authorize_teardown() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(&mut helper, port);

        let legacy = ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: helper.id(),
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        };
        write_test_runfile(&local, &legacy);

        let result = down_impl(&workspace, None);
        let registration_remained = local.exists();
        let helper_remained_live = helper.try_wait().unwrap().is_none();
        let _ = helper.kill();
        let _ = helper.wait();

        assert_eq!(result, ExitCode::FAILURE);
        assert!(
            registration_remained,
            "blocked legacy record must be retained"
        );
        assert!(
            helper_remained_live,
            "a live schema-1 PID must never be signalled without creation identity"
        );
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn same_creation_with_different_process_metadata_blocks_cleanup_and_signal() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(&mut helper, port);

        let mut identity = LiveProcess::acquire_child(&helper)
            .unwrap()
            .inspect(port)
            .unwrap()
            .identity;
        identity.argv.push("--not-the-observed-command".to_string());
        let record = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper.id(),
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &record);

        let result = down_impl(&workspace, None);
        let registration_remained = local.exists();
        let helper_remained_live = helper.try_wait().unwrap().is_none();
        let _ = helper.kill();
        let _ = helper.wait();

        assert_eq!(result, ExitCode::FAILURE);
        assert!(
            registration_remained,
            "a live same-creation metadata mismatch must retain its recovery coordinate"
        );
        assert!(
            helper_remained_live,
            "a live same-creation metadata mismatch must never be signalled"
        );
    }

    #[test]
    fn legacy_adoption_coordinates_require_closed_engine_and_every_recorded_value() {
        let executable = if cfg!(windows) {
            PathBuf::from(r"C:\tools\llama-server.exe")
        } else {
            PathBuf::from("/tools/llama-server")
        };
        let runfile = ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: 42,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            model: Some("model.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        };
        let identity = ProcessIdentity {
            start_token: canonical_test_start_token(1),
            executable,
            argv: vec![
                "llama-server".to_string(),
                "-m".to_string(),
                "model.gguf".to_string(),
                "-c".to_string(),
                "8192".to_string(),
                "--seed".to_string(),
                "42".to_string(),
                "--parallel".to_string(),
                "1".to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ],
        };
        validate_legacy_process_coordinates(&runfile, &identity).unwrap();

        let mut missing_port = identity.clone();
        missing_port.argv.truncate(missing_port.argv.len() - 2);
        let error = validate_legacy_process_coordinates(&runfile, &missing_port).unwrap_err();
        assert!(error.contains("registered port"));

        let mut wrong_engine = identity;
        wrong_engine.executable = if cfg!(windows) {
            PathBuf::from(r"C:\tools\python.exe")
        } else {
            PathBuf::from("/tools/python")
        };
        let error = validate_legacy_process_coordinates(&runfile, &wrong_engine).unwrap_err();
        assert!(error.contains("closed"));
    }

    #[test]
    fn blocked_local_inventory_prevents_global_autodiscovery() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = runfile_path(&workspace);
        let global = root.path().join("config").join("server.json");
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(&local, b"{not-json").unwrap();
        write_test_runfile(
            &global,
            &ServerRunfile {
                schema_version: 1,
                engine: Engine::LlamaServer,
                pid: 1,
                port: 8080,
                base_url: "http://127.0.0.1:8080/v1".to_string(),
                tailscale: false,
                model: None,
                context_size: None,
                sampling_seed: None,
                parallel_slots: None,
                process_identity: None,
                origin_local_runfile: None,
            },
        );

        let error = read_runfile_result_impl(&workspace, Some(global)).unwrap_err();
        assert!(error.contains("blocked"), "unexpected error: {error}");
        assert!(error.contains(&local.display().to_string()));
    }

    #[test]
    fn durable_tailscale_registration_is_retained_as_a_blocker() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = runfile_path(&workspace);
        write_test_runfile(
            &local,
            &ServerRunfile {
                schema_version: 1,
                engine: Engine::LlamaServer,
                pid: u32::MAX,
                port: 8080,
                base_url: "https://example-host.tailnet-example.ts.net/v1".to_string(),
                tailscale: true,
                model: None,
                context_size: None,
                sampling_seed: None,
                parallel_slots: None,
                process_identity: None,
                origin_local_runfile: None,
            },
        );

        assert_eq!(down_impl(&workspace, None), ExitCode::FAILURE);
        assert!(
            local.exists(),
            "the registration must retain the clue to durable proxy state"
        );
    }

    fn llama_args(model: &Path) -> ServerUpArgs {
        ServerUpArgs {
            engine: Engine::LlamaServer,
            model: Some(model.display().to_string()),
            mmproj: None,
            ctx: 8192,
            port: unused_port(),
            threads: None,
            gpu_layers: Some(0),
            batch_size: None,
            seed: None,
            parallel: None,
            tailscale: false,
        }
    }

    fn serve_one_status(path: &'static str, status: &'static str) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 512];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(
                request.starts_with(&format!("GET {path} HTTP/1.1\r\n")),
                "unexpected request: {request}"
            );
            let body = "{}";
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        (port, handle)
    }

    #[cfg(any(
        windows,
        all(
            target_os = "linux",
            target_endian = "little",
            target_pointer_width = "64",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    #[test]
    fn live_registration_inspection_binds_pid_listener_and_health() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, released) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 512];
            let read = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).starts_with("GET /health "));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
            released.recv_timeout(Duration::from_secs(15)).unwrap();
        });
        let runfile = ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: std::process::id(),
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            model: Some("model.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        };
        let inspected = inspect_registered_server(&runfile);
        if inspected.is_err() {
            // A restricted Linux /proc can reject before the HTTP request.
            // Wake the server thread with a valid probe so this native smoke
            // still exits cleanly while reporting the visibility limitation.
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
                let _ = stream.write_all(
                    format!(
                        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                );
            }
        }
        release.send(()).unwrap();
        server.join().unwrap();
        match inspected {
            Ok(inspected) => {
                assert_eq!(inspected.pid, std::process::id());
                assert_eq!(inspected.listener_owner_pid, std::process::id());
                assert!(!inspected.argv.is_empty());
                assert!(inspected.executable.is_file());
            }
            #[cfg(target_os = "linux")]
            Err(error) => assert!(
                is_documented_linux_listener_visibility_error(&error),
                "unexpected Linux registered-server inspection failure: {error}"
            ),
            #[cfg(windows)]
            Err(error) => panic!("registered-server inspection failed: {error}"),
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
        config.seed = Some(42);
        config.parallel = Some(1);
        let c = command(&config);
        assert!(c.args.windows(2).any(|w| w == ["-t", "4"]));
        assert!(c.args.windows(2).any(|w| w == ["-ngl", "20"]));
        assert!(c.args.windows(2).any(|w| w == ["-b", "512"]));
        assert!(c.args.windows(2).any(|w| w == ["--seed", "42"]));
        assert!(c.args.windows(2).any(|w| w == ["--parallel", "1"]));
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
    fn ollama_preflight_rejects_claiming_llama_sampling_controls() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let mut args = llama_args(&model);
        args.engine = Engine::Ollama;
        args.model = Some("example-model".to_string());
        args.seed = Some(42);

        let error = validate_launch_preconditions(dir.path(), &args, None).unwrap_err();
        assert!(error.contains("supported only by llama-server"), "{error}");

        args.seed = None;
        args.parallel = Some(1);
        let error = validate_launch_preconditions(dir.path(), &args, None).unwrap_err();
        assert!(error.contains("supported only by llama-server"), "{error}");
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
    fn launch_preflight_rejects_existing_local_registration() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let local = runfile_path(dir.path());
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(&local, b"stale or live registration").unwrap();

        let error = validate_launch_preconditions(dir.path(), &llama_args(&model), None)
            .expect_err("an existing local registration must block launch");
        assert!(error.contains("local server registration already exists"));
    }

    #[test]
    fn launch_preflight_rejects_existing_global_registration() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let global = dir.path().join("global").join("server.json");
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        std::fs::write(&global, b"stale or live registration").unwrap();

        let error = validate_launch_preconditions(dir.path(), &llama_args(&model), Some(&global))
            .expect_err("an existing global registration must block launch");
        assert!(error.contains("global server registration already exists"));
    }

    #[test]
    fn launch_preflight_rejects_occupied_target_port() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut args = llama_args(&model);
        args.port = listener.local_addr().unwrap().port();

        let error = validate_launch_preconditions(dir.path(), &args, None)
            .expect_err("an occupied port must block launch");
        assert!(error.contains("is already listening"));
    }

    #[test]
    fn launch_preflight_blocks_unowned_durable_tailscale_state() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let mut args = llama_args(&model);
        args.tailscale = true;

        let error = validate_launch_preconditions(dir.path(), &args, None)
            .expect_err("Tailscale Serve mutation must remain fail-closed");
        assert!(error.contains("temporarily fail-closed"), "{error}");
        assert!(error.contains("ownership"), "{error}");
    }

    #[test]
    fn llama_launch_requires_regular_model_and_mmproj_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.gguf");
        let mut args = llama_args(&missing);
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("model must be a regular file")
        );

        args.model = None;
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("--model is required")
        );

        args.model = Some(dir.path().display().to_string());
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("model must be a regular file")
        );

        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        args.model = Some(model.display().to_string());
        args.mmproj = Some(dir.path().to_path_buf());
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("projector must be a regular file")
        );

        let mmproj = dir.path().join("mmproj.gguf");
        std::fs::write(&mmproj, b"projector").unwrap();
        args.mmproj = Some(mmproj);
        validate_launch_preconditions(dir.path(), &args, None).unwrap();
    }

    #[test]
    fn llama_launch_rejects_zero_context_or_port() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("model.gguf");
        std::fs::write(&model, b"model").unwrap();
        let mut args = llama_args(&model);
        args.ctx = 0;
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("--ctx must be greater than zero")
        );

        args.ctx = 8192;
        args.port = 0;
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("--port must be greater than zero")
        );

        args.port = unused_port();
        args.parallel = Some(0);
        assert!(
            validate_launch_preconditions(dir.path(), &args, None)
                .unwrap_err()
                .contains("--parallel must be greater than zero")
        );
    }

    #[test]
    fn http_probe_requires_engine_path_and_status_200() {
        let (ok_port, ok_server) = serve_one_status("/health", "200 OK");
        assert!(http_status_ok("127.0.0.1", ok_port, "/health"));
        ok_server.join().unwrap();

        let (failed_port, failed_server) = serve_one_status("/v1/models", "503 Loading");
        assert!(!http_status_ok("127.0.0.1", failed_port, "/v1/models"));
        failed_server.join().unwrap();
    }

    #[test]
    fn readiness_fails_when_child_exits_before_http_health() {
        #[cfg(windows)]
        let mut child = Command::new("cmd")
            .args(["/C", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = Command::new("sh")
            .args(["-c", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let error = wait_healthy(
            &mut child,
            Engine::LlamaServer,
            "127.0.0.1",
            unused_port(),
            Duration::from_secs(2),
        )
        .expect_err("an exited child cannot become ready");
        assert!(error.contains("exited before readiness"));
        stop_child(&mut child).unwrap();
    }

    #[test]
    fn readiness_succeeds_only_while_child_is_alive_and_http_is_200() {
        let (port, server) = serve_one_status("/health", "200 OK");
        #[cfg(windows)]
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        wait_healthy(
            &mut child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            Duration::from_secs(2),
        )
        .expect("a live child plus HTTP 200 is ready");
        assert!(matches!(child.try_wait(), Ok(None)));
        stop_child(&mut child).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn promised_origin_expansion_keeps_independent_capture_and_source_aware_blocker() {
        use crate::server_registration::{
            PromisedOriginRegistration, RegistrationBlock, RegistrationCoordinate,
        };

        let root = tempfile::tempdir().unwrap();
        let local_path = root
            .path()
            .join("workspace")
            .join(".ferric")
            .join("server.json");
        let global_path = root.path().join("config").join("server.json");
        let blocked_path = root
            .path()
            .join("blocked-workspace")
            .join(".ferric")
            .join("server.json");
        let executable = root.path().join(if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        });
        let runfile = |pid, token_coordinate| ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            model: None,
            context_size: None,
            sampling_seed: None,
            parallel_slots: None,
            process_identity: Some(ProcessIdentity {
                start_token: canonical_test_start_token(token_coordinate),
                executable: executable.clone(),
                argv: vec![
                    "llama-server".to_string(),
                    "--port".to_string(),
                    "8080".to_string(),
                ],
            }),
            origin_local_runfile: Some(local_path.clone()),
        };
        let direct_runfile = runfile(1, 1);
        let changed_origin_runfile = runfile(2, 2);
        let source = RegistrationCoordinate {
            scope: RegistrationScope::Global,
            path: global_path,
        };
        let inventory = RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: RegistrationScope::Local,
                path: local_path.clone(),
                raw: b"direct-local-snapshot".to_vec(),
                runfile: direct_runfile.clone(),
            })),
            global: None,
            promised_origins: vec![
                PromisedOriginRegistration {
                    source: source.clone(),
                    expected_runfile: direct_runfile.clone(),
                    slot: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                        scope: RegistrationScope::Origin,
                        path: local_path,
                        raw: b"changed-origin-snapshot".to_vec(),
                        runfile: changed_origin_runfile.clone(),
                    })),
                },
                PromisedOriginRegistration {
                    source: source.clone(),
                    expected_runfile: direct_runfile,
                    slot: RegistrationSlot::Blocked {
                        scope: RegistrationScope::Origin,
                        path: blocked_path.clone(),
                        reason: RegistrationBlock::NonRegular,
                    },
                },
            ],
        };

        let (captures, observations) = expand_registration_captures(inventory);
        assert_eq!(captures.len(), 2);
        assert!(captures.iter().any(|capture| {
            capture.scope == RegistrationScope::Local && capture.raw == b"direct-local-snapshot"
        }));
        assert!(captures.iter().any(|capture| {
            capture.scope == RegistrationScope::Origin
                && capture.raw == b"changed-origin-snapshot"
                && capture.runfile == changed_origin_runfile
        }));
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observations[0].label,
            registration_label(RegistrationScope::Origin, &blocked_path)
        );
        assert!(matches!(
            &observations[0].candidate.state,
            CandidateState::Unverifiable { reason, .. }
                if reason.contains("promised by global registration")
                    && reason.contains(&source.path.display().to_string())
        ));
    }

    #[test]
    fn runfile_serde_roundtrip() {
        let rf = ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: 4321,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            model: Some("model.gguf".to_string()),
            context_size: Some(4096),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        };
        let s = serde_json::to_string(&rf).unwrap();
        let back: ServerRunfile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.pid, 4321);
        assert_eq!(back.engine, Engine::LlamaServer);
        assert_eq!(back.base_url, rf.base_url);
        assert_eq!(back.sampling_seed, Some(42));
        assert_eq!(back.parallel_slots, Some(1));
        assert_eq!(back.context_size, Some(4096));
        assert_eq!(back.model.as_deref(), Some("model.gguf"));
    }

    #[test]
    fn old_runfile_sampling_metadata_defaults_to_unknown() {
        let old = r#"{"engine":"llama-server","pid":4321,"port":8080,"base_url":"http://127.0.0.1:8080/v1","tailscale":false}"#;
        let runfile: ServerRunfile = serde_json::from_str(old).unwrap();
        assert!(runfile.model.is_none());
        assert!(runfile.context_size.is_none());
        assert!(runfile.sampling_seed.is_none());
        assert!(runfile.parallel_slots.is_none());
    }

    #[test]
    fn read_runfile_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            read_runfile_result_impl(dir.path(), None)
                .unwrap()
                .is_none()
        );
    }
}
