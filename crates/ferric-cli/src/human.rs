//! Small human surface. Ask has no dispatcher; Work starts a fresh Evidence
//! run for each objective. Legacy chat commands are deliberately not parsed.

use std::path::PathBuf;
use std::process::ExitCode;

#[derive(clap::Args, Default)]
pub(crate) struct RunArgs {
    /// Ask a question, or describe a task with --allow-edits.
    pub prompt: Option<String>,
    /// Folder to work in. Defaults to the current folder.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    /// Permit controlled file changes in this folder, for this session only.
    #[arg(long)]
    pub allow_edits: bool,
    /// Select an existing GGUF file (advanced; no download).
    #[arg(long, hide = true)]
    pub model: Option<PathBuf>,
}

#[derive(clap::Args, Default)]
pub(crate) struct DescribeArgs {
    /// Folder to inspect. Defaults to the current folder.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    /// Print a machine-readable description.
    #[arg(long)]
    pub json: bool,
}

#[cfg(feature = "backend-openai")]
fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Shared by interactive setup and the read-only description path. Diagnostic
/// tails are structured, so neither their text nor an action heuristic is needed.
#[cfg(feature = "backend-openai")]
pub(crate) fn render_startup_error(error: &crate::startup::StartupError) -> String {
    tracing::debug!(error = %error, "human startup stopped");
    safe_text(&error.human_message())
}

pub(crate) fn welcome() {
    println!("Ferric — a local model, ready to help.");
    #[cfg(feature = "backend-openai")]
    println!("Run cargo r in a terminal to begin, or ferric run \"your question\".");
    #[cfg(not(feature = "backend-openai"))]
    println!("This build has no real backend. Run cargo r --features backend-openai to enable it.");
    println!("Ask mode cannot change files. Folder work needs your permission.");
    println!(
        "Use ferric explain for a read-only setup summary; ferric advanced for expert commands."
    );
}

pub(crate) fn run(args: RunArgs) -> ExitCode {
    use std::io::IsTerminal;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if args.prompt.is_none() && !interactive {
        welcome();
        return ExitCode::SUCCESS;
    }
    #[cfg(not(feature = "backend-openai"))]
    {
        welcome();
        ExitCode::FAILURE
    }
    #[cfg(feature = "backend-openai")]
    {
        enabled::run(args, interactive)
    }
}

pub(crate) fn describe(args: DescribeArgs) -> ExitCode {
    #[cfg(not(feature = "backend-openai"))]
    {
        let _ = args;
        welcome();
        ExitCode::SUCCESS
    }
    #[cfg(feature = "backend-openai")]
    {
        let result = (|| {
            let root = args
                .workspace
                .or_else(|| std::env::current_dir().ok())
                .ok_or("Cannot identify this folder. Open a folder and try again.".to_string())?;
            let cfg = crate::config::load_layered(&root)
                .map_err(|e| e.to_string())?
                .config;
            let summary = crate::startup::describe(&root, &cfg, None)
                .map_err(|error| render_startup_error(&error))?;
            // The description is read-only; status does not invent completed
            // workflow checkpoints or probe a potentially remote endpoint.
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary)
                        .map_err(|_| "Cannot format setup summary. Request the text summary without --json for the same selected folder.".to_string())?
                );
            } else {
                println!("{}", summary);
            }
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}", safe_text(&error));
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(feature = "backend-openai")]
mod enabled {
    use super::{RunArgs, render_startup_error, safe_text};
    use std::io::Write;
    use std::path::Path;
    use std::process::ExitCode;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    use crate::config::Config;
    use crate::startup::{PreparedSession, Startup};
    use ferric_core::{HarnessPolicy, Message, ModelProfile, Tier};
    use ferric_guard::Workspace;
    use ferric_provider::{Completion, CompletionRequest, Provider, SamplingParams, StreamDelta};
    use ferric_trace::{Event, JsonlSink};

    pub(super) trait HumanIo: Sync {
        fn say(&self, text: &str);
        fn delta(&self, text: &str);
        fn read(&self, prompt: &str) -> Result<Option<String>, String>;
    }

    struct Terminal;
    impl HumanIo for Terminal {
        fn say(&self, text: &str) {
            println!("{text}");
        }
        fn delta(&self, text: &str) {
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        fn read(&self, prompt: &str) -> Result<Option<String>, String> {
            let mut editor = rustyline::DefaultEditor::new().map_err(|_| {
                "Cannot open terminal input. Try ferric run \"your question\".".to_string()
            })?;
            match editor.readline(prompt) {
                Ok(line) => Ok(Some(line)),
                Err(
                    rustyline::error::ReadlineError::Eof
                    | rustyline::error::ReadlineError::Interrupted,
                ) => Ok(None),
                Err(_) => {
                    Err("Terminal input failed. Open a new terminal and try again.".to_string())
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Mode {
        Ask,
        Work,
    }

    // No persisted field can supply Work. Only this invocation's explicit flag
    // or the current terminal answer can grant folder-scoped authority.
    pub(super) fn choose_mode(
        args: &RunArgs,
        interactive: bool,
        io: &dyn HumanIo,
    ) -> Result<Option<Mode>, String> {
        if args.allow_edits {
            return Ok(Some(Mode::Work));
        }
        if !interactive {
            return Ok(Some(Mode::Ask));
        }
        match io.read("Ask only, or allow file work here? [Enter = ask / work / quit] ")? {
            None => Ok(None),
            Some(answer) => match answer.trim().to_ascii_lowercase().as_str() {
                "" | "ask" => Ok(Some(Mode::Ask)),
                "work" => Ok(Some(Mode::Work)),
                "quit" => Ok(None),
                _ => Err(
                    "No file permission granted. Start again and choose ask or work.".to_string(),
                ),
            },
        }
    }

    pub(super) fn choose_model(
        start: &Startup,
        interactive: bool,
        io: &dyn HumanIo,
    ) -> Result<Option<usize>, String> {
        if start.models.is_empty() {
            return Err("No local model was found. Put an existing GGUF in this folder's models directory, then start again.".to_string());
        }
        if let Some(index) = start.preferred_index {
            return Ok(Some(index));
        }
        if start.models.len() == 1 && !start.requires_model_choice {
            return Ok(Some(0));
        }
        if !interactive {
            return Err(
                "A model choice is needed. Run ferric in a terminal to choose one.".to_string(),
            );
        }
        if start.requires_model_choice {
            io.say("The saved model choice changed. Please choose again.");
        }
        for (index, model) in start.models.iter().enumerate() {
            let size = model
                .bytes
                .map(|bytes| format!(" ({:.1} GiB file)", bytes as f64 / 1_073_741_824.0))
                .unwrap_or_default();
            io.say(&format!(
                "  {}. {}{size}",
                index + 1,
                safe_text(&model.label)
            ));
        }
        match io.read("Which model? [number, or Enter to cancel] ")? {
            None => Ok(None),
            Some(answer) if answer.trim().is_empty() => Ok(None),
            Some(answer) => answer
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0 && *n <= start.models.len())
                .map(|n| Some(n - 1))
                .ok_or("No model selected. Start again and choose a listed number.".to_string()),
        }
    }

    pub(super) fn run(args: RunArgs, interactive: bool) -> ExitCode {
        let io = Terminal;
        let root = match args
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
        {
            Some(path) => path,
            None => {
                io.say("Cannot identify this folder. Open a folder and try again.");
                return ExitCode::FAILURE;
            }
        };
        let cfg = match crate::config::load_layered(&root) {
            Ok(loaded) => loaded.config,
            Err(error) => {
                io.say(&error.to_string());
                return ExitCode::FAILURE;
            }
        };
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(_) => {
                io.say("Cannot start the session. Close other apps and try again.");
                return ExitCode::FAILURE;
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = match cancellation_listener(&runtime, cancel.clone()) {
            Ok(signal) => signal,
            Err(_) => {
                io.say(
                    "Cannot install safe cancellation. No model was started; try another terminal.",
                );
                return ExitCode::FAILURE;
            }
        };
        let result = session(
            &args,
            &root,
            &cfg,
            interactive,
            &io,
            &runtime,
            cancel.clone(),
        );
        signal.abort();
        let _ = runtime.block_on(signal);
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                tracing::debug!(error = %error, "human session stopped");
                io.say(&concise_error(&error));
                ExitCode::FAILURE
            }
        }
    }

    fn cancellation_listener(
        runtime: &tokio::runtime::Runtime,
        cancel: Arc<AtomicBool>,
    ) -> std::io::Result<tokio::task::JoinHandle<()>> {
        let _entered = runtime.enter();
        // Registration is synchronous and fallible, BEFORE any process starts.
        // Spawning ctrl_c().await alone would defer registration until polling
        // and leave an early interrupt able to bypass owned process cleanup.
        #[cfg(windows)]
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        #[cfg(unix)]
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        // One owned listener, explicitly aborted and joined before runtime exit.
        Ok(runtime.spawn(async move {
            let _ = interrupt.recv().await;
            cancel.store(true, Ordering::Release);
        }))
    }

    fn concise_error(error: &str) -> String {
        // Full bounded engine output remains available via explicit verbosity,
        // never as a startup error avalanche in the ordinary human surface.
        safe_text(
            error
                .split("\nEngine diagnostics (bounded):")
                .next()
                .unwrap_or(error),
        )
    }

    pub(super) fn session(
        args: &RunArgs,
        root: &Path,
        cfg: &Config,
        interactive: bool,
        io: &dyn HumanIo,
        runtime: &tokio::runtime::Runtime,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        session_with(
            args,
            root,
            cfg,
            interactive,
            io,
            runtime,
            cancel,
            &SystemPreparation,
        )
    }

    trait Preparation {
        fn begin(
            &self,
            root: &Path,
            cfg: &Config,
            model: Option<&Path>,
            cancel: &Arc<AtomicBool>,
        ) -> Result<Startup, crate::startup::StartupError>;
        fn prepare(
            &self,
            start: Startup,
            index: usize,
            cancel: Arc<AtomicBool>,
            progress: &mut dyn FnMut(&str),
        ) -> Result<PreparedSession, crate::startup::StartupError>;
    }

    struct SystemPreparation;
    impl Preparation for SystemPreparation {
        fn begin(
            &self,
            root: &Path,
            cfg: &Config,
            model: Option<&Path>,
            cancel: &Arc<AtomicBool>,
        ) -> Result<Startup, crate::startup::StartupError> {
            Startup::begin(root, cfg, model, cancel)
        }
        fn prepare(
            &self,
            start: Startup,
            index: usize,
            cancel: Arc<AtomicBool>,
            progress: &mut dyn FnMut(&str),
        ) -> Result<PreparedSession, crate::startup::StartupError> {
            start.prepare(index, cancel, progress)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn session_with(
        args: &RunArgs,
        root: &Path,
        cfg: &Config,
        interactive: bool,
        io: &dyn HumanIo,
        runtime: &tokio::runtime::Runtime,
        cancel: Arc<AtomicBool>,
        preparation: &dyn Preparation,
    ) -> Result<(), String> {
        cfg.validate().map_err(|e| e.to_string())?;
        let workspace = Workspace::new(root).map_err(|_| {
            "Cannot open this folder. Choose an existing folder with --workspace.".to_string()
        })?;
        io.say(&format!(
            "Folder: {}",
            safe_text(&workspace.root().display().to_string())
        ));
        let start = match preparation.begin(workspace.root(), cfg, args.model.as_deref(), &cancel) {
            Ok(start) => start,
            Err(error) if error.is_cancelled() => {
                io.say("Cancelled. No session started.");
                return Ok(());
            }
            Err(error) => return Err(render_startup_error(&error)),
        };
        let Some(index) = choose_model(&start, interactive, io)? else {
            io.say("Cancelled. No session started.");
            return Ok(());
        };
        let Some(mode) = choose_mode(args, interactive, io)? else {
            io.say("Cancelled. No session started.");
            return Ok(());
        };
        if start.will_start_engine {
            io.say("This starts a local CPU model and may use substantial memory. Resource fit is not measured.");
            if interactive {
                let answer = io.read("Start the local model? [y/N] ")?;
                if !matches!(
                    answer.as_deref().map(str::trim),
                    Some("y" | "Y" | "yes" | "Yes")
                ) {
                    io.say("Cancelled. No engine started.");
                    return Ok(());
                }
            }
        }
        if cancel.load(Ordering::Acquire) {
            io.say("Cancelled. No engine started.");
            return Ok(());
        }
        let mut prepared =
            match preparation.prepare(start, index, cancel.clone(), &mut |state| io.say(state)) {
                Ok(prepared) => prepared,
                Err(error) if error.is_cancelled() => {
                    io.say("Cancelled. Owned startup resources were cleaned up.");
                    return Ok(());
                }
                Err(error) => return Err(render_startup_error(&error)),
            };
        let result = (|| {
            prepared
                .validate()
                .map_err(|error| render_startup_error(&error))?;
            let provider = runtime.block_on(async {
                ferric_provider::OpenAiProvider::for_prepared_endpoint(
                    ferric_provider::OpenAiConfig {
                        base_url: prepared
                            .backend_opts
                            .api_base
                            .clone()
                            .ok_or("Prepared endpoint is missing. Inspect the server configuration for the selected folder.".to_string())?,
                        api_key: prepared.backend_opts.api_key.clone().ok_or(
                            "Prepared endpoint credential binding is missing. Inspect the server configuration for the selected folder.".to_string(),
                        )?,
                        model: prepared.model.clone(),
                    },
                )
                .map_err(|_| {
                    "Cannot connect to the prepared model. Inspect the server configuration for the selected folder."
                        .to_string()
                })
            })?;
            io.say(&format!(
                "Ready: {} ({})",
                safe_text(&prepared.model),
                prepared.ownership_label()
            ));
            io.say(match mode {
                Mode::Ask => "Ask only — no file changes. Type a question; /quit ends the session.",
                Mode::Work => "Folder work enabled for this session. No shell, hooks or delegation. Type a task; /quit ends the session.",
            });
            if mode == Mode::Work {
                io.say("Conservative, unmeasured tool limits. Ctrl-C can still wait on an existing Git snapshot operation.");
            }
            dialogue(
                args, cfg, mode, &workspace, &prepared, &provider, runtime, io, &cancel,
            )
        })();
        io.say("Closing session…");
        // Cleanup errors take precedence: never report a successful command
        // when an owned child could not be proved reaped.
        prepared
            .cleanup()
            .map_err(|error| render_startup_error(&error))?;
        result
    }

    pub(super) fn ask_request(history: &[Message], sampling: SamplingParams) -> CompletionRequest {
        CompletionRequest {
            messages: history.to_vec(),
            sampling,
            tools: Vec::new(),
            constraint: None,
        }
    }

    async fn ask(
        provider: &dyn Provider,
        request: CompletionRequest,
        stream: bool,
        io: &dyn HumanIo,
        cancel: Arc<AtomicBool>,
    ) -> Result<Completion, String> {
        let output = |delta| {
            if let StreamDelta::Text(text) = delta {
                io.delta(&safe_text(&text));
            }
        };
        let attempt = async {
            if stream {
                provider
                    .complete_streaming(request, &output, Some(cancel.clone()))
                    .await
            } else {
                provider.complete(request, Some(cancel.clone())).await
            }
        };
        // The request future is dropped on timeout, not left running behind a
        // UI timer. The provider's whole-request cancellation covers Ctrl-C.
        match tokio::time::timeout(Duration::from_secs(120), attempt).await {
            Ok(Ok(completion))
                if completion
                    .message
                    .text
                    .as_deref()
                    .is_some_and(|text| !safe_text(text).trim().is_empty()) =>
            {
                Ok(completion)
            }
            Ok(Ok(_)) => {
                Err("The model returned no visible answer. Try a shorter question.".to_string())
            }
            Ok(Err(_)) if cancel.load(Ordering::Acquire) => {
                Err("Interrupted. The request was cancelled.".to_string())
            }
            Ok(Err(_)) => Err("The model could not answer. Try a shorter question.".to_string()),
            Err(_) => {
                cancel.store(true, Ordering::Release);
                Err(
                    "The model did not answer within two minutes. Try a shorter question."
                        .to_string(),
                )
            }
        }
    }

    fn work_config(cfg: &Config, context: u32, provider: &dyn Provider) -> crate::query::RunConfig {
        // Deliberately no ambient skill, prompt-library, hooks or benchmark
        // profile loading on the small front door. Expert surfaces retain it.
        let profile = ModelProfile {
            params_b: cfg.params_b.unwrap_or(1.2),
            quant: cfg.quant.clone().unwrap_or_else(|| "unknown".into()),
            ctx: context,
            family: cfg.family.clone().unwrap_or_else(|| "unknown".into()),
            measured_level: None,
        };
        let mut policy = ferric_core::policy_for_with_override(&profile, Some(Tier::Nano));
        policy.tier_source = ferric_core::TierSource::Conservative;
        policy.max_ring = Some(0);
        let caps = provider.capabilities();
        let protocol = ferric_loop::select_protocol(&policy, &caps, None);
        let mut registry = ferric_tools::Registry::new();
        ferric_tools::register_builtin_tools(&mut registry);
        let sampling = SamplingParams {
            temperature: cfg.temperature.unwrap_or(0.0),
            max_tokens: policy.max_output_tokens,
            ..SamplingParams::default()
        };
        crate::query::RunConfig {
            registry,
            caps,
            protocol,
            harness_policy: Some(HarnessPolicy::Evidence),
            policy,
            sampling,
            system_prompt: None,
            lineage: None,
            prompt_composition_error: None,
            hooks: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn dialogue(
        args: &RunArgs,
        cfg: &Config,
        mode: Mode,
        workspace: &Workspace,
        prepared: &PreparedSession,
        provider: &dyn Provider,
        runtime: &tokio::runtime::Runtime,
        io: &dyn HumanIo,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        let config = work_config(cfg, prepared.context, provider);
        let stream = crate::config::effective_stream(false, cfg.stream);
        let mut history = vec![Message::system(
            "You are Ferric, a helpful assistant. This is ask-only conversation: you cannot read or change files or execute commands.",
        )];
        let mut first = args.prompt.clone();
        loop {
            if cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let prompt = match first.take() {
                Some(prompt) => prompt,
                None if args.prompt.is_some() => return Ok(()),
                None => match io.read("You › ")? {
                    Some(line) => line,
                    None => return Ok(()),
                },
            };
            let prompt = prompt.trim();
            if prompt == "/quit" || prompt == "/exit" {
                return Ok(());
            }
            if prompt.is_empty() {
                continue;
            }
            if prompt.len() > 32 * 1024 {
                return Err("This input is too long. Try a smaller question or task.".to_string());
            }
            prepared
                .validate()
                .map_err(|error| render_startup_error(&error))?;
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)
                .map_err(|_| "Cannot allocate a session ID. Try again.".to_string())?;
            let session = format!("human-{}", hex::encode(random));
            let (trace_path, file) = prepared
                .create_trace_file(&format!("{session}.jsonl"))
                .map_err(|error| render_startup_error(&error))?;
            let mut trace = JsonlSink::from_file(file, &session)
                .map_err(|_| "Cannot initialize the session trace. Inspect trace permissions in the selected folder.".to_string())?;
            if mode == Mode::Ask {
                trace_event(
                    &mut trace,
                    Event::SessionStart {
                        workspace: workspace.root().display().to_string(),
                        resumed_from: None,
                    },
                )?;
                trace_event(
                    &mut trace,
                    Event::Note {
                        text: format!(
                            "ask-only; model={}; runtime={}; context={}; temperature={}; unqualified; user={prompt}",
                            prepared.model,
                            prepared.engine_identity,
                            prepared.context,
                            config.sampling.temperature
                        ),
                    },
                )?;
                history.push(Message::user(prompt));
                let result = runtime.block_on(ask(
                    provider,
                    ask_request(&history, config.sampling.clone()),
                    stream,
                    io,
                    cancel.clone(),
                ));
                match result {
                    Ok(completion) => {
                        let text = completion.message.text.unwrap_or_default();
                        if !stream {
                            io.say(&safe_text(&text));
                        } else {
                            io.say("");
                        }
                        // Discard every returned tool call even from a broken
                        // or hostile provider; only text enters future history.
                        history.push(Message::assistant(text.clone()));
                        trace_event(&mut trace, Event::Note { text })?;
                        trace_event(
                            &mut trace,
                            Event::SessionEnd {
                                reason: if completion.truncated {
                                    "truncated"
                                } else {
                                    "answered"
                                }
                                .into(),
                            },
                        )?;
                        if completion.truncated {
                            io.say(
                                "The answer reached its length limit; ask a narrower follow-up.",
                            );
                        }
                    }
                    Err(error) => {
                        trace_event(
                            &mut trace,
                            Event::SessionEnd {
                                reason: "request_failed".into(),
                            },
                        )?;
                        return Err(error);
                    }
                }
            } else {
                let setup = crate::query::LoopSetup {
                    registry: &config.registry,
                    workspace,
                    policy: &config.policy,
                    protocol: config.protocol,
                    harness_policy: Some(HarnessPolicy::Evidence),
                    sampling: config.sampling.clone(),
                    system_prompt: None,
                    lineage: None,
                    media: Vec::new(),
                    stream_sink: None,
                    resume: None,
                    answer: None,
                    provenance: ferric_guard::Provenance::Clean,
                    sink_policy: ferric_guard::SinkPolicy::deny(),
                    hooks: None,
                    edit_approver: None,
                };
                let recovery = format!(
                    "ferric advanced trace cat {}",
                    crate::query::documented_shell_quote(&trace_path.to_string_lossy())
                );
                let outcome = runtime
                    .block_on(crate::query::run_with_provider(
                        setup.into_run_args(provider, Some(cancel.clone())),
                        &mut trace,
                        Some(prompt),
                    ))
                    .map_err(|_| {
                        format!("The task stopped before completion. Inspect: {recovery}")
                    })?;
                if let Some(text) = outcome.final_text {
                    io.say(&safe_text(&text));
                }
                if let Some(input) = outcome.needs_input {
                    show_question(io, &input.request);
                    return Err(format!(
                        "Task paused for your answer. Start a new task including your answer. Retained trace: {}",
                        safe_text(&trace_path.display().to_string())
                    ));
                }
                if !outcome.stop.is_success() {
                    return Err(format!(
                        "Task incomplete ({}). Inspect: {recovery}",
                        outcome.stop.as_str()
                    ));
                }
            }
            // Bound ask history; never silently discard a previous exchange.
            if history
                .iter()
                .filter_map(|m| m.text.as_ref())
                .map(String::len)
                .sum::<usize>()
                > 32 * 1024
            {
                io.say("This conversation is full. Start a new session for another topic.");
                return Ok(());
            }
        }
    }

    fn trace_event(trace: &mut JsonlSink, event: Event) -> Result<(), String> {
        trace.write_event(event).map(|_| ()).map_err(|_| "Cannot write the session trace. Check free space and folder permissions before trying again.".to_string())
    }

    fn show_question(io: &dyn HumanIo, request: &ferric_core::UserInputRequest) {
        io.say(&safe_text(&request.question));
        io.say(&safe_text(&request.context));
        for option in &request.options {
            io.say(&format!("  - {}", safe_text(option)));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::VecDeque;
        use std::sync::Mutex;

        #[test]
        fn cancellation_is_registered_before_listener_can_be_polled() {
            // The current-thread runtime cannot poll the spawned task until
            // block_on. Registration has already succeeded when this returns.
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let listener = cancellation_listener(&runtime, cancel.clone()).unwrap();
            assert!(!listener.is_finished());
            assert!(!cancel.load(Ordering::Acquire));
            listener.abort();
            assert!(runtime.block_on(listener).unwrap_err().is_cancelled());
        }

        struct ScriptedIo {
            input: Mutex<VecDeque<Option<String>>>,
            output: Mutex<String>,
            prompts: Mutex<Vec<String>>,
            answers: Mutex<Vec<(String, Option<String>)>>,
            started: std::time::Instant,
            ready_after: Mutex<Option<Duration>>,
            response_after: Mutex<Option<Duration>>,
        }
        impl ScriptedIo {
            fn new(lines: &[Option<&str>]) -> Self {
                Self {
                    input: Mutex::new(lines.iter().map(|line| line.map(str::to_string)).collect()),
                    output: Mutex::new(String::new()),
                    prompts: Mutex::new(Vec::new()),
                    answers: Mutex::new(Vec::new()),
                    started: std::time::Instant::now(),
                    ready_after: Mutex::new(None),
                    response_after: Mutex::new(None),
                }
            }
        }
        impl HumanIo for ScriptedIo {
            fn say(&self, text: &str) {
                if text.starts_with("Ready:") {
                    *self.ready_after.lock().unwrap() = Some(self.started.elapsed());
                }
                self.output.lock().unwrap().push_str(&format!("{text}\n"));
            }
            fn delta(&self, text: &str) {
                if !text.trim().is_empty() {
                    self.response_after
                        .lock()
                        .unwrap()
                        .get_or_insert_with(|| self.started.elapsed());
                }
                self.output.lock().unwrap().push_str(text);
            }
            fn read(&self, prompt: &str) -> Result<Option<String>, String> {
                self.prompts.lock().unwrap().push(prompt.to_string());
                let answer = self
                    .input
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected extra question");
                self.answers
                    .lock()
                    .unwrap()
                    .push((prompt.to_string(), answer.clone()));
                Ok(answer)
            }
        }

        #[test]
        fn human_work_requires_scoped_consent() {
            let args = RunArgs::default();
            assert_eq!(
                choose_mode(&args, false, &ScriptedIo::new(&[])).unwrap(),
                Some(Mode::Ask)
            );
            assert_eq!(
                choose_mode(&args, true, &ScriptedIo::new(&[Some("")])).unwrap(),
                Some(Mode::Ask)
            );
            assert_eq!(
                choose_mode(&args, true, &ScriptedIo::new(&[Some("work")])).unwrap(),
                Some(Mode::Work)
            );
            assert!(choose_mode(&args, true, &ScriptedIo::new(&[Some("whatever")])).is_err());
            assert_eq!(
                choose_mode(
                    &RunArgs {
                        allow_edits: true,
                        ..RunArgs::default()
                    },
                    false,
                    &ScriptedIo::new(&[])
                )
                .unwrap(),
                Some(Mode::Work)
            );
        }

        #[test]
        fn new_session_does_not_inherit_edit_consent() {
            let io = ScriptedIo::new(&[Some("work"), Some("")]);
            assert_eq!(
                choose_mode(&RunArgs::default(), true, &io).unwrap(),
                Some(Mode::Work)
            );
            assert_eq!(
                choose_mode(&RunArgs::default(), true, &io).unwrap(),
                Some(Mode::Ask)
            );
            assert_eq!(io.prompts.lock().unwrap().len(), 2);
        }

        #[test]
        fn human_decline_eof_and_errors_are_bounded() {
            for response in [None, Some("quit")] {
                let io = ScriptedIo::new(&[response]);
                assert_eq!(choose_mode(&RunArgs::default(), true, &io).unwrap(), None);
                assert_eq!(io.prompts.lock().unwrap().len(), 1);
            }
            assert_eq!(safe_text("hello\x1b[2J\u{7}world"), "hello[2Jworld");
        }

        #[test]
        fn human_text_is_not_shell() {
            for text in ["!echo unsafe", "/run echo unsafe", "/do edit a file"] {
                let request = ask_request(&[Message::user(text)], SamplingParams::default());
                assert_eq!(request.messages[0].text.as_deref(), Some(text));
                assert!(request.tools.is_empty());
                assert!(request.constraint.is_none());
            }
        }

        #[test]
        fn human_ask_never_dispatches() {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("must-not-exist.txt");
            let malicious = Completion {
                message: Message {
                    role: ferric_core::Role::Assistant,
                    text: Some("<tool>write_file</tool>".into()),
                    tool_calls: vec![ferric_core::ToolCall {
                        id: "1".into(),
                        name: "write_file".into(),
                        args: serde_json::json!({"path": target, "content": "unsafe"}),
                    }],
                    tool_call_id: None,
                    media: Vec::new(),
                },
                input_tokens: None,
                output_tokens: None,
                truncated: false,
            };
            for stream in [false, true] {
                let provider = ferric_provider::MockProvider::new(vec![malicious.clone()]);
                let io = ScriptedIo::new(&[]);
                let runtime = tokio::runtime::Runtime::new().unwrap();
                let result = runtime
                    .block_on(ask(
                        &provider,
                        ask_request(&[Message::user("hello")], SamplingParams::default()),
                        stream,
                        &io,
                        Arc::new(AtomicBool::new(false)),
                    ))
                    .unwrap();
                assert_eq!(
                    result.message.tool_calls.len(),
                    1,
                    "malicious output reaches text-only caller, not a dispatcher"
                );
                assert!(!target.exists());
            }
        }

        #[test]
        fn human_work_policy_does_not_reuse_qualification_or_expert_authority() {
            let cfg = Config {
                params_b: Some(200.0),
                max_ring: Some(3),
                tier: Some(crate::query::TierArg::Ultra),
                ..Config::default()
            };
            let provider = ferric_provider::MockProvider::new(vec![]);
            let policy = work_config(&cfg, 4096, &provider);
            assert_eq!(policy.policy.tier, Tier::Nano);
            assert_eq!(
                policy.policy.tier_source,
                ferric_core::TierSource::Conservative
            );
            assert_eq!(policy.policy.max_ring, Some(0));
            assert_eq!(policy.harness_policy, Some(HarnessPolicy::Evidence));
            assert!(policy.hooks.is_none());
            assert!(policy.lineage.is_none());
            assert!(policy.system_prompt.is_none());
        }

        #[test]
        fn human_empty_answer_is_not_success() {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            for text in [None, Some(""), Some(" \n\t"), Some("\x1b\x07")] {
                for stream in [true, false] {
                    let mut message = Message::assistant("");
                    message.text = text.map(str::to_string);
                    message.tool_calls = vec![ferric_core::ToolCall {
                        id: "empty".into(),
                        name: "write_file".into(),
                        args: serde_json::json!({}),
                    }];
                    let provider = ferric_provider::MockProvider::new(vec![Completion {
                        message,
                        input_tokens: None,
                        output_tokens: None,
                        truncated: true,
                    }]);
                    let io = ScriptedIo::new(&[]);
                    let error = runtime
                        .block_on(ask(
                            &provider,
                            ask_request(&[Message::user("hello")], SamplingParams::default()),
                            stream,
                            &io,
                            Arc::new(AtomicBool::new(false)),
                        ))
                        .unwrap_err();
                    assert!(error.contains("no visible answer"));
                }
            }
        }

        #[test]
        fn human_work_surfaces_required_decisions() {
            let io = ScriptedIo::new(&[]);
            show_question(
                &io,
                &ferric_core::UserInputRequest {
                    question: "Which format?".into(),
                    context: "The output format changes the implementation.".into(),
                    options: vec!["JSON".into(), "CSV".into()],
                },
            );
            let output = io.output.lock().unwrap();
            for text in ["Which format?", "changes the implementation", "JSON", "CSV"] {
                assert!(output.contains(text));
            }
        }

        #[test]
        fn human_failure_is_concise() {
            let detail = format!(
                "The engine exited. Choose a compatible model.\nEngine diagnostics (bounded):\n{}",
                "technical output\n".repeat(100)
            );
            assert_eq!(
                concise_error(&detail),
                "The engine exited. Choose a compatible model."
            );
        }

        include!("human_journey_tests.rs");
    }
}
