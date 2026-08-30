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
    ReplacementOutcome, inventory_runfiles, publish_mirrored, remove_if_unchanged,
    remove_publication_stage_if_unchanged, replace_if_unchanged, validate_runfile,
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

#[cfg(test)]
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
    if args.tailscale {
        return Err(
            "--tailscale is fail-closed before registration, PID, engine, model, or network probes because scoped proxy cleanup is unavailable"
                .to_string(),
        );
    }
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
    fn remove_final(
        &mut self,
        captured: &CapturedRegistration,
    ) -> Result<RemovalOutcome, RemovalError>;

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError>;
}

struct NativePublicationCompensationEffects;

impl PublicationCompensationEffects for NativePublicationCompensationEffects {
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
            Ok(None) => {
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
    published: PublishedRegistrations,
}

/// One authority-preserving launch sequence shared by production `up` and
/// deterministic composition tests. The spawned child is bound to its exact
/// retained process object before any readiness probe, and publication is
/// reachable only after readiness plus final process/listener inspection.
#[allow(clippy::too_many_arguments)]
fn orchestrate_launch_with<C, R, L, H, K, E, S, F>(
    workspace: &Path,
    global_path: Option<&Path>,
    cfg: &ServerConfig,
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
    S: FnOnce() -> Result<C, String>,
    F: FnOnce(&Path, Option<&Path>, &ServerRunfile) -> Result<PublishedRegistrations, PublishError>,
{
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
        model: cfg.model.clone(),
        context_size: (cfg.engine == Engine::LlamaServer).then_some(cfg.ctx),
        sampling_seed: cfg.seed,
        parallel_slots: cfg.parallel,
        process_identity: Some(process_facts.identity),
        origin_local_runfile: Some(local_path),
    };
    let publication = publish(workspace, global_path, &runfile);
    let completion = complete_publication_with(
        &mut child,
        &process,
        cfg.port,
        publication,
        listener,
        compensation,
    );
    if completion.success {
        Ok(LaunchOrchestrationSuccess {
            pid,
            base_url,
            published: completion
                .published
                .expect("successful publication completion retains published registrations"),
        })
    } else {
        Err(LaunchOrchestrationError::Publication(Box::new(completion)))
    }
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
    let mut health = NativeHealthProbe;
    let mut clock = SystemLifecycleClock;
    let mut compensation = NativePublicationCompensationEffects;
    let launched = orchestrate_launch_with(
        workspace,
        global_path.as_deref(),
        &cfg,
        || proc.spawn().map_err(|error| error.to_string()),
        &NativeSpawnedProcessRuntime,
        &NativeListenerInspector,
        &mut health,
        &mut clock,
        publish_mirrored,
        &mut compensation,
    );
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

    println!("server ready: {} (pid {})", launched.base_url, launched.pid);
    println!(
        "registered locally at {}",
        launched.published.local.path.display()
    );
    if let Some(global) = launched.published.global {
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
            "registration port {port} claims durable Tailscale Serve state; scoped proxy cleanup is unavailable, so Ferric will not inspect or signal its PID, delete its registration, invoke Tailscale, or run a blind node-wide reset; inspect and remove only that exact Serve endpoint with Tailscale tooling"
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
}

struct NativeDownEffects {
    scope: ManagedDiscoveryScope,
}

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
            Err(
                "registration inventory changed after teardown resolution; no process was signalled"
                    .to_string(),
            )
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        loopback_listener_state(pid, port)
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        remove_if_unchanged(captured)
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

fn captures_for_indices(
    observations: &[LifecycleObservation],
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

fn down_plan_from_lifecycle(mut discovery: LifecycleDiscovery) -> DownPlan<LiveProcess> {
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
            captures,
            expected_revisions,
        } => {
            let groups = match down_cleanup_groups(&captures) {
                Ok(groups) => groups,
                Err(error) => return failed_down_report(None, &captures, error),
            };
            if let Err(error) = effects.revalidate_registrations(&expected_revisions) {
                return failed_down_report(None, &captures, error);
            }
            let mut checked = Vec::new();
            if let Err(error) = require_absent_cleanup_ports(&captures, effects, &mut checked) {
                return failed_down_report(None, &captures, error);
            }
            let (registrations, complete) = cleanup_registrations(&captures, &groups, effects);
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
                diagnostics: (!complete)
                    .then(|| "stale registration cleanup was partial".to_string())
                    .into_iter()
                    .collect(),
                guidance: None,
                success: complete,
            }
        }
        DownPlan::Target {
            process,
            expected,
            pid,
            port,
            captures,
            expected_revisions,
        } => {
            let groups = match down_cleanup_groups(&captures) {
                Ok(groups) => groups,
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
            let already_exited = match process.inspect(port) {
                Ok(facts) => {
                    if facts.identity != expected {
                        return failed_down_report(
                            Some(pid),
                            &captures,
                            "retained process identity changed after resolution".to_string(),
                        );
                    }
                    if !facts.listener.permits_teardown() {
                        return failed_down_report(
                            Some(pid),
                            &captures,
                            format!(
                                "retained process listener no longer authorizes teardown: {:?}",
                                facts.listener
                            ),
                        );
                    }
                    false
                }
                Err(ProcessError::NotFound(_)) => match process.wait(Duration::ZERO) {
                    Ok(true) => true,
                    Ok(false) => {
                        return failed_down_report(
                            Some(pid),
                            &captures,
                            "retained process identity vanished without exit proof".to_string(),
                        );
                    }
                    Err(error) => {
                        return failed_down_report(
                            Some(pid),
                            &captures,
                            format!("retained process exit inspection failed: {error}"),
                        );
                    }
                },
                Err(error) => {
                    return failed_down_report(
                        Some(pid),
                        &captures,
                        format!("retained process revalidation failed: {error}"),
                    );
                }
            };

            let signalled = if already_exited {
                false
            } else {
                match process.terminate() {
                    Ok(signalled) => signalled,
                    Err(error) => {
                        return failed_down_report(
                            Some(pid),
                            &captures,
                            format!("retained process termination failed: {error}"),
                        );
                    }
                }
            };
            if !already_exited {
                match process.wait(Duration::from_secs(10)) {
                    Ok(true) => {}
                    Ok(false) => {
                        let mut report = failed_down_report(
                            Some(pid),
                            &captures,
                            "retained process did not exit within 10 seconds".to_string(),
                        );
                        report.signalled = signalled;
                        return report;
                    }
                    Err(error) => {
                        let mut report = failed_down_report(
                            Some(pid),
                            &captures,
                            format!("retained process exit confirmation failed: {error}"),
                        );
                        report.signalled = signalled;
                        return report;
                    }
                }
            }

            let mut checked = Vec::new();
            if let Err(error) = require_absent_cleanup_ports(&captures, effects, &mut checked) {
                let mut report = failed_down_report(Some(pid), &captures, error);
                report.signalled = signalled;
                report.exit_proven = true;
                return report;
            }
            let (registrations, complete) = cleanup_registrations(&captures, &groups, effects);
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
                diagnostics: (!complete)
                    .then(|| {
                        "managed process exit is confirmed, but registration cleanup was partial"
                            .to_string()
                    })
                    .into_iter()
                    .collect(),
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
    let mut effects = NativeDownEffects { scope };
    emit_down_report(&execute_down_plan(plan, &mut effects))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    lines: Vec<String>,
    success: bool,
}

fn static_doctor_blocker(args: &ServerUpArgs) -> Option<DoctorReport> {
    if args.tailscale {
        return Some(DoctorReport {
            lines: vec![
                "[BLOCKED] --tailscale is fail-closed before registration, PID, engine, model, or network probes because scoped proxy cleanup is unavailable"
                    .to_string(),
                "[next] leave every registration untouched; Ferric will not inspect or signal a PID, delete registration bytes, invoke Tailscale, or run a blind node-wide reset"
                    .to_string(),
            ],
            success: false,
        });
    }

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
    let mut effects = NativeDoctorProbeEffects;
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
mod tests {
    use super::*;
    use crate::server_process::canonical_test_start_token;
    use crate::server_registration::{
        PersistenceEffects, PersistencePhase, PromisedOriginRegistration, RegistrationBlock,
        StagePersistError, capture_registration_path, publish_mirrored_with,
    };
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::fs;
    use std::io;
    use std::net::TcpListener;
    use std::rc::Rc;
    use std::thread;
    use tempfile::NamedTempFile;

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

    fn discovery_fixture_capture(
        scope: RegistrationScope,
        pid: u32,
        name: &str,
    ) -> CapturedRegistration {
        let runfile = discovery_fixture_runfile(pid, name);
        CapturedRegistration {
            scope,
            path: discovery_fixture_path(name),
            raw: serde_json::to_vec_pretty(&runfile).unwrap(),
            runfile,
        }
    }

    fn legacy_adoption_fixture(pid: u32) -> (Vec<CapturedRegistration>, ProcessFacts) {
        let port = u16::try_from(7600 + pid % 100).unwrap();
        let runfile = ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid,
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
        let identity = ProcessIdentity {
            start_token: canonical_test_start_token(u64::from(pid)),
            executable: discovery_fixture_executable(),
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
                port.to_string(),
            ],
        };
        let raw = serde_json::to_vec_pretty(&runfile).unwrap();
        let captures = [
            (RegistrationScope::Local, "legacy-local"),
            (RegistrationScope::Global, "legacy-global"),
        ]
        .into_iter()
        .map(|(scope, name)| CapturedRegistration {
            scope,
            path: discovery_fixture_path(name),
            raw: raw.clone(),
            runfile: runfile.clone(),
        })
        .collect();
        (
            captures,
            ProcessFacts {
                identity,
                listener: ListenerState::OwnedByTarget,
            },
        )
    }

    fn adoption_fixture_replacement_raw(
        captures: &[CapturedRegistration],
        facts: &ProcessFacts,
    ) -> Vec<u8> {
        let mut runfile = captures[0].runfile.clone();
        runfile.schema_version = RUNFILE_SCHEMA_V2;
        runfile.process_identity = Some(facts.identity.clone());
        runfile.origin_local_runfile = Some(captures[0].path.clone());
        serde_json::to_vec_pretty(&runfile).unwrap()
    }

    fn scripted_remove_event(capture: &CapturedRegistration) -> LifecycleEvent {
        LifecycleEvent::Remove(
            capture.path.clone(),
            ferric_bench::sha256_bytes(&capture.raw),
        )
    }

    fn scripted_replace_event(path: &Path, captured: &[u8], replacement: &[u8]) -> LifecycleEvent {
        LifecycleEvent::Replace(
            path.to_path_buf(),
            ferric_bench::sha256_bytes(captured),
            ferric_bench::sha256_bytes(replacement),
        )
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
        let root = tempfile::tempdir().unwrap();
        for (process_case, pid, simulated_present) in
            [("present", 4201, true), ("absent", 4202, false)]
        {
            let registration_path = root
                .path()
                .join(process_case)
                .join("workspace/.ferric/server.json");
            std::fs::create_dir_all(registration_path.parent().unwrap()).unwrap();
            let mut runfile = discovery_fixture_runfile(pid, process_case);
            runfile.tailscale = true;
            runfile.origin_local_runfile = Some(registration_path.clone());
            let mut raw = if simulated_present {
                serde_json::to_vec_pretty(&runfile).unwrap()
            } else {
                serde_json::to_vec(&runfile).unwrap()
            };
            raw.push(b'\n');
            std::fs::write(&registration_path, &raw).unwrap();
            let inventory = RegistrationInventory {
                local: capture_registration_path(RegistrationScope::Local, &registration_path),
                global: None,
                promised_origins: Vec::new(),
            };
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let calls = Rc::clone(&ledger);
            let discovery = discover_inventory_with(
                inventory,
                move |capture| {
                    calls
                        .borrow_mut()
                        .push(LifecycleEvent::Acquire(capture.runfile.pid));
                    if simulated_present {
                        blocked_observation_with_facts(
                            capture,
                            "simulated present process".to_string(),
                            Some(discovery_fixture_identity(u64::from(pid))),
                            Some(ListenerState::OwnedByTarget),
                            HealthState::NotProbed,
                        )
                    } else {
                        stale_observation(
                            capture,
                            "simulated absent process".to_string(),
                            None,
                            ListenerState::Absent,
                        )
                    }
                },
                &mut PanicHealth,
                |_observation| panic!("Tailscale blocker must precede retained reinspection"),
            );
            assert!(ledger.borrow().is_empty(), "{process_case}");
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
            let plan = down_plan_from_lifecycle(discovery);
            let mut effects = ScriptedDownEffects::new(
                Vec::<ListenerState>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, DownDisposition::Blocked);
            assert!(!report.signalled);
            assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
            assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
            assert_eq!(std::fs::read(&registration_path).unwrap(), raw);
        }
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
    fn doctor_tailscale_block_precedes_binary_model_and_network_probes() {
        let mut tailscale_args = doctor_fixture_args();
        tailscale_args.tailscale = true;
        tailscale_args.port = 0;
        tailscale_args.ctx = 0;
        tailscale_args.model = None;
        tailscale_args.parallel = Some(0);
        let mut effects = RecordingDoctorEffects::default();
        let discovery_calls = Rc::new(RefCell::new(0_usize));
        let calls = Rc::clone(&discovery_calls);
        let report = doctor_report_with(
            &tailscale_args,
            move || {
                *calls.borrow_mut() += 1;
                Ok(discovery_fixture_ready())
            },
            &mut effects,
        );
        assert!(!report.success);
        assert_eq!(
            report.lines,
            vec![
                "[BLOCKED] --tailscale is fail-closed before registration, PID, engine, model, or network probes because scoped proxy cleanup is unavailable",
                "[next] leave every registration untouched; Ferric will not inspect or signal a PID, delete registration bytes, invoke Tailscale, or run a blind node-wide reset",
            ]
        );
        assert_eq!(*discovery_calls.borrow(), 0);
        assert!(effects.events.is_empty());
    }

    #[test]
    fn doctor_blocks_before_external_probes() {
        let mut effects = RecordingDoctorEffects::default();
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
        Spawn(u32),
        Acquire(u32),
        PidMapReplace(u32, &'static str),
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
        Revalidate,
        Remove(PathBuf, String),
        RemoveStage(PathBuf, Option<String>),
        Replace(PathBuf, String, String),
        Persistence(PersistencePhase, PathBuf),
        Render,
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

    struct ScriptedPidMapRuntime {
        processes: RefCell<HashMap<u32, ScriptedProcess>>,
        ledger: EventLedger,
    }

    impl ScriptedPidMapRuntime {
        fn new(process: ScriptedProcess, ledger: EventLedger) -> Self {
            Self {
                processes: RefCell::new(HashMap::from([(process.pid(), process)])),
                ledger,
            }
        }

        fn replace(&self, pid: u32, generation: &'static str, process: ScriptedProcess) {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::PidMapReplace(pid, generation));
            self.processes.borrow_mut().insert(pid, process);
        }
    }

    impl ProcessRuntime for ScriptedPidMapRuntime {
        type Process = ScriptedProcess;

        fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
            self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
            self.processes
                .borrow()
                .get(&pid)
                .cloned()
                .ok_or(ProcessError::NotFound(pid))
        }
    }

    struct ScriptedDownEffects {
        revalidations: VecDeque<Result<(), String>>,
        listeners: VecDeque<ListenerState>,
        removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    }

    impl ScriptedDownEffects {
        fn new(
            listeners: impl IntoIterator<Item = ListenerState>,
            removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
            ledger: EventLedger,
        ) -> Self {
            Self {
                revalidations: VecDeque::from([Ok(())]),
                listeners: listeners.into_iter().collect(),
                removals: removals.into_iter().collect(),
                ledger,
            }
        }
    }

    impl DownEffects for ScriptedDownEffects {
        fn revalidate_registrations(
            &mut self,
            _expected: &[RegistrationRevision],
        ) -> Result<(), String> {
            self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
            self.revalidations
                .pop_front()
                .expect("scripted registration revalidation")
        }

        fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Listener(pid, port));
            self.listeners
                .pop_front()
                .expect("scripted down-listener state")
        }

        fn remove(
            &mut self,
            captured: &CapturedRegistration,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger.borrow_mut().push(LifecycleEvent::Remove(
                captured.path.clone(),
                ferric_bench::sha256_bytes(&captured.raw),
            ));
            self.removals
                .pop_front()
                .expect("scripted conditional-removal result")
        }
    }

    struct FilesystemDownEffects {
        scope: ManagedDiscoveryScope,
        listeners: VecDeque<ListenerState>,
        ledger: EventLedger,
    }

    impl DownEffects for FilesystemDownEffects {
        fn revalidate_registrations(
            &mut self,
            expected: &[RegistrationRevision],
        ) -> Result<(), String> {
            self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
            let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
            let current = discovery_revisions(&flatten_inventory(&inventory));
            if current == expected {
                Ok(())
            } else {
                Err("composition inventory changed before teardown".to_string())
            }
        }

        fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Listener(pid, port));
            self.listeners
                .pop_front()
                .expect("scripted composition listener state")
        }

        fn remove(
            &mut self,
            captured: &CapturedRegistration,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger
                .borrow_mut()
                .push(scripted_remove_event(captured));
            remove_if_unchanged(captured)
        }
    }

    struct RevisionCheckingDownEffects {
        scope: ManagedDiscoveryScope,
        listeners: VecDeque<ListenerState>,
        removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    }

    impl DownEffects for RevisionCheckingDownEffects {
        fn revalidate_registrations(
            &mut self,
            expected: &[RegistrationRevision],
        ) -> Result<(), String> {
            self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
            let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
            let current = discovery_revisions(&flatten_inventory(&inventory));
            if current == expected {
                Ok(())
            } else {
                Err("composition inventory changed before teardown".to_string())
            }
        }

        fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Listener(pid, port));
            self.listeners
                .pop_front()
                .expect("scripted transition listener state")
        }

        fn remove(
            &mut self,
            captured: &CapturedRegistration,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger
                .borrow_mut()
                .push(scripted_remove_event(captured));
            self.removals
                .pop_front()
                .expect("scripted transition removal")
        }
    }

    struct ScriptedPublicationEffects {
        final_removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
        stage_removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    }

    impl ScriptedPublicationEffects {
        fn new(
            final_removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
            stage_removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
            ledger: EventLedger,
        ) -> Self {
            Self {
                final_removals: final_removals.into_iter().collect(),
                stage_removals: stage_removals.into_iter().collect(),
                ledger,
            }
        }
    }

    impl PublicationCompensationEffects for ScriptedPublicationEffects {
        fn remove_final(
            &mut self,
            captured: &CapturedRegistration,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger.borrow_mut().push(LifecycleEvent::Remove(
                captured.path.clone(),
                ferric_bench::sha256_bytes(&captured.raw),
            ));
            self.final_removals
                .pop_front()
                .expect("scripted publication-final removal")
        }

        fn remove_stage(
            &mut self,
            stage: &PublicationStage,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger.borrow_mut().push(LifecycleEvent::RemoveStage(
                stage.path.clone(),
                stage.raw.as_deref().map(ferric_bench::sha256_bytes),
            ));
            self.stage_removals
                .pop_front()
                .expect("scripted publication-stage removal")
        }
    }

    struct CompositionPersistenceEffects {
        failure: Option<(PathBuf, PersistencePhase)>,
        retain_stage_after_persist: Option<PathBuf>,
        serializations: usize,
        ledger: EventLedger,
    }

    impl CompositionPersistenceEffects {
        fn default_with(ledger: EventLedger) -> Self {
            Self {
                failure: None,
                retain_stage_after_persist: None,
                serializations: 0,
                ledger,
            }
        }

        fn failing(final_path: &Path, phase: PersistencePhase, ledger: EventLedger) -> Self {
            Self {
                failure: Some((final_path.to_path_buf(), phase)),
                ..Self::default_with(ledger)
            }
        }

        fn retaining_committed_stage(final_path: &Path, ledger: EventLedger) -> Self {
            Self {
                retain_stage_after_persist: Some(final_path.to_path_buf()),
                ..Self::default_with(ledger)
            }
        }

        fn fails(&self, final_path: &Path, phase: PersistencePhase) -> bool {
            self.failure
                .as_ref()
                .is_some_and(|(path, target)| path == final_path && *target == phase)
        }

        fn record(&self, phase: PersistencePhase, final_path: &Path) {
            self.ledger
                .borrow_mut()
                .push(LifecycleEvent::Persistence(phase, final_path.to_path_buf()));
        }

        fn injected(phase: PersistencePhase) -> io::Error {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("injected {phase:?} composition failure"),
            )
        }
    }

    impl PersistenceEffects for CompositionPersistenceEffects {
        fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
            self.serializations += 1;
            serde_json::to_vec_pretty(runfile)
        }

        fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
            self.record(PersistencePhase::CreateStage, final_path);
            if self.fails(final_path, PersistencePhase::CreateStage) {
                return Err(Self::injected(PersistencePhase::CreateStage));
            }
            tempfile::Builder::new()
                .prefix(".server-registration-composition-")
                .tempfile_in(parent)
        }

        fn write_all(
            &mut self,
            final_path: &Path,
            stage: &mut NamedTempFile,
            raw: &[u8],
        ) -> io::Result<()> {
            self.record(PersistencePhase::WriteAll, final_path);
            if self.fails(final_path, PersistencePhase::WriteAll) {
                stage.write_all(&raw[..raw.len().min(7)])?;
                return Err(Self::injected(PersistencePhase::WriteAll));
            }
            stage.write_all(raw)
        }

        fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
            self.record(PersistencePhase::Flush, final_path);
            if self.fails(final_path, PersistencePhase::Flush) {
                return Err(Self::injected(PersistencePhase::Flush));
            }
            stage.as_file_mut().flush()
        }

        fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
            self.record(PersistencePhase::FileSync, final_path);
            if self.fails(final_path, PersistencePhase::FileSync) {
                return Err(Self::injected(PersistencePhase::FileSync));
            }
            stage.as_file().sync_all()
        }

        fn persist_noclobber(
            &mut self,
            final_path: &Path,
            mut stage: NamedTempFile,
        ) -> Result<(), StagePersistError> {
            self.record(PersistencePhase::PersistNoClobber, final_path);
            if self.fails(final_path, PersistencePhase::PersistNoClobber) {
                return Err(StagePersistError {
                    error: Self::injected(PersistencePhase::PersistNoClobber),
                    stage,
                });
            }
            if self
                .retain_stage_after_persist
                .as_ref()
                .is_some_and(|target| target == final_path)
            {
                if let Err(error) = fs::hard_link(stage.path(), final_path) {
                    return Err(StagePersistError { error, stage });
                }
                stage.disable_cleanup(true);
                drop(stage);
                return Ok(());
            }
            stage
                .persist_noclobber(final_path)
                .map(drop)
                .map_err(|error| StagePersistError {
                    error: error.error,
                    stage: error.file,
                })
        }

        fn sync_parent(&mut self, final_path: &Path, _parent: &Path) -> io::Result<()> {
            self.record(PersistencePhase::ParentSync, final_path);
            if self.fails(final_path, PersistencePhase::ParentSync) {
                return Err(Self::injected(PersistencePhase::ParentSync));
            }
            Ok(())
        }
    }

    struct FilesystemPublicationEffects {
        ledger: EventLedger,
    }

    impl PublicationCompensationEffects for FilesystemPublicationEffects {
        fn remove_final(
            &mut self,
            captured: &CapturedRegistration,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger
                .borrow_mut()
                .push(scripted_remove_event(captured));
            remove_if_unchanged(captured)
        }

        fn remove_stage(
            &mut self,
            stage: &PublicationStage,
        ) -> Result<RemovalOutcome, RemovalError> {
            self.ledger.borrow_mut().push(LifecycleEvent::RemoveStage(
                stage.path.clone(),
                stage.raw.as_deref().map(ferric_bench::sha256_bytes),
            ));
            remove_publication_stage_if_unchanged(stage)
        }
    }

    struct ScriptedAdoptionEffects {
        replacements: VecDeque<Result<ReplacementOutcome, ReplacementError>>,
        ledger: EventLedger,
    }

    impl ScriptedAdoptionEffects {
        fn new(
            replacements: impl IntoIterator<Item = Result<ReplacementOutcome, ReplacementError>>,
            ledger: EventLedger,
        ) -> Self {
            Self {
                replacements: replacements.into_iter().collect(),
                ledger,
            }
        }
    }

    impl AdoptionEffects for ScriptedAdoptionEffects {
        fn replace(
            &mut self,
            captured: &CapturedRegistration,
            replacement: &[u8],
        ) -> Result<ReplacementOutcome, ReplacementError> {
            self.ledger.borrow_mut().push(LifecycleEvent::Replace(
                captured.path.clone(),
                ferric_bench::sha256_bytes(&captured.raw),
                ferric_bench::sha256_bytes(replacement),
            ));
            self.replacements
                .pop_front()
                .expect("scripted conditional-replacement result")
        }
    }

    struct FilesystemAdoptionEffects {
        ledger: EventLedger,
    }

    impl AdoptionEffects for FilesystemAdoptionEffects {
        fn replace(
            &mut self,
            captured: &CapturedRegistration,
            replacement: &[u8],
        ) -> Result<ReplacementOutcome, ReplacementError> {
            self.ledger.borrow_mut().push(scripted_replace_event(
                &captured.path,
                &captured.raw,
                replacement,
            ));
            replace_if_unchanged(captured, replacement)
        }
    }

    fn render_down_with_ledger(report: &DownReport, ledger: &EventLedger) -> RenderedDownReport {
        let rendered = render_down_report(report);
        ledger.borrow_mut().push(LifecycleEvent::Render);
        rendered
    }

    fn render_publication_with_ledger(
        report: &PublicationCompletionReport,
        ledger: &EventLedger,
    ) -> RenderedPublicationReport {
        let rendered = render_publication_report(report);
        ledger.borrow_mut().push(LifecycleEvent::Render);
        rendered
    }

    fn render_adoption_with_ledger(
        report: &AdoptionReport,
        ledger: &EventLedger,
    ) -> RenderedAdoptionReport {
        let rendered = render_adoption_report(report);
        ledger.borrow_mut().push(LifecycleEvent::Render);
        rendered
    }

    fn assert_down_failure_kept_recovery(report: &DownReport, rendered: &RenderedDownReport) {
        assert!(!report.success);
        assert_eq!(report.disposition, DownDisposition::Failed);
        assert!(report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        )));
        assert!(
            rendered.stdout.iter().all(|line| !line.contains("stopped")),
            "failure report must never claim stopped: {:?}",
            rendered.stdout
        );
    }

    #[test]
    fn down_signals_only_the_retained_handle() {
        for (case, listener) in [
            ("owned", ListenerState::OwnedByTarget),
            ("absent", ListenerState::Absent),
        ] {
            for recorded_health in [HealthState::Healthy, HealthState::Unhealthy] {
                let pid = if recorded_health == HealthState::Healthy {
                    6101
                } else {
                    6102
                };
                let capture = discovery_fixture_capture(RegistrationScope::Local, pid, case);
                let port = capture.runfile.port;
                let expected = capture.runfile.process_identity.clone().unwrap();
                let ledger = Rc::new(RefCell::new(Vec::new()));
                let process = ScriptedProcess::new(pid, "retained-target", Rc::clone(&ledger))
                    .with_inspection(Ok(ProcessFacts {
                        identity: expected.clone(),
                        listener: listener.clone(),
                    }));
                let mut effects = ScriptedDownEffects::new(
                    [ListenerState::Absent],
                    [Ok(RemovalOutcome::Removed)],
                    Rc::clone(&ledger),
                );

                let mut server = match discovery_fixture_ready().state {
                    ManagedServerState::Ready(server) => server,
                    _ => unreachable!("ready fixture changed state"),
                };
                server.runfile = capture.runfile.clone();
                server.identity = expected.clone();
                server.listener = listener.clone();
                server.health = recorded_health;
                let recorded_state = if recorded_health == HealthState::Healthy
                    && listener == ListenerState::OwnedByTarget
                {
                    ManagedServerState::Ready(server)
                } else {
                    ManagedServerState::Degraded {
                        server,
                        issues: Vec::new(),
                    }
                };

                let report = execute_down_plan(
                    retained_target_down_plan(
                        &recorded_state,
                        process,
                        vec![capture.clone()],
                        Vec::new(),
                    )
                    .unwrap(),
                    &mut effects,
                );
                let rendered = render_down_with_ledger(&report, &ledger);

                assert!(report.success, "{case}, health={recorded_health:?}");
                assert_eq!(report.disposition, DownDisposition::Stopped);
                assert!(report.signalled);
                assert!(report.exit_proven);
                assert!(report.listener_released);
                assert!(rendered.stdout.iter().any(|line| line.contains("stopped")));
                assert_eq!(
                    *ledger.borrow(),
                    vec![
                        LifecycleEvent::Revalidate,
                        LifecycleEvent::Inspect("retained-target", port),
                        LifecycleEvent::Terminate("retained-target"),
                        LifecycleEvent::RetainedWait("retained-target"),
                        LifecycleEvent::Listener(pid, port),
                        scripted_remove_event(&capture),
                        LifecycleEvent::Render,
                    ],
                    "HTTP health={recorded_health:?} must not affect retained-handle teardown"
                );
            }
        }
    }

    #[test]
    fn down_exit_and_listener_postconditions_gate_success() {
        let capture = discovery_fixture_capture(RegistrationScope::Local, 6201, "down-gates");
        let pid = capture.runfile.pid;
        let port = capture.runfile.port;
        let expected = capture.runfile.process_identity.clone().unwrap();
        let facts = ProcessFacts {
            identity: expected.clone(),
            listener: ListenerState::OwnedByTarget,
        };

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "signal-error", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_terminate(Err(ProcessError::Operation("access denied".to_string())));
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_down_failure_kept_recovery(&report, &rendered);
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("signal-error", port),
                LifecycleEvent::Terminate("signal-error"),
                LifecycleEvent::Render,
            ]
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "revision-change", Rc::clone(&ledger));
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        effects.revalidations = VecDeque::from([Err(
            "a conflicting registration appeared before signal".to_string(),
        )]);
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_down_failure_kept_recovery(&report, &rendered);
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::Revalidate, LifecycleEvent::Render],
            "a changed inventory must block before retained inspection, signal, wait, listener, or cleanup"
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "exit-before-signal", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_terminate(Ok(false))
            .with_wait(Ok(true));
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert!(report.success);
        assert_eq!(report.disposition, DownDisposition::AlreadyExited);
        assert!(!report.signalled);
        assert!(report.exit_proven);
        assert!(report.listener_released);
        let expected_state = format!(
            "[state] managed server pid {pid} was already exited; no process was signalled"
        );
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some(expected_state.as_str())
        );
        assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("exit-before-signal", port),
                LifecycleEvent::Terminate("exit-before-signal"),
                LifecycleEvent::RetainedWait("exit-before-signal"),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&capture),
                LifecycleEvent::Render,
            ],
            "an inspect-to-terminate exit race must still prove exit and listener release before cleanup"
        );

        for (generation, wait) in [
            ("wait-timeout", Ok(false)),
            (
                "wait-error",
                Err(ProcessError::Operation("wait unavailable".to_string())),
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, generation, Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_wait(wait);
            let mut effects = ScriptedDownEffects::new(
                Vec::<ListenerState>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(
                DownPlan::Target {
                    process,
                    expected: expected.clone(),
                    pid,
                    port,
                    captures: vec![capture.clone()],
                    expected_revisions: Vec::new(),
                },
                &mut effects,
            );
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_down_failure_kept_recovery(&report, &rendered);
            assert!(report.signalled);
            assert!(!report.exit_proven);
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Inspect(generation, port),
                    LifecycleEvent::Terminate(generation),
                    LifecycleEvent::RetainedWait(generation),
                    LifecycleEvent::Render,
                ]
            );
        }

        for (case, residual) in [
            ("target", ListenerState::OwnedByTarget),
            ("wildcard", ListenerState::OwnedByTargetWildcard),
            ("foreign", ListenerState::OwnedByOther(vec![7777])),
            (
                "uninspectable",
                ListenerState::Uninspectable("access denied".to_string()),
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, "listener-held", Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()));
            let mut effects = ScriptedDownEffects::new(
                [residual],
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(
                DownPlan::Target {
                    process,
                    expected: expected.clone(),
                    pid,
                    port,
                    captures: vec![capture.clone()],
                    expected_revisions: Vec::new(),
                },
                &mut effects,
            );
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_down_failure_kept_recovery(&report, &rendered);
            assert!(report.signalled, "{case}");
            assert!(report.exit_proven, "{case}");
            assert!(!report.listener_released, "{case}");
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Inspect("listener-held", port),
                    LifecycleEvent::Terminate("listener-held"),
                    LifecycleEvent::RetainedWait("listener-held"),
                    LifecycleEvent::Listener(pid, port),
                    LifecycleEvent::Render,
                ],
                "{case}"
            );
        }

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "released", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()));
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::Stopped);
        assert!(report.success);
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.starts_with("[state] stopped"))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("released", port),
                LifecycleEvent::Terminate("released"),
                LifecycleEvent::RetainedWait("released"),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&capture),
                LifecycleEvent::Render,
            ]
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "already-exited", Rc::clone(&ledger))
            .with_inspection(Err(ProcessError::NotFound(pid)));
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected,
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert!(report.success);
        assert_eq!(report.disposition, DownDisposition::AlreadyExited);
        assert!(!report.signalled);
        assert!(report.exit_proven);
        assert!(
            rendered
                .stdout
                .iter()
                .all(|line| !line.contains("[state] stopped"))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("already-exited", port),
                LifecycleEvent::RetainedWait("already-exited"),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&capture),
                LifecycleEvent::Render,
            ]
        );
    }

    #[test]
    fn down_cleanup_outcome_matrix() {
        let stopped_capture = discovery_fixture_capture(RegistrationScope::Local, 6299, "stopped");
        let stopped_expected = stopped_capture.runfile.process_identity.clone().unwrap();
        let stopped_port = stopped_capture.runfile.port;
        let stopped_ledger = Rc::new(RefCell::new(Vec::new()));
        let stopped_process = ScriptedProcess::new(6299, "stopped", Rc::clone(&stopped_ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: stopped_expected.clone(),
                listener: ListenerState::OwnedByTarget,
            }));
        let mut stopped_effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&stopped_ledger),
        );
        let stopped_report = execute_down_plan(
            DownPlan::Target {
                process: stopped_process,
                expected: stopped_expected,
                pid: 6299,
                port: stopped_port,
                captures: vec![stopped_capture],
                expected_revisions: Vec::new(),
            },
            &mut stopped_effects,
        );
        let stopped_rendered = render_down_with_ledger(&stopped_report, &stopped_ledger);
        assert_eq!(stopped_report.disposition, DownDisposition::Stopped);
        assert!(stopped_report.success);
        assert!(
            stopped_rendered
                .stdout
                .last()
                .is_some_and(|line| line.starts_with("[state] stopped"))
        );

        let holding = discovery_fixture_path("holding");
        let cases = vec![
            ("removed", Ok(RemovalOutcome::Removed), "[removed]", true),
            (
                "absent",
                Ok(RemovalOutcome::Absent),
                "[already-absent]",
                true,
            ),
            (
                "replacement",
                Ok(RemovalOutcome::ReplacementPreserved {
                    path: holding.clone(),
                    detail: "concurrent replacement retained".to_string(),
                }),
                "[replacement-preserved]",
                false,
            ),
            (
                "restore",
                Err(RemovalError {
                    path: discovery_fixture_path("restore"),
                    kind: RemovalFailureKind::Restore,
                    detail: "restore denied".to_string(),
                    preserved_at: Some(holding.clone()),
                }),
                "[restore-failed]",
                false,
            ),
            (
                "remove",
                Err(RemovalError {
                    path: discovery_fixture_path("remove"),
                    kind: RemovalFailureKind::Remove,
                    detail: "remove denied".to_string(),
                    preserved_at: Some(holding.clone()),
                }),
                "[removal-failed]",
                false,
            ),
            (
                "other",
                Err(RemovalError {
                    path: discovery_fixture_path("other"),
                    kind: RemovalFailureKind::Other,
                    detail: "holding directory cleanup failed".to_string(),
                    preserved_at: Some(holding.clone()),
                }),
                "[cleanup-failed]",
                false,
            ),
        ];

        for (index, (name, outcome, marker, complete)) in cases.into_iter().enumerate() {
            let pid = 6301 + u32::try_from(index).unwrap();
            let capture = discovery_fixture_capture(RegistrationScope::Local, pid, name);
            let port = capture.runfile.port;
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut effects =
                ScriptedDownEffects::new([ListenerState::Absent], [outcome], Rc::clone(&ledger));
            let report = execute_down_plan(
                DownPlan::<ScriptedProcess>::Stale {
                    captures: vec![capture.clone()],
                    expected_revisions: Vec::new(),
                },
                &mut effects,
            );
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_eq!(report.success, complete, "{name}");
            assert_eq!(
                report.disposition,
                if complete {
                    DownDisposition::StaleCleaned
                } else {
                    DownDisposition::CleanupPartial
                },
                "{name}"
            );
            let row = rendered
                .stdout
                .iter()
                .find(|line| line.contains(marker))
                .unwrap_or_else(|| {
                    panic!("missing {marker} row for {name}: {:?}", rendered.stdout)
                });
            if !complete {
                assert!(
                    row.contains(&holding.display().to_string()),
                    "{name}: {row}"
                );
            }
            assert_eq!(
                rendered.stdout.last().map(String::as_str),
                Some(if complete {
                    "[state] stale-cleaned"
                } else {
                    "[state] exit/quiescence confirmed where applicable; cleanup partial"
                }),
                "{name}"
            );
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Listener(pid, port),
                    scripted_remove_event(&capture),
                    LifecycleEvent::Render,
                ],
                "{name}"
            );
        }

        let local = discovery_fixture_capture(RegistrationScope::Local, 6401, "partial-local");
        let mut global = local.clone();
        global.scope = RegistrationScope::Global;
        global.path = discovery_fixture_path("partial-global");
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent, ListenerState::Absent],
            [
                Ok(RemovalOutcome::Removed),
                Err(RemovalError {
                    path: global.path.clone(),
                    kind: RemovalFailureKind::Remove,
                    detail: "second alias retained".to_string(),
                    preserved_at: Some(holding.clone()),
                }),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::<ScriptedProcess>::Stale {
                captures: vec![local.clone(), global.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::CleanupPartial);
        assert!(!report.success);
        assert_eq!(report.registrations[0].coordinate.path, local.path);
        assert_eq!(report.registrations[1].coordinate.path, global.path);
        assert!(
            rendered.stdout[1].contains(&holding.display().to_string()),
            "every recovery path must survive rendering: {:?}",
            rendered.stdout
        );
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some("[state] exit/quiescence confirmed where applicable; cleanup partial")
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Listener(local.runfile.pid, local.runfile.port),
                scripted_remove_event(&local),
                scripted_remove_event(&global),
                LifecycleEvent::Render,
            ]
        );

        let physical =
            discovery_fixture_capture(RegistrationScope::Local, 6410, "same-physical-path");
        let mut duplicate_alias = physical.clone();
        duplicate_alias.scope = RegistrationScope::Global;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::<ScriptedProcess>::Stale {
                captures: vec![physical.clone(), duplicate_alias.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert!(report.success);
        assert_eq!(report.registrations.len(), 2);
        assert!(
            report.registrations.iter().all(|registration| matches!(
                registration.outcome,
                DownRegistrationOutcome::Removed
            ))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Listener(physical.runfile.pid, physical.runfile.port),
                scripted_remove_event(&physical),
                LifecycleEvent::Render,
            ],
            "one physical path must receive one conditional removal while retaining both alias reports"
        );
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some("[state] stale-cleaned")
        );

        duplicate_alias.raw.push(b' ');
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::<ScriptedProcess>::Stale {
                captures: vec![physical, duplicate_alias],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_down_failure_kept_recovery(&report, &rendered);
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
    }

    #[test]
    fn ambiguous_or_unverifiable_down_is_non_mutating() {
        fn write_runfile_slot(
            scope: RegistrationScope,
            path: &Path,
            runfile: &ServerRunfile,
        ) -> (RegistrationSlot, Vec<u8>) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let raw = serde_json::to_vec_pretty(runfile).unwrap();
            std::fs::write(path, &raw).unwrap();
            (capture_registration_path(scope, path), raw)
        }

        let root = tempfile::tempdir().unwrap();
        for blocker in [
            "two live keys",
            "malformed peer",
            "unreadable peer",
            "live schema-1 registration",
            "wildcard listener",
            "shared listener",
            "foreign listener",
            "uninspectable listener",
            "invalid creation token",
            "durable Tailscale state",
        ] {
            let case_dir = root.path().join(blocker.replace(' ', "-"));
            let local_path = case_dir
                .join("workspace")
                .join(".ferric")
                .join("server.json");
            let global_path = case_dir.join("global.json");
            let ledger = Rc::new(RefCell::new(Vec::new()));

            let mut local_runfile = discovery_fixture_runfile(6450, "blocked-local");
            local_runfile.origin_local_runfile = Some(local_path.clone());
            let (inventory, originals, expected_acquires) = match blocker {
                "malformed peer" => {
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    std::fs::create_dir_all(global_path.parent().unwrap()).unwrap();
                    let malformed = b"{not-valid-json".to_vec();
                    std::fs::write(&global_path, &malformed).unwrap();
                    (
                        RegistrationInventory {
                            local,
                            global: Some(capture_registration_path(
                                RegistrationScope::Global,
                                &global_path,
                            )),
                            promised_origins: Vec::new(),
                        },
                        vec![
                            (local_path.clone(), local_raw),
                            (global_path.clone(), malformed),
                        ],
                        0,
                    )
                }
                "unreadable peer" => {
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    (
                        RegistrationInventory {
                            local,
                            global: Some(RegistrationSlot::Blocked {
                                scope: RegistrationScope::Global,
                                path: global_path,
                                reason: RegistrationBlock::Unreadable(
                                    "injected permission denial".to_string(),
                                ),
                            }),
                            promised_origins: Vec::new(),
                        },
                        vec![(local_path.clone(), local_raw)],
                        0,
                    )
                }
                "invalid creation token" => {
                    local_runfile.process_identity.as_mut().unwrap().start_token =
                        "invalid-token".to_string();
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    (
                        RegistrationInventory {
                            local,
                            global: None,
                            promised_origins: Vec::new(),
                        },
                        vec![(local_path.clone(), local_raw)],
                        0,
                    )
                }
                "durable Tailscale state" => {
                    local_runfile.tailscale = true;
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    (
                        RegistrationInventory {
                            local,
                            global: None,
                            promised_origins: Vec::new(),
                        },
                        vec![(local_path.clone(), local_raw)],
                        0,
                    )
                }
                "live schema-1 registration" => {
                    local_runfile.schema_version = 1;
                    local_runfile.process_identity = None;
                    local_runfile.origin_local_runfile = None;
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    let (global, global_raw) =
                        write_runfile_slot(RegistrationScope::Global, &global_path, &local_runfile);
                    (
                        RegistrationInventory {
                            local,
                            global: Some(global),
                            promised_origins: Vec::new(),
                        },
                        vec![
                            (local_path.clone(), local_raw),
                            (global_path.clone(), global_raw),
                        ],
                        2,
                    )
                }
                "two live keys" => {
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    let mut global_runfile = local_runfile.clone();
                    global_runfile.pid = 6451;
                    global_runfile.process_identity = Some(discovery_fixture_identity(6451));
                    let (global, global_raw) = write_runfile_slot(
                        RegistrationScope::Global,
                        &global_path,
                        &global_runfile,
                    );
                    (
                        RegistrationInventory {
                            local,
                            global: Some(global),
                            promised_origins: Vec::new(),
                        },
                        vec![
                            (local_path.clone(), local_raw),
                            (global_path.clone(), global_raw),
                        ],
                        2,
                    )
                }
                _ => {
                    let (local, local_raw) =
                        write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                    (
                        RegistrationInventory {
                            local,
                            global: None,
                            promised_origins: Vec::new(),
                        },
                        vec![(local_path.clone(), local_raw)],
                        1,
                    )
                }
            };
            let expected_held = flatten_inventory(&inventory)
                .iter()
                .filter(|observation| {
                    !matches!(observation.state, ManagedRegistrationState::Absent)
                })
                .count();
            let observe_ledger = Rc::clone(&ledger);
            let discovery = discover_inventory_before_health_with(inventory, move |capture| {
                observe_ledger
                    .borrow_mut()
                    .push(LifecycleEvent::Acquire(capture.runfile.pid));
                let listener = match blocker {
                    "wildcard listener" => ListenerState::OwnedByTargetWildcard,
                    "shared listener" => ListenerState::OwnedByOther(vec![6450, 6451]),
                    "foreign listener" => ListenerState::OwnedByOther(vec![6451]),
                    "uninspectable listener" => ListenerState::Uninspectable("denied".to_string()),
                    _ => ListenerState::OwnedByTarget,
                };
                let state = if blocker == "live schema-1 registration"
                    || blocker == "uninspectable listener"
                {
                    CandidateState::Unverifiable {
                        reason: blocker.to_string(),
                        observed_identity: capture.runfile.process_identity.clone(),
                        listener: Some(listener),
                        health: HealthState::NotProbed,
                    }
                } else {
                    CandidateState::Verified {
                        identity: capture.runfile.process_identity.clone().unwrap(),
                        listener,
                        health: HealthState::NotProbed,
                    }
                };
                let label = registration_label(capture.scope, &capture.path);
                LifecycleObservation {
                    candidate: Candidate {
                        coordinate: RegistrationCoordinate {
                            scope: capture.scope,
                            path: capture.path.clone(),
                        },
                        runfile: Some(capture.runfile.clone()),
                        state,
                    },
                    label,
                    capture: Some(capture),
                    process: None,
                }
            });
            assert!(
                down_mutation_blocker(&discovery.managed.state).is_some(),
                "{blocker} must become a typed mutation blocker through inventory and resolution"
            );
            let plan = down_plan_from_lifecycle(discovery);
            let mut effects = ScriptedDownEffects::new(
                Vec::<ListenerState>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, DownDisposition::Blocked, "{blocker}");
            assert!(!report.success, "{blocker}");
            assert!(!report.signalled, "{blocker}");
            assert_eq!(report.registrations.len(), expected_held, "{blocker}");
            assert!(report.registrations.iter().all(|registration| matches!(
                registration.outcome,
                DownRegistrationOutcome::Held { .. }
            )));
            assert!(
                rendered.stdout.iter().all(|line| !line.contains("stopped")),
                "{blocker}: {:?}",
                rendered.stdout
            );
            assert_eq!(
                ledger
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, LifecycleEvent::Acquire(_)))
                    .count(),
                expected_acquires,
                "{blocker} must stop acquiring as soon as its real trigger becomes authoritative"
            );
            assert!(
                ledger.borrow().iter().all(|event| matches!(
                    event,
                    LifecycleEvent::Acquire(_) | LifecycleEvent::Render
                )),
                "{blocker} must have empty signal/listener/delete/HTTP ledgers: {:?}",
                ledger.borrow()
            );
            for (path, original) in originals {
                assert_eq!(std::fs::read(path).unwrap(), original, "{blocker}");
            }
        }
    }

    #[test]
    fn tailscale_blocked_commands_preserve_records_and_never_reset() {
        let root = tempfile::tempdir().unwrap();
        let registration_path = root.path().join("workspace/.ferric/server.json");
        std::fs::create_dir_all(registration_path.parent().unwrap()).unwrap();

        let mut runfile = discovery_fixture_runfile(6490, "tailscale-preserved");
        runfile.tailscale = true;
        runfile.origin_local_runfile = Some(registration_path.clone());
        let mut original = serde_json::to_vec_pretty(&runfile).unwrap();
        original.push(b'\n');
        std::fs::write(&registration_path, &original).unwrap();

        let inventory = RegistrationInventory {
            local: capture_registration_path(RegistrationScope::Local, &registration_path),
            global: None,
            promised_origins: Vec::new(),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let acquire_ledger = Rc::clone(&ledger);
        let discovery = discover_inventory_before_health_with(inventory, move |_capture| {
            acquire_ledger
                .borrow_mut()
                .push(LifecycleEvent::Acquire(6490));
            panic!("Tailscale registration must block before process acquisition")
        });
        assert!(ledger.borrow().is_empty());

        let status = render_status(&status_report(&discovery.managed));
        let expected_guidance = format!(
            "[next] registration port {} claims durable Tailscale Serve state; scoped proxy cleanup is unavailable, so Ferric will not inspect or signal its PID, delete its registration, invoke Tailscale, or run a blind node-wide reset; inspect and remove only that exact Serve endpoint with Tailscale tooling",
            runfile.port
        );
        assert_eq!(status.stdout.last(), Some(&expected_guidance));
        assert!(!status.success);
        assert_eq!(std::fs::read(&registration_path).unwrap(), original);

        let plan = down_plan_from_lifecycle(discovery);
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(plan, &mut effects);
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::Blocked);
        assert!(!report.success);
        assert!(!report.signalled);
        assert!(!report.exit_proven);
        assert_eq!(rendered.stdout.last(), Some(&expected_guidance));
        assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
        assert_eq!(std::fs::read(&registration_path).unwrap(), original);
    }

    #[test]
    fn live_v1_guidance_and_explicit_adoption() {
        let pid = 6501;
        let (captures, facts) = legacy_adoption_fixture(pid);
        let adopted_raw = adoption_fixture_replacement_raw(&captures, &facts);
        let coordinates = captures
            .iter()
            .map(|capture| RegistrationCoordinate {
                scope: capture.scope,
                path: capture.path.clone(),
            })
            .collect::<Vec<_>>();
        let issues = vec![ResolutionIssue {
            coordinates: coordinates.clone(),
            kind: ResolutionIssueKind::Unverifiable,
            detail: format!(
                "live schema-1 PID {pid} has no creation identity and cannot authorize teardown"
            ),
        }];
        let inventory = RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(captures[0].clone())),
            global: Some(RegistrationSlot::Captured(Box::new(captures[1].clone()))),
            promised_origins: Vec::new(),
        };
        let managed_observations = captures
            .iter()
            .enumerate()
            .map(|(index, capture)| ManagedRegistrationObservation {
                id: ObservationId(index),
                coordinate: coordinates[index].clone(),
                promised: None,
                state: ManagedRegistrationState::Captured {
                    runfile: Box::new(capture.runfile.clone()),
                    raw_sha256: format!("legacy-{index}"),
                    runtime: RuntimeObservation::LegacyLive { pid },
                },
            })
            .collect::<Vec<_>>();
        let managed = ManagedServerDiscovery {
            inventory,
            observations: managed_observations,
            state: ManagedServerState::Unverifiable {
                issues: issues.clone(),
            },
        };
        let rendered_status = render_status(&status_report(&managed));
        let expected_command = format!("ferric server adopt --pid {pid}");
        assert!(
            rendered_status
                .stdout
                .iter()
                .any(|line| line.contains(&expected_command))
        );
        let mut global_only = managed.clone();
        global_only.inventory.local = RegistrationSlot::Absent {
            scope: coordinates[0].scope,
            path: coordinates[0].path.clone(),
        };
        global_only
            .observations
            .retain(|observation| observation.coordinate.scope == RegistrationScope::Global);
        assert!(matches!(
            status_report(&global_only).next_action,
            StatusNextAction::RepairUnverifiable { .. }
        ));

        let plan = down_plan_from_lifecycle(LifecycleDiscovery {
            managed,
            observations: Vec::new(),
            resolution: Resolution::Unverifiable { issues },
        });
        let down_ledger = Rc::new(RefCell::new(Vec::new()));
        let mut down_effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&down_ledger),
        );
        let down_report = execute_down_plan(plan, &mut down_effects);
        let rendered_down = render_down_with_ledger(&down_report, &down_ledger);
        assert_eq!(down_report.disposition, DownDisposition::Blocked);
        assert_eq!(down_report.registrations.len(), 2);
        assert!(
            down_report
                .registrations
                .iter()
                .all(|registration| matches!(
                    registration.outcome,
                    DownRegistrationOutcome::Held { .. }
                ))
        );
        assert!(
            rendered_down
                .stdout
                .iter()
                .any(|line| line.contains(&expected_command))
        );
        assert_eq!(*down_ledger.borrow(), vec![LifecycleEvent::Render]);

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "legacy-generation", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);

        assert!(report.success);
        assert_eq!(report.disposition, AdoptionDisposition::Adopted);
        assert!(report.identity_validated);
        assert!(report.listener_validated);
        assert!(report.final_generation_revalidated);
        assert!(report.registrations.iter().all(|registration| {
            matches!(registration.transition, AdoptionAliasTransition::Adopted)
                && registration.rollback.is_none()
        }));
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains("without signalling"))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect("legacy-generation", captures[0].runfile.port),
                LifecycleEvent::RetainedWait("legacy-generation"),
                scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
                scripted_replace_event(&captures[1].path, &captures[1].raw, &adopted_raw),
                LifecycleEvent::Inspect("legacy-generation", captures[0].runfile.port),
                LifecycleEvent::Render,
            ]
        );
    }

    #[test]
    fn legacy_adoption_transition_and_rollback_matrix() {
        let pid = 6601;
        let (captures, facts) = legacy_adoption_fixture(pid);
        let adopted_raw = adoption_fixture_replacement_raw(&captures, &facts);
        let port = captures[0].runfile.port;

        let blocked_rows = [
            (
                "executable",
                {
                    let mut changed = facts.clone();
                    changed.identity.executable = if cfg!(windows) {
                        PathBuf::from(r"C:\fixture\python.exe")
                    } else {
                        PathBuf::from("/fixture/python")
                    };
                    changed
                },
                "closed",
            ),
            (
                "argv",
                {
                    let mut changed = facts.clone();
                    changed
                        .identity
                        .argv
                        .extend(["--port".to_string(), (port + 1).to_string()]);
                    changed
                },
                "conflicting registered port",
            ),
            (
                "listener",
                {
                    let mut changed = facts.clone();
                    changed.listener = ListenerState::OwnedByTargetWildcard;
                    changed
                },
                "not exclusively owned",
            ),
            (
                "listener-absent",
                {
                    let mut changed = facts.clone();
                    changed.listener = ListenerState::Absent;
                    changed
                },
                "not exclusively owned",
            ),
            (
                "listener-foreign",
                {
                    let mut changed = facts.clone();
                    changed.listener = ListenerState::OwnedByOther(vec![9999]);
                    changed
                },
                "not exclusively owned",
            ),
            (
                "listener-uninspectable",
                {
                    let mut changed = facts.clone();
                    changed.listener =
                        ListenerState::Uninspectable("listener table denied".to_string());
                    changed
                },
                "not exclusively owned",
            ),
        ];
        for (case, blocked_facts, expected_fragment) in blocked_rows {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_inspection(Ok(blocked_facts));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut effects = ScriptedAdoptionEffects::new(
                Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            let rendered = render_adoption_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, AdoptionDisposition::Blocked, "{case}");
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.contains(expected_fragment)),
                "{case}: {:?}",
                report.diagnostics
            );
            assert!(
                rendered
                    .stdout
                    .iter()
                    .all(|line| !line.contains("adopted live"))
            );
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Acquire(pid),
                    LifecycleEvent::Inspect(case, port),
                    LifecycleEvent::Render,
                ],
                "{case} must not wait, replace, rollback, or signal"
            );
        }

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let runtime = ScriptedRuntime::new(
            Err("retained handle unavailable".to_string()),
            Rc::clone(&ledger),
        );
        let mut effects = ScriptedAdoptionEffects::new(
            Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Blocked);
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::Acquire(pid), LifecycleEvent::Render]
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "inspect-error", Rc::clone(&ledger))
            .with_inspection(Err(ProcessError::Operation(
                "inspection denied".to_string(),
            )));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Blocked);
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect("inspect-error", port),
                LifecycleEvent::Render,
            ]
        );

        for (case, wait) in [
            ("exited-during-validation", Ok(true)),
            (
                "wait-error",
                Err(ProcessError::Operation("wait denied".to_string())),
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_wait(wait);
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut effects = ScriptedAdoptionEffects::new(
                Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            render_adoption_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, AdoptionDisposition::Blocked, "{case}");
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Acquire(pid),
                    LifecycleEvent::Inspect(case, port),
                    LifecycleEvent::RetainedWait(case),
                    LifecycleEvent::Render,
                ],
                "{case}"
            );
        }

        for failed_index in 0..captures.len() {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, "alias-failure", Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_wait(Ok(false));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut outcomes = (0..failed_index)
                .map(|_| Ok(ReplacementOutcome::Replaced))
                .collect::<Vec<_>>();
            outcomes.push(Ok(ReplacementOutcome::Absent));
            outcomes.extend((0..failed_index).map(|_| Ok(ReplacementOutcome::Replaced)));
            let mut effects = ScriptedAdoptionEffects::new(outcomes, Rc::clone(&ledger));
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            let rendered = render_adoption_with_ledger(&report, &ledger);
            assert!(!report.success);
            let expected_disposition = if failed_index == 0 {
                AdoptionDisposition::Failed
            } else {
                AdoptionDisposition::RecoveryPartial
            };
            assert_eq!(
                report.disposition, expected_disposition,
                "alias {failed_index} is not proven restored at its original path"
            );
            let expected_state = if failed_index == 0 {
                "[state] adoption failed before any committed replacement"
            } else {
                "[state] adoption failed; recovery partial"
            };
            assert_eq!(
                rendered.stdout.last().map(String::as_str),
                Some(expected_state),
                "alias {failed_index}"
            );
            assert!(matches!(
                report.registrations[failed_index].transition,
                AdoptionAliasTransition::Absent
            ));
            for earlier in &report.registrations[..failed_index] {
                assert_eq!(
                    earlier.rollback,
                    Some(AdoptionRollbackOutcome::LegacyRestored)
                );
            }
            assert!(
                ledger
                    .borrow()
                    .iter()
                    .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
                "alias {failed_index}"
            );
        }

        let holding = discovery_fixture_path("adoption-holding");
        for (case, outcome, marker) in [
            (
                "forward-replacement",
                Ok(ReplacementOutcome::ReplacementPreserved {
                    path: holding.clone(),
                    detail: "concurrent replacement retained".to_string(),
                }),
                "replacement-preserved",
            ),
            (
                "forward-error",
                Err(ReplacementError {
                    path: captures[0].path.clone(),
                    detail: "replacement publish failed".to_string(),
                    preserved_at: Some(holding.clone()),
                    replacement_committed: false,
                }),
                "replace-failed",
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_wait(Ok(false));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut effects = ScriptedAdoptionEffects::new([outcome], Rc::clone(&ledger));
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            let rendered = render_adoption_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, AdoptionDisposition::Failed, "{case}");
            assert_eq!(
                rendered.stdout.last().map(String::as_str),
                Some("[state] adoption failed before any committed replacement"),
                "{case}"
            );
            let failed_row = rendered
                .stdout
                .iter()
                .find(|line| line.contains(marker))
                .unwrap();
            assert!(failed_row.contains(&holding.display().to_string()));
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Acquire(pid),
                    LifecycleEvent::Inspect(case, port),
                    LifecycleEvent::RetainedWait(case),
                    scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
                    LifecycleEvent::Render,
                ],
                "{case}"
            );
        }

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut changed_facts = facts.clone();
        changed_facts.identity.start_token = canonical_test_start_token(9999);
        let process = ScriptedProcess::new(pid, "identity-transition", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_inspection(Ok(changed_facts))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::RolledBack);
        assert!(report.registrations.iter().all(|registration| {
            registration.rollback == Some(AdoptionRollbackOutcome::LegacyRestored)
        }));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect("identity-transition", port),
                LifecycleEvent::RetainedWait("identity-transition"),
                scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
                scripted_replace_event(&captures[1].path, &captures[1].raw, &adopted_raw),
                LifecycleEvent::Inspect("identity-transition", port),
                scripted_replace_event(&captures[1].path, &adopted_raw, &captures[1].raw),
                scripted_replace_event(&captures[0].path, &adopted_raw, &captures[0].raw),
                LifecycleEvent::Render,
            ],
            "final generation failure must rollback in reverse order"
        );

        for (case, final_inspection) in [
            (
                "final-inspect-error",
                Err(ProcessError::Operation(
                    "final inspection denied".to_string(),
                )),
            ),
            (
                "final-listener-change",
                Ok(ProcessFacts {
                    identity: facts.identity.clone(),
                    listener: ListenerState::OwnedByOther(vec![9999]),
                }),
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_inspection(final_inspection)
                .with_wait(Ok(false));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut effects = ScriptedAdoptionEffects::new(
                [
                    Ok(ReplacementOutcome::Replaced),
                    Ok(ReplacementOutcome::Replaced),
                    Ok(ReplacementOutcome::Replaced),
                    Ok(ReplacementOutcome::Replaced),
                ],
                Rc::clone(&ledger),
            );
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            render_adoption_with_ledger(&report, &ledger);
            assert_eq!(
                report.disposition,
                AdoptionDisposition::RolledBack,
                "{case}"
            );
            assert!(report.registrations.iter().all(|registration| {
                registration.rollback == Some(AdoptionRollbackOutcome::LegacyRestored)
            }));
            assert_eq!(
                ledger
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, LifecycleEvent::Replace(_, _, _)))
                    .count(),
                4,
                "{case}"
            );
            assert!(
                ledger
                    .borrow()
                    .iter()
                    .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
                "{case}"
            );
        }

        for (case, rollback, expected_rollback) in [
            (
                "concurrent",
                Ok(ReplacementOutcome::ReplacementPreserved {
                    path: holding.clone(),
                    detail: "concurrent winner kept".to_string(),
                }),
                "replacement-preserved",
            ),
            (
                "rollback-error",
                Err(ReplacementError {
                    path: captures[0].path.clone(),
                    detail: "rollback durability failed".to_string(),
                    preserved_at: Some(holding.clone()),
                    replacement_committed: false,
                }),
                "failed",
            ),
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_inspection(Ok(facts.clone()))
                .with_wait(Ok(false));
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let mut effects = ScriptedAdoptionEffects::new(
                [
                    Ok(ReplacementOutcome::Replaced),
                    Ok(ReplacementOutcome::Absent),
                    rollback,
                ],
                Rc::clone(&ledger),
            );
            let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
            let rendered = render_adoption_with_ledger(&report, &ledger);
            assert_eq!(
                report.disposition,
                AdoptionDisposition::RecoveryPartial,
                "{case}"
            );
            assert_eq!(
                rendered.stdout.last().map(String::as_str),
                Some("[state] adoption failed; recovery partial"),
                "{case}"
            );
            let local_row = rendered
                .stdout
                .iter()
                .find(|line| line.contains(&captures[0].path.display().to_string()))
                .unwrap();
            assert!(local_row.contains(expected_rollback), "{case}: {local_row}");
            assert!(
                local_row.contains(&holding.display().to_string()),
                "{case}: {local_row}"
            );
            assert!(
                ledger
                    .borrow()
                    .iter()
                    .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
                "{case}"
            );
        }

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "rollback-absent", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Absent),
                Ok(ReplacementOutcome::Absent),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::RecoveryPartial);
        assert_eq!(
            report.registrations[0].rollback,
            Some(AdoptionRollbackOutcome::Absent)
        );
        let restored_row = rendered
            .stdout
            .iter()
            .find(|line| line.contains(&captures[0].path.display().to_string()))
            .unwrap();
        assert!(restored_row.contains("rollback=absent"), "{restored_row}");
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some("[state] adoption failed; recovery partial")
        );
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Terminate(_)))
        );

        let mut same_path_alias = captures[0].clone();
        same_path_alias.scope = RegistrationScope::Global;
        let same_path_captures = vec![captures[0].clone(), same_path_alias.clone()];
        assert_eq!(adoption_mutation_groups(&same_path_captures).len(), 1);
        let alias_root = tempfile::tempdir().unwrap();
        std::fs::create_dir(alias_root.path().join("nested")).unwrap();
        let alias_file = alias_root.path().join("registration.json");
        std::fs::write(&alias_file, &captures[0].raw).unwrap();
        let mut direct_alias = captures[0].clone();
        direct_alias.path = alias_file.clone();
        let mut lexical_alias = same_path_alias.clone();
        lexical_alias.path = alias_root
            .path()
            .join("nested")
            .join("..")
            .join(alias_file.file_name().unwrap());
        assert!(
            validate_mutation_path_aliases(&[direct_alias, lexical_alias]).is_err(),
            "distinct path spellings of one entry must block rather than collapse mutation reports"
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "same-path", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects =
            ScriptedAdoptionEffects::new([Ok(ReplacementOutcome::Replaced)], Rc::clone(&ledger));
        let report =
            execute_legacy_adoption(same_path_captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert!(report.success);
        assert_eq!(report.registrations.len(), 2);
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect("same-path", port),
                LifecycleEvent::RetainedWait("same-path"),
                scripted_replace_event(
                    &same_path_captures[0].path,
                    &same_path_captures[0].raw,
                    &adopted_raw,
                ),
                LifecycleEvent::Inspect("same-path", port),
                LifecycleEvent::Render,
            ]
        );

        same_path_alias.raw.push(b' ');
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "conflicting-path-token", Rc::clone(&ledger));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(
            vec![captures[0].clone(), same_path_alias],
            pid,
            &runtime,
            &mut effects,
        );
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Blocked);
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::Render],
            "conflicting tokens for one physical path must block before acquisition"
        );

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "committed-error", Rc::clone(&ledger))
            .with_inspection(Ok(facts))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Err(ReplacementError {
                    path: captures[1].path.clone(),
                    detail: "directory sync failed after commit".to_string(),
                    preserved_at: Some(holding),
                    replacement_committed: true,
                }),
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::RolledBack);
        assert!(matches!(
            report.registrations[1].transition,
            AdoptionAliasTransition::ReplaceFailed {
                replacement_committed: true,
                ..
            }
        ));
        assert_eq!(
            report.registrations[1].rollback,
            Some(AdoptionRollbackOutcome::LegacyRestored)
        );
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

    fn composition_runfile(pid: u32, local_path: &Path) -> ServerRunfile {
        let mut runfile = discovery_fixture_runfile(pid, "composition");
        runfile.origin_local_runfile = Some(local_path.to_path_buf());
        runfile
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
            .with_terminate(Err(ProcessError::Operation("access denied".to_string())))
            .with_wait(Ok(false));
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
                LifecycleEvent::RetainedWait("generation-b"),
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
    fn partial_publication_stops_child_and_compensates_exactly() {
        fn mirrored_captures(pid: u32) -> (CapturedRegistration, CapturedRegistration) {
            let local =
                discovery_fixture_capture(RegistrationScope::Local, pid, "publication-local");
            let global = CapturedRegistration {
                scope: RegistrationScope::Global,
                path: discovery_fixture_path("publication-global"),
                raw: local.raw.clone(),
                runfile: local.runfile.clone(),
            };
            (local, global)
        }

        fn publication_stage(scope: RegistrationScope, final_path: &Path) -> PublicationStage {
            PublicationStage {
                scope,
                final_path: final_path.to_path_buf(),
                path: final_path.with_file_name(".server-registration-stage"),
                raw: Some(b"exact staged registration bytes".to_vec()),
                identity: None,
            }
        }

        fn mirror_failure(
            local: &CapturedRegistration,
            stage: &PublicationStage,
        ) -> Result<PublishedRegistrations, PublishError> {
            Err(PublishError::Mirror {
                path: stage.final_path.clone(),
                detail: "injected global precommit failure".to_string(),
                local: Box::new(local.clone()),
                attempt: Box::new(PublicationAttempt {
                    finals: vec![local.clone()],
                    stages: vec![stage.clone()],
                    terminal_phase: PersistencePhase::FileSync,
                    final_committed: false,
                }),
            })
        }

        let pid = 4401;
        let port = 9441;
        let (local, global) = mirrored_captures(pid);
        let global_stage = publication_stage(RegistrationScope::Global, &global.path);

        // A global precommit failure retains the local final and global stage
        // until exact exit/reap/listener release, then removes finals before
        // stages and renders only after every outcome is known.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "global-precommit", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut effects = ScriptedPublicationEffects::new(
            [Ok(RemovalOutcome::Removed)],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            port,
            mirror_failure(&local, &global_stage),
            &listener,
            &mut effects,
        );
        let rendered = render_publication_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert!(!report.success);
        assert!(
            rendered
                .stdout
                .last()
                .unwrap()
                .contains("rollback complete")
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Terminate("global-precommit"),
                LifecycleEvent::RetainedWait("global-precommit"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&local),
                LifecycleEvent::RemoveStage(
                    global_stage.path.clone(),
                    global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
                ),
                LifecycleEvent::Render,
            ]
        );

        // A signal error is not itself exit proof, but the exact retained
        // handle is still deliberately waited. A successful retained wait,
        // reap, and absent-listener check independently authorize rollback.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "signal-error-exited", Rc::clone(&ledger))
            .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
            .with_wait(Ok(true));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut effects = ScriptedPublicationEffects::new(
            [Ok(RemovalOutcome::Removed)],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            port,
            mirror_failure(&local, &global_stage),
            &listener,
            &mut effects,
        );
        render_publication_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("signal denied"))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Terminate("signal-error-exited"),
                LifecycleEvent::RetainedWait("signal-error-exited"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&local),
                LifecycleEvent::RemoveStage(
                    global_stage.path.clone(),
                    global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
                ),
                LifecycleEvent::Render,
            ]
        );

        // A local committed-but-durability failure is also a partial
        // publication, even though no stage remains.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "local-durability", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let published_local = PublishedRegistrations {
            local: local.clone(),
            global: None,
        };
        let failure = Err(PublishError::Durability {
            path: local.path.clone(),
            detail: "injected local parent-sync failure".to_string(),
            published: Box::new(published_local),
            attempt: Box::new(PublicationAttempt {
                finals: vec![local.clone()],
                stages: Vec::new(),
                terminal_phase: PersistencePhase::ParentSync,
                final_committed: true,
            }),
        });
        let mut effects = ScriptedPublicationEffects::new(
            [Ok(RemovalOutcome::Removed)],
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report =
            complete_publication_with(&mut child, &process, port, failure, &listener, &mut effects);
        render_publication_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert_eq!(report.finals.len(), 1);
        assert!(report.stages.is_empty());
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Terminate("local-durability"),
                LifecycleEvent::RetainedWait("local-durability"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&local),
                LifecycleEvent::Render,
            ]
        );

        // A child observed exited after both finals appear still goes through
        // the retained wait/reap/listener proof before either final rolls back.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "exit-during-publication", Rc::clone(&ledger))
            .with_terminate(Ok(false))
            .with_wait(Ok(true));
        let mut child = ScriptedChild::new(
            pid,
            [Ok(Some(ScriptedExit("exited during publication")))],
            Rc::clone(&ledger),
        );
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut effects = ScriptedPublicationEffects::new(
            [Ok(RemovalOutcome::Removed), Ok(RemovalOutcome::Removed)],
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            port,
            Ok(PublishedRegistrations {
                local: local.clone(),
                global: Some(global.clone()),
            }),
            &listener,
            &mut effects,
        );
        render_publication_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Terminate("exit-during-publication"),
                LifecycleEvent::RetainedWait("exit-during-publication"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&local),
                scripted_remove_event(&global),
                LifecycleEvent::Render,
            ]
        );

        // Every unproved exit/reap/listener row holds both finals and stages
        // and produces no removal event.
        for case in [
            "terminate-error",
            "wait-timeout",
            "wait-error",
            "reap-error",
            "listener-survived",
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = match case {
                "terminate-error" => ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                    .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
                    .with_wait(Ok(false)),
                "wait-timeout" => {
                    ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_wait(Ok(false))
                }
                "wait-error" => ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_wait(Err(
                    ProcessError::Operation("retained wait failed".to_string()),
                )),
                "reap-error" | "listener-survived" => {
                    ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                }
                _ => unreachable!(),
            };
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            if case == "reap-error" {
                child.wait = VecDeque::from([Err("child reap failed".to_string())]);
            }
            let listener = ScriptedListener::new(
                if case == "listener-survived" {
                    ListenerState::OwnedByTarget
                } else {
                    ListenerState::Absent
                },
                Rc::clone(&ledger),
            );
            let mut effects = ScriptedPublicationEffects::new(
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = complete_publication_with(
                &mut child,
                &process,
                port,
                mirror_failure(&local, &global_stage),
                &listener,
                &mut effects,
            );
            render_publication_with_ledger(&report, &ledger);
            assert_eq!(
                report.disposition,
                PublicationDisposition::RecoveryHeld,
                "{case}"
            );
            assert!(
                report
                    .finals
                    .iter()
                    .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
            );
            assert!(
                report
                    .stages
                    .iter()
                    .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
            );
            assert!(ledger.borrow().iter().all(|event| !matches!(
                event,
                LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
            )));
            assert_eq!(ledger.borrow().last(), Some(&LifecycleEvent::Render));
        }

        // Cleanup continues across a concurrent final replacement, a second
        // final's conditional-removal failure, and a stage-cleanup failure;
        // every preserved path survives in the structured and rendered report.
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "partial-cleanup", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let replacement_path = discovery_fixture_path("concurrent-replacement");
        let final_holding = discovery_fixture_path("final-holding");
        let stage_holding = discovery_fixture_path("stage-holding");
        let published = PublishedRegistrations {
            local: local.clone(),
            global: Some(global.clone()),
        };
        let failure = Err(PublishError::Durability {
            path: global.path.clone(),
            detail: "injected global durability and stage cleanup failure".to_string(),
            published: Box::new(published),
            attempt: Box::new(PublicationAttempt {
                finals: vec![local.clone(), global.clone()],
                stages: vec![global_stage.clone()],
                terminal_phase: PersistencePhase::StageCleanup,
                final_committed: true,
            }),
        });
        let mut effects = ScriptedPublicationEffects::new(
            [
                Ok(RemovalOutcome::ReplacementPreserved {
                    path: replacement_path.clone(),
                    detail: "concurrent replacement preserved".to_string(),
                }),
                Err(RemovalError {
                    path: global.path.clone(),
                    kind: RemovalFailureKind::Remove,
                    detail: "conditional final cleanup failed".to_string(),
                    preserved_at: Some(final_holding.clone()),
                }),
            ],
            [Err(RemovalError {
                path: global_stage.path.clone(),
                kind: RemovalFailureKind::Remove,
                detail: "conditional stage cleanup failed".to_string(),
                preserved_at: Some(stage_holding.clone()),
            })],
            Rc::clone(&ledger),
        );
        let report =
            complete_publication_with(&mut child, &process, port, failure, &listener, &mut effects);
        let rendered = render_publication_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| { line.contains(&replacement_path.display().to_string()) })
        );
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains(&final_holding.display().to_string()))
        );
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains(&stage_holding.display().to_string()))
        );
        assert_eq!(
            ledger
                .borrow()
                .iter()
                .rev()
                .take(4)
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                LifecycleEvent::Render,
                LifecycleEvent::RemoveStage(
                    global_stage.path.clone(),
                    global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
                ),
                scripted_remove_event(&global),
                scripted_remove_event(&local),
            ],
            "all finals must be attempted before stages and rendering"
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

    #[test]
    fn status_and_discovery_two_scope_matrix() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local_path = runfile_path(&workspace);
        let global_path = root.path().join("global/server.json");
        let pid = 4511;
        let runfile = composition_runfile(pid, &local_path);
        let published = publish_mirrored(&workspace, Some(&global_path), &runfile).unwrap();
        let expected_raw = published.local.raw.clone();
        assert_eq!(published.global.as_ref().unwrap().raw, expected_raw);

        let inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let lifecycle = discover_inventory_with(
            inventory,
            |capture| {
                let identity = capture.runfile.process_identity.clone().unwrap();
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
                    label: registration_label(capture.scope, &capture.path),
                    capture: Some(capture),
                    process: None,
                }
            },
            &mut health,
            |_observation| Ok(()),
        );
        let managed = lifecycle.managed.clone();
        let server = match &managed.state {
            ManagedServerState::Ready(server) => server.clone(),
            state => panic!("two exact mirrors must resolve ready, got {state:?}"),
        };
        assert_eq!(
            server.aliases.len(),
            2,
            "the global mirror retains its promised local-origin observation"
        );
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Health(runfile.port)]);

        let rendered = render_status(&status_report(&managed));
        assert!(rendered.success);
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains("aliases=2"))
        );
        assert_eq!(
            rendered
                .stdout
                .iter()
                .filter(|line| line.starts_with("[captured]"))
                .count(),
            3
        );

        let scope = ManagedDiscoveryScope {
            workspace: workspace.clone(),
            global: Some(global_path.clone()),
        };
        assert!(matches!(
            crate::backend::automatic_endpoint_from_discovery(scope.clone(), managed.clone()),
            Ok(crate::backend::EndpointSelection::Managed { .. })
        ));
        assert!(
            crate::backend::require_managed_endpoint(scope.clone(), managed.clone(), None).is_ok()
        );
        assert_eq!(
            crate::autonomy_cmd::require_matching_pre_health_discovery(
                &managed,
                &server.fingerprint,
            )
            .unwrap()
            .fingerprint,
            server.fingerprint,
            "strict autonomy must consume the same typed two-scope discovery"
        );
        let mut doctor_effects = RecordingDoctorEffects::default();
        let doctor =
            doctor_report_after_discovery(&doctor_fixture_args(), &managed, &mut doctor_effects);
        assert!(doctor.success);
        assert_eq!(
            doctor_effects.events,
            vec![DoctorEvent::Binary, DoctorEvent::File]
        );

        let facts = ProcessFacts {
            identity: server.identity.clone(),
            listener: ListenerState::OwnedByTarget,
        };
        let process = ScriptedProcess::new(pid, "two-scope-generation", Rc::clone(&ledger))
            .with_inspection(Ok(facts));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let retained = runtime.acquire(pid).unwrap();
        let captures = vec![published.local, published.global.unwrap()];
        let plan = retained_target_down_plan(
            &managed.state,
            retained,
            captures.clone(),
            discovery_revisions(&managed.observations),
        )
        .unwrap();
        let mut down_effects = FilesystemDownEffects {
            scope,
            listeners: VecDeque::from([ListenerState::Absent]),
            ledger: Rc::clone(&ledger),
        };
        let report = execute_down_plan(plan, &mut down_effects);
        let down = render_down_with_ledger(&report, &ledger);
        assert!(down.success);
        assert_eq!(report.disposition, DownDisposition::Stopped);
        assert!(
            report
                .registrations
                .iter()
                .all(|registration| { registration.outcome == DownRegistrationOutcome::Removed })
        );
        assert!(!local_path.exists());
        assert!(!global_path.exists());
        assert_eq!(
            ledger.borrow().as_slice(),
            &[
                LifecycleEvent::Health(runfile.port),
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("two-scope-generation", runfile.port),
                LifecycleEvent::Terminate("two-scope-generation"),
                LifecycleEvent::RetainedWait("two-scope-generation"),
                LifecycleEvent::Listener(pid, runfile.port),
                scripted_remove_event(&captures[0]),
                scripted_remove_event(&captures[1]),
                LifecycleEvent::Render,
            ]
        );
    }

    #[test]
    fn down_retained_handle_transition_matrix() {
        let pid = 4521;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("transition-workspace");
        let local_path = runfile_path(&workspace);
        let global_path = root.path().join("transition-global/server.json");
        let runfile = composition_runfile(pid, &local_path);
        let port = runfile.port;
        let identity = runfile.process_identity.clone().unwrap();
        let published = publish_mirrored(&workspace, Some(&global_path), &runfile).unwrap();
        let local = published.local;
        let global = published.global.unwrap();
        let captures = vec![local.clone(), global.clone()];
        let scope = ManagedDiscoveryScope {
            workspace: workspace.clone(),
            global: Some(global_path),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let lifecycle = discover_inventory_with(
            inventory_runfiles(&workspace, scope.global.clone()),
            |capture| {
                let observed_identity = capture.runfile.process_identity.clone().unwrap();
                LifecycleObservation {
                    candidate: Candidate {
                        coordinate: RegistrationCoordinate {
                            scope: capture.scope,
                            path: capture.path.clone(),
                        },
                        runfile: Some(capture.runfile.clone()),
                        state: CandidateState::Verified {
                            identity: observed_identity,
                            listener: ListenerState::OwnedByTarget,
                            health: HealthState::NotProbed,
                        },
                    },
                    label: registration_label(capture.scope, &capture.path),
                    capture: Some(capture),
                    process: None,
                }
            },
            &mut health,
            |_observation| Ok(()),
        );
        let managed = lifecycle.managed;
        assert!(matches!(&managed.state, ManagedServerState::Ready(_)));
        let revisions = discovery_revisions(&managed.observations);

        // Resolve the real inventory first, acquire its exact retained
        // generation once, then remap the same numeric PID in the fake process
        // table. Planning and execution must keep using the already-retained
        // object while real inventory revisions gate pre-signal mutation.
        let process = ScriptedProcess::new(pid, "retained-before-remap", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: identity.clone(),
                listener: ListenerState::OwnedByTarget,
            }));
        let runtime = ScriptedPidMapRuntime::new(process, Rc::clone(&ledger));
        let retained = runtime.acquire(pid).unwrap();
        runtime.replace(
            pid,
            "replacement-generation",
            ScriptedProcess::new(pid, "replacement-generation", Rc::clone(&ledger))
                .with_inspection(Ok(ProcessFacts {
                    identity: discovery_fixture_identity(u64::from(pid) + 1),
                    listener: ListenerState::OwnedByTarget,
                })),
        );
        let replacement = discovery_fixture_path("transition-replacement");
        let mut effects = RevisionCheckingDownEffects {
            scope: scope.clone(),
            listeners: VecDeque::from([ListenerState::Absent]),
            removals: VecDeque::from([
                Ok(RemovalOutcome::Removed),
                Ok(RemovalOutcome::ReplacementPreserved {
                    path: replacement.clone(),
                    detail: "concurrent alias replacement".to_string(),
                }),
            ]),
            ledger: Rc::clone(&ledger),
        };
        let plan = retained_target_down_plan(
            &managed.state,
            retained,
            captures.clone(),
            revisions.clone(),
        )
        .unwrap();
        let report = execute_down_plan(plan, &mut effects);
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::CleanupPartial);
        assert!(report.exit_proven && report.listener_released);
        assert!(rendered.stdout.iter().any(|line| {
            line.contains("replacement-preserved")
                && line.contains(&replacement.display().to_string())
        }));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Health(port),
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::PidMapReplace(pid, "replacement-generation"),
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("retained-before-remap", port),
                LifecycleEvent::Terminate("retained-before-remap"),
                LifecycleEvent::RetainedWait("retained-before-remap"),
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&local),
                scripted_remove_event(&global),
                LifecycleEvent::Render,
            ],
            "observe/acquire, pre-signal revalidation, post-exit listener proof, and per-alias cleanup must remain ordered"
        );
        assert_eq!(
            ledger
                .borrow()
                .iter()
                .filter(|event| matches!(event, LifecycleEvent::Acquire(_)))
                .count(),
            1,
            "PID remap must not trigger a numeric-PID reacquisition"
        );
        assert!(ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Inspect("replacement-generation", _)
                | LifecycleEvent::Terminate("replacement-generation")
                | LifecycleEvent::RetainedWait("replacement-generation")
        )));

        for case in [
            "pre-signal-revision-change",
            "pre-signal-listener-transfer",
            "terminate-failure",
            "wait-failure",
            "post-exit-listener-transfer",
        ] {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let inspected_listener = if case == "pre-signal-listener-transfer" {
                ListenerState::OwnedByOther(vec![9901])
            } else {
                ListenerState::OwnedByTarget
            };
            let mut process = ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_inspection(
                Ok(ProcessFacts {
                    identity: identity.clone(),
                    listener: inspected_listener,
                }),
            );
            if case == "terminate-failure" {
                process = process.with_terminate(Err(ProcessError::Operation(
                    "retained terminate failed".to_string(),
                )));
            }
            if case == "wait-failure" {
                process = process.with_wait(Err(ProcessError::Operation(
                    "retained wait failed".to_string(),
                )));
            }
            let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
            let retained = runtime.acquire(pid).unwrap();
            let mut effects = ScriptedDownEffects::new(
                [if case == "post-exit-listener-transfer" {
                    ListenerState::OwnedByOther(vec![9902])
                } else {
                    ListenerState::Absent
                }],
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            if case == "pre-signal-revision-change" {
                effects.revalidations =
                    VecDeque::from([Err("registration changed before signal".to_string())]);
            }
            let plan = retained_target_down_plan(
                &managed.state,
                retained,
                captures.clone(),
                revisions.clone(),
            )
            .unwrap();
            let report = execute_down_plan(plan, &mut effects);
            let rendered = render_down_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, DownDisposition::Failed, "{case}");
            assert!(!rendered.success, "{case}");
            assert!(
                ledger
                    .borrow()
                    .iter()
                    .all(|event| !matches!(event, LifecycleEvent::Remove(_, _))),
                "{case} must preserve every alias"
            );
            assert_eq!(ledger.borrow().first(), Some(&LifecycleEvent::Acquire(pid)));
            assert_eq!(ledger.borrow().last(), Some(&LifecycleEvent::Render));
            let events = ledger.borrow();
            let inspect = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::Inspect(_, _)));
            let terminate = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::Terminate(_)));
            if case == "pre-signal-revision-change" {
                assert!(inspect.is_none() && terminate.is_none());
            } else if case == "pre-signal-listener-transfer" {
                assert!(inspect.is_some() && terminate.is_none());
            } else {
                assert!(inspect < terminate);
            }
        }
    }

    #[test]
    fn up_spawned_child_binding_precedes_readiness() {
        let pid = 4531;
        let port = 9531;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("orchestrated-up");
        let mut launch_cfg = cfg(Engine::LlamaServer);
        launch_cfg.port = port;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "up-bound-generation", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: discovery_fixture_identity(u64::from(pid)),
                listener: ListenerState::OwnedByTarget,
            }));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&ledger),
        };
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
        let mut compensation = ScriptedPublicationEffects::new(
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let spawn_ledger = Rc::clone(&ledger);
        let launched = orchestrate_launch_with(
            &workspace,
            None,
            &launch_cfg,
            move || {
                spawn_ledger.borrow_mut().push(LifecycleEvent::Spawn(pid));
                Ok(ScriptedChild::new(
                    pid,
                    [Ok(None), Ok(None), Ok(None), Ok(None)],
                    Rc::clone(&spawn_ledger),
                ))
            },
            &runtime,
            &listener,
            &mut health,
            &mut clock,
            |workspace, global, runfile| {
                publish_mirrored_with(workspace, global, runfile, &mut persistence)
            },
            &mut compensation,
        )
        .unwrap();
        assert_eq!(launched.pid, pid);
        assert_eq!(persistence.serializations, 1);
        assert!(launched.published.local.path.exists());
        let events = ledger.borrow();
        assert_eq!(events.first(), Some(&LifecycleEvent::Spawn(pid)));
        let bind = events
            .iter()
            .position(|event| *event == LifecycleEvent::Acquire(pid))
            .unwrap();
        let readiness = events
            .iter()
            .position(|event| *event == LifecycleEvent::Health(port))
            .unwrap();
        let publication = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    LifecycleEvent::Persistence(PersistencePhase::CreateStage, _)
                )
            })
            .unwrap();
        let final_inspection = events
            .iter()
            .position(|event| *event == LifecycleEvent::Inspect("up-bound-generation", port))
            .unwrap();
        assert!(bind < readiness && readiness < final_inspection && final_inspection < publication);
        drop(events);

        let blocked_ledger = Rc::new(RefCell::new(Vec::new()));
        let runtime = ScriptedRuntime::new(
            Err("retained handle unavailable".to_string()),
            Rc::clone(&blocked_ledger),
        );
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&blocked_ledger));
        let mut health = ScriptedHealth {
            results: VecDeque::new(),
            ledger: Rc::clone(&blocked_ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&blocked_ledger),
        };
        let mut persistence =
            CompositionPersistenceEffects::default_with(Rc::clone(&blocked_ledger));
        let mut compensation = ScriptedPublicationEffects::new(
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&blocked_ledger),
        );
        let spawn_ledger = Rc::clone(&blocked_ledger);
        let blocked = orchestrate_launch_with(
            &root.path().join("blocked-up"),
            None,
            &launch_cfg,
            move || {
                spawn_ledger.borrow_mut().push(LifecycleEvent::Spawn(pid));
                Ok(ScriptedChild::new(
                    pid,
                    [Ok(None)],
                    Rc::clone(&spawn_ledger),
                ))
            },
            &runtime,
            &listener,
            &mut health,
            &mut clock,
            |workspace, global, runfile| {
                publish_mirrored_with(workspace, global, runfile, &mut persistence)
            },
            &mut compensation,
        );
        assert!(matches!(
            blocked,
            Err(LaunchOrchestrationError::Bind { .. })
        ));
        assert_eq!(persistence.serializations, 0);
        assert!(blocked_ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Health(_) | LifecycleEvent::Persistence(_, _)
        )));
    }

    #[test]
    fn legacy_adoption_then_down() {
        let pid = 4541;
        let (fixture, facts) = legacy_adoption_fixture(pid);
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("legacy-workspace");
        let local_path = runfile_path(&workspace);
        let global_path = root.path().join("legacy-global/server.json");
        fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        fs::create_dir_all(global_path.parent().unwrap()).unwrap();
        fs::write(&local_path, &fixture[0].raw).unwrap();
        fs::write(&global_path, &fixture[0].raw).unwrap();
        let scope = ManagedDiscoveryScope {
            workspace: workspace.clone(),
            global: Some(global_path.clone()),
        };
        let inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
        let (legacy, blocked) = expand_registration_captures(inventory);
        assert!(blocked.is_empty());
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].path, local_path);
        assert_eq!(legacy[1].path, global_path);

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "adopted-generation", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut adoption_effects = FilesystemAdoptionEffects {
            ledger: Rc::clone(&ledger),
        };
        let adoption = execute_legacy_adoption(legacy, pid, &runtime, &mut adoption_effects);
        let adoption_rendered = render_adoption_with_ledger(&adoption, &ledger);
        assert!(adoption_rendered.success);
        assert_eq!(adoption.disposition, AdoptionDisposition::Adopted);
        assert!(ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
        )));

        let adopted_raw = fs::read(&local_path).unwrap();
        assert_eq!(fs::read(&global_path).unwrap(), adopted_raw);
        let adopted_runfile: ServerRunfile = serde_json::from_slice(&adopted_raw).unwrap();
        assert_eq!(adopted_runfile.schema_version, RUNFILE_SCHEMA_V2);
        let parsed_identity = adopted_runfile.process_identity.clone().unwrap();
        assert_eq!(parsed_identity, facts.identity);
        assert_eq!(
            adopted_runfile.origin_local_runfile.as_deref(),
            Some(local_path.as_path())
        );

        // Re-read and parse the bytes written by the real conditional
        // replacement adapter, then resolve that inventory before deriving
        // teardown authority from its persisted identity.
        let adopted_inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
        let (adopted_captures, blocked) = expand_registration_captures(adopted_inventory.clone());
        assert!(blocked.is_empty());
        assert_eq!(adopted_captures.len(), 3);
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let lifecycle = discover_inventory_with(
            adopted_inventory,
            |capture| {
                let identity = capture.runfile.process_identity.clone().unwrap();
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
                    label: registration_label(capture.scope, &capture.path),
                    capture: Some(capture),
                    process: None,
                }
            },
            &mut health,
            |_observation| Ok(()),
        );
        let managed = lifecycle.managed;
        assert!(matches!(&managed.state, ManagedServerState::Ready(_)));

        let process = ScriptedProcess::new(pid, "adopted-generation", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: parsed_identity.clone(),
                listener: ListenerState::OwnedByTarget,
            }));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let retained = runtime.acquire(pid).unwrap();
        let plan = retained_target_down_plan(
            &managed.state,
            retained,
            adopted_captures,
            discovery_revisions(&managed.observations),
        )
        .unwrap();
        let mut down_effects = FilesystemDownEffects {
            scope,
            listeners: VecDeque::from([ListenerState::Absent]),
            ledger: Rc::clone(&ledger),
        };
        let down = execute_down_plan(plan, &mut down_effects);
        let down_rendered = render_down_with_ledger(&down, &ledger);
        assert!(down_rendered.success);
        assert_eq!(down.disposition, DownDisposition::Stopped);
        assert!(
            down.registrations
                .iter()
                .all(|registration| { registration.outcome == DownRegistrationOutcome::Removed })
        );
        assert!(!local_path.exists());
        assert!(!global_path.exists());

        let events = ledger.borrow();
        let adoption_finish = events
            .iter()
            .position(|event| *event == LifecycleEvent::Render)
            .unwrap();
        let down_acquire = events
            .iter()
            .enumerate()
            .find(|(index, event)| {
                *index > adoption_finish && **event == LifecycleEvent::Acquire(pid)
            })
            .map(|(index, _)| index)
            .unwrap();
        assert!(events[..adoption_finish].iter().all(|event| !matches!(
            event,
            LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
        )));
        assert_eq!(
            events[down_acquire..]
                .iter()
                .filter(|event| matches!(event, LifecycleEvent::Inspect("adopted-generation", _)))
                .count(),
            1
        );
        assert!(events[down_acquire..].contains(&LifecycleEvent::Terminate("adopted-generation")));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                .count(),
            2
        );
    }

    #[test]
    fn registration_publication_failure_matrix() {
        fn attempt(error: &PublishError) -> &PublicationAttempt {
            match error {
                PublishError::Write { attempt, .. }
                | PublishError::Mirror { attempt, .. }
                | PublishError::Durability { attempt, .. } => attempt,
                PublishError::Invalid { .. } | PublishError::Serialize(_) => {
                    panic!("publication fault did not retain an attempt: {error}")
                }
            }
        }

        // First prove both successful shapes cross the real publication
        // algorithm and reach the coordinator only after all persistence
        // phases. The coordinator performs no shutdown or cleanup while the
        // retained child is still live.
        for mirrored in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("success-workspace");
            let local = runfile_path(&workspace);
            let global = root.path().join("success-global/server.json");
            let pid = 4550 + u32::from(mirrored);
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
            let publication = publish_mirrored_with(
                &workspace,
                mirrored.then_some(global.as_path()),
                &runfile,
                &mut persistence,
            );
            assert_eq!(persistence.serializations, 1);
            let process = ScriptedProcess::new(pid, "publication-success", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [Ok(None)], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = ScriptedPublicationEffects::new(
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::Ready);
            assert!(report.success);
            assert_eq!(
                fs::read(&local).unwrap(),
                serde_json::to_vec_pretty(&runfile).unwrap()
            );
            if mirrored {
                assert!(global.exists());
            } else {
                assert!(!global.exists());
            }
            assert_eq!(
                ledger.borrow().last(),
                Some(&LifecycleEvent::ChildTryWait(pid))
            );
            assert!(ledger.borrow().iter().all(|event| !matches!(
                event,
                LifecycleEvent::Terminate(_)
                    | LifecycleEvent::Remove(_, _)
                    | LifecycleEvent::RemoveStage(_, _)
            )));
        }

        // Every local and global persistence boundary feeds the exact
        // PublicationAttempt produced by publish_mirrored_with into the
        // shutdown/compensation coordinator. Cleanup is real and conditional.
        for fail_global in [false, true] {
            for phase in [
                PersistencePhase::CreateStage,
                PersistencePhase::WriteAll,
                PersistencePhase::Flush,
                PersistencePhase::FileSync,
                PersistencePhase::PersistNoClobber,
                PersistencePhase::StageCleanup,
                PersistencePhase::ParentSync,
            ] {
                let root = tempfile::tempdir().unwrap();
                let workspace = root.path().join(format!(
                    "{}-{phase:?}",
                    if fail_global { "global" } else { "local" }
                ));
                let local = runfile_path(&workspace);
                let global = root.path().join("global/server.json");
                let pid = 4560 + u32::from(fail_global);
                let runfile = composition_runfile(pid, &local);
                let target = if fail_global { &global } else { &local };
                let ledger = Rc::new(RefCell::new(Vec::new()));
                let mut persistence = if phase == PersistencePhase::StageCleanup {
                    CompositionPersistenceEffects::retaining_committed_stage(
                        target,
                        Rc::clone(&ledger),
                    )
                } else {
                    CompositionPersistenceEffects::failing(target, phase, Rc::clone(&ledger))
                };
                let publication = publish_mirrored_with(
                    &workspace,
                    fail_global.then_some(global.as_path()),
                    &runfile,
                    &mut persistence,
                );
                assert_eq!(persistence.serializations, 1, "{fail_global} {phase:?}");
                let error = publication.as_ref().unwrap_err();
                let retained = attempt(error).clone();
                assert_eq!(retained.terminal_phase, phase);
                assert_eq!(
                    retained.final_committed,
                    matches!(
                        phase,
                        PersistencePhase::StageCleanup | PersistencePhase::ParentSync
                    )
                );
                assert_eq!(
                    retained.finals.len(),
                    usize::from(fail_global)
                        + usize::from(matches!(
                            phase,
                            PersistencePhase::StageCleanup | PersistencePhase::ParentSync
                        )),
                    "{fail_global} {phase:?} must expose every committed final"
                );
                assert_eq!(
                    retained.stages.len(),
                    usize::from(!matches!(
                        phase,
                        PersistencePhase::CreateStage | PersistencePhase::ParentSync
                    )),
                    "{fail_global} {phase:?} must explain every retained stage"
                );

                let process = ScriptedProcess::new(pid, "publication-boundary", Rc::clone(&ledger));
                let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
                let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
                let mut cleanup = FilesystemPublicationEffects {
                    ledger: Rc::clone(&ledger),
                };
                let report = complete_publication_with(
                    &mut child,
                    &process,
                    runfile.port,
                    publication,
                    &listener,
                    &mut cleanup,
                );
                assert_eq!(
                    report.disposition,
                    PublicationDisposition::RolledBack,
                    "{fail_global} {phase:?}: {report:?}"
                );
                assert!(!local.exists(), "{fail_global} {phase:?}");
                assert!(!global.exists(), "{fail_global} {phase:?}");
                for stage in &retained.stages {
                    assert!(!stage.path.exists(), "{fail_global} {phase:?}");
                }
                let events = ledger.borrow();
                let last_persistence = events
                    .iter()
                    .rposition(|event| matches!(event, LifecycleEvent::Persistence(_, _)))
                    .unwrap();
                let terminate = events
                    .iter()
                    .position(|event| *event == LifecycleEvent::Terminate("publication-boundary"))
                    .unwrap();
                let first_cleanup = events.iter().position(|event| {
                    matches!(
                        event,
                        LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
                    )
                });
                assert!(last_persistence < terminate);
                if let Some(first_cleanup) = first_cleanup {
                    let released = events
                        .iter()
                        .position(|event| *event == LifecycleEvent::Listener(pid, runfile.port))
                        .unwrap();
                    assert!(released < first_cleanup);
                }
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                        .count(),
                    retained.finals.len()
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(event, LifecycleEvent::RemoveStage(_, _)))
                        .count(),
                    retained.stages.len()
                );
            }
        }

        // Real no-clobber conflicts preserve the winner while the
        // coordinator removes only attempt-owned finals and stages.
        for existing_global in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("occupied-workspace");
            let local = runfile_path(&workspace);
            let global = root.path().join("occupied-global/server.json");
            let occupied = if existing_global { &global } else { &local };
            fs::create_dir_all(occupied.parent().unwrap()).unwrap();
            fs::write(occupied, b"external-publication-winner").unwrap();
            let pid = 4570 + u32::from(existing_global);
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
            let publication = publish_mirrored_with(
                &workspace,
                existing_global.then_some(global.as_path()),
                &runfile,
                &mut persistence,
            );
            assert_eq!(
                attempt(publication.as_ref().unwrap_err()).terminal_phase,
                PersistencePhase::PersistNoClobber
            );
            let process = ScriptedProcess::new(pid, "publication-no-clobber", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::RolledBack);
            assert_eq!(fs::read(occupied).unwrap(), b"external-publication-winner");
            if existing_global {
                assert!(!local.exists());
            }
        }

        // Lexical alias rejection occurs before serialization/staging, but
        // still enters the same child-owned failure coordinator.
        {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("alias-workspace");
            let local = runfile_path(&workspace);
            let alias = local.parent().unwrap().join(".").join("server.json");
            let pid = 4581;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
            let publication =
                publish_mirrored_with(&workspace, Some(&alias), &runfile, &mut persistence);
            assert!(matches!(publication, Err(PublishError::Invalid { .. })));
            assert_eq!(persistence.serializations, 0);
            assert!(ledger.borrow().is_empty());
            let process = ScriptedProcess::new(pid, "publication-alias", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::RolledBack);
            assert!(report.finals.is_empty() && report.stages.is_empty());
            assert!(!local.exists());
        }

        // A child exit after a successful mirrored publication is another
        // coordinator boundary: both real finals are rolled back only after
        // retained exit, reap, and listener-release proof.
        {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("post-publish-exit");
            let local = runfile_path(&workspace);
            let global = root.path().join("post-publish-global/server.json");
            let pid = 4591;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            let process = ScriptedProcess::new(pid, "publication-child-exit", Rc::clone(&ledger))
                .with_terminate(Ok(false));
            let mut child = ScriptedChild::new(
                pid,
                [Ok(Some(ScriptedExit("exited after publication")))],
                Rc::clone(&ledger),
            );
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::RolledBack);
            assert!(!local.exists() && !global.exists());
            let events = ledger.borrow();
            let child_exit = events
                .iter()
                .position(|event| *event == LifecycleEvent::ChildTryWait(pid))
                .unwrap();
            let first_remove = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                .unwrap();
            assert!(child_exit < first_remove);
        }

        // Successful real publication followed by an inconclusive Child
        // status check must enter the same retained-object shutdown path. A
        // later retained wait can independently prove exit and authorize
        // rollback; without that proof both finals remain held.
        for retained_exit_proven in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(if retained_exit_proven {
                "try-wait-error-exited"
            } else {
                "try-wait-error-unproved"
            });
            let local = runfile_path(&workspace);
            let global = root.path().join("try-wait-error-global/server.json");
            let pid = 4595 + u32::from(retained_exit_proven);
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            assert!(publication.is_ok());
            let process = ScriptedProcess::new(
                pid,
                if retained_exit_proven {
                    "try-wait-error-exited"
                } else {
                    "try-wait-error-unproved"
                },
                Rc::clone(&ledger),
            )
            .with_wait(Ok(retained_exit_proven));
            let mut child = ScriptedChild::new(
                pid,
                [Err("post-publication child status unavailable".to_string())],
                Rc::clone(&ledger),
            );
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert!(
                report.diagnostics[0]
                    .contains("could not confirm the engine child after publication")
            );
            if retained_exit_proven {
                assert_eq!(report.disposition, PublicationDisposition::RolledBack);
                assert!(!local.exists() && !global.exists());
                assert_eq!(
                    ledger
                        .borrow()
                        .iter()
                        .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                        .count(),
                    2
                );
            } else {
                assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
                assert!(local.exists() && global.exists());
                assert_eq!(report.finals.len(), 2);
                assert!(
                    report
                        .finals
                        .iter()
                        .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
                );
                assert!(ledger.borrow().iter().all(|event| !matches!(
                    event,
                    LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
                )));
            }
            let events = ledger.borrow();
            let child_status = events
                .iter()
                .position(|event| *event == LifecycleEvent::ChildTryWait(pid))
                .unwrap();
            let retained_wait = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::RetainedWait(_)))
                .unwrap();
            assert!(child_status < retained_wait);
        }

        // Runtime proof failures all begin with a real global FileSync fault.
        // No unproved-exit row reaches either conditional store adapter.
        for case in [
            "terminate-timeout",
            "wait-timeout",
            "wait-error",
            "reap-error",
            "listener-survival",
        ] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(format!("runtime-{case}"));
            let local = runfile_path(&workspace);
            let global = root.path().join("runtime-global/server.json");
            let pid = 4601;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::failing(
                &global,
                PersistencePhase::FileSync,
                Rc::clone(&ledger),
            );
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            let mut process = ScriptedProcess::new(pid, case, Rc::clone(&ledger));
            if case == "terminate-timeout" {
                process = process
                    .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
                    .with_wait(Ok(false));
            } else if case == "wait-timeout" {
                process = process.with_wait(Ok(false));
            } else if case == "wait-error" {
                process = process.with_wait(Err(ProcessError::Operation(
                    "retained wait unavailable".to_string(),
                )));
            }
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            if case == "reap-error" {
                child.wait = VecDeque::from([Err("reap failed".to_string())]);
            }
            let listener = ScriptedListener::new(
                if case == "listener-survival" {
                    ListenerState::OwnedByTarget
                } else {
                    ListenerState::Absent
                },
                Rc::clone(&ledger),
            );
            let mut cleanup = ScriptedPublicationEffects::new(
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(
                report.disposition,
                PublicationDisposition::RecoveryHeld,
                "{case}"
            );
            assert!(
                report
                    .finals
                    .iter()
                    .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
            );
            assert!(
                report
                    .stages
                    .iter()
                    .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
            );
            assert!(ledger.borrow().iter().all(|event| !matches!(
                event,
                LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
            )));
        }

        // A terminate error does not itself prove exit, but a later successful
        // wait on that same retained object, reap, and listener release do.
        {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("terminate-error-exited");
            let local = runfile_path(&workspace);
            let global = root.path().join("terminate-error-global/server.json");
            let pid = 4611;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::failing(
                &global,
                PersistencePhase::FileSync,
                Rc::clone(&ledger),
            );
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            let process = ScriptedProcess::new(pid, "terminate-error-exited", Rc::clone(&ledger))
                .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
                .with_wait(Ok(true));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::RolledBack);
            assert!(!local.exists() && !global.exists());
        }

        // A concurrent final replacement is preserved while the unchanged
        // stage is still removed, producing an explicit partial recovery.
        {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("replacement-race");
            let local = runfile_path(&workspace);
            let global = root.path().join("replacement-global/server.json");
            let pid = 4621;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::failing(
                &global,
                PersistencePhase::FileSync,
                Rc::clone(&ledger),
            );
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            fs::write(&local, b"concurrent final replacement").unwrap();
            let process = ScriptedProcess::new(pid, "publication-replacement", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
            assert!(matches!(
                report.finals[0].outcome,
                DownRegistrationOutcome::ReplacementPreserved { .. }
            ));
            assert!(matches!(
                report.stages[0].outcome,
                DownRegistrationOutcome::Removed
            ));
            assert_eq!(fs::read(&local).unwrap(), b"concurrent final replacement");
        }

        // Store-adapter failures after proven exit attempt every final before
        // every stage and retain each explicit recovery location.
        {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("cleanup-failures");
            let local = runfile_path(&workspace);
            let global = root.path().join("cleanup-global/server.json");
            let final_holding = root.path().join("final-holding");
            let stage_holding = root.path().join("stage-holding");
            let pid = 4631;
            let runfile = composition_runfile(pid, &local);
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = CompositionPersistenceEffects::failing(
                &global,
                PersistencePhase::FileSync,
                Rc::clone(&ledger),
            );
            let publication =
                publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
            let process =
                ScriptedProcess::new(pid, "publication-cleanup-failure", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = ScriptedPublicationEffects::new(
                [Err(RemovalError {
                    path: local.clone(),
                    kind: RemovalFailureKind::Remove,
                    detail: "conditional final cleanup failed".to_string(),
                    preserved_at: Some(final_holding.clone()),
                })],
                [Err(RemovalError {
                    path: global.clone(),
                    kind: RemovalFailureKind::Remove,
                    detail: "conditional stage cleanup failed".to_string(),
                    preserved_at: Some(stage_holding.clone()),
                })],
                Rc::clone(&ledger),
            );
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            let rendered = render_publication_with_ledger(&report, &ledger);
            assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
            assert!(
                rendered
                    .stdout
                    .iter()
                    .any(|line| { line.contains(&final_holding.display().to_string()) })
            );
            assert!(
                rendered
                    .stdout
                    .iter()
                    .any(|line| { line.contains(&stage_holding.display().to_string()) })
            );
            let events = ledger.borrow();
            let final_cleanup = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                .unwrap();
            let stage_cleanup = events
                .iter()
                .position(|event| matches!(event, LifecycleEvent::RemoveStage(_, _)))
                .unwrap();
            assert!(final_cleanup < stage_cleanup);
            assert_eq!(events.last(), Some(&LifecycleEvent::Render));
        }
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
    fn malformed_v2_token_blocks_down_without_signal_or_delete() {
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
        identity.start_token = "opaque".to_string();
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
        let original_registration = std::fs::read(&local).unwrap();

        let result = down_impl(&workspace, None);
        let registration_after_down = std::fs::read(&local).unwrap();
        let helper_remained_live = helper.try_wait().unwrap().is_none();
        let _ = helper.kill();
        let _ = helper.wait();

        assert_eq!(result, ExitCode::FAILURE);
        assert_eq!(registration_after_down, original_registration);
        assert!(
            helper_remained_live,
            "a malformed creation token must block before process acquisition or signalling"
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

        for (flag, coordinate) in [
            ("-m", "recorded model"),
            ("-c", "recorded context size"),
            ("--seed", "recorded sampling seed"),
            ("--parallel", "recorded parallel slot count"),
            ("--host", "loopback host"),
            ("--port", "registered port"),
        ] {
            let mut missing = identity.clone();
            let index = missing
                .argv
                .iter()
                .position(|argument| argument == flag)
                .unwrap();
            missing.argv.drain(index..=index + 1);
            let error = validate_legacy_process_coordinates(&runfile, &missing).unwrap_err();
            assert!(error.contains(coordinate), "{flag}: {error}");

            let mut conflicting = identity.clone();
            conflicting
                .argv
                .extend([flag.to_string(), "conflicting-value".to_string()]);
            let error = validate_legacy_process_coordinates(&runfile, &conflicting).unwrap_err();
            assert!(
                error.contains(&format!("conflicting {coordinate}")),
                "{flag}: {error}"
            );
        }

        let mut inline = identity.clone();
        for (flag, replacement) in [
            ("-m", "--model=model.gguf"),
            ("-c", "--ctx-size=8192"),
            ("--host", "--host=127.0.0.1"),
            ("--port", "--port=8080"),
            ("--seed", "--seed=42"),
            ("--parallel", "--parallel=1"),
        ] {
            let index = inline
                .argv
                .iter()
                .position(|argument| argument == flag)
                .unwrap();
            inline
                .argv
                .splice(index..=index + 1, [replacement.to_string()]);
        }
        validate_legacy_process_coordinates(&runfile, &inline).unwrap();
        let mut conflicting_inline = identity.clone();
        conflicting_inline
            .argv
            .push("--model=other.gguf".to_string());
        let error = validate_legacy_process_coordinates(&runfile, &conflicting_inline).unwrap_err();
        assert!(error.contains("conflicting recorded model"));

        let mut missing_port = identity.clone();
        missing_port.argv.truncate(missing_port.argv.len() - 2);
        let error = validate_legacy_process_coordinates(&runfile, &missing_port).unwrap_err();
        assert!(error.contains("registered port"));

        let mut conflicting_port = identity.clone();
        conflicting_port.argv.extend([
            "--port".to_string(),
            "8081".to_string(),
            "--model".to_string(),
            "other.gguf".to_string(),
        ]);
        let error = validate_legacy_process_coordinates(&runfile, &conflicting_port).unwrap_err();
        assert!(error.contains("conflicting registered port"));

        let mut wrong_engine = identity;
        wrong_engine.executable = if cfg!(windows) {
            PathBuf::from(r"C:\tools\python.exe")
        } else {
            PathBuf::from("/tools/python")
        };
        let error = validate_legacy_process_coordinates(&runfile, &wrong_engine).unwrap_err();
        assert!(error.contains("closed"));

        let ollama = ServerRunfile {
            engine: Engine::Ollama,
            model: None,
            context_size: None,
            sampling_seed: None,
            parallel_slots: None,
            ..runfile
        };
        let mut ollama_identity = ProcessIdentity {
            start_token: canonical_test_start_token(2),
            executable: if cfg!(windows) {
                PathBuf::from(r"C:\tools\ollama.exe")
            } else {
                PathBuf::from("/tools/ollama")
            },
            argv: vec!["ollama".to_string(), "serve".to_string()],
        };
        validate_legacy_process_coordinates(&ollama, &ollama_identity).unwrap();
        ollama_identity.argv = vec![
            "ollama".to_string(),
            "status".to_string(),
            "serve".to_string(),
        ];
        let error = validate_legacy_process_coordinates(&ollama, &ollama_identity).unwrap_err();
        assert!(error.contains("closed `ollama serve`"));
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
        let missing_model = dir.path().join("missing.gguf");
        let mut args = llama_args(&missing_model);
        args.tailscale = true;
        args.port = 0;
        args.ctx = 0;
        args.parallel = Some(0);

        let error = validate_launch_preconditions(dir.path(), &args, None)
            .expect_err("Tailscale Serve mutation must remain fail-closed");
        assert_eq!(
            error,
            "--tailscale is fail-closed before registration, PID, engine, model, or network probes because scoped proxy cleanup is unavailable"
        );
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
