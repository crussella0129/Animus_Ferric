//! `ferric chat` — the ADR-011-revision hybrid chat REPL (ADR-052).
//!
//! Two turn kinds share one conversation:
//! - **Talk (default):** a single `provider.complete()` with **no tools and no
//!   constraint** — the harness's FIRST unconstrained-completion path. Its
//!   output is text ONLY: printed and appended to history, never parsed for a
//!   tool call, never dispatched. The safety is STRUCTURAL — the talk path
//!   simply never calls dispatch (it does not go through `ferric_loop::run()`,
//!   which stays always-constrained).
//! - **Escalate (`/do <request>`):** promotes the turn into the EXISTING
//!   constrained agentic loop (`run_with_provider` → `ferric_loop::run()` +
//!   `ferric-guard` + guards + trace), with the conversation history as a
//!   sprint-39 `ReplayedState` resume seed. USER-initiated only — the model can
//!   never self-escalate into acting (ADR-005).
//!
//! Launch-time-fixed containment (ADR-046): workspace/backend/model/protocol are
//! fixed CLI flags held for the session. Trace shape: talk turns → a chat-session
//! log (`Note` per turn); each `/do` → its OWN fresh agentic trace file
//! (mirroring `ferric mcp`, since `run()` emits a whole `SessionStart..SessionEnd`
//! envelope per call and cannot be nested).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use ferric_core::Message;
use ferric_guard::Workspace;
use ferric_loop::ReplayedState;
use ferric_provider::{Completion, CompletionRequest, MockProvider, Provider, SamplingParams};
use ferric_trace::{Event, JsonlSink};

use crate::backend::BackendOpts;
use crate::query::{
    ProtocolArg, RunConfig, RunConfigArgs, build_run_config, mock_provider, now_ms,
    run_with_provider,
};

/// How chat drives the provider, shared with `ferric icm` since sprint 103
/// (ADR-094) — both had declared this enum and its constructor identically.
/// `Mock` builds a FRESH `MockProvider` per turn (talk-shaped vs
/// agentic-shaped) — no ambient runtime, no script exhaustion. `Real` holds the
/// ONE provider + `tokio::runtime::Runtime` for the session.
use crate::backend::ResolvedBackend as ChatBackend;

/// `ferric chat`'s CLI surface: `QueryArgs` minus `prompt`/`resume`/`files` —
/// the conversation is read from stdin instead. Everything here is launch-time-
/// fixed for the session (ADR-046/ADR-052).
#[derive(Args)]
pub struct ChatArgs {
    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Disable token-streaming to stdout. (By default, streaming is ON in chat mode).
    #[arg(long)]
    pub no_stream: bool,

    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Parameter count in billions. Default 1.2 when unset (config/flag).
    #[arg(long)]
    pub params_b: Option<f32>,

    /// Quantization label. Default "Q4_K_M" when unset.
    #[arg(long)]
    pub quant: Option<String>,

    /// Model family label. Default "unknown" when unset.
    #[arg(long)]
    pub family: Option<String>,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006).
    #[arg(long)]
    pub ctx: Option<u32>,

    /// Sampling temperature (0.0 = the deterministic sampler). Default 0.0.
    #[arg(long)]
    pub temperature: Option<f32>,

    /// Action protocol override for escalated (`/do`) turns.
    #[arg(long, value_enum)]
    pub protocol: Option<ProtocolArg>,

    /// Directory of prompt elements to compose the system prompt from.
    #[arg(long)]
    pub prompts_dir: Option<PathBuf>,

    /// Cap the active tool ring (ADR-028) for escalated turns.
    #[arg(long)]
    pub max_ring: Option<u8>,

    /// Directory holding `model_profiles.json` (ADR-029). Default "benchmarks".
    #[arg(long)]
    pub profile_dir: Option<PathBuf>,

    /// Run against a built-in scripted mock instead of a real model.
    #[arg(long)]
    pub mock: bool,
}

/// One parsed REPL line. `parse_chat_input` maps a raw line to this; the REPL
/// then routes on it. Pure and unit-tested — no I/O.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChatInput {
    /// A default (no-command) line → an unconstrained talk turn.
    Talk(String),
    /// `/do <request>` → escalate into the constrained agentic loop.
    Escalate(String),
    /// `!<cmd>` or `/run <cmd>` → run a shell command directly, no LLM (ADR-069).
    Run(String),
    /// `/help` → print the command list.
    Help,
    /// `/exit`, `/quit`, or EOF → end the session.
    Exit,
    /// A blank/whitespace-only line → reprompt, do nothing.
    Empty,
}

/// Map a raw REPL line to a `ChatInput`. An UNKNOWN `/command` is treated as
/// `Talk` (talked, not silently executed) — only the exact known commands act.
pub(crate) fn parse_chat_input(line: &str) -> ChatInput {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ChatInput::Empty;
    }
    if let Some(rest) = trimmed.strip_prefix("/do") {
        // `/do` alone (no request) is talked; `/do <req>` escalates.
        let req = rest.trim();
        if req.is_empty() {
            return ChatInput::Talk(trimmed.to_string());
        }
        return ChatInput::Escalate(req.to_string());
    }
    // Direct terminal passthrough (ADR-069): `!<cmd>` or `/run <cmd>` runs a
    // shell command through the guarded `shell_exec` path with NO LLM. A bare
    // `!` or `/run` (no command) is talked, not run.
    if let Some(rest) = trimmed.strip_prefix('!') {
        let cmd = rest.trim();
        return if cmd.is_empty() {
            ChatInput::Talk(trimmed.to_string())
        } else {
            ChatInput::Run(cmd.to_string())
        };
    }
    if trimmed == "/run" {
        return ChatInput::Talk(trimmed.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("/run ") {
        let cmd = rest.trim();
        return if cmd.is_empty() {
            ChatInput::Talk(trimmed.to_string())
        } else {
            ChatInput::Run(cmd.to_string())
        };
    }
    match trimmed {
        "/help" => ChatInput::Help,
        "/exit" | "/quit" => ChatInput::Exit,
        _ => ChatInput::Talk(trimmed.to_string()),
    }
}

/// Build the talk-turn completion request. **Structurally has no action
/// channel:** `tools` is empty AND `constraint` is `None` (the lawful ADR-010
/// "neither" case), so the completion can only ever be text — it can never carry
/// a tool call, and the talk path never dispatches. This is the load-bearing
/// safety invariant of talk mode (ADR-052); the unit test asserts it directly.
pub(crate) fn talk_request(history: &[Message], sampling: &SamplingParams) -> CompletionRequest {
    CompletionRequest {
        messages: history.to_vec(),
        sampling: sampling.clone(),
        tools: Vec::new(),
        constraint: None,
    }
}

/// A `ReplayedState` seed for a `/do` escalation: the conversation so far
/// becomes the resumed history; `turns: 0` gives the escalated run its own fresh
/// turn budget; `protocol` matches the session config; `source_session` links
/// the escalation trace back to the chat session (ADR-052 / plan-critic C-007).
pub(crate) fn escalation_seed(
    history: &[Message],
    config: &RunConfig,
    chat_session: &str,
) -> ReplayedState {
    ReplayedState {
        messages: history.to_vec(),
        turns: 0,
        last_text: None,
        protocol: config.protocol,
        source_session: chat_session.to_string(),
    }
}

/// The mock talk completion (`--mock`): a fresh one per talk turn so a REPL's
/// stdin-driven turn count can never exhaust a fixed script (plan-critic C-001).
fn mock_talk_completion() -> Completion {
    Completion {
        message: Message::assistant("[mock chat] I hear you (talk mode — no action taken)."),
        input_tokens: Some(12),
        output_tokens: Some(10),
        truncated: false,
    }
}

impl ChatBackend {
    /// Run one talk completion (unconstrained, no tools).
    fn talk(&self, request: CompletionRequest, stream: bool) -> Result<Completion, String> {
        let sink_fn = |d: ferric_provider::StreamDelta| match d {
            ferric_provider::StreamDelta::Text(t) | ferric_provider::StreamDelta::Thought(t) => {
                print!("{t}");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            _ => {}
        };
        let stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)> =
            if stream { Some(&sink_fn) } else { None };

        match self {
            ChatBackend::Mock => {
                let provider = MockProvider::new(vec![mock_talk_completion()]);
                if let Some(sink) = stream_sink {
                    futures_executor::block_on(provider.complete_streaming(request, sink, None))
                        .map_err(|e| format!("talk completion failed: {e}"))
                } else {
                    futures_executor::block_on(provider.complete(request, None))
                        .map_err(|e| format!("talk completion failed: {e}"))
                }
            }
            #[cfg(feature = "backend-openai")]
            ChatBackend::Real { provider, runtime } => {
                if let Some(sink) = stream_sink {
                    runtime
                        .block_on(provider.complete_streaming(request, sink, None))
                        .map_err(|e| format!("talk completion failed: {e}"))
                } else {
                    runtime
                        .block_on(provider.complete(request, None))
                        .map_err(|e| format!("talk completion failed: {e}"))
                }
            }
        }
    }

    /// Escalate one turn into the constrained agentic loop.
    #[allow(clippy::too_many_arguments)]
    fn escalate(
        &self,
        config: &RunConfig,
        workspace: &Workspace,
        sink: &mut JsonlSink,
        prompt: &str,
        seed: ReplayedState,
        stream: bool,
    ) -> Result<ferric_loop::LoopOutcome, String> {
        let sink_fn = |d: ferric_provider::StreamDelta| match d {
            ferric_provider::StreamDelta::Text(t) => {
                print!("{t}");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            ferric_provider::StreamDelta::Thought(t) => {
                print!("\x1b[90m{t}\x1b[0m"); // Dim ANSI color
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            ferric_provider::StreamDelta::ToolNamed(name) => {
                eprintln!("\u{25b8} calling {name}...");
            }
            ferric_provider::StreamDelta::ToolCompleted { name, summary } => {
                eprintln!("\u{2713} {name}: {summary}");
            }
        };
        let stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)> =
            if stream { Some(&sink_fn) } else { None };

        // One setup for both backends; only the provider and the executor differ.
        let setup = || crate::query::LoopSetup {
            registry: &config.registry,
            workspace,
            policy: &config.policy,
            protocol: config.protocol,
            sampling: config.sampling.clone(),
            system_prompt: config.system_prompt.as_deref(),
            lineage: config.lineage.clone(),
            media: Vec::new(),
            stream_sink,
            resume: Some(seed.clone()),
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        };

        match self {
            ChatBackend::Mock => {
                let provider = mock_provider(config.protocol);
                futures_executor::block_on(run_with_provider(
                    setup().into_run_args(&provider, None),
                    sink,
                    Some(prompt),
                ))
            }
            #[cfg(feature = "backend-openai")]
            ChatBackend::Real { provider, runtime } => runtime.block_on(run_with_provider(
                setup().into_run_args(provider.as_ref(), None),
                sink,
                Some(prompt),
            )),
        }
    }
}

const HELP: &str = "\
Commands:
  /do <request>   escalate this turn into the constrained agentic loop (it can
                  act on the workspace, through the same guarded/traced path as
                  `ferric query`)
  !<cmd>          run a shell command directly (no LLM), through the guarded
  /run <cmd>      `shell_exec` path — the same command denylist applies
  /help           show this help
  /exit, /quit    end the session
  <anything else> talk mode — the model responds as text only (no tools, no
                  workspace changes)";

/// Write a chat-session trace event, reporting a failure instead of discarding it.
///
/// `run_chat` returns `ExitCode`, so `?` is not available here — but silently
/// dropping the result is the wrong answer (ADR-079). `JsonlSink::write_event`
/// can genuinely fail on I/O, `run.rs` propagates it at 21 sites, and this file
/// was dropping it at all 6 — so a full disk or a locked file left a chat trace
/// silently incomplete, in a harness whose premise is "if it isn't in the trace,
/// it didn't happen".
///
/// Latched to one warning per session: the failure is visible without a
/// per-event flood when the underlying cause is persistent.
fn log_event(log: &mut JsonlSink, warned: &mut bool, event: Event) {
    if let Err(e) = log.write_event(event)
        && !*warned
    {
        eprintln!("warning: chat trace write failed ({e}); this session's trace is incomplete");
        *warned = true;
    }
}

pub fn run_chat(args: ChatArgs) -> ExitCode {
    let workspace_root = match &args.workspace {
        Some(path) => path.clone(),
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("cannot determine current directory: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let workspace = match Workspace::new(&workspace_root) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("workspace: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Layered config (CLI > project > user > default), same as `ferric query`.
    let loaded_config = crate::config::load_layered(&workspace_root);
    let cfg = loaded_config.config;
    let backend_opts = crate::config::merge_backend_opts(args.backend_opts.clone(), &cfg);
    let config = build_run_config(&RunConfigArgs {
        // Chat is the workspace owner at their own terminal, so their standing
        // allowlist applies. There is no per-message `--skill` surface yet; a
        // `/skill` REPL verb is the natural home for that and is deferred.
        workspace_root: workspace_root.clone(),
        requested_skills: Vec::new(),
        allowed_skills: cfg.allowed_skills.clone().unwrap_or_default(),
        mock: args.mock,
        backend: backend_opts
            .backend
            .unwrap_or(crate::backend::BackendArg::Openai),
        params_b: args.params_b.or(cfg.params_b).unwrap_or(1.2),
        quant: args
            .quant
            .clone()
            .or(cfg.quant)
            .unwrap_or_else(|| "Q4_K_M".to_string()),
        family: args
            .family
            .clone()
            .or(cfg.family)
            .unwrap_or_else(|| "unknown".to_string()),
        ctx: args.ctx.or(cfg.ctx).unwrap_or(4096),
        temperature: args.temperature.or(cfg.temperature).unwrap_or(0.0),
        protocol_override: args.protocol,
        prompts_dir: args.prompts_dir.clone(),
        max_ring: args.max_ring.or(cfg.max_ring),
        profile_dir: args
            .profile_dir
            .clone()
            .or(cfg.profile_dir)
            .unwrap_or_else(|| PathBuf::from("benchmarks")),
        model_key: backend_opts.model.clone(),
        hooks: None,
    });
    for diag in &loaded_config.diagnostics {
        eprintln!("{diag}");
    }
    if let Some(err) = &config.prompt_composition_error {
        eprintln!("{err}");
    }

    let backend = match if args.mock {
        Ok(ChatBackend::Mock)
    } else {
        ChatBackend::real(&backend_opts)
    } {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let trace_dir = ferric_trace::trace_dir(&workspace_root);
    if let Err(e) = std::fs::create_dir_all(&trace_dir) {
        eprintln!("cannot create trace dir {}: {e}", trace_dir.display());
        return ExitCode::FAILURE;
    }
    // The chat-session log: one file, a chat-level SessionStart..SessionEnd
    // envelope holding a Note per talk turn + a Note per escalation (which
    // itself writes a separate agentic trace file). No run() envelope is ever
    // nested here (ADR-052).
    let chat_session = format!("chat-{}", now_ms());
    let log_path = trace_dir.join(format!("{chat_session}.jsonl"));
    let mut log = match JsonlSink::open(&log_path, &chat_session) {
        Ok(sink) => sink,
        Err(e) => {
            eprintln!("cannot open chat trace: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut trace_warned = false;
    log_event(
        &mut log,
        &mut trace_warned,
        Event::SessionStart {
            workspace: workspace_root.display().to_string(),
            resumed_from: None,
        },
    );

    // History seed: the system prompt (empty if none composed).
    let mut history: Vec<Message> = Vec::new();
    if let Some(sys) = &config.system_prompt {
        history.push(Message::system(sys.clone()));
    }

    println!("Ferric chat — /help for commands, /exit to quit.");
    let mut editor = match rustyline::DefaultEditor::new() {
        Ok(ed) => ed,
        Err(e) => {
            eprintln!("failed to initialize rustyline: {e}");
            return ExitCode::FAILURE;
        }
    };
    let history_path = workspace_root.join(".ferric").join("chat_history.txt");
    let _ = editor.load_history(&history_path);

    // Per-session monotonic counter for escalation trace filenames — `now_ms()`
    // alone can collide when two `/do` turns land in the same millisecond
    // (implausible for human typing, but real for scripted/piped chat;
    // test-critic C-001).
    let mut esc_count: u32 = 0;
    // `shell_exec` runs on tokio (sprint 67); the sync REPL has no ambient
    // runtime, so `!cmd` passthrough gets its own, created on first use.
    let mut shell_rt: Option<tokio::runtime::Runtime> = None;
    loop {
        let line = match editor.readline("you> ") {
            Ok(line) => line,
            Err(rustyline::error::ReadlineError::Interrupted)
            | Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("input error: {e}");
                break;
            }
        };
        let _ = editor.add_history_entry(line.as_str());
        match parse_chat_input(&line) {
            ChatInput::Exit => break,
            ChatInput::Empty => {}
            ChatInput::Help => println!("{HELP}"),
            ChatInput::Talk(text) => {
                history.push(Message::user(text));
                let request = talk_request(&history, &config.sampling);
                match backend.talk(request, !args.no_stream) {
                    Ok(completion) => {
                        let resp = completion.message.text.unwrap_or_default();
                        if args.no_stream {
                            println!("{resp}");
                        } else {
                            println!(); // ensure newline after stream
                        }
                        log_event(
                            &mut log,
                            &mut trace_warned,
                            Event::Note {
                                text: format!("chat talk turn ({} response chars)", resp.len()),
                            },
                        );
                        history.push(Message::assistant(resp));
                    }
                    Err(e) => eprintln!("{e}"),
                }
            }
            ChatInput::Escalate(text) => {
                let seed = escalation_seed(&history, &config, &chat_session);
                esc_count += 1;
                let esc_session = format!("chat-esc-{}-{esc_count}", now_ms());
                let esc_path = trace_dir.join(format!("{esc_session}.jsonl"));
                let mut esc_sink = match JsonlSink::open(&esc_path, &esc_session) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("cannot open escalation trace: {e}");
                        continue;
                    }
                };
                match backend.escalate(
                    &config,
                    &workspace,
                    &mut esc_sink,
                    &text,
                    seed,
                    !args.no_stream,
                ) {
                    Ok(outcome) => {
                        let resp = outcome.final_text.unwrap_or_default();
                        println!("{resp}");
                        log_event(
                            &mut log,
                            &mut trace_warned,
                            Event::Note {
                                text: format!("chat escalation → {esc_session}"),
                            },
                        );
                        history.push(Message::user(text));
                        history.push(Message::assistant(resp));
                    }
                    Err(e) => eprintln!("escalation error: {e}"),
                }
            }
            ChatInput::Run(cmd) => {
                if shell_rt.is_none() {
                    shell_rt = tokio::runtime::Runtime::new().ok();
                }
                let Some(rt) = &shell_rt else {
                    eprintln!("cannot start shell runtime for passthrough");
                    continue;
                };
                // Human-initiated, no LLM: run through the SAME guarded
                // `shell_exec` chokepoint the agent uses, so the command
                // denylist (ADR-005) still applies. Not folded into the talk
                // history — it is a terminal side-channel, not conversation.
                let outcome = rt.block_on(async {
                    config.registry.execute(
                        &workspace,
                        "shell_exec",
                        &serde_json::json!({ "command": cmd }),
                        ferric_guard::Provenance::Clean,
                        &ferric_guard::SinkPolicy::deny(),
                        None,
                    )
                });
                match outcome {
                    ferric_tools::ExecuteOutcome::Completed { output, .. } => {
                        print!("{}", output.full);
                        if !output.full.ends_with('\n') {
                            println!();
                        }
                        log_event(
                            &mut log,
                            &mut trace_warned,
                            Event::Note {
                                text: format!("chat !run ({} output chars)", output.full.len()),
                            },
                        );
                    }
                    ferric_tools::ExecuteOutcome::Denied { reason, .. } => {
                        eprintln!("blocked: {reason}");
                        log_event(
                            &mut log,
                            &mut trace_warned,
                            Event::Note {
                                text: format!("chat !run blocked: {reason}"),
                            },
                        );
                    }
                    ferric_tools::ExecuteOutcome::UnknownTool { .. } => {
                        eprintln!("shell_exec is unavailable in this build");
                    }
                }
            }
        }
    }

    let _ = editor.save_history(&history_path);

    log_event(
        &mut log,
        &mut trace_warned,
        Event::SessionEnd {
            reason: "chat session ended".to_string(),
        },
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::ActionProtocol;
    use ferric_provider::Constraint;

    #[test]
    fn parse_chat_input_do_is_escalate() {
        assert_eq!(
            parse_chat_input("/do fix the bug"),
            ChatInput::Escalate("fix the bug".to_string())
        );
        // Trailing/leading whitespace on the request is trimmed.
        assert_eq!(
            parse_chat_input("  /do   write a.txt  "),
            ChatInput::Escalate("write a.txt".to_string())
        );
    }

    #[test]
    fn parse_chat_input_help_exit_quit() {
        assert_eq!(parse_chat_input("/help"), ChatInput::Help);
        assert_eq!(parse_chat_input("/exit"), ChatInput::Exit);
        assert_eq!(parse_chat_input("/quit"), ChatInput::Exit);
    }

    #[test]
    fn parse_chat_input_empty_is_empty() {
        assert_eq!(parse_chat_input(""), ChatInput::Empty);
        assert_eq!(parse_chat_input("   "), ChatInput::Empty);
        assert_eq!(parse_chat_input("\n"), ChatInput::Empty);
    }

    #[test]
    fn parse_chat_input_default_is_talk() {
        assert_eq!(
            parse_chat_input("explain this function"),
            ChatInput::Talk("explain this function".to_string())
        );
        // An UNKNOWN slash-command is talked, never silently executed.
        assert_eq!(
            parse_chat_input("/unknown"),
            ChatInput::Talk("/unknown".to_string())
        );
        // `/do` with no request is talked, not a no-op escalation.
        assert_eq!(parse_chat_input("/do"), ChatInput::Talk("/do".to_string()));
    }

    #[test]
    fn parse_chat_input_bang_and_run_are_passthrough() {
        assert_eq!(
            parse_chat_input("!echo hi"),
            ChatInput::Run("echo hi".to_string())
        );
        assert_eq!(
            parse_chat_input("  !ls -la  "),
            ChatInput::Run("ls -la".to_string())
        );
        assert_eq!(
            parse_chat_input("/run pwd"),
            ChatInput::Run("pwd".to_string())
        );
        // A bare `!` or `/run` (no command) is talked, not run.
        assert_eq!(parse_chat_input("!"), ChatInput::Talk("!".to_string()));
        assert_eq!(
            parse_chat_input("/run"),
            ChatInput::Talk("/run".to_string())
        );
        // `/running` is NOT a passthrough (needs the `/run ` boundary) — talked.
        assert_eq!(
            parse_chat_input("/running late"),
            ChatInput::Talk("/running late".to_string())
        );
    }

    /// The load-bearing structural-safety proof (ADR-052): the talk request has
    /// NO action channel — empty tools AND no constraint — so a talk completion
    /// can never carry a tool call.
    #[test]
    fn talk_request_has_no_action_channel() {
        let history = vec![Message::system("sys"), Message::user("hi")];
        let req = talk_request(&history, &SamplingParams::default());
        assert!(req.tools.is_empty(), "talk mode must offer no tools");
        assert!(
            req.constraint.is_none(),
            "talk mode must apply no constraint"
        );
        // And it is a lawful request under ADR-010 (the "neither" case).
        assert!(req.validate().is_ok());
        // Sanity: not accidentally a constrained request.
        assert!(!matches!(req.constraint, Some(Constraint::JsonSchema(_))));
    }

    #[test]
    fn escalation_seed_carries_history_and_protocol() {
        let history = vec![
            Message::system("sys"),
            Message::user("earlier question"),
            Message::assistant("earlier answer"),
        ];
        let config = build_run_config(&RunConfigArgs {
            workspace_root: std::path::PathBuf::from("."),
            requested_skills: Vec::new(),
            allowed_skills: Vec::new(),
            mock: true,
            backend: crate::backend::BackendArg::Openai,
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            family: "test".to_string(),
            ctx: 4096,
            temperature: 0.0,
            protocol_override: None,
            prompts_dir: None,
            max_ring: None,
            profile_dir: PathBuf::from("benchmarks"),
            model_key: None,
            hooks: None,
        });
        let seed = escalation_seed(&history, &config, "chat-123");
        assert_eq!(
            seed.messages, history,
            "the full conversation seeds the run"
        );
        assert_eq!(seed.protocol, config.protocol);
        assert_eq!(seed.source_session, "chat-123");
        assert_eq!(seed.turns, 0, "a fresh turn budget for the escalated run");
        // The mock's caps → NativeTools/TextXml; confirm it's a real protocol.
        assert!(matches!(
            seed.protocol,
            ActionProtocol::NativeTools | ActionProtocol::ConstrainedJson | ActionProtocol::TextXml
        ));
    }
}
