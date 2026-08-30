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
    ListenerState, LiveProcess, ProcessError, ProcessIdentity, loopback_listener_state,
};
use crate::server_registration::{
    CapturedRegistration, PublishError, RegistrationInventory, RegistrationScope, RegistrationSlot,
    RemovalOutcome, ReplacementOutcome, inventory_runfiles, publish_mirrored, remove_if_unchanged,
    replace_if_unchanged, validate_runfile,
};
use crate::server_resolution::{Candidate, CandidateState, Resolution, resolve};

const RUNFILE_SCHEMA_V2: u8 = 2;

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

/// Resolve one healthy managed server for read-only consumers.
///
/// This uses the same lossless inventory, process identity, listener ownership,
/// and stale-alias reconciliation as status/down. Static file precedence never
/// selects an endpoint, and degraded or unverifiable registrations never fall
/// through to an unrelated default backend.
pub(crate) fn read_runfile_result(workspace: &Path) -> Result<Option<ServerRunfile>, String> {
    read_runfile_result_impl(workspace, global_runfile_path())
}

fn read_runfile_result_impl(
    workspace: &Path,
    global: Option<PathBuf>,
) -> Result<Option<ServerRunfile>, String> {
    let observations = observe_lifecycle(workspace, global);
    match lifecycle_resolution(&observations) {
        Resolution::Empty => Ok(None),
        Resolution::StaleOnly { stale } => {
            let details = stale
                .iter()
                .map(|index| match &observations[*index].candidate.state {
                    CandidateState::Stale { reason } => {
                        format!("{}: {reason}", observations[*index].candidate.label)
                    }
                    _ => observations[*index].candidate.label.clone(),
                })
                .collect::<Vec<_>>()
                .join("; ");
            Err(format!(
                "only stale server registrations remain ({details}); run `ferric server down` to clean them after reviewing the reported listener state"
            ))
        }
        Resolution::Blocked { reasons } => Err(format!(
            "server registration resolution is blocked: {}",
            reasons.join("; ")
        )),
        Resolution::One {
            target,
            http_healthy,
            listener_present,
            listener_loopback_only,
            ..
        } => {
            let capture = observations[target]
                .capture
                .as_ref()
                .expect("resolved target has a captured registration");
            if !listener_present {
                return Err(format!(
                    "managed server PID {} has no listener on its registered port {}",
                    capture.runfile.pid, capture.runfile.port
                ));
            }
            if !listener_loopback_only {
                return Err(format!(
                    "managed server PID {} exposes registered port {} through a wildcard/public listener",
                    capture.runfile.pid, capture.runfile.port
                ));
            }
            if !http_healthy {
                return Err(format!(
                    "managed server PID {} owns the registered loopback listener, but its engine health endpoint is not healthy",
                    capture.runfile.pid
                ));
            }
            Ok(Some(capture.runfile.clone()))
        }
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
    let process = LiveProcess::acquire(runfile.pid)
        .map_err(|error| format!("acquire registered process: {error}"))?;
    let facts = process
        .inspect(runfile.port)
        .map_err(|error| format!("inspect registered process: {error}"))?;
    if let Some(expected) = &runfile.process_identity
        && expected != &facts.identity
    {
        return Err(format!(
            "registered server PID {} no longer matches its creation/executable/argv identity",
            runfile.pid
        ));
    }
    match facts.listener {
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
        ListenerState::Uninspectable(error) => return Err(error),
    }
    if !http_status_ok("127.0.0.1", runfile.port, health_path(runfile.engine)) {
        return Err(format!(
            "registered server PID {} does not have a healthy engine endpoint on loopback port {}",
            runfile.pid, runfile.port
        ));
    }
    Ok(RegisteredServerSnapshot {
        pid: runfile.pid,
        executable: facts.identity.executable,
        argv: facts.identity.argv,
        listener_owner_pid: runfile.pid,
    })
}

/// Retain the spawned child while polling HTTP readiness. This ties a healthy
/// endpoint to a process that has not already exited before any runfile is
/// written. Port-availability preflight closes the ordinary conflicting-listener
/// case; the post-probe `try_wait` closes the child-exited-during-probe race.
fn wait_healthy(
    child: &mut Child,
    engine: Engine,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!("engine process exited before readiness ({status})"));
            }
            Ok(None) => {}
            Err(error) => return Err(format!("could not inspect engine process: {error}")),
        }

        if http_status_ok(host, port, health_path(engine)) {
            return match child.try_wait() {
                Ok(Some(status)) => Err(format!(
                    "engine process exited while readiness was checked ({status})"
                )),
                Ok(None) => Ok(()),
                Err(error) => Err(format!("could not inspect engine process: {error}")),
            };
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "HTTP health endpoint {} did not return 200 within {}s",
                health_path(engine),
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn stop_child(child: &mut Child) -> Result<(), String> {
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => {
            return Err(format!(
                "could not prove spawned child PID {} is still the unreaped child before fallback shutdown: {error}",
                child.id()
            ));
        }
    }

    if let Err(kill_error) = child.kill() {
        return match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!(
                "could not terminate owned child PID {}: {kill_error}",
                child.id()
            )),
            Err(recheck_error) => Err(format!(
                "could not terminate owned child PID {} ({kill_error}) or recheck it ({recheck_error})",
                child.id()
            )),
        };
    }
    child.wait().map(|_| ()).map_err(|error| {
        format!(
            "could not reap terminated child PID {}: {error}",
            child.id()
        )
    })
}

fn require_listener_released(pid: u32, port: u16) -> Result<(), String> {
    match loopback_listener_state(pid, port) {
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

/// Stop the exact process object retained before readiness and publication.
/// Registration rollback is authorized only after this returns `Ok(())`.
fn stop_managed_child(child: &mut Child, process: &LiveProcess, port: u16) -> Result<(), String> {
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
    require_listener_released(process.pid(), port)?;
    child
        .wait()
        .map(|_| ())
        .map_err(|error| format!("reap exited child PID {}: {error}", child.id()))
}

struct LifecycleObservation {
    candidate: Candidate,
    capture: Option<CapturedRegistration>,
    process: Option<LiveProcess>,
    /// Listener owners observed while this registration's process identity was
    /// stale. Cleanup is safe only when every such owner is independently
    /// accounted for by a verified registration in the same inventory.
    stale_listener_owners: Vec<u32>,
}

fn registration_label(scope: RegistrationScope, path: &Path) -> String {
    format!("{scope} registration {}", path.display())
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
        } => observations.push(LifecycleObservation {
            candidate: Candidate {
                label: registration_label(scope, &path),
                state: CandidateState::Blocked {
                    reason: reason.to_string(),
                },
            },
            capture: None,
            process: None,
            stale_listener_owners: Vec::new(),
        }),
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
            } => observations.push(LifecycleObservation {
                candidate: Candidate {
                    label: registration_label(scope, &path),
                    state: CandidateState::Blocked {
                        reason: format!(
                            "{reason}; promised by {} registration {}",
                            promised.source.scope,
                            promised.source.path.display()
                        ),
                    },
                },
                capture: None,
                process: None,
                stale_listener_owners: Vec::new(),
            }),
        }
    }
    (captures, observations)
}

fn stale_observation(
    capture: CapturedRegistration,
    reason: String,
    stale_listener_owners: Vec<u32>,
) -> LifecycleObservation {
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            label,
            state: CandidateState::Stale { reason },
        },
        capture: Some(capture),
        process: None,
        stale_listener_owners,
    }
}

fn stale_observation_from_listener(
    capture: CapturedRegistration,
    reason: String,
    listener: ListenerState,
) -> LifecycleObservation {
    let pid = capture.runfile.pid;
    let port = capture.runfile.port;
    match listener {
        ListenerState::Absent => stale_observation(capture, reason, Vec::new()),
        ListenerState::OwnedByTarget | ListenerState::OwnedByTargetWildcard => {
            stale_observation(capture, reason, vec![pid])
        }
        ListenerState::OwnedByOther(owners) => stale_observation(capture, reason, owners),
        ListenerState::Uninspectable(error) => blocked_observation(
            capture,
            format!(
                "{reason}; listener ownership on registered loopback port {port} is uninspectable: {error}"
            ),
        ),
    }
}

fn blocked_observation(capture: CapturedRegistration, reason: String) -> LifecycleObservation {
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            label,
            state: CandidateState::Blocked { reason },
        },
        capture: Some(capture),
        process: None,
        stale_listener_owners: Vec::new(),
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
                loopback_listener_state(pid, port),
            ),
            Ok(false) => blocked_observation(
                capture,
                format!(
                    "live schema-1 PID {pid} has no creation identity and cannot authorize teardown; from the workspace containing its local registration run `ferric server adopt --pid {pid}` to verify and record the current process generation without signalling it"
                ),
            ),
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
            facts.listener,
        );
    }
    if facts.identity.executable != expected.executable || facts.identity.argv != expected.argv {
        return blocked_observation(
            capture,
            format!(
                "live process creation instance {pid} has executable/argv facts that differ from its registration"
            ),
        );
    }

    let (listener_present, listener_loopback_only) = match facts.listener {
        ListenerState::OwnedByTarget => (true, true),
        ListenerState::OwnedByTargetWildcard => (true, false),
        ListenerState::Absent => (false, true),
        ListenerState::OwnedByOther(owners) => {
            return blocked_observation(
                capture,
                format!("loopback port {} is owned by other PIDs {owners:?}", port),
            );
        }
        ListenerState::Uninspectable(error) => {
            return blocked_observation(
                capture,
                format!("loopback port {} ownership is uninspectable: {error}", port),
            );
        }
    };
    let http_healthy =
        listener_present && http_status_ok("127.0.0.1", port, health_path(capture.runfile.engine));
    let registration_key = serde_json::to_vec(&capture.runfile)
        .expect("a deserialized server runfile must serialize again");
    let label = registration_label(capture.scope, &capture.path);
    LifecycleObservation {
        candidate: Candidate {
            label,
            state: CandidateState::Verified {
                identity: facts.identity,
                registration_key,
                http_healthy,
                listener_present,
                listener_loopback_only,
            },
        },
        capture: Some(capture),
        process: Some(process),
        stale_listener_owners: Vec::new(),
    }
}

fn observe_lifecycle(workspace: &Path, global_path: Option<PathBuf>) -> Vec<LifecycleObservation> {
    let inventory = inventory_runfiles(workspace, global_path);
    let (captures, mut observations) = expand_registration_captures(inventory);
    observations.extend(captures.into_iter().map(observe_registration));
    observations
}

fn lifecycle_resolution(observations: &[LifecycleObservation]) -> Resolution {
    let candidates = observations
        .iter()
        .map(|observation| observation.candidate.clone())
        .collect::<Vec<_>>();
    let resolution = resolve(&candidates);
    let mut listener_blockers = Vec::new();
    match &resolution {
        Resolution::StaleOnly { stale } => {
            for index in stale {
                let observation = &observations[*index];
                if !observation.stale_listener_owners.is_empty() {
                    listener_blockers.push(format!(
                        "{} is stale but registered port ownership remains with PIDs {:?}",
                        observation.candidate.label, observation.stale_listener_owners
                    ));
                }
            }
        }
        Resolution::One {
            target,
            stale,
            listener_present,
            ..
        } => {
            let target_capture = observations[*target]
                .capture
                .as_ref()
                .expect("verified lifecycle target has a capture");
            for index in stale {
                let observation = &observations[*index];
                if observation.stale_listener_owners.is_empty() {
                    continue;
                }
                let stale_capture = observation
                    .capture
                    .as_ref()
                    .expect("stale lifecycle observation has a capture");
                let accounted_by_target = *listener_present
                    && stale_capture.runfile.port == target_capture.runfile.port
                    && observation
                        .stale_listener_owners
                        .iter()
                        .all(|owner| *owner == target_capture.runfile.pid);
                if !accounted_by_target {
                    listener_blockers.push(format!(
                        "{} is stale but registered port ownership by PIDs {:?} is not accounted for by the selected managed server",
                        observation.candidate.label, observation.stale_listener_owners
                    ));
                }
            }
        }
        Resolution::Empty | Resolution::Blocked { .. } => {}
    }
    if listener_blockers.is_empty() {
        resolution
    } else {
        Resolution::Blocked {
            reasons: listener_blockers,
        }
    }
}

/// Is the engine binary runnable? Tries `<program> --version`.
fn binary_present(engine: Engine) -> bool {
    matches!(
        Command::new(engine.program())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status(),
        Ok(status) if status.success()
    )
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
    let managed_process = match LiveProcess::acquire_child(&child) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("could not acquire exact lifecycle control for spawned PID {pid}: {error}");
            if let Err(stop_error) = stop_child(&mut child) {
                eprintln!("could not confirm fallback child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
    };
    debug_assert_eq!(managed_process.pid(), pid);
    match child.try_wait() {
        Ok(None) => {}
        Ok(Some(status)) => {
            eprintln!(
                "spawned engine PID {pid} exited before retained-process binding could be confirmed ({status}); no replacement process was signalled"
            );
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!(
                "could not confirm spawned engine PID {pid} after acquiring its process object ({error}); refusing to signal an unproven binding"
            );
            return ExitCode::FAILURE;
        }
    }

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
    let process_facts = match managed_process.inspect(cfg.port) {
        Ok(facts) => facts,
        Err(error) => {
            eprintln!(
                "server became healthy but its process/listener identity could not be bound: {error}"
            );
            if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
                eprintln!("could not confirm exact child shutdown: {stop_error}");
            }
            return ExitCode::FAILURE;
        }
    };
    if process_facts.listener != ListenerState::OwnedByTarget {
        eprintln!(
            "server child PID {pid} does not exclusively own the expected loopback listener: {:?}",
            process_facts.listener
        );
        if let Err(stop_error) = stop_managed_child(&mut child, &managed_process, cfg.port) {
            eprintln!("could not confirm exact child shutdown: {stop_error}");
        }
        return ExitCode::FAILURE;
    }
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
                CandidateState::Blocked { reason } | CandidateState::Stale { reason } => reason,
                CandidateState::Verified { .. } => "unexpected verified observation".to_string(),
            };
            eprintln!("  - {}: {reason}", observation.candidate.label);
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

fn status_impl(workspace: &Path, global_path: Option<PathBuf>) -> ExitCode {
    let observations = observe_lifecycle(workspace, global_path);
    for observation in &observations {
        match &observation.candidate.state {
            CandidateState::Verified {
                http_healthy,
                listener_present,
                listener_loopback_only,
                ..
            } => {
                let runfile = &observation
                    .capture
                    .as_ref()
                    .expect("verified observations have captures")
                    .runfile;
                println!(
                    "[verified] {}: engine={:?} pid={} base_url={} listener={} http={}",
                    observation.candidate.label,
                    runfile.engine,
                    runfile.pid,
                    runfile.base_url,
                    if !*listener_present {
                        "absent"
                    } else if *listener_loopback_only {
                        "owned-loopback"
                    } else {
                        "wildcard-public"
                    },
                    if *http_healthy {
                        "healthy"
                    } else {
                        "not-healthy"
                    }
                );
            }
            CandidateState::Stale { reason } => {
                println!("[stale] {}: {reason}", observation.candidate.label);
            }
            CandidateState::Blocked { reason } => {
                eprintln!("[blocked] {}: {reason}", observation.candidate.label);
            }
        }
    }

    match lifecycle_resolution(&observations) {
        Resolution::Empty => {
            println!("no server registered in local or global scope");
            ExitCode::FAILURE
        }
        Resolution::StaleOnly { .. } => {
            println!(
                "no live managed server; stale registrations can be removed with `ferric server down`"
            );
            ExitCode::FAILURE
        }
        Resolution::Blocked { reasons } => {
            eprintln!(
                "server registration state is ambiguous or unverifiable; no process action is authorized"
            );
            for reason in reasons {
                eprintln!("  - {reason}");
            }
            ExitCode::FAILURE
        }
        Resolution::One {
            target,
            aliases,
            stale,
            http_healthy,
            listener_present,
            listener_loopback_only,
        } => {
            let runfile = &observations[target]
                .capture
                .as_ref()
                .expect("resolved target has a capture")
                .runfile;
            println!(
                "resolved one managed server: pid={} base_url={} aliases={} stale={}",
                runfile.pid,
                runfile.base_url,
                aliases.len() + 1,
                stale.len()
            );
            if listener_present && listener_loopback_only && http_healthy {
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "managed process identity is exact, but {}{}",
                    if listener_present && !listener_loopback_only {
                        "its registered port is bound through a wildcard/public listener"
                    } else if listener_present {
                        "its HTTP endpoint is not healthy"
                    } else {
                        "its expected loopback listener is absent"
                    },
                    if listener_present && listener_loopback_only && !http_healthy {
                        ""
                    } else {
                        "; teardown remains identity-authorized"
                    }
                );
                ExitCode::FAILURE
            }
        }
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
                observations[index].candidate.label
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
                observations[*index].candidate.label
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

fn down_impl(workspace: &Path, global_path: Option<PathBuf>) -> ExitCode {
    let mut observations = observe_lifecycle(workspace, global_path);
    match lifecycle_resolution(&observations) {
        Resolution::Empty => {
            println!("no server registered");
            ExitCode::SUCCESS
        }
        Resolution::Blocked { reasons } => {
            eprintln!("refusing teardown: server registration state is ambiguous or unverifiable");
            for reason in reasons {
                eprintln!("  - {reason}");
            }
            ExitCode::FAILURE
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
        Resolution::One {
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
                        match facts.listener {
                            ListenerState::OwnedByTarget
                            | ListenerState::OwnedByTargetWildcard
                            | ListenerState::Absent => {}
                            ListenerState::OwnedByOther(owners) => {
                                eprintln!(
                                    "refusing teardown: loopback port {port} is owned by other PIDs {owners:?}"
                                );
                                return ExitCode::FAILURE;
                            }
                            ListenerState::Uninspectable(error) => {
                                eprintln!(
                                    "refusing teardown: loopback ownership is uninspectable: {error}"
                                );
                                return ExitCode::FAILURE;
                            }
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

fn doctor(workspace: &Path, args: &ServerUpArgs) -> ExitCode {
    let mut ok = true;

    let bin = binary_present(args.engine);
    println!(
        "[{}] engine binary `{}`",
        if bin { "ok" } else { "MISSING" },
        args.engine.program()
    );
    ok &= bin;

    if args.port == 0 {
        println!("[INVALID] --port must be greater than zero");
        ok = false;
    }

    if args.engine == Engine::LlamaServer {
        if args.ctx == 0 {
            println!("[INVALID] --ctx must be greater than zero for llama-server");
            ok = false;
        }

        if let Some(model) = &args.model {
            let present = Path::new(model).is_file();
            println!(
                "[{}] model `{}`",
                if present { "ok" } else { "MISSING" },
                model
            );
            ok &= present;
        } else {
            println!("[MISSING] --model is required for llama-server");
            ok = false;
        }

        if let Some(mmproj) = &args.mmproj {
            let present = mmproj.is_file();
            println!(
                "[{}] multimodal projector `{}`",
                if present { "ok" } else { "MISSING" },
                mmproj.display()
            );
            ok &= present;
        }
        if args.parallel == Some(0) {
            println!("[INVALID] --parallel must be greater than zero for llama-server");
            ok = false;
        }
    } else if args.seed.is_some() || args.parallel.is_some() {
        println!("[INVALID] --seed and --parallel are supported only by llama-server");
        ok = false;
    }
    if args.tailscale {
        println!(
            "[BLOCKED] --tailscale is temporarily fail-closed until Ferric can compare-and-remove only the Serve endpoint it owns"
        );
        ok = false;
    }

    match read_runfile_result(workspace) {
        Ok(Some(rf)) => {
            println!(
                "[ok] exact managed process/listener identity and HTTP health at {}",
                rf.base_url
            );
            println!("     health: {}", health_url(rf.engine, &rf.base_url));
            println!("     verify the constrained path: `ferric bench ltd --protocol grammar`");
        }
        Ok(None) => println!("[info] no server running — `ferric server up` to start one"),
        Err(error) => {
            println!("[BLOCKED] server registration inventory: {error}");
            ok = false;
        }
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
    use crate::server_process::canonical_test_start_token;
    use std::net::TcpListener;
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
        assert_eq!(helper_facts.listener, ListenerState::OwnedByTarget);
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
    fn wildcard_listener_fails_status_but_exact_identity_can_be_stopped() {
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
        assert_eq!(helper_facts.listener, ListenerState::OwnedByTargetWildcard);
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

        assert_eq!(status_impl(&workspace, None), ExitCode::FAILURE);
        assert_eq!(down_impl(&workspace, None), ExitCode::SUCCESS);
        let _ = helper.wait().expect("reap terminated wildcard helper");
        assert!(!local.exists(), "exact wildcard registration cleaned");
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
        assert_eq!(helper_facts.listener, ListenerState::OwnedByTarget);
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
        release.send(()).unwrap();
        server.join().unwrap();
        let inspected = inspected.unwrap();
        assert_eq!(inspected.pid, std::process::id());
        assert_eq!(inspected.listener_owner_pid, std::process::id());
        assert!(!inspected.argv.is_empty());
        assert!(inspected.executable.is_file());
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
            observations[0].candidate.label,
            registration_label(RegistrationScope::Origin, &blocked_path)
        );
        assert!(matches!(
            &observations[0].candidate.state,
            CandidateState::Blocked { reason }
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
