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

use ferric_core::{ActionProtocol, MediaPart, Message, ModelProfile, RunPolicy, policy_for};
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

    /// Attach a file to the prompt (repeatable). Text/code files fold into the
    /// prompt as text (works on any model); media files (image/audio/video)
    /// attach as content parts when `--modality` declares them and the backend
    /// carries media (the OpenAI valve).
    #[arg(long = "file")]
    pub files: Vec<PathBuf>,

    /// Declare the model's accepted non-text modalities (comma list:
    /// `image,audio,video`). Explicit config (ADR-006) — media files attach
    /// only for declared modalities; others are skipped with a reason.
    #[arg(long)]
    pub modality: Option<String>,

    /// Cap the active tool ring (ADR-028). `0` pins the model to the Ring-0
    /// navigate/mutate core — the smallest, surest grammar — regardless of its
    /// size. Restrict-only: it cannot raise the ceiling above what the model's
    /// tier/`measured_level` allows.
    #[arg(long)]
    pub max_ring: Option<u8>,

    /// Directory holding `model_profiles.json` (written by `ferric bench` and
    /// `toolbench --calibrate-rings`). When a record exists for this model, its
    /// `measured_level` sets the tier and its `calibrated_ring` defaults
    /// `--max-ring` — the durable promotion (ADR-029). A missing file is a no-op.
    #[arg(long, default_value = "benchmarks")]
    pub profile_dir: PathBuf,

    /// Stream text live to stdout as it's generated, instead of waiting for
    /// the whole multi-turn loop to finish (ADR-047). Opt-in this increment.
    /// Under `ConstrainedJson`, only `task_complete`'s summary streams (an
    /// activity line naming each intermediate tool call goes to stderr);
    /// under `NativeTools`/`TextXml`, assistant prose streams directly.
    #[arg(long)]
    pub stream: bool,
}

/// The shared subset of `QueryArgs` (everything except `prompt`/`files`) that
/// determines a run's configuration. `ferric query` builds one of these per
/// invocation; `ferric mcp` (T-3606) builds exactly one at server launch and
/// reuses the resulting `RunConfig` across every subsequent `tools/call`.
pub(crate) struct RunConfigArgs {
    pub mock: bool,
    pub backend: BackendArg,
    pub params_b: f32,
    pub quant: String,
    pub family: String,
    pub ctx: u32,
    pub temperature: f32,
    pub protocol_override: Option<ProtocolArg>,
    pub prompts_dir: Option<PathBuf>,
    pub max_ring: Option<u8>,
    pub profile_dir: PathBuf,
    /// `--model` (openai backend) or `--model-file` (mistral backend) — the key
    /// used to look up a persisted profile record (ADR-029). `None` skips the
    /// lookup entirely (matches today's behavior when neither flag is set).
    pub model_key: Option<String>,
}

/// Everything derived from `RunConfigArgs` that a loop execution needs, minus
/// what varies per call (the prompt, attached files, the provider, and the
/// trace sink). Computing this once and reusing it is what lets `ferric mcp`
/// avoid re-deriving the protocol/policy/prompt on every `tools/call`.
pub(crate) struct RunConfig {
    pub registry: Registry,
    pub caps: Capabilities,
    pub protocol: ActionProtocol,
    pub policy: RunPolicy,
    pub sampling: SamplingParams,
    pub system_prompt: Option<String>,
    pub lineage: Option<PromptLineage>,
    /// Set when `prompts_dir` was supplied but composition failed — the caller
    /// (which owns the trace sink, not yet open when this is built) is
    /// responsible for recording it as a `Note` once a sink exists.
    pub prompt_composition_error: Option<String>,
}

pub(crate) fn build_run_config(a: &RunConfigArgs) -> RunConfig {
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);

    // Capability seed for auto protocol selection (an explicit `--protocol`
    // always overrides it). The backend's own `capabilities()` is the source of
    // truth, but the provider is constructed later (in drive_real, or once at
    // `ferric mcp` launch), so we mirror it from the chosen backend: the HTTP
    // valve enforces a JSON-Schema constraint (→ ConstrainedJson); mistral.rs
    // does neither — its constrained path hangs upstream (ADR-020) and it
    // strips tools — so it lands on the honest TextXml fallback.
    let caps = if a.mock {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
            supports_media: false,
        }
    } else {
        match a.backend {
            BackendArg::Openai => Capabilities {
                supports_native_tool_calls: true,
                supports_constraint: true,
                exposes_logits: false,
                supports_media: true,
            },
            BackendArg::Mistral => Capabilities {
                supports_native_tool_calls: false,
                supports_constraint: false,
                exposes_logits: false,
                supports_media: false,
            },
        }
    };
    // Protocol is caps/override-driven (`select_protocol` ignores the policy), so
    // resolve it up-front — it keys the persisted profile lookup below.
    let protocol = select_protocol(
        &policy_for(&ModelProfile {
            params_b: a.params_b,
            quant: a.quant.clone(),
            ctx: a.ctx,
            family: a.family.clone(),
            measured_level: None,
        }),
        &caps,
        a.protocol_override.map(ActionProtocol::from),
    );

    // Read-back of a persisted profile (ADR-029): a prior `ferric bench` /
    // `toolbench --calibrate-rings` may have recorded this model's
    // `measured_level` (→ tier) and `calibrated_ring` (→ `max_ring`). The durable
    // promotion — a proven model auto-runs at its earned tier + ring. Operator
    // `--max-ring` still wins; `--mock` or a missing file is a no-op (identical to
    // an un-calibrated run). Read exactly once here — for `ferric mcp` this means
    // the profile is fixed for the server's lifetime; a `ferric bench
    // --calibrate-rings` run while the server is already up is picked up only on
    // restart. Deliberate (ADR-046): matches the launch-time-fixed philosophy
    // already applied to workspace/backend/model.
    let profile_record = a.model_key.as_ref().and_then(|model| {
        ferric_bench::read_profile(&a.profile_dir, model, &format!("{protocol:?}"))
    });
    if let Some(rec) = &profile_record {
        eprintln!(
            "profile {}: measured_level {:?}, calibrated_ring {:?} ({})",
            rec.model,
            rec.measured_level,
            rec.calibrated_ring,
            a.profile_dir.display()
        );
    }

    let profile = ModelProfile {
        params_b: a.params_b,
        quant: a.quant.clone(),
        ctx: a.ctx,
        family: a.family.clone(),
        measured_level: profile_record.as_ref().and_then(|r| r.measured_level),
    };
    let mut policy = policy_for(&profile);
    // `--max-ring` wins; else the persisted `calibrated_ring`; else the tier
    // ceiling. Restrict-only either way (ADR-028).
    policy.max_ring = a
        .max_ring
        .or_else(|| profile_record.as_ref().and_then(|r| r.calibrated_ring));
    let sampling = SamplingParams {
        temperature: a.temperature,
        max_tokens: policy.max_output_tokens,
        ..SamplingParams::default()
    };

    // Compose the system prompt from a library if one is supplied; otherwise
    // the loop falls back to DEFAULT_SYSTEM_PROMPT. A composition failure is
    // returned as data (not written to a trace here — no sink exists yet at
    // this layer) and degrades gracefully (never silent): the caller records it
    // once a sink is available.
    let prompts_dir = a
        .prompts_dir
        .clone()
        .or_else(|| std::env::var_os("FERRIC_PROMPTS_DIR").map(PathBuf::from));
    let mut prompt_composition_error = None;
    let composed = prompts_dir.and_then(|dir| {
        match ferric_prompt::load_library(&dir)
            .and_then(|lib| ferric_prompt::compose_system_prompt(&lib, policy.tier, protocol))
        {
            Ok(c) => Some(c),
            Err(e) => {
                prompt_composition_error =
                    Some(format!("prompt composition failed, using default: {e}"));
                None
            }
        }
    });
    let (system_prompt, lineage) = match composed {
        Some(c) => (
            Some(c.text),
            Some((c.output_id, c.output_version, c.composed_of)),
        ),
        None => (None, None),
    };

    RunConfig {
        registry,
        caps,
        protocol,
        policy,
        sampling,
        system_prompt,
        lineage,
        prompt_composition_error,
    }
}

/// Route each `--file` (ADR-023): text/code folds into the prompt (any model);
/// media attaches as a gated `MediaPart`; anything skipped is surfaced
/// (stderr), never silent. The decision logic (`classify_path`/
/// `decide_attachment`) is pure `ferric_core`; this is the shared orchestration
/// around it, used by both `run_query` and `ferric mcp`'s `tools/call` handler
/// so the two call sites can't drift apart.
pub(crate) fn route_files(
    files: &[PathBuf],
    declared: &[ferric_core::Modality],
    supports_media: bool,
) -> (Vec<MediaPart>, String) {
    let mut media_parts = Vec::new();
    let mut prompt_suffix = String::new();
    for path in files {
        let kind = ferric_core::classify_path(path);
        match ferric_core::decide_attachment(&kind, declared, supports_media) {
            ferric_core::Attachment::AppendText => match std::fs::read_to_string(path) {
                Ok(text) => {
                    prompt_suffix.push_str(&format!("\n\n--- file: {} ---\n{text}", path.display()))
                }
                Err(e) => eprintln!("skip {}: cannot read as text: {e}", path.display()),
            },
            ferric_core::Attachment::Media(_modality, mime) => match std::fs::read(path) {
                Ok(bytes) => media_parts.push(MediaPart {
                    mime,
                    data: ferric_core::base64_encode(&bytes),
                }),
                Err(e) => eprintln!("skip {}: cannot read: {e}", path.display()),
            },
            ferric_core::Attachment::Skip(reason) => {
                eprintln!("skip {}: {reason}", path.display())
            }
        }
    }
    (media_parts, prompt_suffix)
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

    let config = build_run_config(&RunConfigArgs {
        mock: args.mock,
        backend: args.backend_opts.backend,
        params_b: args.params_b,
        quant: args.quant.clone(),
        family: args.family.clone(),
        ctx: args.ctx,
        temperature: args.temperature,
        protocol_override: args.protocol,
        prompts_dir: args.prompts_dir.clone(),
        max_ring: args.max_ring,
        profile_dir: args.profile_dir.clone(),
        model_key: args
            .backend_opts
            .model
            .clone()
            .or_else(|| args.backend_opts.model_file.clone()),
    });

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

    // A composition failure (if any) is recorded as a Note now that the sink
    // exists; the config itself already fell back to DEFAULT_SYSTEM_PROMPT.
    if let Some(err) = &config.prompt_composition_error {
        let _ = sink.write_event(Event::Note { text: err.clone() });
    }

    let declared = ferric_core::parse_modalities(args.modality.as_deref().unwrap_or(""));
    let (media_parts, prompt_suffix) =
        route_files(&args.files, &declared, config.caps.supports_media);
    let effective_prompt = if prompt_suffix.is_empty() {
        args.prompt.clone()
    } else {
        format!("{}{prompt_suffix}", args.prompt)
    };

    // `--stream` (ADR-047): print `Text` deltas to stdout live, flushed per
    // delta; `ToolNamed` goes to stderr as a "which tool" activity line.
    // `streamed_anything` (an AtomicBool, not a Cell, so the sink closure
    // stays `Sync` — required since the closure is held across an `.await`
    // inside `complete_streaming`) tracks whether anything was already
    // printed live, so the final echo below isn't a duplicate.
    let streamed_anything = std::sync::atomic::AtomicBool::new(false);
    let sink_fn = |d: ferric_provider::StreamDelta| match d {
        ferric_provider::StreamDelta::Text(t) => {
            print!("{t}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            streamed_anything.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        ferric_provider::StreamDelta::ToolNamed(name) => {
            eprintln!("\u{25b8} calling {name}...");
        }
    };
    let stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)> =
        if args.stream { Some(&sink_fn) } else { None };

    let outcome = if args.mock {
        let provider = mock_provider(config.protocol);
        drive_mock(
            &provider,
            &config.registry,
            &workspace,
            &config.policy,
            config.protocol,
            config.sampling,
            config.system_prompt.as_deref(),
            config.lineage.clone(),
            &mut sink,
            &effective_prompt,
            media_parts,
            stream_sink,
        )
    } else {
        drive_real(
            &args,
            &config.registry,
            &workspace,
            &config.policy,
            config.protocol,
            config.sampling,
            config.system_prompt.as_deref(),
            config.lineage.clone(),
            &mut sink,
            &effective_prompt,
            media_parts,
            stream_sink,
        )
    };

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    // Skip the final echo when streaming already displayed the text live —
    // avoids double-printing (T-3705 EARS: `--stream` output must not
    // duplicate).
    if !streamed_anything.load(std::sync::atomic::Ordering::Relaxed)
        && let Some(text) = &outcome.final_text
    {
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

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Built-in mock script: one file write, then a structured termination —
/// exercises the full loop/trace/guard path with zero model. Shaped to match
/// `protocol` so `--mock` works in either mode. `pub(crate)` so `ferric mcp`
/// (`mcp.rs`) can build the same scripted provider under `--mock`.
pub(crate) fn mock_provider(protocol: ActionProtocol) -> MockProvider {
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

/// Run one loop turn against an already-constructed provider. The reusable
/// core both `drive_mock`/`drive_real` (one provider per CLI invocation) and
/// `ferric mcp` (one provider built once, reused across many `tools/call`s)
/// drive — provider construction and loop execution are deliberately kept
/// separate so a caller can build a provider once and call this many times.
/// Unconditionally compiled (no backend feature needed): it only requires a
/// `&dyn Provider`, which `MockProvider` already satisfies.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_with_provider(
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
    media: Vec<MediaPart>,
    stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)>,
) -> Result<LoopOutcome, String> {
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
            media,
            stream_sink,
        },
        sink,
        prompt,
    )
    .await
    .map_err(|e| format!("loop error: {e}"))
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
    media: Vec<MediaPart>,
    stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)>,
) -> Result<LoopOutcome, String> {
    futures_executor::block_on(run_with_provider(
        provider,
        registry,
        workspace,
        policy,
        protocol,
        sampling,
        system_prompt,
        lineage,
        sink,
        prompt,
        media,
        stream_sink,
    ))
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
    prompt: &str,
    media: Vec<MediaPart>,
    stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)>,
) -> Result<LoopOutcome, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async move {
        let provider_box = create_provider(&args.backend_opts).await?;
        run_with_provider(
            provider_box.as_ref(),
            registry,
            workspace,
            policy,
            protocol,
            sampling,
            system_prompt,
            lineage,
            sink,
            prompt,
            media,
            stream_sink,
        )
        .await
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
    _prompt: &str,
    _media: Vec<MediaPart>,
    _stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)>,
) -> Result<LoopOutcome, String> {
    Err("this binary was built without backend features; \
         rebuild with `cargo build --features backend-mistralrs,backend-openai`, or use --mock"
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::policy_for;

    /// T-3601: `run_with_provider` is independently callable given only a
    /// `&dyn Provider` — no `create_provider`/backend-feature dependency. This
    /// is exactly the shape `ferric mcp` needs: build a provider once, drive
    /// many loop executions against it without reconstructing anything.
    #[test]
    fn runs_loop_with_prebuilt_provider() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let mut registry = Registry::new();
        register_builtin_tools(&mut registry);
        let protocol = ActionProtocol::ConstrainedJson;
        let provider = mock_provider(protocol);
        let profile = ModelProfile {
            params_b: 1.2,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "unknown".to_string(),
            measured_level: None,
        };
        let policy = policy_for(&profile);
        let sampling = SamplingParams {
            temperature: 0.0,
            max_tokens: policy.max_output_tokens,
            ..SamplingParams::default()
        };
        let trace_path = dir.path().join("trace.jsonl");
        let mut sink = JsonlSink::open(&trace_path, "test").unwrap();

        let outcome = futures_executor::block_on(run_with_provider(
            &provider,
            &registry,
            &workspace,
            &policy,
            protocol,
            sampling,
            None,
            None,
            &mut sink,
            "do a mock task",
            Vec::new(),
            None,
        ))
        .unwrap();

        assert_eq!(outcome.stop, StopReason::TaskComplete);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("ferric-mock.txt")).unwrap(),
            "mock run"
        );
    }

    fn base_run_config_args() -> RunConfigArgs {
        RunConfigArgs {
            mock: true,
            backend: BackendArg::Mistral,
            params_b: 8.0,
            quant: "Q4_K_M".to_string(),
            family: "unknown".to_string(),
            ctx: 4096,
            // Non-default on purpose (test-critique C-003): 0.0 is also
            // `SamplingParams::default()`'s value, so a mis-wired temperature
            // would pass a same-value assertion vacuously.
            temperature: 0.7,
            protocol_override: None,
            prompts_dir: None,
            max_ring: None,
            profile_dir: PathBuf::from("benchmarks"),
            model_key: None,
        }
    }

    /// T-3602: the extracted config builder must derive the same
    /// protocol/tier/output-budget a direct `select_protocol`/`policy_for`
    /// call would, for identical inputs — pins the extraction against drift.
    #[test]
    fn run_config_matches_inline_computation() {
        let a = base_run_config_args();
        let config = build_run_config(&a);

        let caps = Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
            supports_media: false,
        };
        let expected_protocol = select_protocol(
            &policy_for(&ModelProfile {
                params_b: a.params_b,
                quant: a.quant.clone(),
                ctx: a.ctx,
                family: a.family.clone(),
                measured_level: None,
            }),
            &caps,
            None,
        );
        let expected_policy = policy_for(&ModelProfile {
            params_b: a.params_b,
            quant: a.quant.clone(),
            ctx: a.ctx,
            family: a.family.clone(),
            measured_level: None,
        });

        assert_eq!(config.protocol, expected_protocol);
        assert_eq!(config.policy.tier, expected_policy.tier);
        assert_eq!(
            config.policy.max_output_tokens,
            expected_policy.max_output_tokens
        );
        // test-critique C-003: the EARS clause names sampling too, not just
        // protocol/policy — assert it, not just the first two of three legs.
        assert_eq!(config.sampling.temperature, a.temperature);
        assert_eq!(
            config.sampling.max_tokens,
            expected_policy.max_output_tokens
        );
    }

    /// T-3602: the returned config is plain data, safely readable multiple
    /// times without being consumed — the shape `ferric mcp` needs to reuse it
    /// across many `tools/call`s.
    #[test]
    fn run_config_reused_across_calls() {
        let config = build_run_config(&base_run_config_args());
        let (protocol_1, max_ring_1) = (config.protocol, config.policy.max_ring);
        let (protocol_2, max_ring_2) = (config.protocol, config.policy.max_ring);
        assert_eq!(protocol_1, protocol_2);
        assert_eq!(max_ring_1, max_ring_2);
    }
}
