//! Foreground startup owns only the process scope it creates. Saved choices
//! are hints, never model qualification, consent, or managed-server authority.

mod models;
mod probe;
mod runtime;
mod storage;

#[cfg(test)]
mod tests;

use std::fmt;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::backend::BackendOpts;
use crate::config::Config;
use crate::server::{self, ManagedDiscoveryScope, ManagedServer, ManagedServerState};
use crate::server_process::{ListenerState, LiveProcess};
use models::LocalModel;
use runtime::OwnedEngine;
use storage::{Preference, WorkspaceState};

const STARTUP_LIMIT: Duration = Duration::from_secs(180);
const LOCAL_KEY: &str = "ferric-local";
const DEFAULT_CONTEXT: u32 = 4096;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelChoice {
    pub(crate) label: String,
    pub(crate) bytes: Option<u64>,
    pub(crate) path: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct StartupError {
    message: String,
    next_action: Option<&'static str>,
    diagnostics: Option<String>,
    cancelled: bool,
}

impl StartupError {
    /// The caller supplies an already actionable message; do not add another action.
    fn actionable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            next_action: None,
            diagnostics: None,
            cancelled: false,
        }
    }
    /// Cause-only failures always have one selected-folder inspection action.
    fn cause(message: impl Into<String>) -> Self {
        Self {
            next_action: Some(
                "Inspect the model and server configuration for the selected folder.",
            ),
            ..Self::actionable(message)
        }
    }
    fn with_diagnostics(mut self, diagnostics: String) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }
    pub(crate) fn human_message(&self) -> String {
        match self.next_action {
            Some(action) => format!("{} {action}", self.message),
            None => self.message.clone(),
        }
    }
    fn cancelled() -> Self {
        Self {
            message: "Setup cancelled; owned resources have been closed.".into(),
            next_action: None,
            diagnostics: None,
            cancelled: true,
        }
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.human_message())?;
        if let Some(diagnostics) = &self.diagnostics {
            write!(formatter, "\nEngine diagnostics (bounded):\n{diagnostics}")?;
        }
        Ok(())
    }
}
impl std::error::Error for StartupError {}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), StartupError> {
    if cancel.load(Ordering::Relaxed) {
        Err(StartupError::cancelled())
    } else {
        Ok(())
    }
}

struct BorrowedManaged {
    process: LiveProcess,
    scope: ManagedDiscoveryScope,
    server: ManagedServer,
}

impl BorrowedManaged {
    fn acquire(scope: ManagedDiscoveryScope, server: ManagedServer) -> Result<Self, StartupError> {
        let process = LiveProcess::acquire(server.runfile.pid)
            .map_err(|_| StartupError::actionable("The registered server exited or cannot be retained. Resolve its status before restarting setup."))?;
        let borrowed = Self {
            process,
            scope,
            server,
        };
        borrowed.validate()?;
        Ok(borrowed)
    }

    fn validate(&self) -> Result<(), StartupError> {
        let facts = self.process.inspect(self.server.runfile.port)
            .map_err(|_| StartupError::cause("The borrowed server exited or cannot be verified. No borrowed process was stopped."))?;
        if facts.identity != self.server.identity || facts.listener != ListenerState::OwnedByTarget
        {
            return Err(StartupError::cause(
                "The borrowed server no longer owns its exact loopback listener. No borrowed process was stopped.",
            ));
        }
        match server::discover_managed_server_in(&self.scope).state {
            ManagedServerState::Ready(current)
                if current.fingerprint == self.server.fingerprint =>
            {
                Ok(())
            }
            _ => Err(StartupError::actionable(
                "Managed registrations changed. Resolve server status and restart setup; no registration was modified.",
            )),
        }
    }
}

enum Source {
    Local(Vec<LocalModel>),
    Borrowed {
        endpoint: String,
        key: String,
        managed: Option<Box<BorrowedManaged>>,
    },
}

pub(crate) struct Startup {
    pub(crate) models: Vec<ModelChoice>,
    pub(crate) preferred_index: Option<usize>,
    /// A stored choice no longer matches; even one model needs re-selection.
    pub(crate) requires_model_choice: bool,
    pub(crate) will_start_engine: bool,
    source: Source,
    state: WorkspaceState,
    config: Config,
}

impl Startup {
    pub(crate) fn begin(
        workspace: &Path,
        config: &Config,
        explicit_model: Option<&Path>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Self, StartupError> {
        let scope = ManagedDiscoveryScope::for_workspace(workspace)
            .map_err(|_| StartupError::cause("The workspace cannot be resolved."))?;
        Self::begin_in(workspace, config, explicit_model, cancel, scope)
    }

    fn begin_in(
        workspace: &Path,
        config: &Config,
        explicit_model: Option<&Path>,
        cancel: &AtomicBool,
        scope: ManagedDiscoveryScope,
    ) -> Result<Self, StartupError> {
        config
            .validate()
            .map_err(|error| StartupError::actionable(error.to_string()))?;
        check_cancelled(cancel)?;
        // Validate explicit endpoint syntax before any storage effect. Its
        // configured key is never applied to an implicitly discovered server.
        let explicit_endpoint = config
            .api_base
            .as_deref()
            .map(probe::endpoint)
            .transpose()?;
        if explicit_endpoint.is_some() && explicit_model.is_some() {
            return Err(StartupError::actionable(
                "Choose either an explicit endpoint or a local model path, not both.",
            ));
        }
        let state = WorkspaceState::acquire(workspace).map_err(StartupError::actionable)?;
        let preference = state.read_preference().map_err(StartupError::actionable)?;
        let context = config.clone();
        if let Some(endpoint) = explicit_endpoint {
            let key = config
                .api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .unwrap_or_else(|| LOCAL_KEY.into());
            return Self::borrow(
                state,
                context,
                endpoint,
                key,
                None,
                preference.as_ref(),
                cancel,
            );
        }
        check_cancelled(cancel)?;
        let discovery = server::discover_managed_server_in(&scope);
        check_cancelled(cancel)?;
        state.validate().map_err(StartupError::actionable)?;
        match discovery.state {
            ManagedServerState::Ready(server) => {
                let endpoint = probe::endpoint(&server.runfile.base_url)?;
                let managed = Box::new(BorrowedManaged::acquire(scope, server)?);
                Self::borrow(
                    state,
                    context,
                    endpoint,
                    LOCAL_KEY.into(),
                    Some(managed),
                    preference.as_ref(),
                    cancel,
                )
            }
            ManagedServerState::Empty => {
                let configured_path = config.model.as_deref().map(Path::new).filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
                });
                let local = models::scan(workspace, explicit_model.or(configured_path))?;
                let choices = local
                    .iter()
                    .map(|model| model.choice.clone())
                    .collect::<Vec<_>>();
                let preferred = if explicit_model.is_some() || configured_path.is_some() {
                    (!local.is_empty()).then_some(0)
                } else if let Some(configured) = config.model.as_deref() {
                    choices.iter().position(|model| model.label == configured)
                } else {
                    preference
                        .as_ref()
                        .and_then(|saved| local.iter().position(|model| model.matches(saved)))
                };
                if config.model.is_some() && explicit_model.is_none() && preferred.is_none() {
                    return Err(StartupError::actionable(
                        "The configured model is not available. Update the model setting for the selected folder.",
                    ));
                }
                let requires_model_choice = preference.is_some() && preferred.is_none();
                state.validate().map_err(StartupError::actionable)?;
                Ok(Self {
                    models: choices,
                    preferred_index: preferred,
                    requires_model_choice,
                    will_start_engine: true,
                    source: Source::Local(local),
                    state,
                    config: context,
                })
            }
            _ => Err(StartupError::actionable(
                "Managed server state is stale, degraded, conflicting, or unverifiable. Inspect the selected folder's managed server status (the ferric server status view). No server was started or stopped.",
            )),
        }
    }

    fn borrow(
        state: WorkspaceState,
        config: Config,
        endpoint: String,
        key: String,
        managed: Option<Box<BorrowedManaged>>,
        preference: Option<&Preference>,
        cancel: &AtomicBool,
    ) -> Result<Self, StartupError> {
        if let Some(server) = &managed {
            server.validate()?;
        }
        let ids = probe::models(
            &endpoint,
            &key,
            cancel,
            Instant::now() + probe::PROBE_TIMEOUT,
        )?;
        if let Some(server) = &managed {
            server.validate()?;
        }
        state.validate().map_err(StartupError::actionable)?;
        let preferred = if let Some(model) = &config.model {
            ids.iter().position(|id| id == model)
        } else {
            preference
                .filter(|saved| saved.endpoint.as_deref() == Some(&endpoint))
                .and_then(|saved| saved.model_id.as_ref())
                .and_then(|id| ids.iter().position(|candidate| candidate == id))
        };
        if config.model.is_some() && preferred.is_none() {
            return Err(StartupError::actionable(
                "The configured model is not advertised by this server. Update the model setting before trying again.",
            ));
        }
        let requires_model_choice = preference.is_some() && preferred.is_none();
        Ok(Self {
            models: ids
                .into_iter()
                .map(|label| ModelChoice {
                    label,
                    bytes: None,
                    path: None,
                })
                .collect(),
            preferred_index: preferred,
            requires_model_choice,
            will_start_engine: false,
            source: Source::Borrowed {
                endpoint,
                key,
                managed,
            },
            state,
            config,
        })
    }

    pub(crate) fn prepare(
        self,
        index: usize,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&str),
    ) -> Result<PreparedSession, StartupError> {
        self.prepare_with(index, &cancel, progress, native_launch)
    }

    fn prepare_with(
        self,
        index: usize,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(&str),
        launch: impl FnOnce(
            &LocalModel,
            u32,
            u16,
            &AtomicBool,
            Instant,
        ) -> Result<(Command, String), StartupError>,
    ) -> Result<PreparedSession, StartupError> {
        check_cancelled(cancel)?;
        let selected = self
            .models
            .get(index)
            .cloned()
            .ok_or_else(|| StartupError::actionable("Select one of the available models."))?;
        self.state.validate().map_err(StartupError::actionable)?;
        let context = self.config.ctx.unwrap_or(DEFAULT_CONTEXT);
        let deadline = Instant::now() + STARTUP_LIMIT;
        match self.source {
            Source::Borrowed {
                endpoint,
                key,
                managed,
            } => {
                if let Some(server) = &managed {
                    server.validate()?;
                }
                progress("Checking the selected server model…");
                let ids = probe::models(&endpoint, &key, cancel, deadline)?;
                if !ids.contains(&selected.label) {
                    return Err(StartupError::actionable(
                        "The selected server model changed. Select a model again.",
                    ));
                }
                if let Some(server) = &managed {
                    server.validate()?;
                }
                self.state.validate().map_err(StartupError::actionable)?;
                check_cancelled(cancel)?;
                self.state
                    .write_preference(&Preference {
                        schema_version: 1,
                        model_path: None,
                        model_bytes: None,
                        modified_nanos: None,
                        endpoint: Some(endpoint.clone()),
                        model_id: Some(selected.label.clone()),
                    })
                    .map_err(StartupError::actionable)?;
                Ok(PreparedSession {
                    backend_opts: BackendOpts {
                        api_base: Some(endpoint),
                        api_key: Some(key),
                        model: Some(selected.label.clone()),
                    },
                    context,
                    model: selected.label,
                    engine_identity: if managed.is_some() {
                        "retained managed server"
                    } else {
                        "explicit configured endpoint (external lifecycle)"
                    }
                    .into(),
                    ownership: Ownership::Borrowed(managed),
                    local_model: None,
                    state: self.state,
                    closed: false,
                })
            }
            Source::Local(mut local) => {
                let model = local.remove(index);
                model.validate()?;
                progress("Checking the installed engine…");
                let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                    .map_err(|_| StartupError::cause("No local listener port is available."))?;
                let port = listener
                    .local_addr()
                    .map_err(|_| {
                        StartupError::cause("The local listener port could not be inspected.")
                    })?
                    .port();
                let (command, engine_identity) = launch(&model, context, port, cancel, deadline)?;
                self.state.validate().map_err(StartupError::actionable)?;
                model.validate()?;
                check_cancelled(cancel)?;
                drop(listener); // The child must bind it; exact owner checks close the race.
                progress(
                    "Loading the model with conservative CPU settings (not hardware-qualified)…",
                );
                let mut owned = OwnedEngine::spawn(command, port)?;
                let endpoint = format!("http://127.0.0.1:{port}/v1");
                let outcome = wait_ready(&mut owned, &endpoint, cancel, deadline);
                let ids = match outcome {
                    Ok(ids) => ids,
                    Err(error) => {
                        owned.child.cleanup()?;
                        let diagnostics = owned.child.diagnostics();
                        return Err(if error.is_cancelled() || diagnostics.trim().is_empty() {
                            error
                        } else {
                            error.with_diagnostics(diagnostics)
                        });
                    }
                };
                // Closed llama-server launches select exactly one GGUF. Do not
                // invent `default` or infer a model identity from a filename.
                if ids.len() != 1 {
                    return Err(StartupError::actionable(
                        "The owned engine advertised more than one model. Restart setup with a single-model engine.",
                    ));
                }
                let id = ids.into_iter().next().expect("one model");
                model.validate()?;
                owned.validate()?;
                self.state.validate().map_err(StartupError::actionable)?;
                check_cancelled(cancel)?;
                self.state
                    .write_preference(&model.preference())
                    .map_err(StartupError::actionable)?;
                Ok(PreparedSession {
                    backend_opts: BackendOpts {
                        api_base: Some(endpoint),
                        api_key: Some(LOCAL_KEY.into()),
                        model: Some(id.clone()),
                    },
                    context,
                    model: id,
                    engine_identity,
                    ownership: Ownership::Owned(Box::new(owned)),
                    local_model: Some(model),
                    state: self.state,
                    closed: false,
                })
            }
        }
    }
}

fn wait_ready(
    owned: &mut OwnedEngine,
    endpoint: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<String>, StartupError> {
    loop {
        check_cancelled(cancel)?;
        if Instant::now() >= deadline {
            return Err(StartupError::actionable(
                "Model startup exceeded 180 seconds. Select a smaller model.",
            ));
        }
        if owned
            .child
            .tree
            .try_wait_leader()
            .map_err(|_| StartupError::cause("The owned engine could not be inspected."))?
            .is_some()
        {
            return Err(StartupError::actionable(
                "The engine exited while loading the model. Choose a compatible model.",
            ));
        }
        match owned.listener()? {
            ListenerState::Absent => {}
            ListenerState::OwnedByTarget => {
                let ready = probe::health(endpoint, cancel, deadline)?;
                owned.validate()?;
                if ready {
                    let ids = probe::models(endpoint, LOCAL_KEY, cancel, deadline)?;
                    owned.validate()?;
                    return Ok(ids);
                }
            }
            _ => {
                return Err(StartupError::cause(
                    "The selected port is not exclusively owned by the new engine. The owned engine was closed; unrelated listeners were not touched.",
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn native_launch(
    model: &LocalModel,
    context: u32,
    port: u16,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(Command, String), StartupError> {
    if !cfg!(any(windows, target_os = "linux")) {
        return Err(StartupError::actionable(
            "Owned startup requires native process/listener verification on Windows or Linux. Configure an existing endpoint on this host.",
        ));
    }
    let executable = engine_on_path()?;
    let mut version_command = Command::new(&executable);
    version_command.arg("--version");
    let version = runtime::version(version_command, cancel, deadline)?;
    let config = server::ServerConfig {
        engine: server::Engine::LlamaServer,
        model: Some(
            model
                .choice
                .path
                .as_ref()
                .expect("local model")
                .to_str()
                .ok_or_else(|| StartupError::cause("The selected model path must be valid text."))?
                .into(),
        ),
        mmproj: None,
        ctx: context,
        host: "127.0.0.1".into(),
        port,
        threads: None,
        gpu_layers: Some(0),
        batch_size: None,
        seed: None,
        parallel: Some(1),
        tailscale: false,
    };
    let launch = server::command(&config);
    let mut command = Command::new(&executable);
    command.args(launch.args).envs(launch.env);
    Ok((command, format!("{} — {version}", executable.display())))
}

fn engine_on_path() -> Result<PathBuf, StartupError> {
    let paths = std::env::var_os("PATH").ok_or_else(|| StartupError::actionable("The engine search path is unavailable. Configure an existing endpoint for the selected folder."))?;
    let name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    for (index, directory) in std::env::split_paths(&paths).enumerate() {
        if index >= 256 {
            break;
        }
        // Never inherit Windows executable search through the current working
        // directory, nor relative/empty PATH entries supplied by the workspace.
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|_| StartupError::cause("The installed engine path cannot be resolved."));
        }
    }
    Err(StartupError::actionable(
        "llama-server is not installed on PATH. Configure an existing endpoint for the selected folder; setup will not download or execute an unselected installer.",
    ))
}

enum Ownership {
    Borrowed(Option<Box<BorrowedManaged>>),
    Owned(Box<OwnedEngine>),
}

pub(crate) struct PreparedSession {
    pub(crate) backend_opts: BackendOpts,
    pub(crate) context: u32,
    pub(crate) model: String,
    pub(crate) engine_identity: String,
    // Field order matters: owned process cleanup precedes releasing model
    // handles and the workspace lock, including during panic unwinding.
    ownership: Ownership,
    local_model: Option<LocalModel>,
    state: WorkspaceState,
    closed: bool,
}

impl PreparedSession {
    pub(crate) fn ownership_label(&self) -> &'static str {
        match self.ownership {
            Ownership::Borrowed(_) => "borrowed (left running on exit)",
            Ownership::Owned(_) => "owned foreground (closed on exit)",
        }
    }
    pub(crate) fn validate(&self) -> Result<(), StartupError> {
        if self.closed {
            return Err(StartupError::cause("This session has already closed."));
        }
        self.state.validate().map_err(StartupError::actionable)?;
        if let Some(model) = &self.local_model {
            model.validate()?;
        }
        match &self.ownership {
            Ownership::Borrowed(Some(server)) => server.validate(),
            Ownership::Borrowed(None) => Ok(()),
            Ownership::Owned(engine) => engine.validate(),
        }
    }
    pub(crate) fn create_trace_file(
        &self,
        name: &str,
    ) -> Result<(PathBuf, std::fs::File), StartupError> {
        self.validate()?;
        self.state
            .create_trace(name)
            .map_err(StartupError::actionable)
    }
    pub(crate) fn cleanup(&mut self) -> Result<(), StartupError> {
        if let Ownership::Owned(engine) = &mut self.ownership {
            engine.child.cleanup()?;
        }
        self.closed = true;
        Ok(())
    }
}

#[derive(Serialize)]
pub(crate) struct StartupDescription {
    workspace: PathBuf,
    endpoint: Option<String>,
    local_models: Vec<ModelChoice>,
    context: u32,
    resource_policy: &'static str,
    ownership: &'static str,
    effects: &'static str,
    qualification: &'static str,
}

impl fmt::Display for StartupDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Workspace: {}", self.workspace.display())?;
        if let Some(endpoint) = &self.endpoint {
            writeln!(formatter, "Configured endpoint: {endpoint}")?;
        }
        writeln!(
            formatter,
            "Local GGUF choices: {}; context: {}",
            self.local_models.len(),
            self.context
        )?;
        writeln!(
            formatter,
            "Resources: {}\nOwnership: {}\nQualification: {}\nEffects: {}",
            self.resource_policy, self.ownership, self.qualification, self.effects
        )
    }
}

pub(crate) fn describe(
    workspace: &Path,
    config: &Config,
    explicit_model: Option<&Path>,
) -> Result<StartupDescription, StartupError> {
    config
        .validate()
        .map_err(|error| StartupError::actionable(error.to_string()))?;
    let endpoint = config
        .api_base
        .as_deref()
        .map(probe::endpoint)
        .transpose()?;
    let local_models = if endpoint.is_some() {
        Vec::new()
    } else {
        let configured = config.model.as_deref().map(Path::new).filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        });
        models::scan(workspace, explicit_model.or(configured))?
            .into_iter()
            .map(|model| model.choice)
            .collect()
    };
    Ok(StartupDescription {
        workspace: workspace.to_path_buf(),
        endpoint,
        local_models,
        context: config.ctx.unwrap_or(DEFAULT_CONTEXT),
        resource_policy: "local launch defaults: CPU, zero GPU layers, one slot; no memory-fit guarantee; borrowed-server resources are unverified",
        ownership: "explicit endpoint or a verified Ready managed server is borrowed; otherwise a foreground llama-server is owned until exit",
        effects: "explain performs no network, process launch, download, lock creation, or writes; actual setup takes a workspace lock, probes metadata, may start an engine, and atomically saves only the selected model choice",
        qualification: "unqualified: GGUF and server metadata do not establish hardware fit, context support, grammar support, or throughput",
    })
}

/// Cargo-source fixtures can substitute a bounded test engine command while
/// exercising the production lock, model checks, ownership, probes and cleanup.
/// This seam does not exist in a production binary.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn begin(
        workspace: &Path,
        config: &Config,
        explicit_model: Option<&Path>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Startup, StartupError> {
        Startup::begin_in(
            workspace,
            config,
            explicit_model,
            cancel,
            ManagedDiscoveryScope {
                workspace: workspace.to_path_buf(),
                global: None,
            },
        )
    }

    pub(crate) fn prepare(
        startup: Startup,
        index: usize,
        cancel: Arc<AtomicBool>,
        progress: &mut dyn FnMut(&str),
        command_for_port: impl FnOnce(u16) -> Command,
    ) -> Result<PreparedSession, StartupError> {
        startup.prepare_with(index, &cancel, progress, move |_, _, port, _, _| {
            Ok((command_for_port(port), "source-defined test engine".into()))
        })
    }
}
