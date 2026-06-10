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

/// The s0 event vocabulary — deliberately minimal. Prompt-assembly and
/// grammar-state events are *reserved names* to be defined in s1 when real
/// content exists; readers already tolerate them via `ParsedEvent::Unknown`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionStart {
        workspace: String,
    },
    SessionEnd {
        reason: String,
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
