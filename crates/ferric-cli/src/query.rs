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

use clap::Args;

use ferric_core::{Message, ModelProfile, policy_for};
use ferric_guard::Workspace;
use ferric_loop::{LoopOutcome, RunArgs, StopReason, ThreadSleeper, run};
use ferric_provider::{Completion, MockProvider, Provider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::JsonlSink;

#[derive(Args)]
pub struct QueryArgs {
    /// The task prompt
    pub prompt: String,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Directory containing the GGUF model
    #[arg(long)]
    pub model_dir: Option<PathBuf>,

    /// GGUF file name inside --model-dir
    #[arg(long)]
    pub model_file: Option<String>,

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

    /// Sampling temperature (0.0 selects the deterministic sampler)
    #[arg(long, default_value_t = 0.0)]
    pub temperature: f32,

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
    let sampling = SamplingParams {
        temperature: args.temperature,
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

    let outcome = if args.mock {
        let provider = mock_provider();
        drive_mock(
            &provider,
            &registry,
            &workspace,
            &policy,
            sampling,
            &mut sink,
            &args.prompt,
        )
    } else {
        drive_real(&args, &registry, &workspace, &policy, sampling, &mut sink)
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
/// exercises the full loop/trace/guard path with zero model.
fn mock_provider() -> MockProvider {
    use ferric_core::{Role, ToolCall};
    use serde_json::json;
    MockProvider::new(vec![
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "mock-0".to_string(),
                    name: "write_file".to_string(),
                    args: json!({"path": "ferric-mock.txt", "content": "mock run"}),
                }],
                tool_call_id: None,
            },
            input_tokens: Some(40),
            output_tokens: Some(12),
        },
        Completion {
            message: Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "mock-1".to_string(),
                    name: ferric_loop::TASK_COMPLETE.to_string(),
                    args: json!({"summary": "mock run complete"}),
                }],
                tool_call_id: None,
            },
            input_tokens: Some(60),
            output_tokens: Some(10),
        },
    ])
}

#[allow(clippy::too_many_arguments)]
fn drive_mock(
    provider: &dyn Provider,
    registry: &Registry,
    workspace: &Workspace,
    policy: &ferric_core::RunPolicy,
    sampling: SamplingParams,
    sink: &mut JsonlSink,
    prompt: &str,
) -> Result<LoopOutcome, String> {
    futures_executor::block_on(run(
        RunArgs {
            provider,
            registry,
            workspace,
            policy,
            sampling,
            sleeper: &ThreadSleeper,
            system_prompt: None,
        },
        sink,
        prompt,
    ))
    .map_err(|e| format!("loop error: {e}"))
}

#[cfg(feature = "backend-mistralrs")]
fn drive_real(
    args: &QueryArgs,
    registry: &Registry,
    workspace: &Workspace,
    policy: &ferric_core::RunPolicy,
    sampling: SamplingParams,
    sink: &mut JsonlSink,
) -> Result<LoopOutcome, String> {
    use ferric_provider::mistralrs::{MistralRsConfig, MistralRsProvider};

    let model_dir = args
        .model_dir
        .as_ref()
        .ok_or("--model-dir is required without --mock")?;
    let model_file = args
        .model_file
        .as_ref()
        .ok_or("--model-file is required without --mock")?;

    // Belt-and-braces offline enforcement (local paths already skip the HF
    // API). Edition 2024: set_var is unsafe; we are pre-runtime, pre-thread.
    unsafe {
        std::env::set_var("HF_HUB_OFFLINE", "1");
    }

    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async {
        let mut config = MistralRsConfig::new(model_dir, model_file);
        config.chat_template = args.chat_template.as_ref().map(|p| p.display().to_string());
        let provider = MistralRsProvider::load(config)
            .await
            .map_err(|e| format!("backend: {e}"))?;
        run(
            RunArgs {
                provider: &provider,
                registry,
                workspace,
                policy,
                sampling,
                sleeper: &ThreadSleeper,
                system_prompt: None,
            },
            sink,
            &args.prompt,
        )
        .await
        .map_err(|e| format!("loop error: {e}"))
    })
}

#[cfg(not(feature = "backend-mistralrs"))]
fn drive_real(
    _args: &QueryArgs,
    _registry: &Registry,
    _workspace: &Workspace,
    _policy: &ferric_core::RunPolicy,
    _sampling: SamplingParams,
    _sink: &mut JsonlSink,
) -> Result<LoopOutcome, String> {
    Err(
        "this binary was built without the backend-mistralrs feature; \
         rebuild with `cargo build --features backend-mistralrs`, or use --mock"
            .to_string(),
    )
}
