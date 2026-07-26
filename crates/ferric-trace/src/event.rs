use serde::{Deserialize, Serialize};

/// Version stamped into every trace line. Bump on breaking schema change;
/// readers must keep accepting unknown event types regardless (ADR-002).
pub const TRACE_SCHEMA_VERSION: u32 = 1;

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
    /// The run policy chosen for this session (benchmark parity: tier and
    /// protocol are otherwise invisible to trace consumers).
    PolicySelected {
        tier: ferric_core::Tier,
        protocol: ferric_core::ActionProtocol,
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
