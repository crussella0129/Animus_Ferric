/// Why the loop stopped. Every variant maps 1:1 onto a `SessionEnd` reason
/// string in the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model produced a text-only completion.
    FinalText,
    /// The model called the `task_complete` structured terminator.
    TaskComplete,
    /// The policy's turn budget ran out.
    MaxTurns,
    /// The repetition guard stopped a stuck loop.
    RepetitionGuard,
    /// The provider failed permanently (or retries were exhausted).
    ProviderError,
    /// Two consecutive completions carried neither text nor tool calls
    /// (native) or failed to parse into an action (grammar).
    EmptyCompletion,
    /// Two consecutive grammar completions were cut off by the token budget
    /// (`finish_reason == "length"`) — the one malformed-action case the
    /// constraint cannot prevent (ADR-015).
    TruncatedAction,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::FinalText => "final_text",
            StopReason::TaskComplete => "task_complete",
            StopReason::MaxTurns => "max_turns",
            StopReason::RepetitionGuard => "repetition_guard",
            StopReason::ProviderError => "provider_error",
            StopReason::EmptyCompletion => "empty_completion",
            StopReason::TruncatedAction => "truncated_action",
        }
    }
}

/// The result of a loop run. `final_text` is best-effort on non-clean stops
/// (the last assistant text, so callers never end up with nothing to show).
#[derive(Debug, Clone, PartialEq)]
pub struct LoopOutcome {
    pub final_text: Option<String>,
    pub stop: StopReason,
    pub turns: u32,
}
