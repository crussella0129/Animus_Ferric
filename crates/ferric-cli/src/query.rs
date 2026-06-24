//! `ferric query` — the one-shot, workspace-scoped, policy-scaled, fully
//! traced surface (ADR-011: no chat catch-all).
//!
//! Executor boundary (plan C-009): `--mock` drives the loop on
//! `futures_executor::block_on` (no tokio in the default build); the real
//! backend constructs a tokio multi-thread runtime (mistral.rs client
//! futures need ambient tokio).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};

use ferric_core::{ActionProtocol, Message, ModelProfile, RunPolicy, policy_for};
use ferric_guard::Workspace;
use ferric_loop::{
    LoopOutcome, PromptLineage, RunArgs, StopReason, ThreadSleeper, run, select_protocol,
};
use ferric_provider::{Capabilities, Completion, MockProvider, Provider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink};

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
use crate::backend::create_provider;
use crate::backend::{BackendArg, BackendOpts};

/// CLI spelling of `ActionProtocol`. `grammar` is the server-enforced
/// constrained-JSON path (the thesis); `xml` is the unconstrained
/// regex-scraped fallback.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ProtocolArg {
    Native,
    Grammar,
    Xml,
}

impl From<ProtocolArg> for ActionProtocol {
    fn from(p: ProtocolArg) -> Self {
        match p {
            ProtocolArg::Native => ActionProtocol::NativeTools,
            ProtocolArg::Grammar => ActionProtocol::ConstrainedJson,
            ProtocolArg::Xml => ActionProtocol::TextXml,
        }
    }
}

#[derive(Args)]
pub struct QueryArgs {
    /// The task prompt
    pub prompt: String,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Parameter count in billions
    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    /// Quantization label
    #[arg(long, default_value = "Q4_K_M")]
    pub quant: String,

    /// Model family label
    #[arg(long, default_value = "unknown")]
    pub family: String,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006)
    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Sampling temperature (0.0 selects the deterministic sampler)
    #[arg(long, default_value_t = 0.0)]
    pub temperature: f32,

    /// Action protocol override (default: chosen from policy + backend caps)
    #[arg(long, value_enum)]
    pub protocol: Option<ProtocolArg>,

    /// Directory of prompt elements to compose the system prompt from.
    /// Falls back to the built-in default prompt when absent or unloadable.
    /// Also read from FERRIC_PROMPTS_DIR.
    #[arg(long)]
    pub prompts_dir: Option<PathBuf>,

    /// Run against a built-in scripted mock instead of a real model
    #[arg(long)]
    pub mock: bool,
}

pub fn run_query(args: QueryArgs) -> ExitCode {
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

    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);

    let profile = ModelProfile {
        params_b: args.params_b,
        quant: args.quant.clone(),
        ctx: args.ctx,
        family: args.family.clone(),
        measured_level: None,
    };
    let policy = policy_for(&profile);
    // Capability seed for auto protocol selection (an explicit `--protocol`
    // always overrides it). The backend's own `capabilities()` is the source of
    // truth, but the provider is constructed later (in drive_real), so we mirror
    // it from the chosen backend: the HTTP valve enforces a JSON-Schema
    // constraint (→ ConstrainedJson); mistral.rs does neither — its constrained
    // path hangs upstream (ADR-020) and it strips tools — so it lands on the
    // honest TextXml fallback.
    let caps = if args.mock {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
        }
    } else {
        match args.backend_opts.backend {
            BackendArg::Openai => Capabilities {
                supports_native_tool_calls: true,
                supports_constraint: true,
                exposes_logits: false,
            },
            BackendArg::Mistral => Capabilities {
                supports_native_tool_calls: false,
                supports_constraint: false,
                exposes_logits: false,
            },
        }
    };
    let protocol = select_protocol(&policy, &caps, args.protocol.map(ActionProtocol::from));
    let sampling = SamplingParams {
        temperature: args.temperature,
        max_tokens: policy.max_output_tokens,
        ..SamplingParams::default()
    };

    let trace_dir = workspace_root.join(".ferric").join("trace");
    if let Err(e) = std::fs::create_dir_all(&trace_dir) {
        eprintln!("cannot create trace dir {}: {e}", trace_dir.display());
        return ExitCode::FAILURE;
    }
    let session = format!("q-{}", now_ms());
    let trace_path = trace_dir.join(format!("{session}.jsonl"));
    let mut sink = match JsonlSink::open(&trace_path, &session) {
        Ok(sink) => sink,
        Err(e) => {
            eprintln!("cannot open trace {}: {e}", trace_path.display());
            return ExitCode::FAILURE;
        }
    };

    // Compose the system prompt from a library if one is supplied; otherwise
    // the loop falls back to DEFAULT_SYSTEM_PROMPT. A composition failure is
    // recorded as a Note and degrades gracefully (never silent).
    let prompts_dir = args
        .prompts_dir
        .clone()
        .or_else(|| std::env::var_os("FERRIC_PROMPTS_DIR").map(PathBuf::from));
    let composed = prompts_dir.and_then(|dir| {
        match ferric_prompt::load_library(&dir)
            .and_then(|lib| ferric_prompt::compose_system_prompt(&lib, policy.tier, protocol))
        {
            Ok(c) => Some(c),
            Err(e) => {
                let _ = sink.write_event(Event::Note {
                    text: format!("prompt composition failed, using default: {e}"),
                });
                None
            }
        }
    });
    let (system_prompt, lineage): (Option<&str>, Option<PromptLineage>) = match &composed {
        Some(c) => (
            Some(c.text.as_str()),
            Some((
                c.output_id.clone(),
                c.output_version.clone(),
                c.composed_of.clone(),
            )),
        ),
        None => (None, None),
    };

    let outcome = if args.mock {
        let provider = mock_provider(protocol);
        drive_mock(
            &provider,
            &registry,
            &workspace,
            &policy,
            protocol,
            sampling,
            system_prompt,
            lineage,
            &mut sink,
            &args.prompt,
        )
    } else {
        drive_real(
            &args,
            &registry,
            &workspace,
            &policy,
            protocol,
            sampling,
            system_prompt,
            lineage,
            &mut sink,
        )
    };

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(text) = &outcome.final_text {
        println!("{text}");
    }
    eprintln!(
        "[{} after {} turn(s); trace: {}]",
        outcome.stop.as_str(),
        outcome.turns,
        trace_path.display()
    );
    match outcome.stop {
        StopReason::ProviderError => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Built-in mock script: one file write, then a structured termination —
/// exercises the full loop/trace/guard path with zero model. Shaped to match
/// `protocol` so `--mock` works in either mode.
fn mock_provider(protocol: ActionProtocol) -> MockProvider {
    use serde_json::json;

    let write_args = json!({"path": "ferric-mock.txt", "content": "mock run"});
    let done_args = json!({"summary": "mock run complete"});

    let script = match protocol {
        ActionProtocol::NativeTools => vec![
            native_completion("mock-0", "write_file", write_args),
            native_completion("mock-1", ferric_loop::TASK_COMPLETE, done_args),
        ],
        ActionProtocol::ConstrainedJson => vec![
            json_completion("write_file", &write_args),
            json_completion(ferric_loop::TASK_COMPLETE, &done_args),
        ],
        ActionProtocol::TextXml => vec![
            xml_completion("write_file", &write_args),
            xml_completion(ferric_loop::TASK_COMPLETE, &done_args),
        ],
    };
    MockProvider::new(script)
}

fn native_completion(id: &str, name: &str, args: serde_json::Value) -> Completion {
    use ferric_core::{Role, ToolCall};
    Completion {
        message: Message {
            role: Role::Assistant,
            text: None,
            tool_calls: vec![ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                args,
            }],
            tool_call_id: None,
            media: Vec::new(),
        },
        input_tokens: Some(40),
        output_tokens: Some(12),
        truncated: false,
    }
}

/// `ConstrainedJson` mock: the assistant text IS the `{"tool","args"}` action
/// JSON the server constraint would force.
fn json_completion(name: &str, args: &serde_json::Value) -> Completion {
    let json = serde_json::json!({ "tool": name, "args": args }).to_string();
    Completion {
        message: Message::assistant(json),
        input_tokens: Some(40),
        output_tokens: Some(20),
        truncated: false,
    }
}

/// `TextXml` mock: the assistant text is a `<tool_call>` XML block the loop
/// regex-scrapes.
fn xml_completion(name: &str, args: &serde_json::Value) -> Completion {
    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let xml = format!(
        "<tool_call><name>{}</name><args>{}</args></tool_call>",
        name, args_str
    );
    Completion {
        message: Message::assistant(xml),
        input_tokens: Some(40),
        output_tokens: Some(20),
        truncated: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_mock(
    provider: &dyn Provider,
    registry: &Registry,
    workspace: &Workspace,
    policy: &RunPolicy,
    protocol: ActionProtocol,
    sampling: SamplingParams,
    system_prompt: Option<&str>,
    lineage: Option<PromptLineage>,
    sink: &mut JsonlSink,
    prompt: &str,
) -> Result<LoopOutcome, String> {
    futures_executor::block_on(run(
        RunArgs {
            provider,
            registry,
            workspace,
            policy,
            protocol,
            sampling,
            sleeper: &ThreadSleeper,
            system_prompt,
            prompt_lineage: lineage,
        },
        sink,
        prompt,
    ))
    .map_err(|e| format!("loop error: {e}"))
}

#[cfg(any(feature = "backend-mistralrs", feature = "backend-openai"))]
#[allow(clippy::too_many_arguments)]
fn drive_real(
    args: &QueryArgs,
    registry: &Registry,
    workspace: &Workspace,
    policy: &RunPolicy,
    protocol: ActionProtocol,
    sampling: SamplingParams,
    system_prompt: Option<&str>,
    lineage: Option<PromptLineage>,
    sink: &mut JsonlSink,
) -> Result<LoopOutcome, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async {
        let provider_box = create_provider(&args.backend_opts).await?;
        let provider = provider_box.as_ref();
        run(
            RunArgs {
                provider,
                registry,
                workspace,
                policy,
                protocol,
                sampling,
                sleeper: &ThreadSleeper,
                system_prompt,
                prompt_lineage: lineage,
            },
            sink,
            &args.prompt,
        )
        .await
        .map_err(|e| format!("loop error: {e}"))
    })
}

#[cfg(not(any(feature = "backend-mistralrs", feature = "backend-openai")))]
#[allow(clippy::too_many_arguments)]
fn drive_real(
    _args: &QueryArgs,
    _registry: &Registry,
    _workspace: &Workspace,
    _policy: &RunPolicy,
    _protocol: ActionProtocol,
    _sampling: SamplingParams,
    _system_prompt: Option<&str>,
    _lineage: Option<PromptLineage>,
    _sink: &mut JsonlSink,
) -> Result<LoopOutcome, String> {
    Err("this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`, or use --mock"
        .to_string())
}
