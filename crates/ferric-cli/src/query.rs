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
    DEFAULT_SYSTEM_PROMPT, LoopOutcome, PromptLineage, RunArgs, StopReason, ThreadSleeper, run,
    select_protocol,
};
use ferric_provider::{Capabilities, Completion, MockProvider, Provider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink};

#[cfg(feature = "backend-openai")]
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
    /// The task prompt. Required unless `--resume` is given (a pure
    /// continuation needs no new instruction); if BOTH are given, this is
    /// appended as one extra user message after the replayed history.
    #[arg(required_unless_present = "resume")]
    pub prompt: Option<String>,

    /// Resume an interrupted, still-incomplete session by replaying its
    /// trace (sprint 39, ADR-049) and continuing the SAME task with more
    /// turns. Not a chat-continuation mechanism: a trace that already
    /// reached any stop reason (clean or not) is rejected. `--prompts-dir`/
    /// `Animus.md` are inert for a resumed run's system message (frozen
    /// from the replayed trace).
    #[arg(long)]
    pub resume: Option<PathBuf>,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Parameter count in billions. Default 1.2 when neither this flag nor a
    /// config file's `params_b` is set (T-3803).
    #[arg(long)]
    pub params_b: Option<f32>,

    /// Quantization label. Default "Q4_K_M" when neither this flag nor a
    /// config file's `quant` is set (T-3803).
    #[arg(long)]
    pub quant: Option<String>,

    /// Model family label. Default "unknown" when neither this flag nor a
    /// config file's `family` is set (T-3803).
    #[arg(long)]
    pub family: Option<String>,

    /// Context window in tokens (ModelProfile is config-supplied, ADR-006).
    /// Default 4096 when neither this flag nor a config file's `ctx` is set
    /// (T-3803).
    #[arg(long)]
    pub ctx: Option<u32>,

    /// Sampling temperature (0.0 selects the deterministic sampler). Default
    /// 0.0 when neither this flag nor a config file's `temperature` is set
    /// (T-3803).
    #[arg(long)]
    pub temperature: Option<f32>,

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
    /// Default "benchmarks" when neither this flag nor a config file's
    /// `profile_dir` is set (T-3803).
    #[arg(long)]
    pub profile_dir: Option<PathBuf>,

    /// Suppress live streaming of text and tool activity (default: streaming is ON).
    #[arg(long)]
    pub no_stream: bool,

    /// Search the workspace and fold quarantined digests into the prompt.
    /// Keywords, not a URL — this drives the Local-FS plane.
    #[arg(long)]
    pub research: Option<String>,

    /// Fetch this URL through the egress airlock and quarantine it (repeatable).
    ///
    /// Separate from `--research` because the two planes want different things:
    /// Local-FS takes keywords, the Web plane takes an exact URL. The allowlist
    /// is derived from the URLs given here and nothing else, so the sandbox may
    /// reach precisely the hosts you named (ADR-085).
    #[arg(long = "research-url")]
    pub research_urls: Vec<String>,

    /// Run with this installed skill's instructions in scope (repeatable).
    ///
    /// Naming a skill here IS the authorization — it is you asking, on this
    /// invocation. Skills you want available without naming them go in
    /// `allowed_skills` in `.ferric/config.toml`. Nothing else authorizes one:
    /// a skill sitting in `.ferric/skills/` is visible to `ferric skills list`
    /// but contributes nothing to a prompt until you say so (ADR-091).
    #[arg(long = "skill")]
    pub skills: Vec<String>,

    /// Run the web fetch on the standard container runtime instead of gVisor.
    ///
    /// The default requires gVisor and fails closed without it (ADR-074). Network
    /// isolation does NOT depend on this — that is enforced by the airlock's
    /// `--internal` network either way; gVisor is defence in depth against
    /// container escape.
    #[arg(long)]
    pub allow_standard_runtime: bool,

    /// The SinkAction for the CaMeL sink policy. Deny | RequireApproval | Warn.
    /// What to do with a MUTATION once this run has ingested untrusted content
    /// (ADR-080): `requireapproval` (default) | `deny` | `warn`.
    ///
    /// This only ever applies to a contaminated run — an ordinary run is never
    /// gated. `requireapproval` asks a human once per mutation (via
    /// `--accept-edits`); with no approver available there is nobody to ask, so
    /// it denies.
    #[arg(long, default_value = "requireapproval")]
    pub sink_action: String,

    /// Accept-edits mode (ADR-070): pause before each mutating tool call
    /// (write/edit/delete/exec), show a preview, and require `y` to apply it.
    /// A rejected edit is reported to the model, which can adapt. Requires an
    /// interactive stdin; not for `--stream` or piped batch runs.
    #[arg(long)]
    pub accept_edits: bool,
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
    /// used to look up a persisted profile record (ADR-029). `None` skips the
    /// lookup entirely (matches today's behavior when neither flag is set).
    pub model_key: Option<String>,
    pub hooks: Option<ferric_core::HooksConfig>,
    /// Workspace root, for locating `.ferric/skills/`.
    pub workspace_root: PathBuf,
    /// Skills named on this invocation (`--skill`).
    pub requested_skills: Vec<String>,
    /// Skills standing-authorized in `.ferric/config.toml`.
    pub allowed_skills: Vec<String>,
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
    pub hooks: Option<ferric_core::HooksConfig>,
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
        ferric_bench::read_profile(&a.profile_dir, model, &ferric_core::protocol_key(protocol))
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
    let (base_system_prompt, lineage) = match composed {
        Some(c) => (
            Some(c.text),
            Some((c.output_id, c.output_version, c.composed_of)),
        ),
        None => (None, None),
    };

    // Skills the user authorized, folded into the system prompt.
    //
    // `authorize` is given only user-supplied inputs — the `--skill` flags and
    // the config allowlist — so there is no parameter through which the model
    // could clear a skill for itself. An unknown name is surfaced rather than
    // silently ignored: a typo that quietly runs nothing is the failure shape
    // this codebase keeps finding (ADR-090).
    let (discovered, skill_errors) = ferric_skills::discover(&a.workspace_root);
    for e in &skill_errors {
        eprintln!("skill: {e}");
    }
    let (authorized, unknown) =
        ferric_skills::authorize(&discovered, &a.requested_skills, &a.allowed_skills);
    for name in &unknown {
        eprintln!("skill: no skill named `{name}` is installed in .ferric/skills/");
    }
    let system_prompt = match ferric_skills::compose(&authorized) {
        None => base_system_prompt,
        Some(section) => {
            for sk in &authorized {
                println!("skill: {} ({:?})", sk.name(), sk.authority());
            }
            Some(match base_system_prompt {
                Some(base) => format!(
                    "{base}

{section}"
                ),
                None => format!(
                    "{}

{section}",
                    ferric_loop::DEFAULT_SYSTEM_PROMPT
                ),
            })
        }
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
        hooks: a.hooks.clone(),
    }
}

/// `Animus.md` (T-3806): a project-root, freeform, user-authored instructions
/// file — trusted context (the workspace owner's own words), not Ornstein-
/// quarantined content — folded into the system prompt as a distinct,
/// clearly-delimited block. Pure; the caller does the actual file read (so it
/// can also decide whether to trace a `Note`) and reuses whichever system
/// prompt `build_run_config` already produced (oovra-composed, or
/// `DEFAULT_SYSTEM_PROMPT` when no library is configured).
pub(crate) fn fold_animus_md(existing: Option<&str>, animus_md: &str) -> String {
    let base = existing.unwrap_or(DEFAULT_SYSTEM_PROMPT);
    format!("{base}\n\n--- Animus.md (project instructions) ---\n{animus_md}")
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

pub fn run_query(mut args: QueryArgs) -> ExitCode {
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

    // T-3803: layered config (CLI flag > project `.ferric/config.toml` > user
    // config > today's hardcoded default). `BackendOpts`' fields are resolved
    // IN PLACE on `args` so the same merged values reach `create_provider` in
    // `drive_real` below, not just this function's own `RunConfigArgs` build.
    let loaded_config = crate::config::load_layered(&workspace_root);
    let cfg = loaded_config.config;
    args.backend_opts = crate::config::merge_backend_opts(args.backend_opts, &cfg);
    let resolved_params_b = args.params_b.or(cfg.params_b).unwrap_or(1.2);
    let resolved_quant = args
        .quant
        .clone()
        .or(cfg.quant)
        .unwrap_or_else(|| "Q4_K_M".to_string());
    let resolved_family = args
        .family
        .clone()
        .or(cfg.family)
        .unwrap_or_else(|| "unknown".to_string());
    let resolved_ctx = args.ctx.or(cfg.ctx).unwrap_or(4096);
    let resolved_temperature = args.temperature.or(cfg.temperature).unwrap_or(0.0);
    let resolved_max_ring = args.max_ring.or(cfg.max_ring);
    let resolved_profile_dir = args
        .profile_dir
        .clone()
        .or(cfg.profile_dir)
        .unwrap_or_else(|| PathBuf::from("benchmarks"));
    let resolved_stream = !args.no_stream && cfg.stream.unwrap_or(true);

    let mut config = build_run_config(&RunConfigArgs {
        mock: args.mock,
        backend: args.backend_opts.backend.unwrap_or(BackendArg::Openai),
        params_b: resolved_params_b,
        quant: resolved_quant,
        family: resolved_family,
        ctx: resolved_ctx,
        temperature: resolved_temperature,
        protocol_override: args.protocol,
        prompts_dir: args.prompts_dir.clone(),
        max_ring: resolved_max_ring,
        profile_dir: resolved_profile_dir,
        // C-001 (plan-critic): derived from the POST-merge, config-resolved
        // `model` (already merged above) — a config-only-set
        // `model` must still hit the ADR-029 profile lookup, not silently
        // skip it because `model_key` was built from raw CLI args.
        model_key: args.backend_opts.model.clone(),
        hooks: cfg.hooks.clone(),
        workspace_root: workspace_root.clone(),
        requested_skills: args.skills.clone(),
        allowed_skills: cfg.allowed_skills.clone().unwrap_or_default(),
    });

    // T-3905 (sprint 39): `--resume <path>` replays an interrupted, still-
    // incomplete session. Resolved here (needs `config.protocol` for the
    // match-validation) and threaded into `RunArgs.resume` below.
    let resume = match &args.resume {
        Some(path) => match ferric_loop::replay(path) {
            Ok(replayed) => {
                if replayed.protocol != config.protocol {
                    eprintln!(
                        "cannot resume {}: recorded protocol {:?} does not match this \
                         invocation's resolved protocol {:?}",
                        path.display(),
                        replayed.protocol,
                        config.protocol
                    );
                    return ExitCode::FAILURE;
                }
                Some(replayed)
            }
            Err(e) => {
                eprintln!("cannot resume {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // T-3806: `Animus.md` — read-only, no parsing. Presence folds its content
    // into the system prompt as a distinct block; absence is a silent no-op
    // (unchanged from today).
    let animus_md = std::fs::read_to_string(workspace_root.join("Animus.md")).ok();
    if let Some(content) = &animus_md {
        config.system_prompt = Some(fold_animus_md(config.system_prompt.as_deref(), content));
    }
    // C-009 (plan-critic): a resumed run's system message is frozen from the
    // replayed trace — `--prompts-dir`/`Animus.md` are silently inert for it.
    // Surface this rather than let a user expect an edited `Animus.md` to
    // apply to a continuation with no signal that it didn't. Checks whether
    // prompts_dir composition was actually ATTEMPTED (lineage on success, the
    // error field on failure), not just whether `--prompts-dir` was passed —
    // `FERRIC_PROMPTS_DIR` can resolve it too.
    if resume.is_some()
        && (config.lineage.is_some()
            || config.prompt_composition_error.is_some()
            || animus_md.is_some())
    {
        eprintln!(
            "note: --resume ignores --prompts-dir/Animus.md for the system message \
             (frozen from the replayed session)"
        );
    }

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
    // A malformed config layer (if any) is both reported to stderr and traced
    // as a Note (C-004: testable data, not a bare eprintln) — never silent.
    for diag in &loaded_config.diagnostics {
        eprintln!("{diag}");
        let _ = sink.write_event(Event::Note { text: diag.clone() });
    }
    // T-3806 (C-005, narrowed): `Animus.md`'s PRESENCE is traced as a Note —
    // its absence stays silent, matching the existing precedent that the
    // ordinary default path (e.g. no `prompts_dir` configured) is untraced.
    if let Some(content) = &animus_md {
        let _ = sink.write_event(Event::Note {
            text: format!("Animus.md applied ({} chars)", content.len()),
        });
    }

    let declared = ferric_core::parse_modalities(args.modality.as_deref().unwrap_or(""));
    let (media_parts, prompt_suffix) =
        route_files(&args.files, &declared, config.caps.supports_media);
    // `Option<String>`: `None` when resuming with no extra prompt/files given
    // (a pure continuation) — `run()` only requires a prompt when NOT
    // resuming, a precondition clap's `required_unless_present` guarantees.
    let effective_prompt = match (&args.prompt, prompt_suffix.is_empty()) {
        (Some(p), true) => Some(p.clone()),
        (Some(p), false) => Some(format!("{p}{prompt_suffix}")),
        (None, true) => None,
        (None, false) => Some(prompt_suffix),
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
            eprintln!("\n\u{25b8} calling {name}...");
        }
        ferric_provider::StreamDelta::ToolCompleted { name, summary } => {
            println!("✓ {name}: {summary}");
        }
        ferric_provider::StreamDelta::Thought(t) => {
            print!("\x1b[90m{t}\x1b[0m"); // Dim ANSI color
            let _ = std::io::Write::flush(&mut std::io::stdout());
            streamed_anything.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let stream_sink: Option<&(dyn Fn(ferric_provider::StreamDelta) + Sync)> = if resolved_stream {
        Some(&sink_fn)
    } else {
        None
    };

    // Accept-edits (ADR-070): an interactive approver that previews each mutating
    // call and requires an explicit `y` (empty/`n`/EOF reject — conservative).
    // `None` unless --accept-edits, so default behavior is unchanged.
    let approver = |preview: &ferric_loop::EditPreview| -> bool {
        eprintln!(
            "\n\u{2500}\u{2500} proposed: {} \u{2500}\u{2500}",
            preview.tool
        );
        for t in &preview.targets {
            eprintln!("   target: {t}");
        }
        let detail: String = preview.detail.chars().take(2000).collect();
        eprintln!("{detail}");
        eprint!("apply this edit? [y/N] ");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => false,
            Ok(_) => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
            Err(_) => false,
        }
    };
    let approver_ref: Option<ferric_loop::EditApprover<'_>> = if args.accept_edits {
        Some(&approver)
    } else {
        None
    };

    // Built once and handed to whichever driver runs. Previously both branches
    // repeated the same 16-odd positional arguments, including this `match`
    // inline, twice.
    let setup = LoopSetup {
        registry: &config.registry,
        workspace: &workspace,
        policy: &config.policy,
        protocol: config.protocol,
        sampling: config.sampling,
        system_prompt: config.system_prompt.as_deref(),
        lineage: config.lineage.clone(),
        media: media_parts,
        stream_sink,
        resume,
        provenance: ferric_guard::Provenance::Clean,
        sink_policy: match args.sink_action.to_lowercase().as_str() {
            "warn" => ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::Warn),
            "deny" => ferric_guard::SinkPolicy::deny(),
            _ => ferric_guard::SinkPolicy::require_approval(),
        },
        hooks: config.hooks.clone(),
        edit_approver: approver_ref,
    };

    let outcome = if args.mock {
        let provider = mock_provider(config.protocol);
        drive_mock(setup, &provider, &mut sink, effective_prompt.as_deref())
    } else {
        drive_real(
            setup,
            &args,
            &mut sink,
            effective_prompt.as_deref(),
            args.research.clone(),
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
        ActionProtocol::Plan => vec![
            json_completion("grep_search", &json!({"query": "mock", "path": "."})),
            json_completion(
                ferric_loop::SUBMIT_PLAN,
                &json!({"plan": "mock plan complete"}),
            ),
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
/// Drive the agent loop and map its error into the CLI's `String` error type.
///
/// Takes `RunArgs` directly. It used to take **18 positional parameters** and
/// immediately re-pack them into this very struct, which meant six call sites
/// each threading 18 unlabelled arguments past each other — the exact shape
/// argument-order bugs live in — behind four
/// `#[allow(clippy::too_many_arguments)]` suppressions (ADR-074).
pub(crate) async fn run_with_provider(
    args: RunArgs<'_>,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
) -> Result<LoopOutcome, String> {
    run(args, sink, prompt)
        .await
        .map_err(|e| format!("loop error: {e}"))
}

/// The host part of an `http(s)://` URL, for deriving the airlock allowlist.
///
/// Deliberately strict and dependency-free: a URL whose host we cannot read is a
/// URL we cannot allowlist, and guessing would widen the airlock (ADR-085).
// Only the backend-gated web-research path calls this, but its tests are
// security tests (userinfo spoofing, injection-shaped hosts) and should run in
// every build — so allow it to be unused rather than gating the tests away.
#[cfg_attr(not(feature = "backend-openai"), allow(dead_code))]
pub(crate) fn url_host(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("{url:?}: only http and https URLs can be researched"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Drop userinfo and port; neither belongs in an allowlist entry.
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        return Err(format!("{url:?}: no host"));
    }
    ferric_research::airlock::validate_host(host)
        .map_err(|e| format!("{url:?}: host {:?} {}", e.host, e.reason))?;
    Ok(host.to_string())
}

/// Everything the loop needs **except the provider**.
///
/// The provider is the one thing `drive_real` cannot supply up front — it has to
/// build it with `create_provider` inside its own tokio runtime — which is why
/// this exists as a separate struct rather than callers just constructing
/// `RunArgs`. Everything else is named at the call site instead of riding along
/// as the 11th positional argument.
pub(crate) struct LoopSetup<'a> {
    pub registry: &'a Registry,
    pub workspace: &'a Workspace,
    pub policy: &'a RunPolicy,
    pub protocol: ActionProtocol,
    pub sampling: SamplingParams,
    pub system_prompt: Option<&'a str>,
    pub lineage: Option<PromptLineage>,
    pub media: Vec<MediaPart>,
    pub stream_sink: Option<&'a (dyn Fn(ferric_provider::StreamDelta) + Sync)>,
    pub resume: Option<ferric_loop::ReplayedState>,
    pub provenance: ferric_guard::Provenance,
    pub sink_policy: ferric_guard::SinkPolicy,
    pub hooks: Option<ferric_core::HooksConfig>,
    pub edit_approver: Option<ferric_loop::EditApprover<'a>>,
}

impl<'a> LoopSetup<'a> {
    /// Complete the setup with the provider (and, for the real path, the
    /// interrupt flag) into the loop's own argument struct.
    pub(crate) fn into_run_args(
        self,
        provider: &'a dyn Provider,
        cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> RunArgs<'a> {
        RunArgs {
            provider,
            registry: self.registry,
            workspace: self.workspace,
            policy: self.policy,
            protocol: self.protocol,
            sampling: self.sampling,
            sleeper: &ThreadSleeper,
            system_prompt: self.system_prompt,
            prompt_lineage: self.lineage,
            media: self.media,
            stream_sink: self.stream_sink,
            resume: self.resume,
            cancel_flag,
            provenance: self.provenance,
            sink_policy: self.sink_policy,
            hooks: self.hooks,
            edit_approver: self.edit_approver,
        }
    }
}

/// The mock path has no ambient runtime by design (ADR-010 keeps the loop
/// executor-agnostic), so it drives the future on `futures_executor`.
fn drive_mock(
    setup: LoopSetup<'_>,
    provider: &dyn Provider,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
) -> Result<LoopOutcome, String> {
    futures_executor::block_on(run_with_provider(
        setup.into_run_args(provider, None),
        sink,
        prompt,
    ))
}

#[cfg(feature = "backend-openai")]
fn drive_real(
    mut setup: LoopSetup<'_>,
    args: &QueryArgs,
    sink: &mut JsonlSink,
    prompt: Option<&str>,
    research_query: Option<String>,
) -> Result<LoopOutcome, String> {
    let workspace = setup.workspace;
    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async move {
        let provider_box = create_provider(&args.backend_opts).await?;
        let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_flag_clone = cancel_flag.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel_flag_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                eprintln!("\n[Received Ctrl-C, interrupting gracefully...]");
            }
        });

        let mut effective_prompt = prompt.map(|s| s.to_string());
        // --- Web plane (ADR-085): fetch named URLs through the egress airlock ---
        //
        // One airlock per RUN, not per URL: standing one up costs ~15s (ADR-083),
        // and every URL in this run shares the same allowlist anyway. RAII means
        // it is torn down even if the run below panics.
        if !args.research_urls.is_empty() {
            let mut hosts: Vec<String> = Vec::new();
            for url in &args.research_urls {
                let host = url_host(url)?;
                if !hosts.contains(&host) {
                    hosts.push(host);
                }
            }
            eprintln!(
                "research: opening egress airlock for {} host(s): {}",
                hosts.len(),
                hosts.join(", ")
            );

            let lock = ferric_research::airlock::Airlock::start(&hosts)
                .map_err(|e| format!("could not open the egress airlock: {e}"))?;

            let mut web = ferric_research::WebRetriever::new().with_network(lock.policy());
            if args.allow_standard_runtime {
                web = web.with_runsc(false);
            }

            let mut cx = String::new();
            let mut fetched = 0usize;
            for url in &args.research_urls {
                match ferric_research::research(&web, provider_box.as_ref(), url).await {
                    Ok(digests) if digests.is_empty() => {
                        eprintln!("research: {url} returned nothing");
                    }
                    Ok(digests) => {
                        for d in digests {
                            cx.push_str(&d.summary);
                            cx.push_str(
                                "
---
",
                            );
                            fetched += 1;
                        }
                    }
                    // Fail loud: a URL the user named that could not be fetched
                    // must not vanish into a normal run (the ADR-078 lesson).
                    Err(e) => return Err(format!("research: fetching {url} failed: {e}")),
                }
            }

            if fetched > 0 {
                // Untrusted content reached the prompt, so the run is
                // contaminated and every later mutation is gated (ADR-080).
                setup.provenance = ferric_guard::Provenance::UntrustedIngested;
                let p = effective_prompt.unwrap_or_default();
                effective_prompt = Some(format!(
                    "{p}

<research_context>
{cx}</research_context>
"
                ));
            }
        }

        if let Some(rq) = research_query {
            // Perform research
            let local_retriever = ferric_research::LocalFsRetriever::with_caps(
                workspace.root().to_path_buf(),
                50,
                1024 * 1024,
            );
            let retrievers: Vec<&dyn ferric_research::Retriever> = vec![&local_retriever];
            match ferric_research::research_all(&retrievers, provider_box.as_ref(), &rq).await {
                Ok(multi) if multi.digests.is_empty() => {
                    // A flag the user explicitly passed must not degrade into an
                    // ordinary run in silence (ADR-078). This was invisible for
                    // three sprints and hid a real retrieval bug behind it.
                    let planes: Vec<String> = multi
                        .planes
                        .iter()
                        .map(|p| {
                            format!(
                                "{} ({})",
                                p.plane,
                                if p.available {
                                    "available, 0 matches"
                                } else {
                                    "unavailable"
                                }
                            )
                        })
                        .collect();
                    eprintln!(
                        "research: no sources matched {rq:?} — planes: {}. \
                         Continuing without research context.",
                        planes.join(", ")
                    );
                }
                Ok(multi) => {
                    {
                        let mut cx = String::new();
                        cx.push_str("\n\n<research_context>\n");
                        // The run is now contaminated (ADR-080). That single
                        // fact IS the gate — there is no per-argument taint to
                        // track, because tracking it never worked: the digests
                        // below are already a paraphrase of their sources, and
                        // anything the model writes paraphrases again.
                        //
                        // Stamped here, inside the non-empty branch: if research
                        // returned nothing, nothing untrusted reached the prompt
                        // and the run stays Clean.
                        setup.provenance = ferric_guard::Provenance::UntrustedIngested;
                        for d in multi.digests {
                            cx.push_str(&d.summary);
                            cx.push_str("\n---\n");
                        }
                        cx.push_str("</research_context>\n");
                        let p = effective_prompt.unwrap_or_default();
                        effective_prompt = Some(format!("{}{}", p, cx));
                    }
                }
                Err(e) => {
                    eprintln!("research failed: {e}");
                }
            }
        }

        run_with_provider(
            setup.into_run_args(provider_box.as_ref(), Some(cancel_flag)),
            sink,
            effective_prompt.as_deref(),
        )
        .await
    })
}

#[cfg(not(feature = "backend-openai"))]
fn drive_real(
    _setup: LoopSetup<'_>,
    _args: &QueryArgs,
    _sink: &mut JsonlSink,
    _prompt: Option<&str>,
    _research_query: Option<String>,
) -> Result<LoopOutcome, String> {
    Err("this binary was built without backend features; \
         rebuild with `cargo build --features backend-openai`, or use --mock"
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::policy_for;

    /// T-3806: `Animus.md` folds in AFTER whichever base prompt already
    /// exists (composed or default), as a distinct, clearly-delimited block.
    #[test]
    fn fold_animus_md_appends_a_distinct_block() {
        let folded = fold_animus_md(Some("BASE"), "project rules");
        assert!(folded.starts_with("BASE"));
        assert!(folded.contains("project rules"));
        assert!(folded.contains("Animus.md"));
    }

    /// Absent `existing` falls back to `DEFAULT_SYSTEM_PROMPT` — mirrors what
    /// the loop itself does when `system_prompt` is `None`.
    #[test]
    fn fold_animus_md_falls_back_to_default_prompt() {
        let folded = fold_animus_md(None, "project rules");
        assert!(folded.starts_with(DEFAULT_SYSTEM_PROMPT));
        assert!(folded.contains("project rules"));
    }

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

        let setup = LoopSetup {
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol,
            sampling,
            system_prompt: None,
            lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        };
        let outcome = futures_executor::block_on(run_with_provider(
            setup.into_run_args(&provider, None),
            &mut sink,
            Some("do a mock task"),
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
            workspace_root: std::path::PathBuf::from("."),
            requested_skills: Vec::new(),
            allowed_skills: Vec::new(),
            mock: true,
            backend: BackendArg::Openai,
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
            hooks: None,
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

    // --- ADR-085: the allowlist is derived from the URLs, so host parsing is a
    // security boundary, not a convenience ---

    #[test]
    fn url_host_reads_ordinary_urls() {
        for (url, want) in [
            ("http://example.com", "example.com"),
            ("https://example.com/", "example.com"),
            ("https://sub.example.com/a/b?q=1#f", "sub.example.com"),
            ("http://example.com:8080/x", "example.com"),
        ] {
            assert_eq!(url_host(url).as_deref(), Ok(want), "for {url}");
        }
    }

    /// Userinfo is the classic way to make a URL *look* like it points somewhere
    /// safe. The allowlist must key on the real host, never the decoration.
    #[test]
    fn url_host_ignores_userinfo() {
        assert_eq!(
            url_host("http://example.com@evil.test/x").as_deref(),
            Ok("evil.test"),
            "the host is what follows '@' — allowlisting example.com here would              have opened evil.test"
        );
    }

    #[test]
    fn url_host_rejects_what_it_cannot_allowlist() {
        for url in [
            "ftp://example.com",
            "example.com",
            "http://",
            "file:///etc/passwd",
            // Would be shell-injected into the gateway if it ever reached it.
            "http://evil.test;wget",
            "http://ev il.test",
        ] {
            assert!(url_host(url).is_err(), "{url:?} must be refused");
        }
    }
}
