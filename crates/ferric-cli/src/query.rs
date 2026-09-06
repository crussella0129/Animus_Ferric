//! `ferric query` — the one-shot, workspace-scoped, policy-scaled, fully
//! traced surface (ADR-011: no chat catch-all).
//!
//! Executor boundary (plan C-009): `--mock` drives the loop on
//! `futures_executor::block_on` (no tokio in the default build); the real
//! backend constructs a tokio multi-thread runtime (the OpenAI HTTP client's
//! async futures need ambient tokio).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};

use ferric_core::{
    ActionProtocol, HarnessPolicy, MediaPart, Message, ModelProfile, RunPolicy, policy_for,
};
use ferric_guard::{Decision, PermissionLevel, Workspace, check_with_ignore};
use ferric_loop::{
    DEFAULT_SYSTEM_PROMPT, LoopOutcome, PromptLineage, RunArgs, ThreadSleeper, run, select_protocol,
};
use ferric_provider::{Capabilities, Completion, MockProvider, Provider, SamplingParams};
use ferric_tools::{NamedCheck, Registry, register_builtin_tools, register_run_checks};
use ferric_trace::{Event, JsonlSink};

use crate::backend::BackendOpts;
#[cfg(feature = "backend-openai")]
use crate::backend::create_provider_in;

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

/// CLI spelling of the autonomous harness policy. Clap exposes the compound
/// variant as `evidence-planner`; persistent config uses the core type's
/// stable `evidence_planner` wire spelling.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum HarnessPolicyArg {
    Legacy,
    Evidence,
    EvidencePlanner,
}

impl From<HarnessPolicyArg> for HarnessPolicy {
    fn from(policy: HarnessPolicyArg) -> Self {
        match policy {
            HarnessPolicyArg::Legacy => HarnessPolicy::Legacy,
            HarnessPolicyArg::Evidence => HarnessPolicy::Evidence,
            HarnessPolicyArg::EvidencePlanner => HarnessPolicy::EvidencePlanner,
        }
    }
}

/// CLI spelling of `Tier` for `--tier` (ADR-098). Kebab-case via `ValueEnum`,
/// and `Deserialize` so `.ferric/config.toml` can spell it the same way.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TierArg {
    Nano,
    Small,
    Medium,
    Large,
    Xl,
    Ultra,
}

impl From<TierArg> for ferric_core::Tier {
    fn from(t: TierArg) -> Self {
        match t {
            TierArg::Nano => ferric_core::Tier::Nano,
            TierArg::Small => ferric_core::Tier::Small,
            TierArg::Medium => ferric_core::Tier::Medium,
            TierArg::Large => ferric_core::Tier::Large,
            TierArg::Xl => ferric_core::Tier::Xl,
            TierArg::Ultra => ferric_core::Tier::Ultra,
        }
    }
}

/// CLI spelling of the CaMeL sink action (ADR-080). A `ValueEnum` rather than a
/// free-form string so an unrecognized value is **rejected at parse time**
/// instead of silently collapsing to `requireapproval` — quietly defaulting a
/// typo is the wrong failure mode for a security control. The canonical
/// spellings (`requireapproval`/`deny`/`warn`) are unchanged; `require-approval`
/// is accepted as an alias.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SinkActionArg {
    /// Ask a human once per mutation (via `--accept-edits`); with no approver
    /// available, deny. The default.
    #[value(name = "requireapproval", alias = "require-approval")]
    RequireApproval,
    /// Block the mutation outright.
    Deny,
    /// Allow the mutation but warn on stderr.
    Warn,
}

impl SinkActionArg {
    pub(crate) fn into_policy(self) -> ferric_guard::SinkPolicy {
        match self {
            SinkActionArg::RequireApproval => ferric_guard::SinkPolicy::require_approval(),
            SinkActionArg::Deny => ferric_guard::SinkPolicy::deny(),
            SinkActionArg::Warn => ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::Warn),
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

    /// Resume a supported incomplete session by replaying its trace (sprint
    /// 39, ADR-049) and continuing the SAME task with more turns. Successful
    /// terminal traces are rejected; supported incomplete stops and
    /// structurally recoverable interrupted traces are resumable. This is not
    /// a chat-continuation mechanism. `--prompts-dir`/`Animus.md` are inert for
    /// a resumed run's system message (frozen from the replayed trace).
    #[arg(long)]
    pub resume: Option<PathBuf>,

    /// Answer a pending `request_user_input` clarification. Requires
    /// `--resume` and cannot be combined with a new positional prompt.
    #[arg(long, requires = "resume", conflicts_with = "prompt")]
    pub answer: Option<String>,

    /// Workspace root (containment boundary). Default: current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Store this query's trace outside the workspace. The directory must be
    /// disjoint from the canonical workspace in both directions and must not
    /// contain a symbolic link or Windows reparse-point component. Default:
    /// `<workspace>/.ferric/trace`.
    ///
    /// Resuming a trace written outside the default root requires explicitly
    /// repeating the same `--trace-dir`. Printed resume commands use
    /// PowerShell quoting on Windows and POSIX-sh quoting on Unix.
    #[arg(long)]
    pub trace_dir: Option<PathBuf>,

    #[command(flatten)]
    pub backend_opts: BackendOpts,

    /// Parameter count in billions. Default 1.2 when neither this flag nor a
    /// config file's `params_b` is set (T-3803).
    ///
    /// This is a **fact about the model**, not a way to pick a tier — use
    /// `--tier` for that (ADR-098). Misstating it to reach a tier also
    /// corrupts the profile store and the trace, and leaves no way to tell an
    /// earned tier from a claimed one.
    #[arg(long)]
    pub params_b: Option<f32>,

    /// Run at this tier regardless of size or measured level (ADR-098).
    ///
    /// Overrides both the `measured_level` read-back and the parameter-count
    /// prior, in **either** direction — you can hold a capable model down as
    /// well as lift a small one up. The run says so on stderr and the trace
    /// records `tier_source: "override"`, so an asked-for tier is never
    /// mistaken later for one the model earned on the ladder.
    ///
    /// Raising the tier widens turn/tool budgets and the tool-ring ceiling; a
    /// model that cannot use them just fails more expensively (the loop guards
    /// bound the waste, ADR-037/038/077). `ferric bench` is how a tier gets
    /// *earned*.
    #[arg(long, value_enum)]
    pub tier: Option<TierArg>,

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

    /// Autonomous harness policy. Omitted fresh runs use `legacy`; omitted
    /// resumes inherit the policy recorded in their source trace.
    #[arg(long, value_enum)]
    pub harness_policy: Option<HarnessPolicyArg>,

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

    /// Override the selected policy's turn budget for this invocation.
    /// Ordinary queries use the tier-derived budget when this is absent.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..))]
    pub max_turns: Option<u8>,

    /// Main-action output cap only; must fit declared context minus prompt
    /// reserve. Does not change tools, turns, reasoning, compaction or timeouts.
    /// Invocation-scoped: repeat on resume; generated guidance repeats cap/ctx.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub max_output_tokens: Option<u32>,

    /// Directory holding `model_profiles.json` (written by `ferric bench` and
    /// `toolbench --calibrate-rings`). When a record exists for this model, its
    /// `measured_level` sets the tier and its `calibrated_ring` defaults
    /// `--max-ring` — the durable promotion (ADR-029). A missing file is a no-op.
    /// Default "benchmarks" when neither this flag nor a config file's
    /// `profile_dir` is set (T-3803).
    #[arg(long)]
    pub profile_dir: Option<PathBuf>,

    /// Explicit operator authorization for model-visible verification checks.
    /// The TOML file contains fixed program/argv definitions; the model can
    /// choose only a check name. Checks are never loaded implicitly from a
    /// repository or the ordinary layered config.
    #[arg(long)]
    pub checks_file: Option<PathBuf>,

    /// Ignore project and user config files for this invocation. CLI flags and
    /// built-in defaults still apply. Intended for isolated benchmark runs.
    #[arg(long)]
    pub no_config: bool,

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

    /// What to do with a MUTATION once this run has ingested untrusted content
    /// (ADR-080): `requireapproval` (default) | `deny` | `warn`.
    ///
    /// This only ever applies to a contaminated run — an ordinary run is never
    /// gated. `requireapproval` asks a human once per mutation (via
    /// `--accept-edits`); with no approver available there is nobody to ask, so
    /// it denies.
    #[arg(long, value_enum, default_value = "requireapproval")]
    pub sink_action: SinkActionArg,

    /// Accept-edits mode (ADR-070): pause before each mutating tool call
    /// (write/edit/delete/exec), show a preview, and require `y` to apply it.
    /// A rejected edit is reported to the model, which can adapt. Requires an
    /// interactive stdin; do not enable it for unattended batch runs.
    #[arg(long)]
    pub accept_edits: bool,
}

/// The shared subset of `QueryArgs` (everything except `prompt`/`files`) that
/// determines a run's configuration. `ferric query` builds one of these per
/// invocation; `ferric mcp` (T-3606) builds exactly one at server launch and
/// reuses the resulting `RunConfig` across every subsequent `tools/call`.
pub(crate) struct RunConfigArgs {
    pub mock: bool,
    pub params_b: f32,
    pub quant: String,
    pub family: String,
    pub ctx: u32,
    pub temperature: f32,
    pub protocol_override: Option<ProtocolArg>,
    pub harness_policy: Option<HarnessPolicy>,
    pub prompts_dir: Option<PathBuf>,
    pub max_ring: Option<u8>,
    /// Explicit operator tier (ADR-098). Wins over the measured read-back and
    /// the parameter prior, in either direction, and is recorded as an
    /// override rather than silently becoming indistinguishable from an
    /// earned tier.
    pub tier_override: Option<ferric_core::Tier>,
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
    pub harness_policy: Option<HarnessPolicy>,
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedChecksFile {
    #[serde(default, rename = "check")]
    checks: Vec<NamedCheck>,
}

fn load_named_checks(path: &Path) -> Result<Vec<NamedCheck>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read checks file {}: {error}", path.display()))?;
    let parsed: NamedChecksFile = toml::from_str(&text)
        .map_err(|error| format!("invalid checks file {}: {error}", path.display()))?;
    if parsed.checks.is_empty() {
        return Err(format!(
            "checks file {} defines no [[check]] entries",
            path.display()
        ));
    }
    Ok(parsed.checks)
}

pub(crate) fn build_run_config(a: &RunConfigArgs) -> Result<RunConfig, crate::config::ConfigError> {
    crate::config::validate_effective_numbers(a.params_b, a.ctx, a.temperature, a.max_ring)?;
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);

    // Capability seed for auto protocol selection (an explicit `--protocol`
    // always overrides it). The backend's own `capabilities()` is the source of
    // truth, but the provider is constructed later (in drive_real, or once at
    // `ferric mcp` launch), so we mirror the sole backend here: the OpenAI HTTP
    // valve enforces a JSON-Schema constraint server-side (→ ConstrainedJson)
    // and carries media.
    let caps = if a.mock {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: false,
            exposes_logits: false,
            supports_media: false,
        }
    } else {
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: true,
            exposes_logits: false,
            supports_media: true,
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
    // An explicit `--tier` wins over both the measured read-back and the
    // parameter prior, and is recorded as such (ADR-098) — the point of the
    // flag is to make an operator decision *sayable* instead of forcing it to
    // be smuggled through `--params-b`, which is a fact about the model.
    let (tier, tier_source) = ferric_core::tier_decision(&profile, a.tier_override);
    if tier_source == ferric_core::TierSource::Override {
        eprintln!(
            "tier: {tier:?} (operator override; not measured — `ferric bench` is how a tier is earned)"
        );
    }
    let mut policy = ferric_core::policy_for_with_override(&profile, a.tier_override);
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

    Ok(RunConfig {
        registry,
        caps,
        protocol,
        harness_policy: a.harness_policy,
        policy,
        sampling,
        system_prompt,
        lineage,
        prompt_composition_error,
        hooks: a.hooks.clone(),
    })
}

/// Fail closed before a product allocates a trace for policies whose complete
/// execution protocol is not available.
pub(crate) fn ensure_supported_harness_policy(
    harness_policy: Option<HarnessPolicy>,
) -> Result<(), String> {
    match harness_policy {
        None | Some(HarnessPolicy::Legacy | HarnessPolicy::Evidence) => Ok(()),
        Some(HarnessPolicy::EvidencePlanner) => {
            Err("harness policy evidence_planner is not implemented yet".to_string())
        }
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

/// A single externally supplied attachment is bounded before it is read. Eight
/// MiB leaves room for ordinary source documents and demo-sized media while
/// preventing one path from turning into an unbounded prompt allocation.
const MAX_ATTACHMENT_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// The aggregate raw-byte budget across one request. Base64 expands media by
/// roughly one third, so keeping the raw side at sixteen MiB also bounds the
/// encoded prompt payload to a predictable size.
const MAX_ATTACHMENT_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
struct AttachmentLimits {
    per_file: u64,
    total: u64,
}

const ATTACHMENT_LIMITS: AttachmentLimits = AttachmentLimits {
    per_file: MAX_ATTACHMENT_FILE_BYTES,
    total: MAX_ATTACHMENT_TOTAL_BYTES,
};

/// Route each externally supplied file (ADR-023): text/code folds into the
/// prompt (any model); media attaches as a gated `MediaPart`; anything skipped
/// is surfaced (stderr), never silent. Every path is first resolved through the
/// selected [`Workspace`] and checked at read permission, including the
/// hardcoded sensitive-file floor and `.ferricignore`. Boundary, permission,
/// I/O, and size failures reject the request rather than quietly dropping a
/// caller-supplied attachment.
pub(crate) fn route_files(
    workspace: &Workspace,
    files: &[PathBuf],
    declared: &[ferric_core::Modality],
    supports_media: bool,
) -> Result<(Vec<MediaPart>, String), String> {
    route_files_with_limits(
        workspace,
        files,
        declared,
        supports_media,
        ATTACHMENT_LIMITS,
    )
}

fn route_files_with_limits(
    workspace: &Workspace,
    files: &[PathBuf],
    declared: &[ferric_core::Modality],
    supports_media: bool,
    limits: AttachmentLimits,
) -> Result<(Vec<MediaPart>, String), String> {
    let mut media_parts = Vec::new();
    let mut prompt_suffix = String::new();
    let mut total_bytes = 0_u64;

    for requested in files {
        let resolved = workspace.resolve(requested).map_err(|e| {
            format!(
                "attachment `{}` rejected by workspace containment: {e}",
                requested.display()
            )
        })?;
        if let Decision::Deny(reason) = check_with_ignore(
            PermissionLevel::Read,
            &resolved,
            workspace.root(),
            workspace.ignore(),
        ) {
            return Err(format!(
                "attachment `{}` denied by read guard: {} matched {}",
                requested.display(),
                reason.rule,
                reason.matched
            ));
        }

        let label = workspace_relative_label(workspace, &resolved);
        let kind = ferric_core::classify_path(&resolved);
        match ferric_core::decide_attachment(&kind, declared, supports_media) {
            ferric_core::Attachment::AppendText => {
                let bytes = read_attachment(&resolved, &label, &mut total_bytes, limits)?;
                let text = String::from_utf8(bytes).map_err(|e| {
                    format!("attachment `{label}` cannot be read as UTF-8 text: {e}")
                })?;
                prompt_suffix.push_str(&format!("\n\n--- file: {label} ---\n{text}"));
            }
            ferric_core::Attachment::Media(_modality, mime) => {
                let bytes = read_attachment(&resolved, &label, &mut total_bytes, limits)?;
                media_parts.push(MediaPart {
                    mime,
                    data: ferric_core::base64_encode(&bytes),
                });
            }
            ferric_core::Attachment::Skip(reason) => {
                eprintln!("skip {label}: {reason}")
            }
        }
    }
    Ok((media_parts, prompt_suffix))
}

fn workspace_relative_label(workspace: &Workspace, resolved: &Path) -> String {
    resolved
        .strip_prefix(workspace.root())
        .unwrap_or(resolved)
        .display()
        .to_string()
}

/// Open and read one file without ever allocating beyond the remaining
/// request budget. Metadata provides an early, clear failure for ordinary
/// files; `take(limit + 1)` closes the race where a file grows after metadata
/// is read.
fn read_attachment(
    path: &Path,
    label: &str,
    total_bytes: &mut u64,
    limits: AttachmentLimits,
) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("attachment `{label}` cannot be opened: {e}"))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("attachment `{label}` metadata cannot be read: {e}"))?;
    if !metadata.is_file() {
        return Err(format!("attachment `{label}` is not a regular file"));
    }
    if metadata.len() > limits.per_file {
        return Err(format!(
            "attachment `{label}` is {} bytes; per-file limit is {} bytes",
            metadata.len(),
            limits.per_file
        ));
    }

    let remaining = limits.total.saturating_sub(*total_bytes);
    if metadata.len() > remaining {
        return Err(format!(
            "attachment `{label}` would exceed the {}-byte aggregate limit ({} bytes already routed)",
            limits.total, *total_bytes
        ));
    }

    let read_limit = limits.per_file.min(remaining);
    let capacity = usize::try_from(metadata.len().min(read_limit)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(read_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| format!("attachment `{label}` cannot be read: {e}"))?;

    let actual = bytes.len() as u64;
    if actual > limits.per_file {
        return Err(format!(
            "attachment `{label}` exceeds the {}-byte per-file limit while being read",
            limits.per_file
        ));
    }
    if actual > remaining {
        return Err(format!(
            "attachment `{label}` exceeds the {}-byte aggregate limit while being read ({} bytes already routed)",
            limits.total, *total_bytes
        ));
    }
    *total_bytes += actual;
    Ok(bytes)
}

/// Stable failure classes for the query-only external trace-root boundary.
/// Keeping the class separate from the human detail lets tests pin which
/// predicate refused a path without depending on platform-specific I/O text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalTraceErrorClass {
    InvalidPath,
    Io,
    NotDirectory,
    LinkOrReparse,
    EqualWorkspace,
    AncestorOfWorkspace,
    DescendantOfWorkspace,
    ExplicitRootRequired,
    ResumeRootMismatch,
}

impl ExternalTraceErrorClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidPath => "trace_dir_invalid_path",
            Self::Io => "trace_dir_io",
            Self::NotDirectory => "trace_dir_not_directory",
            Self::LinkOrReparse => "trace_dir_link_or_reparse",
            Self::EqualWorkspace => "trace_dir_equal_workspace",
            Self::AncestorOfWorkspace => "trace_dir_ancestor_of_workspace",
            Self::DescendantOfWorkspace => "trace_dir_descendant_of_workspace",
            Self::ExplicitRootRequired => "trace_dir_required_for_external_resume",
            Self::ResumeRootMismatch => "trace_dir_mismatch_for_external_resume",
        }
    }
}

#[derive(Debug)]
struct ExternalTraceError {
    class: ExternalTraceErrorClass,
    detail: String,
}

impl ExternalTraceError {
    fn new(class: ExternalTraceErrorClass, detail: impl Into<String>) -> Self {
        Self {
            class,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ExternalTraceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "trace directory [{}]: {}",
            self.class.as_str(),
            self.detail
        )
    }
}

#[derive(Debug, Clone)]
struct PreparedExternalTraceRoot {
    /// Canonical deepest-existing ancestor plus the untouched absent tail.
    /// Once materialized and revalidated this becomes fully canonical.
    root: PathBuf,
}

/// Lexically collapse `.` and `..` without touching the filesystem. Refuse a
/// parent traversal above the prefix/root rather than manufacturing a path.
fn lexical_normalize_trace_path(path: &Path) -> Option<PathBuf> {
    use std::path::Component;

    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match components.last() {
                Some(Component::Normal(_)) => {
                    components.pop();
                }
                _ => return None,
            },
            other => components.push(other),
        }
    }
    Some(components.iter().collect())
}

fn absolutize_trace_path(path: &Path) -> Result<PathBuf, ExternalTraceError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let current = std::env::current_dir().map_err(|error| {
            ExternalTraceError::new(
                ExternalTraceErrorClass::Io,
                format!("cannot determine the current directory: {error}"),
            )
        })?;
        current.join(path)
    };
    let normalized = lexical_normalize_trace_path(&joined).ok_or_else(|| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::InvalidPath,
            format!("{} traverses above its filesystem root", path.display()),
        )
    })?;
    if !normalized.is_absolute() {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::InvalidPath,
            format!("{} does not resolve to an absolute path", path.display()),
        ));
    }
    Ok(normalized)
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    // FILE_ATTRIBUTE_REPARSE_POINT. Rust exposes the raw attribute word but
    // intentionally does not duplicate the Win32 constant in `std`.
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Inspect every existing component without following its final component.
/// This deliberately rejects links/reparse points even when they would resolve
/// to a directory that is otherwise disjoint from the workspace.
fn validate_existing_trace_components(path: &Path) -> Result<(), ExternalTraceError> {
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    for component_path in ancestors {
        if component_path.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(component_path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
                    return Err(ExternalTraceError::new(
                        ExternalTraceErrorClass::LinkOrReparse,
                        format!(
                            "{} is a symbolic link or reparse point",
                            component_path.display()
                        ),
                    ));
                }
                if !metadata.is_dir() {
                    return Err(ExternalTraceError::new(
                        ExternalTraceErrorClass::NotDirectory,
                        format!("{} is not a directory", component_path.display()),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ExternalTraceError::new(
                    ExternalTraceErrorClass::Io,
                    format!("cannot inspect {}: {error}", component_path.display()),
                ));
            }
        }
    }
    Ok(())
}

/// Canonicalize the deepest existing directory, then reconstruct the absent
/// tail. Component inspection must run first so canonicalization is not used
/// to silently accept a symlink/reparse traversal.
fn canonicalize_existing_trace_prefix(path: &Path) -> Result<PathBuf, ExternalTraceError> {
    let mut existing = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name().map(ToOwned::to_owned) else {
                    return Err(ExternalTraceError::new(
                        ExternalTraceErrorClass::Io,
                        format!("no existing ancestor could be found for {}", path.display()),
                    ));
                };
                let Some(parent) = existing.parent() else {
                    return Err(ExternalTraceError::new(
                        ExternalTraceErrorClass::Io,
                        format!("no existing ancestor could be found for {}", path.display()),
                    ));
                };
                tail.push(name);
                existing = parent.to_path_buf();
            }
            Err(error) => {
                return Err(ExternalTraceError::new(
                    ExternalTraceErrorClass::Io,
                    format!("cannot inspect {}: {error}", existing.display()),
                ));
            }
        }
    }

    let mut resolved = std::fs::canonicalize(&existing).map_err(|error| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::Io,
            format!("cannot canonicalize {}: {error}", existing.display()),
        )
    })?;
    for component in tail.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

#[cfg(windows)]
fn trace_component_eq(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(not(windows))]
fn trace_component_eq(left: std::path::Component<'_>, right: std::path::Component<'_>) -> bool {
    left == right
}

/// Component-wise prefix comparison. On Windows this also closes the lexical
/// case-alias gap left by `Path::starts_with` on a case-insensitive filesystem.
fn trace_path_has_prefix(path: &Path, prefix: &Path) -> bool {
    let mut path_components = path.components();
    for expected in prefix.components() {
        let Some(actual) = path_components.next() else {
            return false;
        };
        if !trace_component_eq(actual, expected) {
            return false;
        }
    }
    true
}

fn trace_paths_equal(left: &Path, right: &Path) -> bool {
    trace_path_has_prefix(left, right) && trace_path_has_prefix(right, left)
}

fn validate_trace_workspace_disjointness(
    trace_root: &Path,
    workspace_root: &Path,
) -> Result<(), ExternalTraceError> {
    if trace_paths_equal(trace_root, workspace_root) {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::EqualWorkspace,
            format!("{} resolves to the workspace root", trace_root.display()),
        ));
    }
    if trace_path_has_prefix(workspace_root, trace_root) {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::AncestorOfWorkspace,
            format!(
                "{} is an ancestor of workspace {}",
                trace_root.display(),
                workspace_root.display()
            ),
        ));
    }
    if trace_path_has_prefix(trace_root, workspace_root) {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::DescendantOfWorkspace,
            format!(
                "{} is inside workspace {}",
                trace_root.display(),
                workspace_root.display()
            ),
        ));
    }
    Ok(())
}

/// Resolve and validate an operator-supplied external trace root without
/// mutating the filesystem.
fn prepare_external_trace_root(
    requested: &Path,
    workspace_root: &Path,
) -> Result<PreparedExternalTraceRoot, ExternalTraceError> {
    let absolute = absolutize_trace_path(requested)?;
    validate_existing_trace_components(&absolute)?;
    let root = canonicalize_existing_trace_prefix(&absolute)?;
    validate_trace_workspace_disjointness(&root, workspace_root)?;
    Ok(PreparedExternalTraceRoot { root })
}

fn validate_materialized_external_trace_root(
    root: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, ExternalTraceError> {
    validate_existing_trace_components(root)?;
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::Io,
            format!(
                "cannot inspect created directory {}: {error}",
                root.display()
            ),
        )
    })?;
    if metadata.file_type().is_symlink() || metadata_is_reparse(&metadata) {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::LinkOrReparse,
            format!("{} became a symbolic link or reparse point", root.display()),
        ));
    }
    if !metadata.is_dir() {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::NotDirectory,
            format!("{} is not a directory", root.display()),
        ));
    }
    let canonical = std::fs::canonicalize(root).map_err(|error| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::Io,
            format!(
                "cannot canonicalize created directory {}: {error}",
                root.display()
            ),
        )
    })?;
    validate_trace_workspace_disjointness(&canonical, workspace_root)?;
    Ok(canonical)
}

fn materialize_external_trace_root(
    prepared: &PreparedExternalTraceRoot,
    workspace_root: &Path,
) -> Result<PathBuf, ExternalTraceError> {
    std::fs::create_dir_all(&prepared.root).map_err(|error| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::Io,
            format!("cannot create {}: {error}", prepared.root.display()),
        )
    })?;

    // The test build can atomically substitute one post-create filesystem
    // state on this test thread. The call, hook machinery, and branch are all
    // compiled out of release builds; production proceeds directly from
    // `create_dir_all` to validation below.
    #[cfg(test)]
    {
        let validation_root = run_external_trace_post_create_hook(&prepared.root);
        validate_materialized_external_trace_root(&validation_root, workspace_root)
    }

    #[cfg(not(test))]
    {
        validate_materialized_external_trace_root(&prepared.root, workspace_root)
    }
}

#[cfg(test)]
type ExternalTracePostCreateHook = Box<dyn FnOnce(&Path) -> PathBuf>;

#[cfg(test)]
struct ExternalTracePostCreateHookState {
    next_id: u64,
    installed: Option<(u64, ExternalTracePostCreateHook)>,
}

#[cfg(test)]
std::thread_local! {
    /// Thread-local makes parallel unit tests independent; the id lets an old
    /// guard drop without clearing a newer hook installed after its one-shot
    /// callback was consumed.
    static EXTERNAL_TRACE_POST_CREATE_HOOK: std::cell::RefCell<ExternalTracePostCreateHookState> =
        const { std::cell::RefCell::new(ExternalTracePostCreateHookState {
            next_id: 0,
            installed: None,
        }) };
}

#[cfg(test)]
struct ExternalTracePostCreateHookGuard {
    id: u64,
}

#[cfg(test)]
impl Drop for ExternalTracePostCreateHookGuard {
    fn drop(&mut self) {
        EXTERNAL_TRACE_POST_CREATE_HOOK.with(|state| {
            let mut state = state.borrow_mut();
            if state
                .installed
                .as_ref()
                .is_some_and(|(installed_id, _)| *installed_id == self.id)
            {
                state.installed = None;
            }
        });
    }
}

/// Install one callback for the next materialization on this test thread.
/// The returned guard clears an unconsumed callback during panic/early return.
#[cfg(test)]
fn install_external_trace_post_create_hook<F>(hook: F) -> ExternalTracePostCreateHookGuard
where
    F: FnOnce(&Path) -> PathBuf + 'static,
{
    EXTERNAL_TRACE_POST_CREATE_HOOK.with(|state| {
        let mut state = state.borrow_mut();
        assert!(
            state.installed.is_none(),
            "an external trace post-create hook is already installed on this test thread"
        );
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        state.installed = Some((id, Box::new(hook)));
        ExternalTracePostCreateHookGuard { id }
    })
}

#[cfg(test)]
fn run_external_trace_post_create_hook(root: &Path) -> PathBuf {
    let hook = EXTERNAL_TRACE_POST_CREATE_HOOK
        .with(|state| state.borrow_mut().installed.take().map(|(_, hook)| hook));
    hook.map_or_else(|| root.to_path_buf(), |hook| hook(root))
}

fn canonical_resume_trace_parent(path: &Path) -> Result<PathBuf, ExternalTraceError> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::Io,
            format!(
                "cannot canonicalize resume trace {}: {error}",
                path.display()
            ),
        )
    })?;
    canonical.parent().map(Path::to_path_buf).ok_or_else(|| {
        ExternalTraceError::new(
            ExternalTraceErrorClass::InvalidPath,
            format!("resume trace {} has no parent directory", path.display()),
        )
    })
}

/// External-source continuations must repeat the same canonical output root.
/// Default-source continuations may explicitly opt into a valid external root.
fn validate_resume_trace_root(
    resume_path: Option<&Path>,
    workspace_root: &Path,
    selected_external: Option<&PreparedExternalTraceRoot>,
) -> Result<(), ExternalTraceError> {
    let Some(resume_path) = resume_path else {
        return Ok(());
    };
    let source_root = canonical_resume_trace_parent(resume_path)?;
    let default_root =
        canonicalize_existing_trace_prefix(&workspace_root.join(".ferric").join("trace"))?;
    if trace_paths_equal(&source_root, &default_root) {
        return Ok(());
    }

    let Some(selected) = selected_external else {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::ExplicitRootRequired,
            format!(
                "resume trace {} is outside the default workspace trace root; repeat its parent with --trace-dir",
                resume_path.display()
            ),
        ));
    };
    if !trace_paths_equal(&source_root, &selected.root) {
        return Err(ExternalTraceError::new(
            ExternalTraceErrorClass::ResumeRootMismatch,
            format!(
                "--trace-dir {} does not match resume trace parent {}",
                selected.root.display(),
                source_root.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn documented_shell_quote(value: &str) -> String {
    // PowerShell single-quoted strings are literal; a single quote is escaped
    // by doubling it. `$`, backticks, separators, and double quotes stay inert.
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(windows))]
pub(crate) fn documented_shell_quote(value: &str) -> String {
    // POSIX sh single-quoted strings are literal. Close the string, emit one
    // quoted single quote, then reopen it.
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn format_resume_command(
    trace_path: &Path,
    workspace_root: &Path,
    external_trace_root: Option<&Path>,
    needs_answer: bool,
    output_budget: Option<&ferric_core::OutputBudget>,
) -> String {
    let mut command = format!(
        "ferric query --resume {} --workspace {}",
        documented_shell_quote(&trace_path.to_string_lossy()),
        documented_shell_quote(&workspace_root.to_string_lossy())
    );
    if let Some(trace_root) = external_trace_root {
        command.push_str(" --trace-dir ");
        command.push_str(&documented_shell_quote(&trace_root.to_string_lossy()));
    }
    if needs_answer {
        command.push_str(" --answer ");
        command.push_str(&documented_shell_quote("<answer>"));
    }
    if let Some((cap, ctx)) =
        output_budget.and_then(|budget| budget.requested.zip(budget.declared_ctx))
    {
        command.push_str(&format!(
            " --max-output-tokens {} --ctx {}",
            documented_shell_quote(&cap.to_string()),
            documented_shell_quote(&ctx.to_string())
        ));
    }
    command
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
    // Query-only external trace policy. Resolve and reject unsafe paths before
    // configuration, resume, trace, or provider code can create anything.
    // Actual directory creation remains delayed until every other read-only
    // query precondition has passed.
    let prepared_external_trace_root = match args.trace_dir.as_deref() {
        Some(requested) => match prepare_external_trace_root(requested, workspace.root()) {
            Ok(prepared) => Some(prepared),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    // T-3803: layered config (CLI flag > project `.ferric/config.toml` > user
    // config > today's hardcoded default). `BackendOpts`' fields are resolved
    // IN PLACE on `args` so the same merged values reach `create_provider` in
    // `drive_real` below, not just this function's own `RunConfigArgs` build.
    let loaded_config = if args.no_config {
        Ok(crate::config::LoadedConfig::default())
    } else {
        crate::config::load_layered(&workspace_root)
    };
    let loaded_config = match loaded_config {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    // Hooks are the one config field that becomes arbitrary command execution
    // (`run_hook` -> `sh -c` with the full inherited environment), and the user
    // layer's location is chosen by environment variable. Naming the file is
    // not a permission check — it is the difference between a hook you wrote
    // and a hook that arrived from a config you did not know was being read
    // (ADR-097).
    if let (Some(src), Some(_)) = (&loaded_config.hooks_source, &loaded_config.config.hooks) {
        eprintln!("hooks: loaded from {}", src.display());
    }
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
    let resolved_harness_policy = args.harness_policy.map(Into::into).or(cfg.harness_policy);
    let resolved_max_ring = args.max_ring.or(cfg.max_ring);
    let resolved_tier = args.tier.or(cfg.tier);
    let resolved_profile_dir = args
        .profile_dir
        .clone()
        .or(cfg.profile_dir)
        .unwrap_or_else(|| PathBuf::from("benchmarks"));
    let resolved_stream = crate::config::effective_stream(args.no_stream, cfg.stream);

    let config = build_run_config(&RunConfigArgs {
        mock: args.mock,
        params_b: resolved_params_b,
        quant: resolved_quant,
        family: resolved_family,
        ctx: resolved_ctx,
        temperature: resolved_temperature,
        protocol_override: args.protocol,
        harness_policy: resolved_harness_policy,
        prompts_dir: args.prompts_dir.clone(),
        max_ring: resolved_max_ring,
        tier_override: resolved_tier.map(Into::into),
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
    let mut config = match config {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let output_budget = match ferric_core::resolve_output_budget(
        &config.policy,
        resolved_ctx,
        args.max_output_tokens,
    ) {
        Ok(budget) => budget,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    config.policy.max_output_tokens = output_budget.effective;
    config.sampling.max_tokens = output_budget.effective;
    config.policy.output_budget = Some(output_budget);
    if let Some(max_turns) = args.max_turns {
        config.policy.max_turns = max_turns;
    }
    if let Some(path) = &args.checks_file {
        let checks = match load_named_checks(path) {
            Ok(checks) => checks,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        let names = checks
            .iter()
            .map(|check| check.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if let Err(error) = register_run_checks(&mut config.registry, checks) {
            eprintln!("invalid checks file {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("verification checks authorized: {names}");
    }

    // T-3905 (sprint 39): `--resume <path>` replays an interrupted, still-
    // incomplete session. Resolved here (needs `config.protocol` for the
    // match-validation) and threaded into `RunArgs.resume` below.
    let resume = match &args.resume {
        Some(path) => match ferric_loop::replay(path) {
            Ok(replayed) => {
                if let Err(error) = ferric_loop::validate_resume_target(
                    &replayed,
                    workspace.root(),
                    config.protocol,
                    config.harness_policy,
                ) {
                    eprintln!("cannot resume {}: {error}", path.display());
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
    if let Err(error) = validate_resume_trace_root(
        args.resume.as_deref(),
        workspace.root(),
        prepared_external_trace_root.as_ref(),
    ) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = ensure_supported_harness_policy(config.harness_policy) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

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

    let declared = ferric_core::parse_modalities(args.modality.as_deref().unwrap_or(""));
    let (media_parts, prompt_suffix) = match route_files(
        &workspace,
        &args.files,
        &declared,
        config.caps.supports_media,
    ) {
        Ok(routed) => routed,
        Err(e) => {
            eprintln!("files: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (trace_dir, external_trace_root) = match prepared_external_trace_root.as_ref() {
        Some(prepared) => match materialize_external_trace_root(prepared, workspace.root()) {
            Ok(canonical) => (canonical.clone(), Some(canonical)),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        None => {
            let trace_dir = workspace_root.join(".ferric").join("trace");
            if let Err(e) = std::fs::create_dir_all(&trace_dir) {
                eprintln!("cannot create trace dir {}: {e}", trace_dir.display());
                return ExitCode::FAILURE;
            }
            (trace_dir, None)
        }
    };
    let (_session, trace_path, mut sink) = match create_trace_sink(&trace_dir, "q") {
        Ok(trace) => trace,
        Err(e) => {
            eprintln!("cannot allocate query trace: {e}");
            return ExitCode::FAILURE;
        }
    };

    // A composition failure (if any) is recorded as a Note now that the sink
    // exists; the config itself already fell back to DEFAULT_SYSTEM_PROMPT.
    if let Some(err) = &config.prompt_composition_error {
        let _ = sink.write_event(Event::Note { text: err.clone() });
    }
    // T-3806 (C-005, narrowed): `Animus.md`'s PRESENCE is traced as a Note —
    // its absence stays silent, matching the existing precedent that the
    // ordinary default path (e.g. no `prompts_dir` configured) is untraced.
    if let Some(content) = &animus_md {
        let _ = sink.write_event(Event::Note {
            text: format!("Animus.md applied ({} chars)", content.len()),
        });
    }

    // `Option<String>`: `None` when resuming with no extra prompt/files given
    // (a pure continuation) — `run()` only requires a prompt when NOT
    // resuming, a precondition clap's `required_unless_present` guarantees.
    let effective_prompt = match (&args.prompt, prompt_suffix.is_empty()) {
        (Some(p), true) => Some(p.clone()),
        (Some(p), false) => Some(format!("{p}{prompt_suffix}")),
        (None, true) => None,
        (None, false) => Some(prompt_suffix),
    };

    // Live streaming (the default; ADR-047): print `Text` deltas to stdout,
    // flushed per
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
        harness_policy: config.harness_policy,
        sampling: config.sampling,
        system_prompt: config.system_prompt.as_deref(),
        lineage: config.lineage.clone(),
        media: media_parts,
        stream_sink,
        resume,
        answer: args.answer.as_deref(),
        provenance: ferric_guard::Provenance::Clean,
        sink_policy: args.sink_action.into_policy(),
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
    // avoids double-printing (T-3705 EARS: live output must not
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
    if let Some(needs_input) = &outcome.needs_input {
        eprintln!("Question: {}", needs_input.request.question);
        eprintln!("Context: {}", needs_input.request.context);
        if !needs_input.request.options.is_empty() {
            eprintln!("Options:");
            for (index, option) in needs_input.request.options.iter().enumerate() {
                eprintln!("  {}. {}", index + 1, option);
            }
        }
    }
    if !outcome.stop.is_success() {
        let canonical_trace_path =
            std::fs::canonicalize(&trace_path).unwrap_or_else(|_| trace_path.clone());
        eprintln!(
            "Resume: {}",
            format_resume_command(
                &canonical_trace_path,
                workspace.root(),
                external_trace_root.as_deref(),
                outcome.needs_input.is_some(),
                config.policy.output_budget.as_ref(),
            )
        );
    }
    if outcome.stop.is_success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

static TRACE_SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Allocate an opaque, collision-resistant session id and a trace that cannot
/// append to an existing session. The `create_new` boundary matters for API
/// and MCP servers where multiple requests can begin in the same millisecond.
pub(crate) fn create_trace_sink(
    trace_dir: &Path,
    prefix: &str,
) -> Result<(String, PathBuf, JsonlSink), ferric_core::FerricError> {
    std::fs::create_dir_all(trace_dir)?;
    for _ in 0..32 {
        let counter = TRACE_SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let session = format!("{prefix}-{}-{}-{counter}", now_ms(), std::process::id());
        let path = trace_dir.join(format!("{session}.jsonl"));
        match JsonlSink::create_new(&path, &session) {
            Ok(sink) => return Ok((session, path, sink)),
            Err(ferric_core::FerricError::Io(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(ferric_core::FerricError::Other(
        "could not allocate a unique trace after 32 attempts".to_string(),
    ))
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
    pub harness_policy: Option<HarnessPolicy>,
    pub sampling: SamplingParams,
    pub system_prompt: Option<&'a str>,
    pub lineage: Option<PromptLineage>,
    pub media: Vec<MediaPart>,
    pub stream_sink: Option<&'a (dyn Fn(ferric_provider::StreamDelta) + Sync)>,
    pub resume: Option<ferric_loop::ReplayedState>,
    pub answer: Option<&'a str>,
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
            harness_policy: self.harness_policy,
            sampling: self.sampling,
            sleeper: &ThreadSleeper,
            system_prompt: self.system_prompt,
            prompt_lineage: self.lineage,
            media: self.media,
            stream_sink: self.stream_sink,
            resume: self.resume,
            answer: self.answer,
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
        let provider_box = create_provider_in(&args.backend_opts, workspace.root()).await?;
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

    fn trace_test_workspace(directory: &tempfile::TempDir) -> (PathBuf, Workspace) {
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let workspace = Workspace::new(&root).unwrap();
        (root, workspace)
    }

    fn assert_external_trace_error(
        result: Result<PreparedExternalTraceRoot, ExternalTraceError>,
        expected: ExternalTraceErrorClass,
    ) {
        let error = result.unwrap_err();
        assert_eq!(error.class, expected, "{error}");
    }

    #[test]
    fn external_trace_root_resolves_nonexistent_tail() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let external = directory.path().join("evidence");
        std::fs::create_dir(&external).unwrap();
        let requested = external.join("nested").join("trace");

        let prepared = prepare_external_trace_root(&requested, workspace.root()).unwrap();

        assert_eq!(
            prepared.root,
            std::fs::canonicalize(&external)
                .unwrap()
                .join("nested")
                .join("trace")
        );
        assert!(
            !requested.exists(),
            "prevalidation must not create the tail"
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejects_equal() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        assert_external_trace_error(
            prepare_external_trace_root(workspace.root(), workspace.root()),
            ExternalTraceErrorClass::EqualWorkspace,
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejects_descendant() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let requested = workspace.root().join("sealed").join("traces");
        assert_external_trace_error(
            prepare_external_trace_root(&requested, workspace.root()),
            ExternalTraceErrorClass::DescendantOfWorkspace,
        );
        assert!(!requested.exists());
        assert!(!workspace.root().join(".ferric").exists());
        assert!(!workspace.root().join("ferric-mock.txt").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejects_dotdot_overlap_alias() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let unused = directory.path().join("unused-component");
        let aliased_target = workspace.root().join("aliased-traces");
        let requested = unused.join("..").join("workspace").join("aliased-traces");

        assert_external_trace_error(
            prepare_external_trace_root(&requested, workspace.root()),
            ExternalTraceErrorClass::DescendantOfWorkspace,
        );
        assert!(
            !unused.exists(),
            "lexical resolution must not create its alias prefix"
        );
        assert!(
            !aliased_target.exists(),
            "overlap rejection must precede directory creation"
        );
        assert!(!workspace.root().join(".ferric").exists());
        assert!(!workspace.root().join("ferric-mock.txt").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejects_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        assert_external_trace_error(
            prepare_external_trace_root(directory.path(), workspace.root()),
            ExternalTraceErrorClass::AncestorOfWorkspace,
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejects_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let requested = directory.path().join("trace-file");
        std::fs::write(&requested, "not a directory").unwrap();
        assert_external_trace_error(
            prepare_external_trace_root(&requested, workspace.root()),
            ExternalTraceErrorClass::NotDirectory,
        );
        assert_eq!(
            std::fs::read_to_string(&requested).unwrap(),
            "not a directory"
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[cfg(unix)]
    #[test]
    fn external_trace_root_precreate_rejects_symlink_component() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let target = directory.path().join("actual-evidence");
        let link = directory.path().join("linked-evidence");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_external_trace_error(
            prepare_external_trace_root(&link.join("trace"), workspace.root()),
            ExternalTraceErrorClass::LinkOrReparse,
        );
        assert!(!target.join("trace").exists());
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[cfg(windows)]
    fn create_test_junction(link: &Path, target: &Path) {
        let mut command = std::process::Command::new("cmd.exe");
        command
            .arg("/D")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target);
        let output = crate::test_process_containment::output_bounded(
            &mut command,
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert!(
            output.status.success(),
            "junction creation failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn external_trace_root_precreate_rejects_windows_junction() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let target = directory.path().join("actual-evidence");
        let junction = directory.path().join("junction-evidence");
        std::fs::create_dir(&target).unwrap();
        create_test_junction(&junction, &target);
        assert_external_trace_error(
            prepare_external_trace_root(&junction.join("trace"), workspace.root()),
            ExternalTraceErrorClass::LinkOrReparse,
        );
        assert!(!target.join("trace").exists());
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[cfg(windows)]
    #[test]
    fn external_trace_root_precreate_rejects_windows_case_alias() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let alias = workspace.root().with_file_name("WoRkSpAcE");
        assert_external_trace_error(
            prepare_external_trace_root(&alias, workspace.root()),
            ExternalTraceErrorClass::EqualWorkspace,
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn external_trace_root_precreate_rejection_matrix() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let descendant = workspace.root().join("new-traces");
        let file = directory.path().join("not-a-directory");
        std::fs::write(&file, "sentinel").unwrap();

        for (requested, expected) in [
            (
                workspace.root().to_path_buf(),
                ExternalTraceErrorClass::EqualWorkspace,
            ),
            (
                descendant.clone(),
                ExternalTraceErrorClass::DescendantOfWorkspace,
            ),
            (
                directory.path().to_path_buf(),
                ExternalTraceErrorClass::AncestorOfWorkspace,
            ),
            (file.clone(), ExternalTraceErrorClass::NotDirectory),
        ] {
            assert_external_trace_error(
                prepare_external_trace_root(&requested, workspace.root()),
                expected,
            );
        }
        assert!(!descendant.exists());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "sentinel");
        assert!(!workspace.root().join(".ferric").exists());
        assert!(!workspace.root().join("ferric-mock.txt").exists());
    }

    fn prepared_postcreate_root(
        directory: &tempfile::TempDir,
        workspace: &Workspace,
        name: &str,
    ) -> PreparedExternalTraceRoot {
        prepare_external_trace_root(&directory.path().join(name), workspace.root()).unwrap()
    }

    #[test]
    fn external_trace_root_postcreate_rejects_non_directory() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-file");
        let _hook = install_external_trace_post_create_hook(|created| {
            std::fs::remove_dir(created).unwrap();
            std::fs::write(created, "replacement").unwrap();
            created.to_path_buf()
        });
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::NotDirectory,
            "{error}"
        );
        assert!(!prepared.root.join("q-test.jsonl").exists());
    }

    #[test]
    fn external_trace_root_postcreate_rejects_equal() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-equal");
        let workspace_substitute = workspace.root().to_path_buf();
        let _hook = install_external_trace_post_create_hook(move |_| workspace_substitute);
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::EqualWorkspace,
            "{error}"
        );
        assert!(std::fs::read_dir(&prepared.root).unwrap().next().is_none());
    }

    #[test]
    fn external_trace_root_postcreate_rejects_descendant() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-descendant");
        let inside = workspace.root().join("substituted-traces");
        let _hook = install_external_trace_post_create_hook(move |_| {
            std::fs::create_dir(&inside).unwrap();
            inside
        });
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::DescendantOfWorkspace,
            "{error}"
        );
        assert!(std::fs::read_dir(&prepared.root).unwrap().next().is_none());
    }

    #[test]
    fn external_trace_root_postcreate_rejects_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-ancestor");
        let ancestor = directory.path().to_path_buf();
        let _hook = install_external_trace_post_create_hook(move |_| ancestor);
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::AncestorOfWorkspace,
            "{error}"
        );
        assert!(std::fs::read_dir(&prepared.root).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn external_trace_root_postcreate_rejects_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let target = directory.path().join("post-symlink-target");
        std::fs::create_dir(&target).unwrap();
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-symlink");
        let hook_target = target.clone();
        let _hook = install_external_trace_post_create_hook(move |created| {
            std::fs::remove_dir(created).unwrap();
            std::os::unix::fs::symlink(&hook_target, created).unwrap();
            created.to_path_buf()
        });
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::LinkOrReparse,
            "{error}"
        );
        assert!(!target.join("q-test.jsonl").exists());
    }

    #[cfg(windows)]
    #[test]
    fn external_trace_root_postcreate_rejects_windows_reparse() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let target = directory.path().join("post-junction-target");
        std::fs::create_dir(&target).unwrap();
        let prepared = prepared_postcreate_root(&directory, &workspace, "post-junction");
        let hook_target = target.clone();
        let _hook = install_external_trace_post_create_hook(move |created| {
            std::fs::remove_dir(created).unwrap();
            create_test_junction(created, &hook_target);
            created.to_path_buf()
        });
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(
            error.class,
            ExternalTraceErrorClass::LinkOrReparse,
            "{error}"
        );
        assert!(!target.join("q-test.jsonl").exists());
    }

    #[test]
    fn external_trace_root_postcreate_rejection_matrix() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);

        let file = prepared_postcreate_root(&directory, &workspace, "matrix-file");
        let _hook = install_external_trace_post_create_hook(|created| {
            std::fs::remove_dir(created).unwrap();
            std::fs::write(created, "replacement").unwrap();
            created.to_path_buf()
        });
        let error = materialize_external_trace_root(&file, workspace.root()).unwrap_err();
        assert_eq!(error.class, ExternalTraceErrorClass::NotDirectory);

        for (name, substituted, expected) in [
            (
                "matrix-equal",
                workspace.root().to_path_buf(),
                ExternalTraceErrorClass::EqualWorkspace,
            ),
            (
                "matrix-ancestor",
                directory.path().to_path_buf(),
                ExternalTraceErrorClass::AncestorOfWorkspace,
            ),
        ] {
            let prepared = prepared_postcreate_root(&directory, &workspace, name);
            let _hook = install_external_trace_post_create_hook(move |_| substituted);
            let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
            assert_eq!(error.class, expected);
            assert!(std::fs::read_dir(prepared.root).unwrap().next().is_none());
        }

        let descendant = workspace.root().join("matrix-inside");
        std::fs::create_dir(&descendant).unwrap();
        let prepared = prepared_postcreate_root(&directory, &workspace, "matrix-descendant");
        let _hook = install_external_trace_post_create_hook(move |_| descendant);
        let error = materialize_external_trace_root(&prepared, workspace.root()).unwrap_err();
        assert_eq!(error.class, ExternalTraceErrorClass::DescendantOfWorkspace);
        assert!(std::fs::read_dir(prepared.root).unwrap().next().is_none());
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn external_trace_postcreate_hook_is_one_shot_and_cleanup_safe() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);

        let first = prepared_postcreate_root(&directory, &workspace, "hook-first");
        let workspace_substitute = workspace.root().to_path_buf();
        let _consumed_guard =
            install_external_trace_post_create_hook(move |_| workspace_substitute);
        let first_error = materialize_external_trace_root(&first, workspace.root()).unwrap_err();
        assert_eq!(first_error.class, ExternalTraceErrorClass::EqualWorkspace);

        // The callback was consumed, even though its guard remains live. A
        // second call on the same test thread must take the production path.
        let second = prepared_postcreate_root(&directory, &workspace, "hook-second");
        let second_root = materialize_external_trace_root(&second, workspace.root()).unwrap();
        assert_eq!(second_root, std::fs::canonicalize(&second.root).unwrap());

        // Dropping an unconsumed guard clears its callback, so an early return
        // or panic in a test cannot contaminate a later materialization.
        {
            let _unconsumed_guard =
                install_external_trace_post_create_hook(|_| panic!("stale hook executed"));
        }
        let third = prepared_postcreate_root(&directory, &workspace, "hook-third");
        let third_root = materialize_external_trace_root(&third, workspace.root()).unwrap();
        assert_eq!(third_root, std::fs::canonicalize(&third.root).unwrap());
    }

    #[test]
    fn external_trace_resume_requires_and_reuses_explicit_root() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let source_root = directory.path().join("external-source");
        std::fs::create_dir(&source_root).unwrap();
        let source_trace = source_root.join("q-source.jsonl");
        std::fs::write(&source_trace, "source").unwrap();

        let omitted =
            validate_resume_trace_root(Some(&source_trace), workspace.root(), None).unwrap_err();
        assert_eq!(
            omitted.class,
            ExternalTraceErrorClass::ExplicitRootRequired,
            "{omitted}"
        );

        let same = prepare_external_trace_root(&source_root, workspace.root()).unwrap();
        validate_resume_trace_root(Some(&source_trace), workspace.root(), Some(&same)).unwrap();

        let different =
            prepare_external_trace_root(&directory.path().join("different"), workspace.root())
                .unwrap();
        let mismatch =
            validate_resume_trace_root(Some(&source_trace), workspace.root(), Some(&different))
                .unwrap_err();
        assert_eq!(
            mismatch.class,
            ExternalTraceErrorClass::ResumeRootMismatch,
            "{mismatch}"
        );
        assert!(
            !different.root.exists(),
            "mismatch must fail before creation"
        );
        assert!(!workspace.root().join(".ferric").exists());
    }

    #[test]
    fn default_source_resume_may_select_external_output() {
        let directory = tempfile::tempdir().unwrap();
        let (_, workspace) = trace_test_workspace(&directory);
        let default = workspace.root().join(".ferric").join("trace");
        std::fs::create_dir_all(&default).unwrap();
        let source_trace = default.join("q-source.jsonl");
        std::fs::write(&source_trace, "source").unwrap();
        let external =
            prepare_external_trace_root(&directory.path().join("external"), workspace.root())
                .unwrap();

        validate_resume_trace_root(Some(&source_trace), workspace.root(), Some(&external)).unwrap();
        assert!(!external.root.exists());
    }

    #[cfg(windows)]
    #[test]
    fn powershell_quote_round_trips_argv() {
        const ENTERED: &str = "ferric-argv-script-entered";
        const COMPLETE: &str = "ferric-argv-script-complete";
        let trace = PathBuf::from("C:\\trace dir\\quo'te\"$`;&.jsonl");
        let workspace = PathBuf::from("C:\\work dir\\quo'te\"$`;&");
        let root = PathBuf::from("C:\\evidence dir\\quo'te\"$`;&");
        let command = format_resume_command(&trace, &workspace, Some(&root), true, None);
        let script = format!(
            "[Console]::Error.WriteLine('{ENTERED}'); function ferric {{ foreach ($value in $args) {{ [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$value)) }} }}; {command}; [Console]::Error.WriteLine('{COMPLETE}')"
        );
        crate::test_process_containment::ensure_current_process_tree_is_contained()
            .expect("source harness containment before PowerShell admission");
        let mut command = std::process::Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(script)
            .stdin(std::process::Stdio::null());
        // Use the same capture/cleanup boundary directly so timeout evidence is
        // retained. The normal adapter converts it to a generic io::Error.
        // PowerShell cold-start on a loaded CI runner can exceed a tight budget
        // (observed script_entered=false at 10s on a post-merge Windows runner):
        // a real-parser round-trip needs headroom for process startup, not just
        // for quoting. The job's own timeout-minutes still bounds a true hang.
        let output = ferric_process::run_bounded(
            &mut command,
            std::time::Duration::from_secs(60),
            ferric_process::CapturePlan::head(64 * 1024, 64 * 1024),
        )
        .expect("bounded PowerShell capture and checked scope cleanup");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let entered = stderr.lines().any(|line| line == ENTERED);
        let complete = stderr.lines().any(|line| line == COMPLETE);
        // Retain one fixed-size stage summary even on success; libtest normally
        // hides captured prints. Raw command output remains failure-only.
        let _ = std::io::Write::write_fmt(
            &mut std::io::stderr(),
            format_args!(
                "PowerShell argv fixture: execution_wall={:?}, spawn_wall={:?}, script_entered={entered}, script_complete={complete}, timed_out={}\n",
                output.wall, output.spawn_wall, output.timed_out
            ),
        );
        assert!(
            output.status.is_some_and(|status| status.success()),
            "PowerShell failed after checked cleanup: status={:?}, timed_out={}, execution_wall={:?}, spawn_wall={:?}, script_entered={}, script_complete={}, stdout_bytes={}, stderr_bytes={}, stdout={:?}, stderr={:?}",
            output.status,
            output.timed_out,
            output.wall,
            output.spawn_wall,
            entered,
            complete,
            output.stdout.len(),
            output.stderr.len(),
            String::from_utf8_lossy(&output.stdout),
            stderr
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let observed: Vec<&str> = stdout.lines().collect();
        let expected = [
            "query",
            "--resume",
            trace.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--trace-dir",
            root.to_str().unwrap(),
            "--answer",
            "<answer>",
        ];
        let expected: Vec<String> = expected
            .iter()
            .map(|value| ferric_core::base64_encode(value.as_bytes()))
            .collect();
        assert_eq!(observed, expected);
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_sh_quote_round_trips_argv() {
        let trace = PathBuf::from("/tmp/trace dir/quo'te\"$`;&.jsonl");
        let workspace = PathBuf::from("/tmp/work dir/quo'te\"$`;&");
        let root = PathBuf::from("/tmp/evidence dir/quo'te\"$`;&");
        let command = format_resume_command(&trace, &workspace, Some(&root), true, None);
        let script =
            format!("ferric() {{ for value do printf '%s\\0' \"$value\"; done; }}; {command}");
        let mut command = std::process::Command::new("/bin/sh");
        command.arg("-c").arg(script);
        let output = crate::test_process_containment::output_bounded(
            &mut command,
            std::time::Duration::from_secs(10),
        )
        .unwrap();
        assert!(
            output.status.success(),
            "sh failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let observed: Vec<&[u8]> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .collect();
        let expected = [
            "query",
            "--resume",
            trace.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--trace-dir",
            root.to_str().unwrap(),
            "--answer",
            "<answer>",
        ];
        let expected: Vec<&[u8]> = expected.iter().map(|value| value.as_bytes()).collect();
        assert_eq!(observed, expected);
    }

    #[test]
    fn trace_allocator_never_appends_same_millisecond_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let (first_id, first_path, mut first) = create_trace_sink(dir.path(), "api").unwrap();
        let (second_id, second_path, mut second) = create_trace_sink(dir.path(), "api").unwrap();
        assert_ne!(first_id, second_id);
        assert_ne!(first_path, second_path);
        first
            .write_event(Event::Note {
                text: "first".to_string(),
            })
            .unwrap();
        second
            .write_event(Event::Note {
                text: "second".to_string(),
            })
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(first_path).unwrap().lines().count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(second_path)
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    /// Each `--sink-action` spelling maps to the policy it names — the security
    /// control must not drift from its label.
    #[test]
    fn sink_action_maps_to_the_named_policy() {
        assert_eq!(
            SinkActionArg::RequireApproval.into_policy(),
            ferric_guard::SinkPolicy::require_approval()
        );
        assert_eq!(
            SinkActionArg::Deny.into_policy(),
            ferric_guard::SinkPolicy::deny()
        );
        assert_eq!(
            SinkActionArg::Warn.into_policy(),
            ferric_guard::SinkPolicy::new(ferric_guard::SinkAction::Warn)
        );
    }

    /// The point of making this a `ValueEnum`: a typo is **rejected**, not
    /// silently treated as `requireapproval` (which is what the old free-form
    /// `String` match did). The canonical spellings and the `require-approval`
    /// alias still parse.
    #[test]
    fn sink_action_accepts_known_spellings_and_rejects_typos() {
        use clap::ValueEnum;
        for ok in ["requireapproval", "require-approval"] {
            assert_eq!(
                SinkActionArg::from_str(ok, false),
                Ok(SinkActionArg::RequireApproval),
                "{ok} should parse"
            );
        }
        assert_eq!(
            SinkActionArg::from_str("deny", false),
            Ok(SinkActionArg::Deny)
        );
        assert_eq!(
            SinkActionArg::from_str("warn", false),
            Ok(SinkActionArg::Warn)
        );
        // A near-miss for a security control must fail loudly, not default.
        assert!(SinkActionArg::from_str("deni", false).is_err());
    }

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

    #[test]
    fn route_files_resolves_inside_workspace_and_routes_text_and_media() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "demo notes").unwrap();
        std::fs::write(dir.path().join("photo.png"), [0_u8, 1, 2]).unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();

        let (media, suffix) = route_files(
            &workspace,
            &[PathBuf::from("notes.md"), PathBuf::from("photo.png")],
            &[ferric_core::Modality::Image],
            true,
        )
        .unwrap();

        assert!(suffix.contains("--- file: notes.md ---"));
        assert!(suffix.contains("demo notes"));
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].mime, "image/png");
        assert_eq!(media[0].data, "AAEC");
    }

    #[test]
    fn route_files_enforces_read_guard_and_ferricignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();
        std::fs::write(dir.path().join("ignored.md"), "private notes").unwrap();
        std::fs::write(dir.path().join(".ferricignore"), "ignored.md\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();

        let sensitive = route_files(&workspace, &[PathBuf::from(".env")], &[], false).unwrap_err();
        assert!(sensitive.contains("denied_read_file"), "{sensitive}");

        let ignored =
            route_files(&workspace, &[PathBuf::from("ignored.md")], &[], false).unwrap_err();
        assert!(ignored.contains("ferricignore"), "{ignored}");
    }

    #[test]
    fn route_files_enforces_per_file_and_aggregate_byte_limits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("large.txt"), "12345").unwrap();
        std::fs::write(dir.path().join("first.txt"), "1234").unwrap();
        std::fs::write(dir.path().join("second.txt"), "567").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();

        let per_file = route_files_with_limits(
            &workspace,
            &[PathBuf::from("large.txt")],
            &[],
            false,
            AttachmentLimits {
                per_file: 4,
                total: 16,
            },
        )
        .unwrap_err();
        assert!(per_file.contains("per-file limit is 4 bytes"), "{per_file}");

        let aggregate = route_files_with_limits(
            &workspace,
            &[PathBuf::from("first.txt"), PathBuf::from("second.txt")],
            &[],
            false,
            AttachmentLimits {
                per_file: 4,
                total: 6,
            },
        )
        .unwrap_err();
        assert!(aggregate.contains("6-byte aggregate limit"), "{aggregate}");
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
            harness_policy: None,
            sampling,
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
        let outcome = futures_executor::block_on(run_with_provider(
            setup.into_run_args(&provider, None),
            &mut sink,
            Some("do a mock task"),
        ))
        .unwrap();

        assert_eq!(outcome.stop, ferric_loop::StopReason::TaskComplete);
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
            params_b: 8.0,
            quant: "Q4_K_M".to_string(),
            family: "unknown".to_string(),
            ctx: 4096,
            // Non-default on purpose (test-critique C-003): 0.0 is also
            // `SamplingParams::default()`'s value, so a mis-wired temperature
            // would pass a same-value assertion vacuously.
            temperature: 0.7,
            protocol_override: None,
            harness_policy: None,
            prompts_dir: None,
            max_ring: None,
            tier_override: None,
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
        let config = build_run_config(&a).unwrap();

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
        let config = build_run_config(&base_run_config_args()).unwrap();
        let (protocol_1, max_ring_1) = (config.protocol, config.policy.max_ring);
        let (protocol_2, max_ring_2) = (config.protocol, config.policy.max_ring);
        assert_eq!(protocol_1, protocol_2);
        assert_eq!(max_ring_1, max_ring_2);
    }

    #[test]
    fn checks_file_parses_fixed_argv_and_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.toml");
        std::fs::write(
            &path,
            r#"
                [[check]]
                name = "unit"
                program = "cargo"
                args = ["test", "--workspace"]
            "#,
        )
        .unwrap();

        let checks = load_named_checks(&path).unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "unit");
        assert_eq!(checks[0].args, ["test", "--workspace"]);
        assert_eq!(checks[0].timeout_s, 120);
        assert_eq!(checks[0].output_limit, 4_000);
    }

    #[test]
    fn checks_file_is_explicit_and_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let empty = directory.path().join("empty.toml");
        std::fs::write(&empty, "# no checks\n").unwrap();
        assert!(
            load_named_checks(&empty)
                .unwrap_err()
                .contains("no [[check]]")
        );

        let unknown = directory.path().join("unknown.toml");
        std::fs::write(
            &unknown,
            "[[check]]\nname='unit'\nprogram='cargo'\ncommand='arbitrary'\n",
        )
        .unwrap();
        assert!(
            load_named_checks(&unknown)
                .unwrap_err()
                .contains("unknown field")
        );
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
