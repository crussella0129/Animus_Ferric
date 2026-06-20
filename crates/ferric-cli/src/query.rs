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

/// CLI spelling of `ActionProtocol`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ProtocolArg {
    Native,
    Grammar,
}

impl From<ProtocolArg> for ActionProtocol {
    fn from(p: ProtocolArg) -> Self {
        match p {
            ProtocolArg::Native => ActionProtocol::NativeTools,
            ProtocolArg::Grammar => ActionProtocol::UnifiedGrammar,
        }
    }
}

/// Backend choice for the CLI
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum BackendArg {
    Mistral,
    Openai,
}

#[derive(Args)]
pub struct QueryArgs {
    /// The task prompt
    pub prompt: String,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Which backend to use
    #[arg(long, value_enum, default_value = "mistral")]
    pub backend: BackendArg,

    /// Directory containing the GGUF model (required for mistral backend)
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir (required for mistral backend)
    #[arg(long)]
    pub model_file: Option<String>,

    /// The model string identifier (required for openai backend)
    #[arg(long)]
    pub model: Option<String>,

    /// The OpenAI-compatible API base URL (for openai backend)
    #[arg(long, default_value = "http://localhost:1234/v1")]
    pub api_base: String,

    /// The API key for the OpenAI-compatible API (for openai backend)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006)
    #[arg(long, default_value_t = 4096)]
    pub ctx: u32,

    /// Parameter count in billions
    #[arg(long, default_value_t = 1.2)]
    pub params_b: f32,

    /// Quantization label
    #[arg(long, default_value = "Q4_K_M")]
    pub quant: String,

    /// Model family label
    #[arg(long, default_value = "unknown")]
    pub family: String,

    /// Path to a chat template override (for GGUFs without an embedded one)
    #[arg(long)]
    pub chat_template: Option<PathBuf>,

    /// Path to the model's real tokenizer.json. REQUIRED for `--protocol
    /// grammar` on GGUF models (mistral.rs's synthesized tokenizer breaks the
    /// llguidance toktrie — ADR-020). Also read from FERRIC_TOKENIZER_JSON.
    #[arg(long)]
    pub tokenizer_json: Option<PathBuf>,

    /// Alternatively, an HF model id to source tokenizer.json from.
    #[arg(long)]
    pub tok_model_id: Option<String>,

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
    // Both backends (mock and mistralrs) enforce constraints.
    let caps = Capabilities {
        supports_native_tool_calls: true,
        exposes_logits: false,
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
        ActionProtocol::UnifiedGrammar => vec![
            grammar_completion("write_file", &write_args),
            grammar_completion(ferric_loop::TASK_COMPLETE, &done_args),
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
        },
        input_tokens: Some(40),
        output_tokens: Some(12),
        truncated: false,
    }
}

fn grammar_completion(name: &str, args: &serde_json::Value) -> Completion {
    let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let xml = format!("<tool_call><name>{}</name><args>{}</args></tool_call>", name, args_str);
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
        match args.backend {
            BackendArg::Mistral => {
                #[cfg(feature = "backend-mistralrs")]
                {
                    use ferric_provider::mistralrs::{MistralRsConfig, MistralRsProvider};
                    
                    let model_dir = args
                        .model_dir
                        .as_ref()
                        .ok_or("--model-dir is required for mistral backend")?;
                    let model_file = args
                        .model_file
                        .as_ref()
                        .ok_or("--model-file is required for mistral backend")?;

                    let tokenizer_json = args
                        .tokenizer_json
                        .clone()
                        .or_else(|| std::env::var_os("FERRIC_TOKENIZER_JSON").map(std::path::PathBuf::from));

                    if args.tok_model_id.is_none() {
                        unsafe {
                            std::env::set_var("HF_HUB_OFFLINE", "1");
                        }
                    }

                    let mut config = MistralRsConfig::new(model_dir, model_file);
                    config.chat_template = args.chat_template.as_ref().map(|p| p.display().to_string());
                    config.tokenizer_json = tokenizer_json;
                    config.tok_model_id = args.tok_model_id.clone();
                    let provider = MistralRsProvider::load(config)
                        .await
                        .map_err(|e| format!("backend: {e}"))?;
                    
                    run(
                        RunArgs {
                            provider: &provider,
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
                }
                #[cfg(not(feature = "backend-mistralrs"))]
                {
                    Err("binary built without mistralrs backend".to_string())
                }
            }
            BackendArg::Openai => {
                #[cfg(feature = "backend-openai")]
                {
                    use ferric_provider::openai::{OpenAiProvider, OpenAiConfig};
                    let model_id = args.model.clone().ok_or("--model is required for openai backend")?;
                    let api_key = args.api_key.clone().or_else(|| std::env::var("OPENAI_API_KEY").ok());
                    let config = OpenAiConfig {
                        base_url: args.api_base.clone(),
                        api_key: api_key.unwrap_or_else(|| "ollama".to_string()),
                        model: model_id,
                    };
                    let provider = OpenAiProvider::new(config);

                    run(
                        RunArgs {
                            provider: &provider,
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
                }
                #[cfg(not(feature = "backend-openai"))]
                {
                    Err("binary built without openai backend".to_string())
                }
            }
        }
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
    Err(
        "this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`, or use --mock"
            .to_string(),
    )
}
