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
    CapturedRegistration, PublicationAttempt, PublicationStage, PublishError,
    PublishedRegistrations, RegistrationCoordinate, RegistrationInventory, RegistrationScope,
    RegistrationSlot, RemovalError, RemovalFailureKind, RemovalOutcome, ReplacementError,
    ReplacementOutcome, capture_registration_path, inventory_runfiles, publish_mirrored,
    remove_if_unchanged, remove_publication_stage_if_unchanged, replace_if_unchanged,
    validate_runfile,
};
use crate::server_resolution::{
    Candidate, CandidateState, HealthState, Resolution, ResolutionIssue, ResolutionIssueKind,
    resolve,
};
use crate::tailscale_serve::{
    OwnedServeState, ServePathState, TailscaleServeAdapter, TailscaleServeEffects,
    TailscaleServeOwnership, coordinate_from_token, generate_token,
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
    /// Publish one owned, token-scoped Tailscale Serve HTTPS endpoint.
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
    /// Exact endpoint-scoped Tailscale Serve recovery authority. Historical
    /// boolean-only records deserialize without this object and remain
    /// deliberately non-authorizing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_serve: Option<TailscaleServeOwnership>,
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

impl ServerRunfile {
    pub(crate) fn same_lifecycle_authority(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        if let Some(ownership) = &mut left.tailscale_serve {
            ownership.apply_confirmed = false;
        }
        if let Some(ownership) = &mut right.tailscale_serve {
            ownership.apply_confirmed = false;
        }
        left == right
    }
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

#[cfg(test)]
impl SpawnedChild for crate::test_process_containment::ContainedChild {
    type ExitStatus = std::process::ExitStatus;

    fn pid(&self) -> u32 {
        self.child().id()
    }

    fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String> {
        self.try_wait_leader().map_err(|error| error.to_string())
    }

    fn wait(&mut self) -> Result<Self::ExitStatus, String> {
        self.wait_for_exit_and_disarm(Duration::from_secs(5))
            .map_err(|error| error.to_string())
    }

    fn kill(&mut self) -> Result<(), String> {
        self.terminate_leader().map_err(|error| error.to_string())
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

#[cfg(test)]
fn wait_healthy<C: SpawnedChild>(
    child: &mut C,
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

fn listener_release_error(pid: u32, port: u16, listener: &ListenerState) -> Option<String> {
    match listener {
        ListenerState::Absent => None,
        ListenerState::OwnedByTarget | ListenerState::OwnedByTargetWildcard => Some(format!(
            "numeric PID {pid} still owns registered port {port} after retained-process exit"
        )),
        ListenerState::OwnedByOther(owners) => Some(format!(
            "registered port {port} remains owned by PIDs {owners:?} after retained-process exit"
        )),
        ListenerState::Uninspectable(error) => Some(format!(
            "registered port {port} ownership is uninspectable after retained-process exit: {error}"
        )),
    }
}

/// Stop the exact process object retained before readiness and publication.
/// Registration rollback is authorized only after this returns `Ok(())`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RetainedTerminateOutcome {
    Signalled,
    AlreadyExited,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RetainedWaitOutcome {
    Exited,
    TimedOut,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChildReapOutcome {
    Reaped,
    Failed(String),
    NotAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedChildShutdownReport {
    pid: u32,
    port: u16,
    terminate: RetainedTerminateOutcome,
    wait: RetainedWaitOutcome,
    reap: ChildReapOutcome,
    listener: Option<ListenerState>,
}

impl ManagedChildShutdownReport {
    fn exit_proven(&self) -> bool {
        matches!(self.wait, RetainedWaitOutcome::Exited)
    }

    fn cleanup_authorized(&self) -> bool {
        self.exit_proven()
            && self.reap == ChildReapOutcome::Reaped
            && self.listener == Some(ListenerState::Absent)
    }

    fn diagnostics(&self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        if let RetainedTerminateOutcome::Failed(error) = &self.terminate {
            diagnostics.push(format!(
                "terminate retained process object for PID {}: {error}",
                self.pid
            ));
        }
        match &self.wait {
            RetainedWaitOutcome::Exited => {}
            RetainedWaitOutcome::TimedOut => diagnostics.push(format!(
                "retained process object for PID {} did not exit within 10s",
                self.pid
            )),
            RetainedWaitOutcome::Failed(error) => diagnostics.push(format!(
                "wait for retained process object for PID {}: {error}",
                self.pid
            )),
        }
        match &self.reap {
            ChildReapOutcome::Reaped | ChildReapOutcome::NotAttempted => {}
            ChildReapOutcome::Failed(error) => diagnostics.push(error.clone()),
        }
        match &self.listener {
            Some(ListenerState::Absent) | None => {}
            Some(listener) => {
                diagnostics.extend(listener_release_error(self.pid, self.port, listener));
            }
        }
        diagnostics
    }

    fn into_result(self) -> Result<(), String> {
        if self.cleanup_authorized() {
            Ok(())
        } else {
            Err(self.diagnostics().join("; "))
        }
    }
}

fn stop_managed_child_report_with<C, P, L>(
    child: &mut C,
    process: &P,
    port: u16,
    listener: &L,
) -> ManagedChildShutdownReport
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
{
    let pid = process.pid();
    let terminate = match process.terminate() {
        Ok(true) => RetainedTerminateOutcome::Signalled,
        Ok(false) => RetainedTerminateOutcome::AlreadyExited,
        Err(error) => RetainedTerminateOutcome::Failed(error.to_string()),
    };
    let wait = match process.wait(Duration::from_secs(10)) {
        Ok(true) => RetainedWaitOutcome::Exited,
        Ok(false) => RetainedWaitOutcome::TimedOut,
        Err(error) => RetainedWaitOutcome::Failed(error.to_string()),
    };
    if wait != RetainedWaitOutcome::Exited {
        return ManagedChildShutdownReport {
            pid,
            port,
            terminate,
            wait,
            reap: ChildReapOutcome::NotAttempted,
            listener: None,
        };
    }

    // Once retained-handle exit is proven, reaping the original Child is
    // unconditional. Listener inspection follows even when reaping reports an
    // error so the recovery report retains every independently known fact.
    let reap = match child.wait() {
        Ok(_) => ChildReapOutcome::Reaped,
        Err(error) => {
            ChildReapOutcome::Failed(format!("reap exited child PID {}: {error}", child.pid()))
        }
    };
    let listener = listener.listener_state(pid, port);
    ManagedChildShutdownReport {
        pid,
        port,
        terminate,
        wait,
        reap,
        listener: Some(listener),
    }
}

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
    stop_managed_child_report_with(child, process, port, listener).into_result()
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
pub(crate) enum TailscaleRecoverySubject {
    ManagedProcess,
    StaleRegistration,
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
    RecoverOwnedTailscale {
        remote_base_url: String,
        mount_path: String,
        reason: String,
        subject: TailscaleRecoverySubject,
        apply_confirmed: bool,
    },
    ResolveConflict {
        coordinates: Vec<RegistrationCoordinate>,
    },
    RepairUnverifiable {
        coordinates: Vec<RegistrationCoordinate>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TailscaleProxyStatus {
    Active,
    Pending,
    Replaced { observed_target: String },
    Uninspectable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TailscaleStatusReport {
    pub ownership: TailscaleServeOwnership,
    pub status: TailscaleProxyStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerStatusReport {
    pub registrations: Vec<ManagedRegistrationObservation>,
    pub state: ManagedServerState,
    pub tailscale: Option<TailscaleStatusReport>,
    pub tailscale_issue: Option<String>,
    pub next_action: StatusNextAction,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedServerStatus {
    stdout: Vec<String>,
    stderr: Vec<String>,
    success: bool,
}

struct LifecycleDiscovery<P = LiveProcess> {
    managed: ManagedServerDiscovery,
    observations: Vec<LifecycleObservation<P>>,
    resolution: Resolution,
}

struct LifecycleObservation<P = LiveProcess> {
    candidate: Candidate,
    label: String,
    capture: Option<CapturedRegistration>,
    process: Option<P>,
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
                if runfile.tailscale && runfile.tailscale_serve.is_none() {
                    issues.push(ResolutionIssue {
                        coordinates: vec![observation.coordinate.clone()],
                        kind: ResolutionIssueKind::Unverifiable,
                        detail: "registration owns durable Tailscale Serve state".to_string(),
                    });
                }
                if let Some(promised) = &observation.promised
                    && !runfile.same_lifecycle_authority(&promised.expected_runfile)
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
            if same_process_key && !left.same_lifecycle_authority(right) {
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
    if capture.runfile.tailscale && capture.runfile.tailscale_serve.is_none() {
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
            CandidateState::Unverifiable { .. }
                if capture.runfile.tailscale && capture.runfile.tailscale_serve.is_none() =>
            {
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
    fn tailscale_identity(&mut self) -> Result<String, String>;
    fn tailscale_status(&mut self, fqdn: &str) -> Result<(), String>;
}

struct NativeDoctorProbeEffects {
    serve: TailscaleServeAdapter,
}

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

    fn tailscale_identity(&mut self) -> Result<String, String> {
        self.serve
            .self_identity()
            .map(|identity| identity.fqdn)
            .map_err(|error| error.to_string())
    }

    fn tailscale_status(&mut self, fqdn: &str) -> Result<(), String> {
        self.serve
            .probe_status(fqdn)
            .map(drop)
            .map_err(|error| error.to_string())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicationDisposition {
    Ready,
    RolledBack,
    RecoveryHeld,
    RecoveryPartial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicationStageReport {
    scope: RegistrationScope,
    final_path: PathBuf,
    path: PathBuf,
    outcome: DownRegistrationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicationCompletionReport {
    disposition: PublicationDisposition,
    published: Option<PublishedRegistrations>,
    shutdown: Option<ManagedChildShutdownReport>,
    finals: Vec<DownRegistrationReport>,
    stages: Vec<PublicationStageReport>,
    diagnostics: Vec<String>,
    success: bool,
}

trait PublicationCompensationEffects {
    fn replace_final(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError>;

    fn remove_final(
        &mut self,
        captured: &CapturedRegistration,
    ) -> Result<RemovalOutcome, RemovalError>;

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError>;
}

struct NativePublicationCompensationEffects;

impl PublicationCompensationEffects for NativePublicationCompensationEffects {
    fn replace_final(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        replace_if_unchanged(captured, replacement)
    }

    fn remove_final(
        &mut self,
        captured: &CapturedRegistration,
    ) -> Result<RemovalOutcome, RemovalError> {
        remove_if_unchanged(captured)
    }

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError> {
        remove_publication_stage_if_unchanged(stage)
    }
}

fn publication_failure_parts(
    error: PublishError,
) -> (String, Vec<CapturedRegistration>, Vec<PublicationStage>) {
    let rendered = error.to_string();
    let attempt = match error {
        PublishError::Write { attempt, .. }
        | PublishError::Mirror { attempt, .. }
        | PublishError::Durability { attempt, .. } => Some(*attempt),
        PublishError::Invalid { .. } | PublishError::Serialize(_) => None,
    };
    let Some(PublicationAttempt {
        finals,
        stages,
        terminal_phase,
        final_committed,
    }) = attempt
    else {
        return (rendered, Vec::new(), Vec::new());
    };
    (
        format!(
            "{rendered}; terminal persistence phase={terminal_phase:?} terminal-final-committed={final_committed} published-finals={} retained-stages={}",
            finals.len(),
            stages.len()
        ),
        finals,
        stages,
    )
}

fn published_finals(published: &PublishedRegistrations) -> Vec<CapturedRegistration> {
    let mut finals = vec![published.local.clone()];
    finals.extend(published.global.iter().cloned());
    finals
}

fn require_published_journals_unchanged(published: &PublishedRegistrations) -> Result<(), String> {
    for expected in published_finals(published) {
        match capture_registration_path(expected.scope, &expected.path) {
            RegistrationSlot::Captured(current)
                if current.raw == expected.raw && current.runfile == expected.runfile => {}
            RegistrationSlot::Captured(_) => {
                return Err(format!(
                    "{} registration {} changed after write-ahead publication",
                    expected.scope,
                    expected.path.display()
                ));
            }
            RegistrationSlot::Absent { .. } => {
                return Err(format!(
                    "{} registration {} disappeared after write-ahead publication",
                    expected.scope,
                    expected.path.display()
                ));
            }
            RegistrationSlot::Blocked { reason, .. } => {
                return Err(format!(
                    "{} registration {} became non-authorizing after write-ahead publication: {reason}",
                    expected.scope,
                    expected.path.display()
                ));
            }
        }
    }
    Ok(())
}

fn confirm_tailscale_capture_with<F>(
    captured: &CapturedRegistration,
    ownership: &TailscaleServeOwnership,
    replace: &mut F,
) -> Result<CapturedRegistration, String>
where
    F: FnMut(&CapturedRegistration, &[u8]) -> Result<ReplacementOutcome, ReplacementError>,
{
    let Some(captured_ownership) = captured.runfile.tailscale_serve.as_ref() else {
        return Err(format!(
            "{} registration {} does not contain the expected write-ahead Tailscale ownership",
            captured.scope,
            captured.path.display()
        ));
    };
    if !captured_ownership.same_coordinate(ownership) {
        return Err(format!(
            "{} registration {} does not contain the expected write-ahead Tailscale ownership",
            captured.scope,
            captured.path.display()
        ));
    }
    if captured_ownership.apply_confirmed {
        return Ok(captured.clone());
    }
    let mut confirmed_ownership = captured_ownership.clone();
    confirmed_ownership.apply_confirmed = true;
    let mut runfile = captured.runfile.clone();
    runfile.tailscale_serve = Some(confirmed_ownership);
    let raw = serde_json::to_vec_pretty(&runfile).map_err(|error| {
        format!(
            "could not serialize confirmed Tailscale ownership for {} registration {}: {error}",
            captured.scope,
            captured.path.display()
        )
    })?;
    match replace(captured, &raw) {
        Ok(ReplacementOutcome::Replaced) => Ok(CapturedRegistration {
            scope: captured.scope,
            path: captured.path.clone(),
            raw,
            runfile,
        }),
        Ok(ReplacementOutcome::Absent) => Err(format!(
            "{} registration {} disappeared while confirming Tailscale apply",
            captured.scope,
            captured.path.display()
        )),
        Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => Err(format!(
            "{} registration {} changed while confirming Tailscale apply; preserved at {}: {detail}",
            captured.scope,
            captured.path.display(),
            path.display()
        )),
        Err(error) => Err(format!(
            "could not durably confirm Tailscale apply in {} registration {}: {error}",
            captured.scope,
            captured.path.display()
        )),
    }
}

fn refresh_tailscale_capture_with<F>(
    captured: &CapturedRegistration,
    refreshed: &TailscaleServeOwnership,
    replace: &mut F,
) -> Result<CapturedRegistration, String>
where
    F: FnMut(&CapturedRegistration, &[u8]) -> Result<ReplacementOutcome, ReplacementError>,
{
    refreshed.validate().map_err(|error| error.to_string())?;
    if refreshed.apply_confirmed {
        return Err("authoritative pre-apply ownership cannot already be confirmed".to_string());
    }
    let Some(captured_ownership) = captured.runfile.tailscale_serve.as_ref() else {
        return Err(format!(
            "{} registration {} does not contain write-ahead Tailscale ownership",
            captured.scope,
            captured.path.display()
        ));
    };
    if !captured_ownership.same_endpoint_coordinate(refreshed) || captured_ownership.apply_confirmed
    {
        return Err(format!(
            "{} registration {} changed before authoritative Tailscale pre-apply refresh",
            captured.scope,
            captured.path.display()
        ));
    }
    let mut runfile = captured.runfile.clone();
    runfile.tailscale_serve = Some(refreshed.clone());
    let raw = serde_json::to_vec_pretty(&runfile).map_err(|error| {
        format!(
            "could not serialize refreshed Tailscale ownership for {} registration {}: {error}",
            captured.scope,
            captured.path.display()
        )
    })?;
    match replace(captured, &raw) {
        Ok(ReplacementOutcome::Replaced) => Ok(CapturedRegistration {
            scope: captured.scope,
            path: captured.path.clone(),
            raw,
            runfile,
        }),
        Ok(ReplacementOutcome::Absent) => Err(format!(
            "{} registration {} disappeared during authoritative Tailscale pre-apply refresh",
            captured.scope,
            captured.path.display()
        )),
        Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => Err(format!(
            "{} registration {} changed during authoritative Tailscale pre-apply refresh; preserved at {}: {detail}",
            captured.scope,
            captured.path.display(),
            path.display()
        )),
        Err(error) => Err(format!(
            "could not durably refresh Tailscale ownership in {} registration {}: {error}",
            captured.scope,
            captured.path.display()
        )),
    }
}

fn refresh_tailscale_publication<E: PublicationCompensationEffects>(
    published: &mut PublishedRegistrations,
    refreshed: &TailscaleServeOwnership,
    effects: &mut E,
) -> Result<(), String> {
    let mut replace =
        |captured: &CapturedRegistration, raw: &[u8]| effects.replace_final(captured, raw);
    published.local = refresh_tailscale_capture_with(&published.local, refreshed, &mut replace)?;
    if let Some(global) = &mut published.global {
        *global = refresh_tailscale_capture_with(global, refreshed, &mut replace)?;
    }
    Ok(())
}

fn confirm_tailscale_publication<E: PublicationCompensationEffects>(
    published: &mut PublishedRegistrations,
    ownership: &TailscaleServeOwnership,
    effects: &mut E,
) -> Result<(), String> {
    let mut replace =
        |captured: &CapturedRegistration, raw: &[u8]| effects.replace_final(captured, raw);
    published.local = confirm_tailscale_capture_with(&published.local, ownership, &mut replace)?;
    if let Some(global) = &mut published.global {
        *global = confirm_tailscale_capture_with(global, ownership, &mut replace)?;
    }
    Ok(())
}

fn confirm_tailscale_captures_with<F>(
    captures: &mut [CapturedRegistration],
    ownership: &TailscaleServeOwnership,
    mut replace: F,
) -> Result<(), String>
where
    F: FnMut(&CapturedRegistration, &[u8]) -> Result<ReplacementOutcome, ReplacementError>,
{
    validate_mutation_path_aliases(captures)?;
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, capture) in captures.iter().enumerate() {
        let should_confirm = capture
            .runfile
            .tailscale_serve
            .as_ref()
            .is_some_and(|candidate| {
                candidate.same_coordinate(ownership) && !candidate.apply_confirmed
            });
        if !should_confirm {
            continue;
        }
        let key = mutation_path_key(&capture.path);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| mutation_path_key(&captures[group[0]].path) == key)
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    for group in groups {
        let confirmed =
            confirm_tailscale_capture_with(&captures[group[0]], ownership, &mut replace)?;
        for index in group {
            captures[index].raw = confirmed.raw.clone();
            captures[index].runfile = confirmed.runfile.clone();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyCleanupReport {
    resolved: bool,
    off_failed: bool,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyReconcileContext {
    EstablishedOwnership,
    AmbiguousApply,
}

fn tailscale_coordinate_label(ownership: &TailscaleServeOwnership) -> String {
    format!(
        "fqdn={} mount={} remote-base={}",
        ownership.fqdn, ownership.mount_path, ownership.remote_base_url
    )
}

fn reconcile_owned_proxy<S, F>(
    ownership: &TailscaleServeOwnership,
    serve: &S,
    context: ProxyReconcileContext,
    mut confirm_established: F,
) -> ProxyCleanupReport
where
    S: TailscaleServeEffects,
    F: FnMut() -> Result<(), String>,
{
    let (before, before_route_shadow, before_foreground_shadow, before_cleanup_semantics_pinned) =
        match serve.observe_coordinate_for_cleanup(&ownership.fqdn, &ownership.mount_path) {
            Ok(observation) => match observation
                .require_cleanup_identity(ownership)
                .and_then(|()| observation.owned_state(ownership))
            {
                Ok(state) => (
                    state,
                    observation.route_shadow.clone(),
                    observation.foreground_shadows,
                    observation.cleanup_semantics_pinned,
                ),
                Err(error) => {
                    return ProxyCleanupReport {
                        resolved: false,
                        off_failed: false,
                        diagnostics: vec![format!(
                            "could not authorize exact Tailscale Serve cleanup for {}: {error}",
                            tailscale_coordinate_label(ownership)
                        )],
                    };
                }
            },
            Err(error) => {
                return ProxyCleanupReport {
                    resolved: false,
                    off_failed: false,
                    diagnostics: vec![format!(
                        "could not inspect the owned Tailscale Serve coordinate {} before cleanup: {error}",
                        tailscale_coordinate_label(ownership)
                    )],
                };
            }
        };
    match before {
        OwnedServeState::Absent if !before_cleanup_semantics_pinned => ProxyCleanupReport {
            resolved: false,
            off_failed: false,
            diagnostics: vec![format!(
                "Tailscale Serve exact handler is absent for {}, but the daemon version has unknown routing semantics; ownership journals are retained because endpoint absence cannot be proved",
                tailscale_coordinate_label(ownership),
            )],
        },
        OwnedServeState::Absent if before_route_shadow.is_some() || before_foreground_shadow => {
            let residual = before_route_shadow.unwrap_or_else(|| {
                format!(
                    "foreground state on {}:{}",
                    ownership.fqdn, ownership.https_port
                )
            });
            ProxyCleanupReport {
                resolved: false,
                off_failed: false,
                diagnostics: vec![format!(
                    "Tailscale Serve exact proxy is absent for {}, but effective route {residual} still shadows the journaled mount; ownership journals are retained for manual resolution",
                    tailscale_coordinate_label(ownership),
                )],
            }
        }
        OwnedServeState::Absent => match context {
            ProxyReconcileContext::EstablishedOwnership => ProxyCleanupReport {
                resolved: true,
                off_failed: false,
                diagnostics: Vec::new(),
            },
            ProxyReconcileContext::AmbiguousApply => ProxyCleanupReport {
                resolved: false,
                off_failed: false,
                diagnostics: vec![format!(
                    "endpoint-scoped Tailscale Serve apply returned ambiguously for {}; an immediate absent observation is not a completion barrier, so the ownership journals are retained for later reconciliation",
                    tailscale_coordinate_label(ownership)
                )],
            },
        },
        OwnedServeState::Replaced { observed_target } => ProxyCleanupReport {
            resolved: false,
            off_failed: false,
            diagnostics: vec![format!(
                "owned Tailscale Serve coordinate {} now targets {observed_target}; endpoint mutation was refused",
                tailscale_coordinate_label(ownership)
            )],
        },
        OwnedServeState::Exact => {
            if let Err(error) = confirm_established() {
                return ProxyCleanupReport {
                    resolved: false,
                    off_failed: false,
                    diagnostics: vec![format!(
                        "the Tailscale Serve apply is exact, but durable confirmation failed before scoped cleanup: {error}"
                    )],
                };
            }
            let off_error = serve.off(ownership).err();
            let off_failed = off_error.is_some();
            let after =
                serve.observe_coordinate_for_cleanup(&ownership.fqdn, &ownership.mount_path);
            let resolved = after.as_ref().is_ok_and(|observation| {
                observation.cleanup_semantics_pinned
                    && observation.route_shadow.is_none()
                    && !observation.foreground_shadows
                    && observation
                        .require_cleanup_identity(ownership)
                        .and_then(|()| observation.owned_state(ownership))
                        .ok()
                        == Some(OwnedServeState::Absent)
            });
            let mut diagnostics = Vec::new();
            if let Some(error) = off_error {
                diagnostics.push(format!(
                    "endpoint-scoped Tailscale Serve off for {} reported failure: {error}",
                    tailscale_coordinate_label(ownership)
                ));
            }
            match after {
                Ok(observation) => match observation
                    .require_cleanup_identity(ownership)
                    .and_then(|()| observation.owned_state(ownership))
                {
                    Ok(OwnedServeState::Absent) if !observation.cleanup_semantics_pinned => {
                        diagnostics.push(format!(
                            "Tailscale Serve exact handler is absent for {}, but the daemon version has unknown routing semantics; ownership journals are retained because endpoint absence cannot be proved",
                            tailscale_coordinate_label(ownership),
                        ));
                    }
                    Ok(OwnedServeState::Absent)
                        if observation.route_shadow.is_some()
                            || observation.foreground_shadows =>
                    {
                        let residual = observation.route_shadow.clone().unwrap_or_else(|| {
                            format!(
                                "foreground state on {}:{}",
                                ownership.fqdn, ownership.https_port
                            )
                        });
                        diagnostics.push(format!(
                            "Tailscale Serve exact proxy is absent for {}, but effective route {residual} still shadows the journaled mount; ownership journals are retained",
                            tailscale_coordinate_label(ownership),
                        ));
                    }
                    Ok(OwnedServeState::Absent) => {}
                    Ok(OwnedServeState::Exact) => diagnostics.push(format!(
                        "owned Tailscale Serve coordinate {} remains active after scoped cleanup",
                        tailscale_coordinate_label(ownership)
                    )),
                    Ok(OwnedServeState::Replaced { observed_target }) => diagnostics.push(
                        format!(
                            "Tailscale Serve coordinate {} changed to {observed_target} during scoped cleanup",
                            tailscale_coordinate_label(ownership)
                        ),
                    ),
                    Err(error) => diagnostics.push(format!(
                        "post-cleanup Tailscale Serve observation for {} was non-authorizing: {error}",
                        tailscale_coordinate_label(ownership)
                    )),
                },
                Err(error) => diagnostics.push(format!(
                    "could not prove Tailscale Serve absence for {} after scoped cleanup: {error}",
                    tailscale_coordinate_label(ownership)
                )),
            }
            ProxyCleanupReport {
                resolved,
                off_failed,
                diagnostics,
            }
        }
    }
}

fn require_post_publication_authority<P: RetainedProcessHandle>(
    process: &P,
    port: u16,
    published: &PublishedRegistrations,
) -> Result<(), String> {
    let runfile = &published.local.runfile;
    if runfile.pid != process.pid() || runfile.port != port {
        return Err(format!(
            "published registration coordinates PID {} port {}, but the retained child is PID {} port {port}",
            runfile.pid,
            runfile.port,
            process.pid()
        ));
    }
    let expected = runfile.process_identity.as_ref().ok_or_else(|| {
        "published schema-v2 registration has no process identity for final authority validation"
            .to_string()
    })?;
    let current = process.inspect(port).map_err(|error| {
        format!(
            "could not revalidate retained process/listener authority after registration publication: {error}"
        )
    })?;
    if &current.identity != expected {
        return Err(
            "retained process identity changed during registration publication".to_string(),
        );
    }
    if current.listener != ListenerState::OwnedByTarget {
        return Err(format!(
            "retained process no longer exclusively owns the expected loopback listener after registration publication: {:?}",
            current.listener
        ));
    }
    Ok(())
}

fn publication_removal_outcome(
    result: Result<RemovalOutcome, RemovalError>,
) -> DownRegistrationOutcome {
    match result {
        Ok(RemovalOutcome::Removed) => DownRegistrationOutcome::Removed,
        Ok(RemovalOutcome::Absent) => DownRegistrationOutcome::AlreadyAbsent,
        Ok(RemovalOutcome::ReplacementPreserved { path, detail }) => {
            DownRegistrationOutcome::ReplacementPreserved { path, detail }
        }
        Err(error) => match error.kind {
            RemovalFailureKind::Restore => DownRegistrationOutcome::RestoreFailed {
                preserved_at: error.preserved_at,
                detail: error.detail,
            },
            RemovalFailureKind::Remove => DownRegistrationOutcome::RemovalFailed {
                preserved_at: error.preserved_at,
                detail: error.detail,
            },
            RemovalFailureKind::Other => DownRegistrationOutcome::CleanupFailed {
                preserved_at: error.preserved_at,
                detail: error.detail,
            },
        },
    }
}

fn publication_cleanup_complete(outcome: &DownRegistrationOutcome) -> bool {
    matches!(
        outcome,
        DownRegistrationOutcome::Removed | DownRegistrationOutcome::AlreadyAbsent
    )
}

fn publication_cleanup_alias_error(
    finals: &[CapturedRegistration],
    stages: &[PublicationStage],
) -> Option<String> {
    let mut paths = finals
        .iter()
        .map(|capture| ("published final", capture.path.as_path()))
        .chain(
            stages
                .iter()
                .map(|stage| ("publication stage", stage.path.as_path())),
        )
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.1.cmp(right.1));
    for (index, (left_kind, left)) in paths.iter().enumerate() {
        for (right_kind, right) in &paths[index + 1..] {
            if left == right {
                return Some(format!(
                    "{left_kind} and {right_kind} share one cleanup path {}; no cleanup was attempted",
                    left.display()
                ));
            }
            if distinct_mutation_paths_may_alias(left, right) {
                return Some(format!(
                    "{left_kind} {} and {right_kind} {} may alias; no cleanup was attempted",
                    left.display(),
                    right.display()
                ));
            }
        }
    }
    None
}

fn held_publication_final(capture: &CapturedRegistration, detail: &str) -> DownRegistrationReport {
    DownRegistrationReport {
        coordinate: RegistrationCoordinate {
            scope: capture.scope,
            path: capture.path.clone(),
        },
        outcome: DownRegistrationOutcome::Held {
            detail: detail.to_string(),
        },
    }
}

fn held_publication_stage(stage: &PublicationStage, detail: &str) -> PublicationStageReport {
    PublicationStageReport {
        scope: stage.scope,
        final_path: stage.final_path.clone(),
        path: stage.path.clone(),
        outcome: DownRegistrationOutcome::Held {
            detail: detail.to_string(),
        },
    }
}

fn complete_publication_with<C, P, L, E>(
    child: &mut C,
    process: &P,
    port: u16,
    publication: Result<PublishedRegistrations, PublishError>,
    listener: &L,
    effects: &mut E,
) -> PublicationCompletionReport
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
    E: PublicationCompensationEffects,
{
    let (failure, finals, stages) = match publication {
        Ok(published) => match child.try_wait() {
            Ok(None) => match require_post_publication_authority(process, port, &published) {
                Ok(()) => {
                    return PublicationCompletionReport {
                        disposition: PublicationDisposition::Ready,
                        published: Some(published),
                        shutdown: None,
                        finals: Vec::new(),
                        stages: Vec::new(),
                        diagnostics: Vec::new(),
                        success: true,
                    };
                }
                Err(error) => (error, published_finals(&published), Vec::new()),
            },
            Ok(Some(status)) => (
                format!("engine process exited during registration publication ({status})"),
                published_finals(&published),
                Vec::new(),
            ),
            Err(error) => (
                format!("could not confirm the engine child after publication: {error}"),
                published_finals(&published),
                Vec::new(),
            ),
        },
        Err(error) => publication_failure_parts(error),
    };

    let shutdown = stop_managed_child_report_with(child, process, port, listener);
    let mut diagnostics = vec![failure];
    diagnostics.extend(shutdown.diagnostics());
    if !shutdown.cleanup_authorized() {
        let detail = "published recovery state is held because exact child exit, reap, and listener release were not all proven";
        return PublicationCompletionReport {
            disposition: PublicationDisposition::RecoveryHeld,
            published: None,
            shutdown: Some(shutdown),
            finals: finals
                .iter()
                .map(|capture| held_publication_final(capture, detail))
                .collect(),
            stages: stages
                .iter()
                .map(|stage| held_publication_stage(stage, detail))
                .collect(),
            diagnostics,
            success: false,
        };
    }

    if let Some(error) = publication_cleanup_alias_error(&finals, &stages) {
        diagnostics.push(error.clone());
        return PublicationCompletionReport {
            disposition: PublicationDisposition::RecoveryPartial,
            published: None,
            shutdown: Some(shutdown),
            finals: finals
                .iter()
                .map(|capture| held_publication_final(capture, &error))
                .collect(),
            stages: stages
                .iter()
                .map(|stage| held_publication_stage(stage, &error))
                .collect(),
            diagnostics,
            success: false,
        };
    }

    let final_reports = finals
        .iter()
        .map(|capture| DownRegistrationReport {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            outcome: publication_removal_outcome(effects.remove_final(capture)),
        })
        .collect::<Vec<_>>();
    let stage_reports = stages
        .iter()
        .map(|stage| PublicationStageReport {
            scope: stage.scope,
            final_path: stage.final_path.clone(),
            path: stage.path.clone(),
            outcome: publication_removal_outcome(effects.remove_stage(stage)),
        })
        .collect::<Vec<_>>();
    let complete = final_reports
        .iter()
        .all(|report| publication_cleanup_complete(&report.outcome))
        && stage_reports
            .iter()
            .all(|report| publication_cleanup_complete(&report.outcome));
    if !complete {
        diagnostics.push(
            "publication compensation was partial; every preserved path is reported".to_string(),
        );
    }
    PublicationCompletionReport {
        disposition: if complete {
            PublicationDisposition::RolledBack
        } else {
            PublicationDisposition::RecoveryPartial
        },
        published: None,
        shutdown: Some(shutdown),
        finals: final_reports,
        stages: stage_reports,
        diagnostics,
        success: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn compensate_owned_publication_with<C, P, L, E, S>(
    child: &mut C,
    process: &P,
    port: u16,
    mut published: PublishedRegistrations,
    listener: &L,
    effects: &mut E,
    serve: &S,
    ownership: &TailscaleServeOwnership,
    failure: String,
    serve_context: Option<ProxyReconcileContext>,
) -> PublicationCompletionReport
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
    E: PublicationCompensationEffects,
    S: TailscaleServeEffects,
{
    let proxy = match serve_context {
        Some(context) => reconcile_owned_proxy(ownership, serve, context, || {
            confirm_tailscale_publication(&mut published, ownership, effects)
        }),
        None => ProxyCleanupReport {
            resolved: true,
            off_failed: false,
            diagnostics: vec![
                "failure preceded every Tailscale Serve mutation; no external cleanup was needed"
                    .to_string(),
            ],
        },
    };
    // External cleanup is attempted first, but its ambiguity never erases the
    // independently retained authority over the exact spawned child.
    let shutdown = stop_managed_child_report_with(child, process, port, listener);
    let finals = published_finals(&published);
    let mut diagnostics = vec![failure];
    diagnostics.extend(proxy.diagnostics);
    diagnostics.extend(shutdown.diagnostics());

    if !proxy.resolved || !shutdown.cleanup_authorized() {
        let detail = if !proxy.resolved && !shutdown.cleanup_authorized() {
            "ownership journals are held because neither exact Serve absence nor exact child quiescence was proven"
        } else if !proxy.resolved {
            "ownership journals are held because exact Tailscale Serve absence was not proven"
        } else {
            "ownership journals are held because exact child exit, reap, and listener release were not all proven"
        };
        return PublicationCompletionReport {
            disposition: PublicationDisposition::RecoveryHeld,
            published: None,
            shutdown: Some(shutdown),
            finals: finals
                .iter()
                .map(|capture| held_publication_final(capture, detail))
                .collect(),
            stages: Vec::new(),
            diagnostics,
            success: false,
        };
    }

    if let Some(error) = publication_cleanup_alias_error(&finals, &[]) {
        diagnostics.push(error.clone());
        return PublicationCompletionReport {
            disposition: PublicationDisposition::RecoveryPartial,
            published: None,
            shutdown: Some(shutdown),
            finals: finals
                .iter()
                .map(|capture| held_publication_final(capture, &error))
                .collect(),
            stages: Vec::new(),
            diagnostics,
            success: false,
        };
    }

    let final_reports = finals
        .iter()
        .map(|capture| DownRegistrationReport {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            outcome: publication_removal_outcome(effects.remove_final(capture)),
        })
        .collect::<Vec<_>>();
    let complete = final_reports
        .iter()
        .all(|report| publication_cleanup_complete(&report.outcome));
    if !complete {
        diagnostics.push(
            "owned launch compensation was partial; every preserved journal is reported"
                .to_string(),
        );
    }
    PublicationCompletionReport {
        disposition: if complete {
            PublicationDisposition::RolledBack
        } else {
            PublicationDisposition::RecoveryPartial
        },
        published: None,
        shutdown: Some(shutdown),
        finals: final_reports,
        stages: Vec::new(),
        diagnostics,
        success: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_tailscale_publication_with<C, P, L, E, S>(
    child: &mut C,
    process: &P,
    port: u16,
    publication: Result<PublishedRegistrations, PublishError>,
    listener: &L,
    effects: &mut E,
    serve: &S,
    ownership: &TailscaleServeOwnership,
) -> PublicationCompletionReport
where
    C: SpawnedChild,
    P: RetainedProcessHandle,
    L: ListenerInspector,
    E: PublicationCompensationEffects,
    S: TailscaleServeEffects,
{
    let mut published = match publication {
        Ok(published) => published,
        Err(error) => {
            return complete_publication_with(child, process, port, Err(error), listener, effects);
        }
    };

    let before_mutation = (|| {
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                return Err(format!(
                    "engine process exited after write-ahead publication ({status})"
                ));
            }
            Err(error) => {
                return Err(format!(
                    "could not confirm the engine child after write-ahead publication: {error}"
                ));
            }
        }
        require_post_publication_authority(process, port, &published)?;
        require_published_journals_unchanged(&published)?;
        let observation = serve
            .observe_coordinate(&ownership.fqdn, &ownership.mount_path)
            .map_err(|error| {
                format!("could not recheck the owned Serve path before mutation: {error}")
            })?;
        match observation
            .owned_state(ownership)
            .map_err(|error| error.to_string())?
        {
            OwnedServeState::Absent => {}
            OwnedServeState::Exact => {
                return Err(
                    "the generated Tailscale Serve coordinate became active before Ferric applied it"
                        .to_string(),
                );
            }
            OwnedServeState::Replaced { observed_target } => {
                return Err(format!(
                    "the generated Tailscale Serve coordinate was concurrently claimed by {observed_target}"
                ));
            }
        }
        // Close the registration replacement window immediately before the
        // externally visible mutation.
        require_published_journals_unchanged(&published)?;
        ownership
            .refreshed_for_preapply(&observation)
            .map_err(|error| error.to_string())
    })();
    let refreshed_ownership = match before_mutation {
        Ok(refreshed) => refreshed,
        Err(failure) => {
            return compensate_owned_publication_with(
                child, process, port, published, listener, effects, serve, ownership, failure, None,
            );
        }
    };

    if let Err(failure) =
        refresh_tailscale_publication(&mut published, &refreshed_ownership, effects)
            .and_then(|()| require_post_publication_authority(process, port, &published))
            .and_then(|()| require_published_journals_unchanged(&published))
    {
        return compensate_owned_publication_with(
            child,
            process,
            port,
            published,
            listener,
            effects,
            serve,
            &refreshed_ownership,
            format!("could not freeze authoritative pre-apply ownership: {failure}"),
            None,
        );
    }

    if let Err(error) = serve.apply(&refreshed_ownership) {
        let reconcile = error
            .may_have_mutated()
            .then_some(ProxyReconcileContext::AmbiguousApply);
        return compensate_owned_publication_with(
            child,
            process,
            port,
            published,
            listener,
            effects,
            serve,
            &refreshed_ownership,
            format!("endpoint-scoped Tailscale Serve apply was not confirmed: {error}"),
            reconcile,
        );
    }

    let after_mutation = (|| -> Result<(), String> {
        let observation = serve
            .observe_coordinate(&refreshed_ownership.fqdn, &refreshed_ownership.mount_path)
            .map_err(|error| format!("could not verify the applied Serve path: {error}"))?;
        observation
            .require_publication_identity(&refreshed_ownership)
            .map_err(|error| error.to_string())?;
        match observation
            .owned_state(&refreshed_ownership)
            .map_err(|error| error.to_string())?
        {
            OwnedServeState::Exact => {
                if let Some(hazard) = observation.publication_hazard() {
                    return Err(format!(
                        "the applied Tailscale Serve path is not safely reachable: {hazard}"
                    ));
                }
            }
            OwnedServeState::Absent => {
                return Err(
                    "Tailscale Serve apply returned without publishing the owned path".to_string(),
                );
            }
            OwnedServeState::Replaced { observed_target } => {
                return Err(format!(
                    "the applied Tailscale Serve path targets {observed_target}, not the recorded loopback target"
                ));
            }
        }
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                return Err(format!(
                    "engine process exited during Tailscale Serve publication ({status})"
                ));
            }
            Err(error) => {
                return Err(format!(
                    "could not confirm the engine child after Tailscale Serve publication: {error}"
                ));
            }
        }
        require_post_publication_authority(process, port, &published)?;
        require_published_journals_unchanged(&published)
    })();
    if let Err(failure) = after_mutation {
        return compensate_owned_publication_with(
            child,
            process,
            port,
            published,
            listener,
            effects,
            serve,
            &refreshed_ownership,
            failure,
            Some(ProxyReconcileContext::EstablishedOwnership),
        );
    }

    if let Err(failure) =
        confirm_tailscale_publication(&mut published, &refreshed_ownership, effects)
    {
        return compensate_owned_publication_with(
            child,
            process,
            port,
            published,
            listener,
            effects,
            serve,
            &refreshed_ownership,
            failure,
            Some(ProxyReconcileContext::EstablishedOwnership),
        );
    }
    if let Err(failure) = require_post_publication_authority(process, port, &published)
        .and_then(|()| require_published_journals_unchanged(&published))
    {
        return compensate_owned_publication_with(
            child,
            process,
            port,
            published,
            listener,
            effects,
            serve,
            &refreshed_ownership,
            format!("confirmed Tailscale ownership lost final authority: {failure}"),
            Some(ProxyReconcileContext::EstablishedOwnership),
        );
    }

    PublicationCompletionReport {
        disposition: PublicationDisposition::Ready,
        published: Some(published),
        shutdown: None,
        finals: Vec::new(),
        stages: Vec::new(),
        diagnostics: Vec::new(),
        success: true,
    }
}

fn render_publication_cleanup(subject: &str, outcome: &DownRegistrationOutcome) -> String {
    match outcome {
        DownRegistrationOutcome::Removed => format!("[removed] {subject}"),
        DownRegistrationOutcome::AlreadyAbsent => format!("[already-absent] {subject}"),
        DownRegistrationOutcome::ReplacementPreserved { path, detail } => format!(
            "[replacement-preserved] {subject} preserved-at={} detail={detail}",
            path.display()
        ),
        DownRegistrationOutcome::RestoreFailed {
            preserved_at,
            detail,
        } => format!(
            "[restore-failed] {subject} holding={} detail={detail}",
            preserved_at
                .as_ref()
                .map_or_else(|| "none".to_string(), |path| path.display().to_string())
        ),
        DownRegistrationOutcome::RemovalFailed {
            preserved_at,
            detail,
        } => format!(
            "[removal-failed] {subject} holding={} detail={detail}",
            preserved_at
                .as_ref()
                .map_or_else(|| "none".to_string(), |path| path.display().to_string())
        ),
        DownRegistrationOutcome::CleanupFailed {
            preserved_at,
            detail,
        } => format!(
            "[cleanup-failed] {subject} holding={} detail={detail}",
            preserved_at
                .as_ref()
                .map_or_else(|| "none".to_string(), |path| path.display().to_string())
        ),
        DownRegistrationOutcome::Held { detail } => {
            format!("[held] {subject} detail={detail}")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedPublicationReport {
    stdout: Vec<String>,
    stderr: Vec<String>,
    success: bool,
}

fn render_publication_report(report: &PublicationCompletionReport) -> RenderedPublicationReport {
    let mut stdout = report
        .finals
        .iter()
        .map(|final_report| {
            render_publication_cleanup(
                &format!(
                    "{} published registration {}",
                    final_report.coordinate.scope,
                    final_report.coordinate.path.display()
                ),
                &final_report.outcome,
            )
        })
        .chain(report.stages.iter().map(|stage_report| {
            render_publication_cleanup(
                &format!(
                    "{} publication stage {} for final {}",
                    stage_report.scope,
                    stage_report.path.display(),
                    stage_report.final_path.display()
                ),
                &stage_report.outcome,
            )
        }))
        .collect::<Vec<_>>();
    if let Some(shutdown) = &report.shutdown {
        stdout.push(format!(
            "[shutdown] pid={} terminate={:?} wait={:?} reap={:?} listener={:?}",
            shutdown.pid, shutdown.terminate, shutdown.wait, shutdown.reap, shutdown.listener
        ));
    }
    stdout.push(match report.disposition {
        PublicationDisposition::Ready => "[state] ready".to_string(),
        PublicationDisposition::RolledBack => {
            "[state] publication failed; rollback complete".to_string()
        }
        PublicationDisposition::RecoveryHeld => {
            "[state] publication failed; recovery state held".to_string()
        }
        PublicationDisposition::RecoveryPartial => {
            "[state] publication failed; rollback partial".to_string()
        }
    });
    RenderedPublicationReport {
        stdout,
        stderr: report
            .diagnostics
            .iter()
            .map(|diagnostic| format!("[diagnostic] {diagnostic}"))
            .collect(),
        success: report.success,
    }
}

fn emit_publication_report(report: PublicationCompletionReport) -> Option<PublishedRegistrations> {
    let rendered = render_publication_report(&report);
    if !rendered.success {
        for line in rendered.stdout {
            println!("{line}");
        }
        for line in rendered.stderr {
            eprintln!("{line}");
        }
    }
    report.published
}

#[derive(Debug)]
enum LaunchOrchestrationError {
    Spawn(String),
    Bind {
        pid: u32,
        detail: String,
    },
    Readiness {
        base_url: String,
        detail: String,
        shutdown: Option<String>,
    },
    Inspect(String),
    LocalPath {
        detail: String,
        shutdown: Option<String>,
    },
    Publication(Box<PublicationCompletionReport>),
}

#[derive(Debug)]
struct LaunchOrchestrationSuccess {
    pid: u32,
    base_url: String,
    remote_base_url: Option<String>,
    published: PublishedRegistrations,
}

fn render_launch_success(launched: &LaunchOrchestrationSuccess) -> Vec<String> {
    let mut lines = vec![format!(
        "server ready: {} (pid {})",
        launched.base_url, launched.pid
    )];
    if let Some(remote_base_url) = &launched.remote_base_url {
        lines.push(format!("Tailscale Serve endpoint ready: {remote_base_url}"));
    }
    lines.push(format!(
        "registered locally at {}",
        launched.published.local.path.display()
    ));
    if let Some(global) = &launched.published.global {
        lines.push(format!("registered globally at {}", global.path.display()));
    }
    lines
}

/// One authority-preserving launch sequence shared by production `up` and
/// deterministic composition tests. The spawned child is bound to its exact
/// retained process object before any readiness probe, and publication is
/// reachable only after readiness plus final process/listener inspection.
#[allow(clippy::too_many_arguments)]
fn orchestrate_launch_with<C, R, L, H, K, E, S, F, T>(
    workspace: &Path,
    global_path: Option<&Path>,
    cfg: &ServerConfig,
    tailscale_serve: Option<TailscaleServeOwnership>,
    serve: &T,
    spawn: S,
    runtime: &R,
    listener: &L,
    health: &mut H,
    clock: &mut K,
    publish: F,
    compensation: &mut E,
) -> Result<LaunchOrchestrationSuccess, LaunchOrchestrationError>
where
    C: SpawnedChild,
    R: SpawnedProcessRuntime<C>,
    L: ListenerInspector,
    H: HealthProbe,
    K: LifecycleClock,
    E: PublicationCompensationEffects,
    T: TailscaleServeEffects,
    S: FnOnce() -> Result<C, String>,
    F: FnOnce(&Path, Option<&Path>, &ServerRunfile) -> Result<PublishedRegistrations, PublishError>,
{
    if cfg.tailscale != tailscale_serve.is_some() {
        return Err(LaunchOrchestrationError::Inspect(
            "Tailscale launch mode and prepared ownership metadata disagree".to_string(),
        ));
    }
    if let Some(ownership) = &tailscale_serve {
        ownership
            .validate_for_port(cfg.port)
            .map_err(|error| LaunchOrchestrationError::Inspect(error.to_string()))?;
    }
    let mut child = spawn().map_err(LaunchOrchestrationError::Spawn)?;
    let pid = child.pid();
    let process = bind_spawned_child(&mut child, runtime, cfg.port, listener)
        .map_err(|detail| LaunchOrchestrationError::Bind { pid, detail })?;
    debug_assert_eq!(process.pid(), pid);

    let base_url = cfg.base_url();
    if let Err(detail) = wait_healthy_with(
        &mut child,
        cfg.engine,
        &cfg.host,
        cfg.port,
        Duration::from_secs(300),
        health,
        clock,
    ) {
        let shutdown = stop_managed_child_with(&mut child, &process, cfg.port, listener).err();
        return Err(LaunchOrchestrationError::Readiness {
            base_url,
            detail,
            shutdown,
        });
    }

    let process_facts =
        inspect_bound_child_for_publication(&mut child, &process, cfg.port, listener)
            .map_err(LaunchOrchestrationError::Inspect)?;
    let local_path = match std::path::absolute(runfile_path(workspace)) {
        Ok(path) => path,
        Err(error) => {
            let shutdown = stop_managed_child_with(&mut child, &process, cfg.port, listener).err();
            return Err(LaunchOrchestrationError::LocalPath {
                detail: error.to_string(),
                shutdown,
            });
        }
    };
    let runfile = ServerRunfile {
        schema_version: RUNFILE_SCHEMA_V2,
        engine: cfg.engine,
        pid,
        port: cfg.port,
        base_url: base_url.clone(),
        tailscale: cfg.tailscale,
        tailscale_serve: tailscale_serve.clone(),
        model: cfg.model.clone(),
        context_size: (cfg.engine == Engine::LlamaServer).then_some(cfg.ctx),
        sampling_seed: cfg.seed,
        parallel_slots: cfg.parallel,
        process_identity: Some(process_facts.identity),
        origin_local_runfile: Some(local_path),
    };
    let publication = publish(workspace, global_path, &runfile);
    let completion = if let Some(ownership) = &tailscale_serve {
        complete_tailscale_publication_with(
            &mut child,
            &process,
            cfg.port,
            publication,
            listener,
            compensation,
            serve,
            ownership,
        )
    } else {
        complete_publication_with(
            &mut child,
            &process,
            cfg.port,
            publication,
            listener,
            compensation,
        )
    };
    if completion.success {
        Ok(LaunchOrchestrationSuccess {
            pid,
            base_url,
            remote_base_url: tailscale_serve
                .as_ref()
                .map(|ownership| ownership.remote_base_url.clone()),
            published: completion
                .published
                .expect("successful publication completion retains published registrations"),
        })
    } else {
        Err(LaunchOrchestrationError::Publication(Box::new(completion)))
    }
}

fn prepare_tailscale_ownership_with<S, G>(
    port: u16,
    serve: &S,
    generate: G,
) -> Result<TailscaleServeOwnership, String>
where
    S: TailscaleServeEffects,
    G: FnOnce() -> Result<String, String>,
{
    // Draw entropy before even a read-only Tailscale LocalAPI operation so an
    // RNG failure is a literal zero-effect precondition failure.
    let token = generate()?;
    let identity = serve.self_identity().map_err(|error| error.to_string())?;
    let coordinate =
        coordinate_from_token(port, &identity, token).map_err(|error| error.to_string())?;
    let observation = serve
        .observe_coordinate(&coordinate.fqdn, &coordinate.mount_path)
        .map_err(|error| format!("could not inspect the proposed Tailscale Serve path: {error}"))?;
    match observation.path_state {
        ServePathState::Absent => coordinate
            .into_ownership(&observation)
            .map_err(|error| error.to_string()),
        ServePathState::Proxy { target } => Err(format!(
            "the generated Tailscale Serve path is already claimed by {target}; no engine or Serve mutation was attempted"
        )),
    }
}

fn with_prepared_tailscale_launch<S, G, L, T>(
    enabled: bool,
    port: u16,
    serve: &S,
    generate: G,
    launch: L,
) -> Result<T, String>
where
    S: TailscaleServeEffects,
    G: FnOnce() -> Result<String, String>,
    L: FnOnce(Option<TailscaleServeOwnership>) -> T,
{
    let ownership = if enabled {
        Some(prepare_tailscale_ownership_with(port, serve, generate)?)
    } else {
        None
    };
    Ok(launch(ownership))
}

fn up(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    let global_path = global_runfile_path();
    if let Err(error) = validate_launch_preconditions(workspace, args, global_path.as_deref()) {
        eprintln!("server launch preflight failed: {error}");
        return ExitCode::FAILURE;
    }

    let cfg = config_from(args);
    let serve = TailscaleServeAdapter::native();
    let launch = command(&cfg);
    let launched = match with_prepared_tailscale_launch(
        cfg.tailscale,
        cfg.port,
        &serve,
        || generate_token().map_err(|error| error.to_string()),
        |tailscale_serve| {
            let mut proc = Command::new(&launch.program);
            proc.args(&launch.args);
            for (k, v) in &launch.env {
                proc.env(k, v);
            }
            println!("Launching {} on {} ...", launch.program, cfg.base_url());
            let mut health = NativeHealthProbe;
            let mut clock = SystemLifecycleClock;
            let mut compensation = NativePublicationCompensationEffects;
            orchestrate_launch_with(
                workspace,
                global_path.as_deref(),
                &cfg,
                tailscale_serve,
                &serve,
                || proc.spawn().map_err(|error| error.to_string()),
                &NativeSpawnedProcessRuntime,
                &NativeListenerInspector,
                &mut health,
                &mut clock,
                publish_mirrored,
                &mut compensation,
            )
        },
    ) {
        Ok(launched) => launched,
        Err(error) => {
            eprintln!("Tailscale Serve launch preparation failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let launched = match launched {
        Ok(launched) => launched,
        Err(LaunchOrchestrationError::Spawn(error)) => {
            eprintln!(
                "could not start `{}`: {error}\n(is it installed and on PATH?)",
                launch.program
            );
            return ExitCode::FAILURE;
        }
        Err(LaunchOrchestrationError::Bind { pid, detail }) => {
            eprintln!(
                "could not establish exact lifecycle control for spawned PID {pid}: {detail}"
            );
            return ExitCode::FAILURE;
        }
        Err(LaunchOrchestrationError::Readiness {
            base_url,
            detail,
            shutdown,
        }) => {
            eprintln!("server did not become HTTP-healthy at {base_url}: {detail}");
            if let Some(stop_error) = shutdown {
                eprintln!("could not confirm exact child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
        Err(LaunchOrchestrationError::Inspect(detail)) => {
            eprintln!("server launch was rejected before publication: {detail}");
            return ExitCode::FAILURE;
        }
        Err(LaunchOrchestrationError::LocalPath { detail, shutdown }) => {
            eprintln!("could not resolve the local registration path: {detail}");
            if let Some(stop_error) = shutdown {
                eprintln!("could not confirm exact child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
        Err(LaunchOrchestrationError::Publication(report)) => {
            let published = emit_publication_report(*report);
            debug_assert!(published.is_none());
            return ExitCode::FAILURE;
        }
    };

    for line in render_launch_success(&launched) {
        println!("{line}");
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

fn require_exact_argv_coordinate(
    argv: &[String],
    flags: &[&str],
    expected: &str,
    coordinate: &str,
) -> Result<(), String> {
    let mut occurrences = Vec::new();
    for (index, argument) in argv.iter().enumerate() {
        if flags.iter().any(|flag| argument == flag) {
            let Some(value) = argv.get(index + 1) else {
                return Err(format!(
                    "observed argv ends after `{argument}` for the expected {coordinate}"
                ));
            };
            occurrences.push((argument.as_str(), value.as_str()));
        }
        for flag in flags.iter().filter(|flag| flag.starts_with("--")) {
            let prefix = format!("{flag}=");
            if let Some(value) = argument.strip_prefix(&prefix) {
                occurrences.push((*flag, value));
            }
        }
    }
    if occurrences.is_empty() {
        return Err(format!(
            "observed argv does not contain the expected {coordinate} pair `{}` `{expected}`",
            flags.join("` or `")
        ));
    }
    if let Some((flag, value)) = occurrences.iter().find(|(_, value)| *value != expected) {
        return Err(format!(
            "observed argv has conflicting {coordinate} pair `{flag} {value}`; expected `{expected}`"
        ));
    }
    Ok(())
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
            require_exact_argv_coordinate(
                &identity.argv,
                &["--host"],
                "127.0.0.1",
                "loopback host",
            )?;
            require_exact_argv_coordinate(
                &identity.argv,
                &["--port"],
                &runfile.port.to_string(),
                "registered port",
            )?;
            if let Some(model) = &runfile.model {
                require_exact_argv_coordinate(
                    &identity.argv,
                    &["-m", "--model"],
                    model,
                    "recorded model",
                )?;
            }
            if let Some(context) = runfile.context_size {
                require_exact_argv_coordinate(
                    &identity.argv,
                    &["-c", "--ctx-size"],
                    &context.to_string(),
                    "recorded context size",
                )?;
            }
            if let Some(seed) = runfile.sampling_seed {
                require_exact_argv_coordinate(
                    &identity.argv,
                    &["--seed"],
                    &seed.to_string(),
                    "recorded sampling seed",
                )?;
            }
            if let Some(parallel) = runfile.parallel_slots {
                require_exact_argv_coordinate(
                    &identity.argv,
                    &["--parallel"],
                    &parallel.to_string(),
                    "recorded parallel slot count",
                )?;
            }
        }
        Engine::Ollama => {
            if identity.argv.len() != 2 || identity.argv.get(1).map(String::as_str) != Some("serve")
            {
                return Err(
                    "observed Ollama argv is not the closed `ollama serve` command shape"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdoptionAliasTransition {
    Held {
        detail: String,
    },
    Adopted,
    Absent,
    ReplacementPreserved {
        path: PathBuf,
        detail: String,
    },
    ReplaceFailed {
        preserved_at: Option<PathBuf>,
        detail: String,
        replacement_committed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdoptionRollbackOutcome {
    LegacyRestored,
    Absent,
    ReplacementPreserved {
        path: PathBuf,
        detail: String,
    },
    Failed {
        preserved_at: Option<PathBuf>,
        detail: String,
        replacement_committed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdoptionAliasReport {
    coordinate: RegistrationCoordinate,
    transition: AdoptionAliasTransition,
    rollback: Option<AdoptionRollbackOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdoptionDisposition {
    Blocked,
    Adopted,
    Failed,
    RolledBack,
    RecoveryPartial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdoptionReport {
    disposition: AdoptionDisposition,
    pid: u32,
    identity_validated: bool,
    listener_validated: bool,
    final_generation_revalidated: bool,
    registrations: Vec<AdoptionAliasReport>,
    diagnostics: Vec<String>,
    success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedAdoptionReport {
    stdout: Vec<String>,
    stderr: Vec<String>,
    success: bool,
}

trait AdoptionEffects {
    fn replace(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError>;
}

struct NativeAdoptionEffects;

impl AdoptionEffects for NativeAdoptionEffects {
    fn replace(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        replace_if_unchanged(captured, replacement)
    }
}

fn held_adoption_reports(
    captures: &[CapturedRegistration],
    detail: &str,
) -> Vec<AdoptionAliasReport> {
    captures
        .iter()
        .map(|capture| AdoptionAliasReport {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            transition: AdoptionAliasTransition::Held {
                detail: detail.to_string(),
            },
            rollback: None,
        })
        .collect()
}

fn blocked_adoption_report(
    pid: u32,
    captures: &[CapturedRegistration],
    diagnostic: String,
) -> AdoptionReport {
    AdoptionReport {
        disposition: AdoptionDisposition::Blocked,
        pid,
        identity_validated: false,
        listener_validated: false,
        final_generation_revalidated: false,
        registrations: held_adoption_reports(captures, &diagnostic),
        diagnostics: vec![diagnostic],
        success: false,
    }
}

fn validate_legacy_adoption_inputs(
    captures: &[CapturedRegistration],
    requested_pid: u32,
) -> Result<(ServerRunfile, PathBuf), String> {
    if requested_pid == 0 {
        return Err("adoption requires a nonzero --pid".to_string());
    }
    let Some(reference) = captures.first() else {
        return Err("no server registration exists".to_string());
    };
    let Some(origin) = captures
        .iter()
        .find(|capture| capture.scope == RegistrationScope::Local)
        .map(|capture| capture.path.clone())
    else {
        return Err(
            "the originating local schema-1 registration is not present in this workspace"
                .to_string(),
        );
    };
    if captures
        .iter()
        .any(|capture| capture.runfile.schema_version != 1)
    {
        return Err("every selected registration must use legacy schema 1".to_string());
    }
    if captures
        .iter()
        .any(|capture| capture.runfile != reference.runfile)
    {
        return Err("local/global legacy registrations disagree".to_string());
    }
    validate_mutation_path_aliases(captures)?;
    if reference.runfile.pid != requested_pid {
        return Err(format!(
            "--pid {requested_pid} does not match registered PID {}",
            reference.runfile.pid
        ));
    }
    if reference.runfile.tailscale {
        return Err(
            "tailscale=true owns external Serve state that Ferric cannot yet compare-and-replace safely"
                .to_string(),
        );
    }
    let expected_base_url = format!("http://127.0.0.1:{}/v1", reference.runfile.port);
    if reference.runfile.port == 0 || reference.runfile.base_url != expected_base_url {
        return Err(format!(
            "legacy endpoint must be exactly {expected_base_url} with a nonzero port"
        ));
    }
    Ok((reference.runfile.clone(), origin))
}

fn adoption_mutation_groups(captures: &[CapturedRegistration]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, capture) in captures.iter().enumerate() {
        let key = mutation_path_key(&capture.path);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| mutation_path_key(&captures[group[0]].path) == key)
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups
}

struct AppliedAdoption {
    legacy: CapturedRegistration,
    adopted: CapturedRegistration,
    report_indices: Vec<usize>,
}

fn set_adoption_transition(
    reports: &mut [AdoptionAliasReport],
    indices: &[usize],
    transition: AdoptionAliasTransition,
) {
    for index in indices {
        reports[*index].transition = transition.clone();
    }
}

fn rollback_adoption<E: AdoptionEffects>(
    replacements: &[AppliedAdoption],
    reports: &mut [AdoptionAliasReport],
    effects: &mut E,
) -> (bool, Vec<String>) {
    let mut complete = true;
    let mut diagnostics = Vec::new();
    for replacement in replacements.iter().rev() {
        let outcome = match effects.replace(&replacement.adopted, &replacement.legacy.raw) {
            Ok(ReplacementOutcome::Replaced) => AdoptionRollbackOutcome::LegacyRestored,
            Ok(ReplacementOutcome::Absent) => {
                complete = false;
                diagnostics.push(format!(
                    "rollback could not restore absent registration {}",
                    replacement.legacy.path.display()
                ));
                AdoptionRollbackOutcome::Absent
            }
            Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => {
                complete = false;
                diagnostics.push(format!(
                    "rollback preserved a concurrent replacement for {} at {}: {detail}",
                    replacement.legacy.path.display(),
                    path.display()
                ));
                AdoptionRollbackOutcome::ReplacementPreserved { path, detail }
            }
            Err(error) => {
                complete = false;
                diagnostics.push(format!("adoption rollback incomplete: {error}"));
                AdoptionRollbackOutcome::Failed {
                    preserved_at: error.preserved_at,
                    detail: error.detail,
                    replacement_committed: error.replacement_committed,
                }
            }
        };
        for index in &replacement.report_indices {
            reports[*index].rollback = Some(outcome.clone());
        }
    }
    (complete, diagnostics)
}

fn failed_adoption_after_replacement<E: AdoptionEffects>(
    pid: u32,
    mut reports: Vec<AdoptionAliasReport>,
    replacements: &[AppliedAdoption],
    effects: &mut E,
    diagnostic: String,
    final_generation_revalidated: bool,
) -> AdoptionReport {
    let had_replacements = !replacements.is_empty();
    let (rollback_complete, rollback_diagnostics) =
        rollback_adoption(replacements, &mut reports, effects);
    let every_alias_recovered = reports.iter().all(|registration| {
        matches!(
            registration.transition,
            AdoptionAliasTransition::Held { .. }
        ) || registration.rollback == Some(AdoptionRollbackOutcome::LegacyRestored)
    });
    let mut diagnostics = vec![diagnostic];
    diagnostics.extend(rollback_diagnostics);
    AdoptionReport {
        disposition: if !had_replacements {
            AdoptionDisposition::Failed
        } else if rollback_complete && every_alias_recovered {
            AdoptionDisposition::RolledBack
        } else {
            AdoptionDisposition::RecoveryPartial
        },
        pid,
        identity_validated: true,
        listener_validated: true,
        final_generation_revalidated,
        registrations: reports,
        diagnostics,
        success: false,
    }
}

fn execute_legacy_adoption<R, E>(
    captures: Vec<CapturedRegistration>,
    requested_pid: u32,
    runtime: &R,
    effects: &mut E,
) -> AdoptionReport
where
    R: ProcessRuntime,
    E: AdoptionEffects,
{
    let (reference, origin) = match validate_legacy_adoption_inputs(&captures, requested_pid) {
        Ok(validated) => validated,
        Err(error) => return blocked_adoption_report(requested_pid, &captures, error),
    };
    let process = match runtime.acquire(requested_pid) {
        Ok(process) => process,
        Err(error) => {
            return blocked_adoption_report(
                requested_pid,
                &captures,
                format!("could not acquire exact process handle: {error}"),
            );
        }
    };
    if process.pid() != requested_pid {
        return blocked_adoption_report(
            requested_pid,
            &captures,
            format!(
                "retained process handle names PID {}, expected {requested_pid}",
                process.pid()
            ),
        );
    }
    let facts = match process.inspect(reference.port) {
        Ok(facts) => facts,
        Err(error) => {
            return blocked_adoption_report(
                requested_pid,
                &captures,
                format!("could not inspect exact process/listener facts: {error}"),
            );
        }
    };
    if let Err(error) = validate_legacy_process_coordinates(&reference, &facts.identity) {
        return blocked_adoption_report(requested_pid, &captures, error);
    }
    if facts.listener != ListenerState::OwnedByTarget {
        let mut report = blocked_adoption_report(
            requested_pid,
            &captures,
            format!(
                "registered endpoint is not exclusively owned on IPv4 loopback by PID {requested_pid}: {:?}",
                facts.listener
            ),
        );
        report.identity_validated = true;
        return report;
    }
    match process.wait(Duration::ZERO) {
        Ok(false) => {}
        Ok(true) => {
            let mut report = blocked_adoption_report(
                requested_pid,
                &captures,
                "registered process exited during validation".to_string(),
            );
            report.identity_validated = true;
            report.listener_validated = true;
            return report;
        }
        Err(error) => {
            let mut report = blocked_adoption_report(
                requested_pid,
                &captures,
                format!("could not confirm retained process liveness: {error}"),
            );
            report.identity_validated = true;
            report.listener_validated = true;
            return report;
        }
    }

    let mut adopted_runfile = reference.clone();
    adopted_runfile.schema_version = RUNFILE_SCHEMA_V2;
    adopted_runfile.process_identity = Some(facts.identity.clone());
    adopted_runfile.origin_local_runfile = Some(origin);
    for capture in &captures {
        if let Err(error) = validate_runfile(capture.scope, &capture.path, &adopted_runfile) {
            let mut report = blocked_adoption_report(
                requested_pid,
                &captures,
                format!(
                    "schema-v2 replacement for {} is invalid: {error}",
                    capture.path.display()
                ),
            );
            report.identity_validated = true;
            report.listener_validated = true;
            return report;
        }
    }
    let replacement_raw = match serde_json::to_vec_pretty(&adopted_runfile) {
        Ok(raw) => raw,
        Err(error) => {
            let mut report = blocked_adoption_report(
                requested_pid,
                &captures,
                format!("could not serialize schema-v2 registration: {error}"),
            );
            report.identity_validated = true;
            report.listener_validated = true;
            return report;
        }
    };

    let mut reports = held_adoption_reports(&captures, "adoption not attempted");
    let mut replacements = Vec::new();
    for indices in adoption_mutation_groups(&captures) {
        let legacy = captures[indices[0]].clone();
        let adopted_capture = CapturedRegistration {
            scope: legacy.scope,
            path: legacy.path.clone(),
            raw: replacement_raw.clone(),
            runfile: adopted_runfile.clone(),
        };
        match effects.replace(&legacy, &replacement_raw) {
            Ok(ReplacementOutcome::Replaced) => {
                set_adoption_transition(&mut reports, &indices, AdoptionAliasTransition::Adopted);
                replacements.push(AppliedAdoption {
                    legacy,
                    adopted: adopted_capture,
                    report_indices: indices,
                });
            }
            Ok(ReplacementOutcome::Absent) => {
                set_adoption_transition(&mut reports, &indices, AdoptionAliasTransition::Absent);
                return failed_adoption_after_replacement(
                    requested_pid,
                    reports,
                    &replacements,
                    effects,
                    format!(
                        "adoption stopped because {} disappeared",
                        legacy.path.display()
                    ),
                    false,
                );
            }
            Ok(ReplacementOutcome::ReplacementPreserved { path, detail }) => {
                set_adoption_transition(
                    &mut reports,
                    &indices,
                    AdoptionAliasTransition::ReplacementPreserved {
                        path: path.clone(),
                        detail: detail.clone(),
                    },
                );
                return failed_adoption_after_replacement(
                    requested_pid,
                    reports,
                    &replacements,
                    effects,
                    format!(
                        "adoption stopped because {} changed; replacement preserved at {}: {detail}",
                        legacy.path.display(),
                        path.display()
                    ),
                    false,
                );
            }
            Err(error) => {
                set_adoption_transition(
                    &mut reports,
                    &indices,
                    AdoptionAliasTransition::ReplaceFailed {
                        preserved_at: error.preserved_at.clone(),
                        detail: error.detail.clone(),
                        replacement_committed: error.replacement_committed,
                    },
                );
                if error.replacement_committed {
                    replacements.push(AppliedAdoption {
                        legacy,
                        adopted: adopted_capture,
                        report_indices: indices,
                    });
                }
                return failed_adoption_after_replacement(
                    requested_pid,
                    reports,
                    &replacements,
                    effects,
                    format!("adoption replacement failed: {error}"),
                    false,
                );
            }
        }
    }

    let final_revalidation = process.inspect(reference.port);
    let still_exact = final_revalidation.as_ref().is_ok_and(|current| {
        current.identity == facts.identity && current.listener == ListenerState::OwnedByTarget
    });
    if !still_exact {
        let detail = match final_revalidation {
            Ok(current) => format!(
                "retained process changed before completion: identity-match={} listener={:?}",
                current.identity == facts.identity,
                current.listener
            ),
            Err(error) => format!("retained process final inspection failed: {error}"),
        };
        return failed_adoption_after_replacement(
            requested_pid,
            reports,
            &replacements,
            effects,
            detail,
            false,
        );
    }

    AdoptionReport {
        disposition: AdoptionDisposition::Adopted,
        pid: requested_pid,
        identity_validated: true,
        listener_validated: true,
        final_generation_revalidated: true,
        registrations: reports,
        diagnostics: Vec::new(),
        success: true,
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyAdoptionTestSummary {
    pub success: bool,
    pub identity_validated: bool,
    pub listener_validated: bool,
    pub final_generation_revalidated: bool,
    pub replacement_preserved_at: Option<PathBuf>,
}

/// Narrow test seam for cross-process adoption races. The caller supplies the
/// retained-process runtime and owns its inspect/signal ledger; production
/// conditional replacement remains in `NativeAdoptionEffects` so the race
/// crosses the same filesystem CAS boundary as the CLI.
#[cfg(test)]
pub(crate) fn execute_legacy_adoption_for_test<R: ProcessRuntime>(
    captures: Vec<CapturedRegistration>,
    pid: u32,
    runtime: &R,
) -> LegacyAdoptionTestSummary {
    let mut effects = NativeAdoptionEffects;
    let report = execute_legacy_adoption(captures, pid, runtime, &mut effects);
    let replacement_preserved_at =
        report
            .registrations
            .iter()
            .find_map(|registration| match &registration.transition {
                AdoptionAliasTransition::ReplacementPreserved { path, .. } => Some(path.clone()),
                AdoptionAliasTransition::Held { .. }
                | AdoptionAliasTransition::Adopted
                | AdoptionAliasTransition::Absent
                | AdoptionAliasTransition::ReplaceFailed { .. } => None,
            });
    LegacyAdoptionTestSummary {
        success: report.success,
        identity_validated: report.identity_validated,
        listener_validated: report.listener_validated,
        final_generation_revalidated: report.final_generation_revalidated,
        replacement_preserved_at,
    }
}

fn render_adoption_report(report: &AdoptionReport) -> RenderedAdoptionReport {
    let mut stdout = report
        .registrations
        .iter()
        .map(|registration| {
            let transition = match &registration.transition {
                AdoptionAliasTransition::Held { detail } => format!("held detail={detail}"),
                AdoptionAliasTransition::Adopted => "adopted".to_string(),
                AdoptionAliasTransition::Absent => "absent".to_string(),
                AdoptionAliasTransition::ReplacementPreserved { path, detail } => format!(
                    "replacement-preserved holding={} detail={detail}",
                    path.display()
                ),
                AdoptionAliasTransition::ReplaceFailed {
                    preserved_at,
                    detail,
                    replacement_committed,
                } => format!(
                    "replace-failed holding={} committed={replacement_committed} detail={detail}",
                    preserved_at
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |path| path.display().to_string())
                ),
            };
            let rollback = match &registration.rollback {
                None => "rollback=not-required".to_string(),
                Some(AdoptionRollbackOutcome::LegacyRestored) => {
                    "rollback=legacy-restored".to_string()
                }
                Some(AdoptionRollbackOutcome::Absent) => "rollback=absent".to_string(),
                Some(AdoptionRollbackOutcome::ReplacementPreserved { path, detail }) => format!(
                    "rollback=replacement-preserved holding={} detail={detail}",
                    path.display()
                ),
                Some(AdoptionRollbackOutcome::Failed {
                    preserved_at,
                    detail,
                    replacement_committed,
                }) => format!(
                    "rollback=failed holding={} committed={replacement_committed} detail={detail}",
                    preserved_at
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |path| path.display().to_string())
                ),
            };
            format!(
                "[{transition}] {} registration {} {rollback}",
                registration.coordinate.scope,
                registration.coordinate.path.display()
            )
        })
        .collect::<Vec<_>>();
    stdout.push(match report.disposition {
        AdoptionDisposition::Blocked => {
            "[state] adoption blocked; legacy registrations kept".to_string()
        }
        AdoptionDisposition::Adopted => format!(
            "[state] adopted live schema-1 server PID {} into schema 2 without signalling it",
            report.pid
        ),
        AdoptionDisposition::Failed => {
            "[state] adoption failed before any committed replacement".to_string()
        }
        AdoptionDisposition::RolledBack => {
            "[state] adoption failed; legacy registrations restored".to_string()
        }
        AdoptionDisposition::RecoveryPartial => {
            "[state] adoption failed; recovery partial".to_string()
        }
    });
    RenderedAdoptionReport {
        stdout,
        stderr: report
            .diagnostics
            .iter()
            .map(|diagnostic| format!("[diagnostic] {diagnostic}"))
            .collect(),
        success: report.success,
    }
}

fn emit_adoption_report(report: &AdoptionReport) -> ExitCode {
    let rendered = render_adoption_report(report);
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

fn adopt(workspace: &Path, args: &ServerAdoptArgs) -> ExitCode {
    adopt_impl(workspace, global_runfile_path(), args)
}

fn adopt_impl(workspace: &Path, global_path: Option<PathBuf>, args: &ServerAdoptArgs) -> ExitCode {
    if args.pid == 0 {
        return emit_adoption_report(&blocked_adoption_report(
            args.pid,
            &[],
            "adoption requires a nonzero --pid".to_string(),
        ));
    }
    let inventory = inventory_runfiles(workspace, global_path);
    let (captures, blocked) = expand_registration_captures(inventory);
    if !blocked.is_empty() {
        let mut report = blocked_adoption_report(
            args.pid,
            &captures,
            "registration inventory is blocked".to_string(),
        );
        for observation in blocked {
            let reason = match observation.candidate.state {
                CandidateState::Unverifiable { reason, .. }
                | CandidateState::Stale { reason, .. } => reason,
                CandidateState::Verified { .. } => "unexpected verified observation".to_string(),
            };
            report
                .diagnostics
                .push(format!("{}: {reason}", observation.label));
            report.registrations.push(AdoptionAliasReport {
                coordinate: observation.candidate.coordinate,
                transition: AdoptionAliasTransition::Held { detail: reason },
                rollback: None,
            });
        }
        return emit_adoption_report(&report);
    }
    let runtime = NativeProcessRuntime;
    let mut effects = NativeAdoptionEffects;
    emit_adoption_report(&execute_legacy_adoption(
        captures,
        args.pid,
        &runtime,
        &mut effects,
    ))
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
                    && discovery.observations.iter().any(|observation| {
                        observation.coordinate.scope == RegistrationScope::Local
                            && matches!(
                                &observation.state,
                                ManagedRegistrationState::Captured {
                                    runfile: candidate,
                                    runtime: RuntimeObservation::LegacyLive { pid: candidate_pid },
                                    ..
                                } if candidate_pid == pid && candidate.as_ref() == *runfile
                            )
                    })
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
    let (ownership, tailscale_issue) = match unique_tailscale_ownership_from_managed(discovery) {
        Ok(ownership) => (ownership, None),
        Err(error) => (None, Some(error)),
    };
    let mut next_action = status_next_action(discovery);
    if tailscale_issue.is_some()
        && matches!(
            discovery.state,
            ManagedServerState::Ready(_)
                | ManagedServerState::Degraded { .. }
                | ManagedServerState::StaleOnly { .. }
        )
    {
        let mut coordinates = discovery
            .observations
            .iter()
            .filter_map(|observation| match &observation.state {
                ManagedRegistrationState::Captured { runfile, .. } if runfile.tailscale => {
                    Some(observation.coordinate.clone())
                }
                ManagedRegistrationState::Absent
                | ManagedRegistrationState::Blocked { .. }
                | ManagedRegistrationState::Captured { .. } => None,
            })
            .collect::<Vec<_>>();
        coordinates.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.scope.to_string().cmp(&right.scope.to_string()))
        });
        coordinates.dedup();
        next_action = StatusNextAction::ResolveConflict { coordinates };
    }
    ServerStatusReport {
        registrations: discovery.observations.clone(),
        state: discovery.state.clone(),
        tailscale: ownership.clone().map(|ownership| TailscaleStatusReport {
            ownership,
            status: TailscaleProxyStatus::Uninspectable {
                reason: "owned Tailscale Serve coordinate was not observed".to_string(),
            },
        }),
        tailscale_issue: tailscale_issue.clone(),
        next_action,
        success: tailscale_issue.is_none()
            && ownership.is_none()
            && matches!(discovery.state, ManagedServerState::Ready(_)),
    }
}

fn unique_tailscale_ownership_from_managed(
    discovery: &ManagedServerDiscovery,
) -> Result<Option<TailscaleServeOwnership>, String> {
    let mut ownership: Option<TailscaleServeOwnership> = None;
    for observation in &discovery.observations {
        let ManagedRegistrationState::Captured { runfile, .. } = &observation.state else {
            continue;
        };
        if !runfile.tailscale {
            continue;
        }
        let candidate = runfile.tailscale_serve.as_ref().ok_or_else(|| {
            "legacy Tailscale registration has no endpoint-scoped ownership metadata".to_string()
        })?;
        if let Some(existing) = &mut ownership {
            if !existing.same_coordinate(candidate) {
                return Err(
                    "captured registrations disagree about Tailscale Serve ownership".to_string(),
                );
            }
            // Confirmation is monotonic and is written only after observing
            // the exact applied path. One durable confirmed mirror is enough
            // positive evidence to recover a crash between mirror updates.
            existing.apply_confirmed |= candidate.apply_confirmed;
        } else {
            ownership = Some(candidate.clone());
        }
    }
    Ok(ownership)
}

fn status_report_with_tailscale<S: TailscaleServeEffects>(
    discovery: &ManagedServerDiscovery,
    serve: &S,
) -> ServerStatusReport {
    let mut report = status_report(discovery);
    if !matches!(
        discovery.state,
        ManagedServerState::Ready(_)
            | ManagedServerState::Degraded { .. }
            | ManagedServerState::StaleOnly { .. }
    ) {
        return report;
    }
    if report.tailscale_issue.is_some() {
        return report;
    }
    let ownership = match report.tailscale.as_ref() {
        Some(tailscale) => tailscale.ownership.clone(),
        None => return report,
    };
    let status = match serve.observe_coordinate_for_cleanup(&ownership.fqdn, &ownership.mount_path)
    {
        Err(error) => TailscaleProxyStatus::Uninspectable {
            reason: error.to_string(),
        },
        Ok(observation) if !observation.cleanup_semantics_pinned => {
            TailscaleProxyStatus::Uninspectable {
                reason: "the Tailscale daemon has newer routing semantics; a cleanup-only observation cannot prove the owned endpoint is active"
                    .to_string(),
            }
        }
        Ok(observation) => match observation.require_publication_identity(&ownership) {
            Err(error) => TailscaleProxyStatus::Uninspectable {
                reason: error.to_string(),
            },
            Ok(()) => match observation.owned_state(&ownership) {
                Ok(OwnedServeState::Exact) => {
                    if let Some(reason) = observation.publication_hazard() {
                        TailscaleProxyStatus::Uninspectable { reason }
                    } else {
                        TailscaleProxyStatus::Active
                    }
                }
                Ok(OwnedServeState::Absent) => {
                    if let Some(path) = &observation.route_shadow {
                        TailscaleProxyStatus::Uninspectable {
                            reason: format!(
                                "Web handler {path} overrides owned path {}",
                                observation.mount_path
                            ),
                        }
                    } else if observation.foreground_shadows {
                        TailscaleProxyStatus::Uninspectable {
                            reason: format!(
                                "foreground Serve state shadows {}:{}",
                                observation.fqdn, observation.https_port
                            ),
                        }
                    } else {
                        TailscaleProxyStatus::Pending
                    }
                }
                Ok(OwnedServeState::Replaced { observed_target }) => {
                    TailscaleProxyStatus::Replaced { observed_target }
                }
                Err(error) => TailscaleProxyStatus::Uninspectable {
                    reason: error.to_string(),
                },
            },
        },
    };
    report.success = matches!(report.state, ManagedServerState::Ready(_))
        && status == TailscaleProxyStatus::Active;
    if status != TailscaleProxyStatus::Active {
        let reason = match &status {
            TailscaleProxyStatus::Pending => {
                "owned endpoint is absent or launch-pending".to_string()
            }
            TailscaleProxyStatus::Replaced { observed_target } => {
                format!("owned coordinate was replaced by target {observed_target}")
            }
            TailscaleProxyStatus::Uninspectable { reason } => {
                format!("owned coordinate is uninspectable: {reason}")
            }
            TailscaleProxyStatus::Active => unreachable!(),
        };
        let subject = match &report.state {
            ManagedServerState::Ready(_) => Some(TailscaleRecoverySubject::ManagedProcess),
            ManagedServerState::Degraded { server, .. } if server.listener.permits_teardown() => {
                Some(TailscaleRecoverySubject::ManagedProcess)
            }
            ManagedServerState::StaleOnly { .. } => {
                Some(TailscaleRecoverySubject::StaleRegistration)
            }
            ManagedServerState::Empty
            | ManagedServerState::Degraded { .. }
            | ManagedServerState::Conflict { .. }
            | ManagedServerState::Unverifiable { .. } => None,
        };
        if let Some(subject) = subject {
            report.next_action = StatusNextAction::RecoverOwnedTailscale {
                remote_base_url: ownership.remote_base_url.clone(),
                mount_path: ownership.mount_path.clone(),
                reason,
                subject,
                apply_confirmed: ownership.apply_confirmed,
            };
        }
    }
    report.tailscale = Some(TailscaleStatusReport { ownership, status });
    report
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
            let tailscale = runfile.tailscale_serve.as_ref().map_or_else(
                || {
                    if runfile.tailscale {
                        " tailscale=legacy-unowned".to_string()
                    } else {
                        String::new()
                    }
                },
                |ownership| {
                    format!(
                        " tailscale=owned mount={} remote-base={}",
                        ownership.mount_path, ownership.remote_base_url
                    )
                },
            );
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
                "[captured] {prefix}: schema={} engine={:?} pid={} base-url={} recorded-identity={recorded}{tailscale} {observed}",
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
            "registration port {port} claims durable Tailscale Serve state; scoped proxy cleanup is unavailable, so Ferric will not inspect or signal its PID, delete its registration, invoke Tailscale, or run a blind node-wide reset; inspect and remove only that exact Serve endpoint with Tailscale tooling"
        ),
        StatusNextAction::RecoverOwnedTailscale {
            remote_base_url,
            mount_path,
            reason,
            subject,
            apply_confirmed,
        } => {
            let phase = if *apply_confirmed {
                ""
            } else {
                "; this journal is in the unconfirmed-apply phase, so an absent-only check cannot authorize deletion: `server down` will retain it unless the delayed exact path appears and is scoped-off or a separate daemon-generation/manual proof establishes that the request cannot still land"
            };
            match subject {
            TailscaleRecoverySubject::ManagedProcess => format!(
                "{reason} at {mount_path} ({remote_base_url}); run `ferric server down` to stop the independently owned process and retry exact scoped cleanup; Ferric will retain the ownership journal unless both resources resolve"
            ) + phase,
            TailscaleRecoverySubject::StaleRegistration => format!(
                "{reason} at {mount_path} ({remote_base_url}); no managed process is present, so run `ferric server down` to reconcile that exact coordinate and conditionally remove unchanged stale journals; no process will be signalled, and Ferric will retain the journals unless exact cleanup is fully resolved"
            ) + phase,
        }
        },
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
    if let Some(tailscale) = &report.tailscale {
        stdout.push(match &tailscale.status {
            TailscaleProxyStatus::Active => format!(
                "[tailscale] active remote-base={} mount={} target={} apply-confirmed={}",
                tailscale.ownership.remote_base_url,
                tailscale.ownership.mount_path,
                tailscale.ownership.proxy_target,
                tailscale.ownership.apply_confirmed
            ),
            TailscaleProxyStatus::Pending => format!(
                "[tailscale] pending remote-base={} mount={} target={} apply-confirmed={}",
                tailscale.ownership.remote_base_url,
                tailscale.ownership.mount_path,
                tailscale.ownership.proxy_target,
                tailscale.ownership.apply_confirmed
            ),
            TailscaleProxyStatus::Replaced { observed_target } => format!(
                "[tailscale] replaced remote-base={} mount={} expected-target={} observed-target={observed_target} apply-confirmed={}",
                tailscale.ownership.remote_base_url,
                tailscale.ownership.mount_path,
                tailscale.ownership.proxy_target,
                tailscale.ownership.apply_confirmed
            ),
            TailscaleProxyStatus::Uninspectable { reason } => format!(
                "[tailscale] uninspectable remote-base={} mount={} reason={reason} apply-confirmed={}",
                tailscale.ownership.remote_base_url,
                tailscale.ownership.mount_path,
                tailscale.ownership.apply_confirmed
            ),
        });
    }
    if let Some(reason) = &report.tailscale_issue {
        stdout.push(format!("[tailscale] ownership-blocked reason={reason}"));
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
    let serve = TailscaleServeAdapter::native();
    let rendered = render_status(&status_report_with_tailscale(&discovery, &serve));
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum DownRegistrationOutcome {
    Removed,
    AlreadyAbsent,
    ReplacementPreserved {
        path: PathBuf,
        detail: String,
    },
    RestoreFailed {
        preserved_at: Option<PathBuf>,
        detail: String,
    },
    RemovalFailed {
        preserved_at: Option<PathBuf>,
        detail: String,
    },
    CleanupFailed {
        preserved_at: Option<PathBuf>,
        detail: String,
    },
    Held {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownRegistrationReport {
    coordinate: RegistrationCoordinate,
    outcome: DownRegistrationOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownDisposition {
    Empty,
    Blocked,
    StaleCleaned,
    Stopped,
    AlreadyExited,
    Failed,
    CleanupPartial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownReport {
    disposition: DownDisposition,
    pid: Option<u32>,
    signalled: bool,
    exit_proven: bool,
    listener_released: bool,
    registrations: Vec<DownRegistrationReport>,
    diagnostics: Vec<String>,
    guidance: Option<String>,
    success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedDownReport {
    stdout: Vec<String>,
    stderr: Vec<String>,
    success: bool,
}

enum DownPlan<P> {
    Empty,
    Blocked {
        registrations: Vec<DownRegistrationReport>,
        diagnostics: Vec<String>,
        guidance: Option<String>,
    },
    Stale {
        captures: Vec<CapturedRegistration>,
        expected_revisions: Vec<RegistrationRevision>,
    },
    Target {
        process: P,
        expected: ProcessIdentity,
        pid: u32,
        port: u16,
        captures: Vec<CapturedRegistration>,
        expected_revisions: Vec<RegistrationRevision>,
    },
}

trait DownEffects {
    fn revalidate_registrations(&mut self, expected: &[RegistrationRevision])
    -> Result<(), String>;
    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState;
    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError>;
    fn reconcile_tailscale(
        &mut self,
        _ownership: &TailscaleServeOwnership,
        _captures: &mut [CapturedRegistration],
    ) -> ProxyCleanupReport {
        ProxyCleanupReport {
            resolved: false,
            off_failed: false,
            diagnostics: vec![
                "Tailscale Serve cleanup effect was not configured; ownership journals are held"
                    .to_string(),
            ],
        }
    }
}

struct NativeDownEffects {
    scope: ManagedDiscoveryScope,
    serve: TailscaleServeAdapter,
}

const REGISTRATION_REVISION_CHANGED: &str =
    "registration inventory changed after teardown resolution; registration cleanup was refused";

impl DownEffects for NativeDownEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
        let current = discovery_revisions(&flatten_inventory(&inventory));
        if current == expected {
            Ok(())
        } else {
            Err(REGISTRATION_REVISION_CHANGED.to_string())
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        loopback_listener_state(pid, port)
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        remove_if_unchanged(captured)
    }

    fn reconcile_tailscale(
        &mut self,
        ownership: &TailscaleServeOwnership,
        captures: &mut [CapturedRegistration],
    ) -> ProxyCleanupReport {
        reconcile_owned_proxy(
            ownership,
            &self.serve,
            if ownership.apply_confirmed {
                ProxyReconcileContext::EstablishedOwnership
            } else {
                ProxyReconcileContext::AmbiguousApply
            },
            || {
                confirm_tailscale_captures_with(captures, ownership, |captured, raw| {
                    replace_if_unchanged(captured, raw)
                })
            },
        )
    }
}

fn refresh_expected_revisions_after_confirmation(
    expected: &mut [RegistrationRevision],
    captures: &[CapturedRegistration],
) {
    for revision in expected {
        if let Some(capture) = captures.iter().find(|capture| {
            capture.scope == revision.coordinate.scope && capture.path == revision.coordinate.path
        }) {
            revision.state =
                RegistrationRevisionState::Captured(ferric_bench::sha256_bytes(&capture.raw));
        }
        if let Some(promised) = &mut revision.promised
            && let Some(source) = captures.iter().find(|capture| {
                capture.scope == promised.source.scope && capture.path == promised.source.path
            })
        {
            promised.expected_runfile = source.runfile.clone();
        }
    }
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

fn captures_for_indices<P>(
    observations: &[LifecycleObservation<P>],
    mut indices: Vec<usize>,
) -> Result<Vec<CapturedRegistration>, String> {
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .map(|index| {
            observations[index].capture.clone().ok_or_else(|| {
                format!(
                    "{} has no exact-byte registration capture",
                    observations[index].label
                )
            })
        })
        .collect()
}

fn retained_target_down_plan<P>(
    state: &ManagedServerState,
    process: P,
    captures: Vec<CapturedRegistration>,
    expected_revisions: Vec<RegistrationRevision>,
) -> Result<DownPlan<P>, String> {
    let server = match state {
        ManagedServerState::Ready(server) | ManagedServerState::Degraded { server, .. } => server,
        ManagedServerState::Empty
        | ManagedServerState::StaleOnly { .. }
        | ManagedServerState::Conflict { .. }
        | ManagedServerState::Unverifiable { .. } => {
            return Err("typed discovery did not retain one teardown target".to_string());
        }
    };
    if !server.listener.permits_teardown() {
        return Err(format!(
            "target listener {:?} does not authorize teardown",
            server.listener
        ));
    }
    let expected = server
        .runfile
        .process_identity
        .clone()
        .ok_or_else(|| "resolved teardown target has no creation identity".to_string())?;
    Ok(DownPlan::Target {
        process,
        expected,
        pid: server.runfile.pid,
        port: server.runfile.port,
        captures,
        expected_revisions,
    })
}

fn down_plan_from_lifecycle<P>(mut discovery: LifecycleDiscovery<P>) -> DownPlan<P> {
    if let Some(issues) = down_mutation_blocker(&discovery.managed.state) {
        let guidance = match status_next_action(&discovery.managed) {
            StatusNextAction::AdoptLegacy { pid } => {
                Some(format!("ferric server adopt --pid {pid}"))
            }
            action => Some(next_action_text(&action)),
        };
        return DownPlan::Blocked {
            registrations: discovery
                .managed
                .observations
                .iter()
                .filter(|observation| {
                    !matches!(observation.state, ManagedRegistrationState::Absent)
                })
                .map(|observation| DownRegistrationReport {
                    coordinate: observation.coordinate.clone(),
                    outcome: DownRegistrationOutcome::Held {
                        detail: "typed discovery blocked teardown mutation".to_string(),
                    },
                })
                .collect(),
            diagnostics: issues.iter().map(|issue| issue.detail.clone()).collect(),
            guidance,
        };
    }

    let expected_revisions = discovery_revisions(&discovery.managed.observations);
    match discovery.resolution {
        Resolution::Empty => DownPlan::Empty,
        Resolution::StaleOnly { stale } => {
            match captures_for_indices(&discovery.observations, stale) {
                Ok(captures) => DownPlan::Stale {
                    captures,
                    expected_revisions,
                },
                Err(error) => DownPlan::Blocked {
                    registrations: Vec::new(),
                    diagnostics: vec![error],
                    guidance: None,
                },
            }
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
            let process = discovery.observations[target]
                .process
                .take()
                .expect("resolved target retains its exact process handle");
            let mut cleanup = vec![target];
            cleanup.extend(aliases);
            cleanup.extend(stale);
            match captures_for_indices(&discovery.observations, cleanup) {
                Ok(captures) => retained_target_down_plan(
                    &discovery.managed.state,
                    process,
                    captures,
                    expected_revisions,
                )
                .unwrap_or_else(|error| DownPlan::Blocked {
                    registrations: held_registration_reports(
                        &discovery
                            .observations
                            .iter()
                            .filter_map(|observation| observation.capture.clone())
                            .collect::<Vec<_>>(),
                        &error,
                    ),
                    diagnostics: vec![error],
                    guidance: None,
                }),
                Err(error) => DownPlan::Blocked {
                    registrations: Vec::new(),
                    diagnostics: vec![error],
                    guidance: None,
                },
            }
        }
        Resolution::Conflict { .. } | Resolution::Unverifiable { .. } => {
            unreachable!("typed down blocker returned before plan construction")
        }
    }
}

fn held_registration_reports(
    captures: &[CapturedRegistration],
    detail: &str,
) -> Vec<DownRegistrationReport> {
    captures
        .iter()
        .map(|capture| DownRegistrationReport {
            coordinate: RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            },
            outcome: DownRegistrationOutcome::Held {
                detail: detail.to_string(),
            },
        })
        .collect()
}

fn mutation_path_key(path: &Path) -> PathBuf {
    // Inventory capture already stores an absolute, lexically normalized path.
    // Mutation grouping deliberately requires that exact lossless spelling;
    // broader aliases are blockers, never silently collapsed mutation keys.
    path.to_path_buf()
}

fn distinct_mutation_paths_may_alias(left: &Path, right: &Path) -> bool {
    let left_key = mutation_path_key(left);
    let right_key = mutation_path_key(right);
    if left_key == right_key {
        return false;
    }
    if matches!(
        (std::fs::canonicalize(left), std::fs::canonicalize(right)),
        (Ok(left), Ok(right)) if left == right
    ) {
        return true;
    }
    #[cfg(windows)]
    {
        // This fallback is only a conservative blocker. It is never used as a
        // grouping key, so lossy or incomplete Unicode folding can at worst
        // refuse a mutation; it cannot collapse distinct entries and report a
        // mutation that did not happen.
        left_key
            .to_string_lossy()
            .eq_ignore_ascii_case(&right_key.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_mutation_path_aliases(captures: &[CapturedRegistration]) -> Result<(), String> {
    for (index, left) in captures.iter().enumerate() {
        for right in &captures[index + 1..] {
            let left_key = mutation_path_key(&left.path);
            let right_key = mutation_path_key(&right.path);
            if left_key == right_key && left.raw != right.raw {
                return Err(format!(
                    "registration path {} has conflicting exact-byte mutation captures",
                    right.path.display()
                ));
            }
            if distinct_mutation_paths_may_alias(&left.path, &right.path) {
                return Err(format!(
                    "registration entry may be reachable through two distinct paths, {} and {}; repair the aliases before mutation",
                    left.path.display(),
                    right.path.display()
                ));
            }
        }
    }
    Ok(())
}

fn unique_tailscale_ownership_from_captures(
    captures: &[CapturedRegistration],
) -> Result<Option<TailscaleServeOwnership>, String> {
    let mut ownership: Option<TailscaleServeOwnership> = None;
    for capture in captures {
        if !capture.runfile.tailscale {
            continue;
        }
        let candidate = capture.runfile.tailscale_serve.as_ref().ok_or_else(|| {
            format!(
                "{} registration {} has legacy Tailscale state without endpoint-scoped ownership",
                capture.scope,
                capture.path.display()
            )
        })?;
        candidate
            .validate_for_port(capture.runfile.port)
            .map_err(|error| error.to_string())?;
        if let Some(existing) = &mut ownership {
            if !existing.same_coordinate(candidate) {
                return Err(
                    "teardown captures disagree about Tailscale Serve ownership".to_string()
                );
            }
            existing.apply_confirmed |= candidate.apply_confirmed;
        } else {
            ownership = Some(candidate.clone());
        }
    }
    Ok(ownership)
}

struct DownCleanupGroup {
    key: PathBuf,
    capture: CapturedRegistration,
    indices: Vec<usize>,
}

fn down_cleanup_groups(captures: &[CapturedRegistration]) -> Result<Vec<DownCleanupGroup>, String> {
    validate_mutation_path_aliases(captures)?;
    let mut groups: Vec<DownCleanupGroup> = Vec::new();
    for (index, capture) in captures.iter().enumerate() {
        let key = mutation_path_key(&capture.path);
        if let Some(existing) = groups.iter_mut().find(|group| group.key == key) {
            if existing.capture.raw != capture.raw {
                return Err(format!(
                    "registration path {} has conflicting exact-byte cleanup captures",
                    capture.path.display()
                ));
            }
            existing.indices.push(index);
            continue;
        }
        groups.push(DownCleanupGroup {
            key,
            capture: capture.clone(),
            indices: vec![index],
        });
    }
    Ok(groups)
}

fn cleanup_registrations<E: DownEffects>(
    captures: &[CapturedRegistration],
    groups: &[DownCleanupGroup],
    effects: &mut E,
) -> (Vec<DownRegistrationReport>, bool) {
    let mut complete = true;
    let outcomes = groups
        .iter()
        .map(|group| match effects.remove(&group.capture) {
            Ok(RemovalOutcome::Removed) => DownRegistrationOutcome::Removed,
            Ok(RemovalOutcome::Absent) => DownRegistrationOutcome::AlreadyAbsent,
            Ok(RemovalOutcome::ReplacementPreserved { path, detail }) => {
                complete = false;
                DownRegistrationOutcome::ReplacementPreserved { path, detail }
            }
            Err(error) => {
                complete = false;
                match error.kind {
                    RemovalFailureKind::Restore => DownRegistrationOutcome::RestoreFailed {
                        preserved_at: error.preserved_at,
                        detail: error.detail,
                    },
                    RemovalFailureKind::Remove => DownRegistrationOutcome::RemovalFailed {
                        preserved_at: error.preserved_at,
                        detail: error.detail,
                    },
                    RemovalFailureKind::Other => DownRegistrationOutcome::CleanupFailed {
                        preserved_at: error.preserved_at,
                        detail: error.detail,
                    },
                }
            }
        })
        .collect::<Vec<_>>();
    let reports = captures
        .iter()
        .enumerate()
        .map(|(index, capture)| {
            let outcome = groups
                .iter()
                .enumerate()
                .find(|(_, group)| group.indices.contains(&index))
                .expect("every cleanup capture belongs to a planned path group")
                .0;
            let outcome = outcomes[outcome].clone();
            DownRegistrationReport {
                coordinate: RegistrationCoordinate {
                    scope: capture.scope,
                    path: capture.path.clone(),
                },
                outcome,
            }
        })
        .collect();
    (reports, complete)
}

fn require_absent_cleanup_ports<E: DownEffects>(
    captures: &[CapturedRegistration],
    effects: &mut E,
    checked: &mut Vec<(u32, u16)>,
) -> Result<(), String> {
    for capture in captures {
        let key = (capture.runfile.pid, capture.runfile.port);
        if checked.contains(&key) {
            continue;
        }
        let listener = effects.listener_state(key.0, key.1);
        if listener != ListenerState::Absent {
            return Err(format!(
                "registered endpoint {}:{} is not quiescent after exit: {listener:?}",
                key.0, key.1
            ));
        }
        checked.push(key);
    }
    Ok(())
}

fn failed_down_report(
    pid: Option<u32>,
    captures: &[CapturedRegistration],
    diagnostic: String,
) -> DownReport {
    DownReport {
        disposition: DownDisposition::Failed,
        pid,
        signalled: false,
        exit_proven: false,
        listener_released: false,
        registrations: held_registration_reports(captures, &diagnostic),
        diagnostics: vec![diagnostic],
        guidance: None,
        success: false,
    }
}

fn held_down_report(
    pid: Option<u32>,
    captures: &[CapturedRegistration],
    signalled: bool,
    exit_proven: bool,
    listener_released: bool,
    diagnostics: Vec<String>,
) -> DownReport {
    let detail = diagnostics.join("; ").trim().to_string();
    DownReport {
        disposition: DownDisposition::Failed,
        pid,
        signalled,
        exit_proven,
        listener_released,
        registrations: held_registration_reports(
            captures,
            if detail.is_empty() {
                "resource cleanup was not fully resolved"
            } else {
                &detail
            },
        ),
        diagnostics,
        guidance: None,
        success: false,
    }
}

fn retain_owned_proxy_diagnostic(ownership: &TailscaleServeOwnership) -> String {
    format!(
        "ownership journals are retained for {}; retry `ferric server down` so a fresh exact-coordinate comparison can converge without a node-wide mutation",
        tailscale_coordinate_label(ownership)
    )
}

fn failed_down_report_with_proxy(
    pid: Option<u32>,
    captures: &[CapturedRegistration],
    error: String,
    proxy: &ProxyCleanupReport,
) -> DownReport {
    let mut report = failed_down_report(pid, captures, error);
    if !proxy.diagnostics.is_empty() {
        let mut diagnostics = proxy.diagnostics.clone();
        diagnostics.extend(report.diagnostics);
        report.diagnostics = diagnostics;
    }
    report
}

fn execute_down_plan<P, E>(plan: DownPlan<P>, effects: &mut E) -> DownReport
where
    P: RetainedProcessHandle,
    E: DownEffects,
{
    match plan {
        DownPlan::Empty => DownReport {
            disposition: DownDisposition::Empty,
            pid: None,
            signalled: false,
            exit_proven: true,
            listener_released: true,
            registrations: Vec::new(),
            diagnostics: Vec::new(),
            guidance: None,
            success: true,
        },
        DownPlan::Blocked {
            registrations,
            diagnostics,
            guidance,
        } => DownReport {
            disposition: DownDisposition::Blocked,
            pid: None,
            signalled: false,
            exit_proven: false,
            listener_released: false,
            registrations,
            diagnostics,
            guidance,
            success: false,
        },
        DownPlan::Stale {
            mut captures,
            mut expected_revisions,
        } => {
            if let Err(error) = validate_mutation_path_aliases(&captures) {
                return failed_down_report(None, &captures, error);
            }
            let ownership = match unique_tailscale_ownership_from_captures(&captures) {
                Ok(ownership) => ownership,
                Err(error) => return failed_down_report(None, &captures, error),
            };
            if let Err(error) = effects.revalidate_registrations(&expected_revisions) {
                return failed_down_report(None, &captures, error);
            }
            let mut proxy = ownership.as_ref().map_or_else(
                || ProxyCleanupReport {
                    resolved: true,
                    off_failed: false,
                    diagnostics: Vec::new(),
                },
                |ownership| effects.reconcile_tailscale(ownership, &mut captures),
            );
            refresh_expected_revisions_after_confirmation(&mut expected_revisions, &captures);
            if (!proxy.resolved || proxy.off_failed)
                && let Some(ownership) = ownership.as_ref()
            {
                proxy
                    .diagnostics
                    .push(retain_owned_proxy_diagnostic(ownership));
            }
            let mut checked = Vec::new();
            if let Err(error) = require_absent_cleanup_ports(&captures, effects, &mut checked) {
                let mut diagnostics = proxy.diagnostics;
                diagnostics.push(error);
                return held_down_report(None, &captures, false, true, false, diagnostics);
            }
            if !proxy.resolved || proxy.off_failed {
                return held_down_report(None, &captures, false, true, true, proxy.diagnostics);
            }
            if ownership.is_some()
                && let Err(error) = effects.revalidate_registrations(&expected_revisions)
            {
                let mut diagnostics = proxy.diagnostics;
                diagnostics.push(error);
                return held_down_report(None, &captures, false, true, true, diagnostics);
            }
            let groups = match down_cleanup_groups(&captures) {
                Ok(groups) => groups,
                Err(error) => return failed_down_report(None, &captures, error),
            };
            let (registrations, complete) = cleanup_registrations(&captures, &groups, effects);
            let mut diagnostics = proxy.diagnostics;
            if !complete {
                diagnostics.push("stale registration cleanup was partial".to_string());
            }
            DownReport {
                disposition: if complete {
                    DownDisposition::StaleCleaned
                } else {
                    DownDisposition::CleanupPartial
                },
                pid: None,
                signalled: false,
                exit_proven: true,
                listener_released: true,
                registrations,
                diagnostics,
                guidance: None,
                success: complete,
            }
        }
        DownPlan::Target {
            process,
            expected,
            pid,
            port,
            mut captures,
            mut expected_revisions,
        } => {
            if let Err(error) = validate_mutation_path_aliases(&captures) {
                return failed_down_report(Some(pid), &captures, error);
            }
            let ownership = match unique_tailscale_ownership_from_captures(&captures) {
                Ok(ownership) => ownership,
                Err(error) => return failed_down_report(Some(pid), &captures, error),
            };
            if process.pid() != pid {
                return failed_down_report(
                    Some(pid),
                    &captures,
                    format!(
                        "retained process handle names PID {}, expected {pid}",
                        process.pid()
                    ),
                );
            }
            if let Err(error) = effects.revalidate_registrations(&expected_revisions) {
                return failed_down_report(Some(pid), &captures, error);
            }
            let mut proxy = ownership.as_ref().map_or_else(
                || ProxyCleanupReport {
                    resolved: true,
                    off_failed: false,
                    diagnostics: Vec::new(),
                },
                |ownership| effects.reconcile_tailscale(ownership, &mut captures),
            );
            refresh_expected_revisions_after_confirmation(&mut expected_revisions, &captures);
            if (!proxy.resolved || proxy.off_failed)
                && let Some(ownership) = ownership.as_ref()
            {
                proxy
                    .diagnostics
                    .push(retain_owned_proxy_diagnostic(ownership));
            }
            if ownership.is_some()
                && let Err(error) = effects.revalidate_registrations(&expected_revisions)
            {
                return failed_down_report_with_proxy(
                    Some(pid),
                    &captures,
                    format!(
                        "{error}; registration authority changed during Tailscale reconciliation, so the retained process was not signalled"
                    ),
                    &proxy,
                );
            }
            let already_exited = match process.inspect(port) {
                Ok(facts) => {
                    if facts.identity != expected {
                        return failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            "retained process identity changed after resolution".to_string(),
                            &proxy,
                        );
                    }
                    if !facts.listener.permits_teardown() {
                        return failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            format!(
                                "retained process listener no longer authorizes teardown: {:?}",
                                facts.listener
                            ),
                            &proxy,
                        );
                    }
                    false
                }
                Err(ProcessError::NotFound(_)) => match process.wait(Duration::ZERO) {
                    Ok(true) => true,
                    Ok(false) => {
                        return failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            "retained process identity vanished without exit proof".to_string(),
                            &proxy,
                        );
                    }
                    Err(error) => {
                        return failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            format!("retained process exit inspection failed: {error}"),
                            &proxy,
                        );
                    }
                },
                Err(error) => {
                    return failed_down_report_with_proxy(
                        Some(pid),
                        &captures,
                        format!("retained process revalidation failed: {error}"),
                        &proxy,
                    );
                }
            };

            let signalled = if already_exited {
                false
            } else {
                match process.terminate() {
                    Ok(signalled) => signalled,
                    Err(error) => {
                        return failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            format!("retained process termination failed: {error}"),
                            &proxy,
                        );
                    }
                }
            };
            if !already_exited {
                match process.wait(Duration::from_secs(10)) {
                    Ok(true) => {}
                    Ok(false) => {
                        let mut report = failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            "retained process did not exit within 10 seconds".to_string(),
                            &proxy,
                        );
                        report.signalled = signalled;
                        return report;
                    }
                    Err(error) => {
                        let mut report = failed_down_report_with_proxy(
                            Some(pid),
                            &captures,
                            format!("retained process exit confirmation failed: {error}"),
                            &proxy,
                        );
                        report.signalled = signalled;
                        return report;
                    }
                }
            }

            let mut checked = Vec::new();
            if let Err(error) = require_absent_cleanup_ports(&captures, effects, &mut checked) {
                let mut diagnostics = proxy.diagnostics;
                diagnostics.push(error);
                return held_down_report(Some(pid), &captures, signalled, true, false, diagnostics);
            }
            if !proxy.resolved || proxy.off_failed {
                return held_down_report(
                    Some(pid),
                    &captures,
                    signalled,
                    true,
                    true,
                    proxy.diagnostics,
                );
            }
            if ownership.is_some()
                && let Err(error) = effects.revalidate_registrations(&expected_revisions)
            {
                let mut diagnostics = proxy.diagnostics;
                diagnostics.push(format!(
                    "{error}; exact process exit and listener release were already proven"
                ));
                return held_down_report(Some(pid), &captures, signalled, true, true, diagnostics);
            }
            let groups = match down_cleanup_groups(&captures) {
                Ok(groups) => groups,
                Err(error) => return failed_down_report(Some(pid), &captures, error),
            };
            let (registrations, complete) = cleanup_registrations(&captures, &groups, effects);
            let mut diagnostics = proxy.diagnostics;
            if !complete {
                diagnostics.push(
                    "managed process exit is confirmed, but registration cleanup was partial"
                        .to_string(),
                );
            }
            DownReport {
                disposition: if complete {
                    if signalled {
                        DownDisposition::Stopped
                    } else {
                        DownDisposition::AlreadyExited
                    }
                } else {
                    DownDisposition::CleanupPartial
                },
                pid: Some(pid),
                signalled,
                exit_proven: true,
                listener_released: true,
                registrations,
                diagnostics,
                guidance: None,
                success: complete,
            }
        }
    }
}

fn render_down_report(report: &DownReport) -> RenderedDownReport {
    let mut stdout = report
        .registrations
        .iter()
        .map(|registration| {
            let coordinate = &registration.coordinate;
            match &registration.outcome {
                DownRegistrationOutcome::Removed => format!(
                    "[removed] {} registration {}",
                    coordinate.scope,
                    coordinate.path.display()
                ),
                DownRegistrationOutcome::AlreadyAbsent => format!(
                    "[already-absent] {} registration {}",
                    coordinate.scope,
                    coordinate.path.display()
                ),
                DownRegistrationOutcome::ReplacementPreserved { path, detail } => format!(
                    "[replacement-preserved] {} registration {} preserved-at={} detail={detail}",
                    coordinate.scope,
                    coordinate.path.display(),
                    path.display()
                ),
                DownRegistrationOutcome::RestoreFailed {
                    preserved_at,
                    detail,
                } => format!(
                    "[restore-failed] {} registration {} holding={} detail={detail}",
                    coordinate.scope,
                    coordinate.path.display(),
                    preserved_at
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |path| path.display().to_string())
                ),
                DownRegistrationOutcome::RemovalFailed {
                    preserved_at,
                    detail,
                } => format!(
                    "[removal-failed] {} registration {} holding={} detail={detail}",
                    coordinate.scope,
                    coordinate.path.display(),
                    preserved_at
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |path| path.display().to_string())
                ),
                DownRegistrationOutcome::CleanupFailed {
                    preserved_at,
                    detail,
                } => format!(
                    "[cleanup-failed] {} registration {} holding={} detail={detail}",
                    coordinate.scope,
                    coordinate.path.display(),
                    preserved_at
                        .as_ref()
                        .map_or_else(|| "none".to_string(), |path| path.display().to_string())
                ),
                DownRegistrationOutcome::Held { detail } => format!(
                    "[held] {} registration {} detail={detail}",
                    coordinate.scope,
                    coordinate.path.display()
                ),
            }
        })
        .collect::<Vec<_>>();
    stdout.push(match report.disposition {
        DownDisposition::Empty => "[state] no server registered".to_string(),
        DownDisposition::Blocked => "[state] teardown blocked; registrations kept".to_string(),
        DownDisposition::StaleCleaned => "[state] stale-cleaned".to_string(),
        DownDisposition::Stopped => format!(
            "[state] stopped managed server pid {} through its retained process handle",
            report.pid.expect("stopped report has PID")
        ),
        DownDisposition::AlreadyExited => format!(
            "[state] managed server pid {} was already exited; no process was signalled",
            report.pid.expect("already-exited report has PID")
        ),
        DownDisposition::Failed => "[state] teardown failed; registrations kept".to_string(),
        DownDisposition::CleanupPartial => {
            "[state] exit/quiescence confirmed where applicable; cleanup partial".to_string()
        }
    });
    if let Some(guidance) = &report.guidance {
        stdout.push(format!("[next] {guidance}"));
    }
    RenderedDownReport {
        stdout,
        stderr: report
            .diagnostics
            .iter()
            .map(|diagnostic| format!("[diagnostic] {diagnostic}"))
            .collect(),
        success: report.success,
    }
}

fn emit_down_report(report: &DownReport) -> ExitCode {
    let rendered = render_down_report(report);
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

fn down(workspace: &Path) -> ExitCode {
    down_impl(workspace, global_runfile_path())
}

fn down_impl(workspace: &Path, global_path: Option<PathBuf>) -> ExitCode {
    let scope = ManagedDiscoveryScope {
        workspace: workspace.to_path_buf(),
        global: global_path,
    };
    let discovery = discover_lifecycle_before_health_in(&scope);
    let plan = down_plan_from_lifecycle(discovery);
    let mut effects = NativeDownEffects {
        scope,
        serve: TailscaleServeAdapter::native(),
    };
    emit_down_report(&execute_down_plan(plan, &mut effects))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    lines: Vec<String>,
    success: bool,
}

fn static_doctor_blocker(args: &ServerUpArgs) -> Option<DoctorReport> {
    let mut lines = Vec::new();
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

    if args.tailscale {
        match effects.tailscale_identity() {
            Ok(fqdn) => {
                lines.push(format!("[ok] Tailscale canonical self identity `{fqdn}`"));
                match effects.tailscale_status(&fqdn) {
                    Ok(()) => lines.push(
                        "[ok] Tailscale Serve status is readable through a bounded read-only probe"
                            .to_string(),
                    ),
                    Err(error) => {
                        lines.push(format!(
                            "[BLOCKED] Tailscale Serve status is not authorizing: {error}"
                        ));
                        ok = false;
                    }
                }
            }
            Err(error) => {
                lines.push(format!(
                    "[BLOCKED] Tailscale canonical self identity is unavailable: {error}"
                ));
                lines.push(
                    "[next] verify the local Tailscale daemon reports capability 142 and version core 1.102.2, is logged in, and is reachable; retry `ferric server doctor --tailscale`"
                        .to_string(),
                );
                ok = false;
            }
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

fn doctor_report_with<D, E>(args: &ServerUpArgs, discover: D, effects: &mut E) -> DoctorReport
where
    D: FnOnce() -> Result<ManagedServerDiscovery, String>,
    E: DoctorProbeEffects,
{
    if let Some(report) = static_doctor_blocker(args) {
        return report;
    }
    let discovery = match discover() {
        Ok(discovery) => discovery,
        Err(error) => {
            return DoctorReport {
                lines: vec![format!(
                    "[BLOCKED] resolve managed discovery scope: {error}"
                )],
                success: false,
            };
        }
    };
    doctor_report_after_discovery(args, &discovery, effects)
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
    let mut effects = NativeDoctorProbeEffects {
        serve: TailscaleServeAdapter::native(),
    };
    emit_doctor_report(doctor_report_with(
        args,
        || {
            let scope = ManagedDiscoveryScope::for_workspace(workspace)?;
            Ok(discover_managed_server_in(&scope))
        },
        &mut effects,
    ))
}

#[cfg(test)]
pub(crate) mod tests;
