use serde::{Deserialize, Serialize};

/// Version stamped into every trace line. Bump on breaking schema change;
/// readers must keep accepting unknown event types regardless (ADR-002).
pub const TRACE_SCHEMA_VERSION: u32 = 1;

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
    },
    /// Prompt-composition genealogy (oovra lineage): which versioned elements
    /// built the system prompt.
    PromptComposed {
        output_id: String,
        output_version: String,
        composed_of: Vec<(String, String)>,
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
}
