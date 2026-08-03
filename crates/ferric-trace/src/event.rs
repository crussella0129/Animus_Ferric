use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Version stamped into every trace line. Bump on breaking schema change;
/// readers must keep accepting unknown event types regardless (ADR-002).
pub const TRACE_SCHEMA_VERSION: u32 = 1;

/// Version of the payload carried by [`Event::RecoveryCheckpoint`].
///
/// Checkpoint payloads evolve independently from the long-lived JSONL
/// envelope. A reader must reject checkpoint versions it does not understand.
pub const RECOVERY_CHECKPOINT_VERSION: u32 = 1;

/// Version shared by the typed observation/block/effect/check records added by
/// the evidence controller. These records are additive trace vocabulary; their
/// payload version evolves independently from the JSONL envelope.
pub const CONTROLLER_RECORD_VERSION: u32 = 1;

/// Version of the payload carried by [`Event::ControllerCheckpoint`].
pub const CONTROLLER_CHECKPOINT_VERSION: u32 = 1;

/// Version of the payload carried by [`Event::RecoveryPacketInjected`].
pub const RECOVERY_PACKET_VERSION: u32 = 1;

/// The model-message index at which one committed turn begins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnBoundary {
    pub turn: u32,
    pub message_index: usize,
}

/// One action-bearing turn retained to reconstruct loop-guard state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardTurn {
    pub turn: u32,
    pub calls: Vec<ferric_core::ToolCall>,
    pub dispatched: u32,
    pub errored: u32,
}

/// One inclusive, one-indexed range of lines shown to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRangeV1 {
    pub start: u64,
    pub end: u64,
}

/// Literal optional range arguments supplied to a file read. Keeping the two
/// bounds independent distinguishes start-only, end-only, both, and neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedLineRangeV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

/// Exact metadata for a file read. The digest always describes the complete
/// file bytes, while `returned_range` describes only the content shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileObservationV1 {
    pub path: String,
    pub sha256: String,
    pub total_bytes: u64,
    pub total_lines: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_range: Option<RequestedLineRangeV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_range: Option<LineRangeV1>,
    /// True only when the complete current file was shown to the model.
    pub complete: bool,
    /// True when the registry's model-facing output cap removed any content.
    pub model_truncated: bool,
}

/// Exact metadata for literal repository navigation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationObservationV1 {
    pub root: String,
    pub literal: String,
    pub match_count: u64,
    pub max_results: u64,
    /// True when traversal reached the end rather than stopping at the cap.
    pub exhausted: bool,
    pub result_sha256: String,
}

/// The typed result behind one successful observation tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ObservationDetailV1 {
    File(FileObservationV1),
    Search(NavigationObservationV1),
    Find(NavigationObservationV1),
}

/// Versioned observation record carried by [`Event::ObservationRecorded`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationV1 {
    pub version: u32,
    pub detail: ObservationDetailV1,
}

/// Machine-readable reason an evidence-controller call did not reach commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerBlockReason {
    BlindMutation,
    SameTurnObservation,
    StaleObservation,
    UnsupportedMutation,
    RepairInspectionRequired,
    NoEffect,
    SyntaxRegression,
    RepeatedCheck,
}

/// Redacted identity of one path while a call was prepared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreparedPathIdentityV1 {
    Absent,
    File { sha256: String, bytes: u64 },
    Directory,
    Other,
}

/// Before/candidate identities proving a path-level no-effect refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedPathStateV1 {
    pub path: String,
    pub before: PreparedPathIdentityV1,
    pub candidate: PreparedPathIdentityV1,
}

/// Which typed preparation boundary proved a mutation was unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedMutationKindV1 {
    OpaqueMutation,
    UnsupportedOperation,
}

/// Syntax status measured for a mutation preimage or candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxStateV1 {
    Absent,
    Valid,
    Invalid,
    Unchecked,
}

/// Typed proof for refusals whose cause cannot be reconstructed from the
/// observation/check ledger alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ControllerBlockWitnessV1 {
    StaleObservation {
        expected: PreparedPathIdentityV1,
        current: PreparedPathIdentityV1,
    },
    NoEffect {
        states: Vec<PreparedPathStateV1>,
    },
    SyntaxRegression {
        before: SyntaxStateV1,
        candidate: SyntaxStateV1,
        diagnostic_sha256: String,
    },
    UnsupportedMutation {
        control_kind: UnsupportedMutationKindV1,
    },
}

/// Versioned controller-admission refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerBlockV1 {
    pub version: u32,
    pub reason: ControllerBlockReason,
    pub mutation_epoch: u64,
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_name: Option<String>,
    /// Typed witness for causes that cannot be derived from controller state.
    /// Ledger-derived refusals omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness: Option<ControllerBlockWitnessV1>,
}

/// How one path changed during a measured workspace effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathEffectKind {
    Created,
    Modified,
    Deleted,
    /// A directory came into existence (`make_dir`, or the destination of a
    /// directory move). Structural effects carry no content digest.
    CreatedDirectory,
    /// A directory ceased to exist (`delete_path` on an empty directory, or the
    /// source of a directory move).
    DeletedDirectory,
    Opaque,
}

/// Before/after identity for one path touched by a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathEffectV1 {
    pub path: String,
    pub kind: PathEffectKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_sha256: Option<String>,
    /// Exact postimage size for effects that leave a file present. Paired with
    /// `after_lines`; old typed-event fixtures omit both safely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_bytes: Option<u64>,
    /// Exact logical-line count for effects that leave a file present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_lines: Option<u64>,
}

/// Versioned, measured workspace effect. One call advances the epoch once even
/// when it changes more than one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEffectV1 {
    pub version: u32,
    pub mutation_epoch: u64,
    pub effects: Vec<PathEffectV1>,
}

/// Outcome of a named check process that actually executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Passed,
    Failed,
}

/// Versioned named-check execution record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCheckV1 {
    pub version: u32,
    pub name: String,
    pub mutation_epoch: u64,
    pub attempt: u32,
    pub outcome: VerificationOutcome,
    /// Required for failed checks; absent for a passing check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_sha256: Option<String>,
}

/// Why the controller considers file content known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileEvidenceOrigin {
    ModelRead,
    AuthoredMutation,
}

/// File-evidence ledger entry persisted across process segments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEvidenceV1 {
    pub path: String,
    pub sha256: String,
    pub total_bytes: u64,
    pub total_lines: u64,
    pub covered_ranges: Vec<LineRangeV1>,
    pub complete: bool,
    pub fresh: bool,
    pub observed_turn: u32,
    pub origin: FileEvidenceOrigin,
}

/// One real named-check execution retained in the controller checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckExecutionV1 {
    pub turn: u32,
    pub name: String,
    pub mutation_epoch: u64,
    pub attempt: u32,
    pub outcome: VerificationOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_sha256: Option<String>,
}

/// The latest failed named check, formatted directly into recovery guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedCheckV1 {
    pub turn: u32,
    pub name: String,
    pub mutation_epoch: u64,
    pub attempt: u32,
    pub diagnostic_sha256: String,
}

/// Evidence-controller state stored separately from the legacy recovery
/// checkpoint. No scalar or collection field has a serde default: an evidence
/// checkpoint is either complete for its declared version or rejected by its
/// consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerCheckpointV1 {
    pub version: u32,
    pub harness_policy: ferric_core::HarnessPolicy,
    pub mutation_epoch: u64,
    pub required_checks: Vec<String>,
    pub passed_checks: BTreeMap<String, u64>,
    pub file_evidence: Vec<FileEvidenceV1>,
    pub check_executions: Vec<CheckExecutionV1>,
    pub last_failed_check: Option<FailedCheckV1>,
    pub changed_paths: Vec<String>,
    pub repair_paths: Vec<String>,
    pub repair_observation_after_turn: Option<u32>,
    pub inherited_pause_reason: Option<String>,
}

/// Typed facts injected into a non-clarification continuation. `message` lives
/// on the event itself so later render changes cannot alter replayed history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPacketV1 {
    pub version: u32,
    pub pause_reason: String,
    pub mutation_epoch: u64,
    pub required_checks: Vec<String>,
    pub passed_checks: BTreeMap<String, u64>,
    pub last_failed_check: Option<FailedCheckV1>,
    pub changed_paths: Vec<String>,
    pub reread_paths: Vec<String>,
}

/// A self-contained base for a session that resumes another trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryCheckpointV1 {
    pub version: u32,
    pub messages: Vec<ferric_core::Message>,
    /// Absolute identifier for the next turn across a resume chain.
    pub next_turn: u32,
    pub last_text: Option<String>,
    pub head_len: usize,
    pub committed_turn_starts: Vec<TurnBoundary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guard_history: Vec<GuardTurn>,
    pub nudged_for_no_action: bool,
    pub truncated_once: bool,
    pub last_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_input: Option<ferric_core::UserInputRequest>,
    /// Successful mutating calls advance the epoch; check evidence is fresh
    /// only when it was recorded at the current value.
    #[serde(default)]
    pub mutation_epoch: u64,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub passed_checks: std::collections::BTreeMap<String, u64>,
}

/// Serde default for `PolicySelected.tier_source` on pre-ADR-098 traces.
fn default_tier_source() -> String {
    ferric_core::TierSource::Params.label().to_string()
}

/// The envelope written as one JSONL line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub v: u32,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: u64,
    pub session: String,
    /// Monotonic per-session sequence number, assigned by the sink.
    pub seq: u64,
    pub event: Event,
}

/// The trace event vocabulary. s0 shipped the session/tool events; s1 added
/// the turn-level loop events. Readers tolerate unknown variants via
/// `ParsedEvent::Unknown`, so this enum grows additively (ADR-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionStart {
        workspace: String,
        /// The prior session's `session` id, when this run continues an
        /// interrupted one (sprint 39, ADR-049). `None` for every ordinary
        /// (non-resumed) run. Additive: an old `session_start` line with no
        /// key here still parses as `Known` with `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resumed_from: Option<String>,
    },
    SessionEnd {
        reason: String,
    },
    /// Marks an incomplete, intentionally resumable stop. New writers emit
    /// `SessionEnd`, then a recovery checkpoint, then this event. Keeping
    /// `SessionEnd` first makes pre-recovery readers fail closed while
    /// recovery-aware readers recognize the terminal suffix as a pause.
    SessionPaused {
        reason: String,
    },
    /// The run policy chosen for this session (benchmark parity: tier and
    /// protocol are otherwise invisible to trace consumers).
    PolicySelected {
        tier: ferric_core::Tier,
        protocol: ferric_core::ActionProtocol,
        /// Autonomous-controller behavior. Additive: every trace written
        /// before Sprint 113 omitted this key and therefore means `legacy`.
        #[serde(default)]
        harness_policy: ferric_core::HarnessPolicy,
        max_turns: u32,
        max_tools: u32,
        prompt_budget_tokens: u32,
        max_output_tokens: u32,
        /// Characters of a single tool result this run showed the model
        /// (ADR-002's cap). Recorded because everything that rebuilds a
        /// context window from a trace — `replay`, `trace verify` — has only
        /// the trace to work from, and without this key it had to assume the
        /// default while `run()` used whatever the registry was configured
        /// with. Additive: a `policy_selected` line written before ADR-093
        /// parses as `Known` with the default, which is what those runs used.
        #[serde(default = "ferric_core::default_truncation_limit")]
        truncation_limit: usize,
        /// Why this run is at this `tier`: `"measured"` (earned on the L0–L6
        /// ladder), `"params"` (the parameter-count prior), or `"override"`
        /// (the operator asked for it). ADR-098.
        ///
        /// The tier governs turn/tool budgets, prompt and output ceilings,
        /// planner use, subagents and the tool-ring ceiling, so recording only
        /// the answer left a tier a model *earned* and a tier someone *asked
        /// for* looking identical afterwards. Additive: a `policy_selected`
        /// line written before ADR-098 reads back as `"params"`, which is what
        /// those runs used whenever no profile was found — and where one was,
        /// the profile store still holds the measurement.
        #[serde(default = "default_tier_source")]
        tier_source: String,
    },
    /// Prompt-composition genealogy (oovra lineage): which versioned elements
    /// built the system prompt.
    PromptComposed {
        output_id: String,
        output_version: String,
        composed_of: Vec<(String, String)>,
    },
    /// The literal system + user prompt (and any attached media) turn 0's
    /// request was built from (sprint 39, ADR-049) — written once per
    /// session, before `TurnStart(0)`, unless this session IS a resume (its
    /// initial prompt already lives in the session it resumed from). Closes
    /// the gap where only derived metadata (`PromptComposed`'s lineage,
    /// `PromptAssembled`'s char count) was ever traced, never the actual text
    /// — needed to losslessly replay a session's message history.
    SessionPrompt {
        system: String,
        user: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<ferric_core::MediaPart>,
    },
    /// Self-contained inherited state at the start of a resumed session.
    RecoveryCheckpoint {
        state: RecoveryCheckpointV1,
    },
    /// Additional user input supplied while resuming. It amends the original
    /// objective; it never replaces the session prompt.
    ResumePrompt {
        user: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<ferric_core::MediaPart>,
    },
    TurnStart {
        turn: u32,
    },
    /// Closes the s0 gap where assistant text was never traced: every turn's
    /// completion is recorded, including the text of non-final turns.
    TurnEnd {
        turn: u32,
        text: Option<String>,
        tool_call_count: u32,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        /// Whether this turn's completion was cut off by the token budget
        /// (sprint 39, ADR-049) — needed to replay the truncation-retry nudge
        /// correctly. Additive: an old line with no key here parses as
        /// `false`.
        #[serde(default)]
        truncated: bool,
    },
    /// Complete action batch decoded before guard evaluation or dispatch.
    ActionsProposed {
        turn: u32,
        calls: Vec<ferric_core::ToolCall>,
    },
    /// What was about to be sent to the provider: size, shape, and which
    /// tools were on offer.
    PromptAssembled {
        turn: u32,
        message_count: u32,
        chars: u64,
        offered_tools: Vec<String>,
    },
    /// A decoding constraint was attached to the request (`kind` names the
    /// Constraint variant, e.g. "json_schema").
    ConstraintApplied {
        kind: String,
    },
    /// The repetition guard fired. `action` is "warned" or "stopped".
    RepetitionGuard {
        action: String,
    },
    /// The no-progress guard fired on a same-tool-name flail (different args
    /// each turn — the mode the repetition guard misses, ADR-031/037).
    /// `action` is "warned" or "stopped".
    NoProgressGuard {
        action: String,
    },
    /// The repeated-failure guard fired — the model's tool calls all errored for
    /// several turns in a row (ADR-038). `action` is "warned" or "stopped".
    FailureGuard {
        action: String,
    },
    /// The oscillation guard fired — a small set of distinct actions repeated
    /// across a window of turns (an A-B-A-B cycle), which every streak-based
    /// guard misses because alternation resets them (ADR-077). `action` is
    /// "warned" or "stopped".
    OscillationGuard {
        action: String,
    },
    /// A guard decision made at the tool-dispatch chokepoint. `rule` and
    /// `matched` are present on denials.
    PermissionCheck {
        path: String,
        decision: String,
        rule: Option<String>,
        matched: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    /// `output` is the FULL, untruncated tool output. The model may see a
    /// truncated copy; the trace never does (ADR-002).
    ToolResult {
        id: String,
        name: String,
        output: String,
        is_error: bool,
        duration_ms: u64,
    },
    /// A successful observation tool returned machine-readable evidence in
    /// addition to its human/model-facing [`Event::ToolResult`].
    ObservationRecorded {
        turn: u32,
        call_id: String,
        observation: ObservationV1,
    },
    /// The evidence controller refused a call before any effect was committed.
    ControllerBlocked {
        turn: u32,
        call_id: String,
        tool: String,
        block: ControllerBlockV1,
    },
    /// Measured before/after effects for one call. Unlike the legacy
    /// `WorkspaceMutation`, this can coexist with an errored tool result when a
    /// handler partially changed the workspace before failing.
    WorkspaceEffectRecorded {
        turn: u32,
        call_id: String,
        tool: String,
        effect: WorkspaceEffectV1,
    },
    /// One named check process that actually executed. A repeated same-epoch
    /// attempt is represented by `ControllerBlocked`, never by this event.
    VerificationCheckRecorded {
        turn: u32,
        call_id: String,
        check: VerificationCheckV1,
    },
    /// A successful mutating tool advanced the workspace evidence epoch.
    WorkspaceMutation {
        turn: u32,
        tool: String,
        mutation_epoch: u64,
    },
    /// One operator-authorized check passed at the named workspace epoch.
    VerificationCheckPassed {
        turn: u32,
        name: String,
        mutation_epoch: u64,
    },
    /// Evidence-controller state, deliberately separate from
    /// `RecoveryCheckpointV1` so legacy absence cannot default into trusted
    /// safety state.
    ControllerCheckpoint {
        state: ControllerCheckpointV1,
    },
    /// Literal model-facing recovery guidance plus the typed facts from which
    /// it was rendered.
    RecoveryPacketInjected {
        packet: RecoveryPacketV1,
        message: String,
    },
    /// Durable barrier proving that one turn finished processing.
    TurnCommitted {
        turn: u32,
        dispatched: u32,
        errored: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot_commit: Option<String>,
    },
    /// Evidence used when accepting or rejecting an attempted completion.
    CompletionGate {
        mutation_epoch: u64,
        required_checks: Vec<String>,
        fresh_checks: Vec<String>,
        /// `passed` or `blocked`.
        decision: String,
    },
    Note {
        text: String,
    },
    /// Older turns were folded into one synthetic summary message because
    /// `input_tokens` crossed the context-budget trigger fraction (sprint 40,
    /// ADR-050). `through_turn` names the highest-numbered folded turn
    /// (absolute turn number, matching `TurnStart.turn`); turns numbered
    /// `<= through_turn` are represented solely by `summary` from this point
    /// on. `dropped_turns` is the count folded THIS round (informational —
    /// `through_turn` is what a reader needs to reconstruct state; repeated
    /// compactions each supersede the prior one, so only the LATEST
    /// `HistoryCompacted` in a trace matters for reconstruction). A brand-new
    /// variant, not an extension of an existing one — no `#[serde(default)]`
    /// needed (old readers already tolerate unknown variants, ADR-002).
    HistoryCompacted {
        through_turn: u32,
        dropped_turns: u32,
        summary: String,
    },
}
