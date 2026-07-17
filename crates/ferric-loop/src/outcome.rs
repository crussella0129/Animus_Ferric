/// Why the loop stopped. Every variant maps 1:1 onto a `SessionEnd` reason
/// string in the trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model produced a text-only completion.
    FinalText,
    /// The model called the `task_complete` structured terminator.
    TaskComplete,
    /// The model called the `submit_plan` terminating tool in Plan mode.
    PlanSubmitted,
    /// The policy's turn budget ran out.
    MaxTurns,
    /// The repetition guard stopped a stuck loop.
    RepetitionGuard,
    /// The no-progress guard stopped a same-tool-name flail (different args
    /// each turn — the mode the repetition guard misses, ADR-031/037).
    NoProgress,
    /// The repeated-failure guard stopped a model whose tool calls all errored
    /// for several turns in a row (ADR-038).
    RepeatedFailure,
    /// The provider failed permanently (or retries were exhausted).
    ProviderError,
    /// Two consecutive completions carried neither text nor tool calls
    /// (native) or failed to parse into an action (grammar).
    EmptyCompletion,
    /// Two consecutive grammar completions were cut off by the token budget
    /// (`finish_reason == "length"`) — the one malformed-action case the
    /// constraint cannot prevent (ADR-015).
    TruncatedAction,
    /// The user gracefully aborted execution (e.g. via Ctrl-C).
    Interrupted,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::FinalText => "final_text",
            StopReason::TaskComplete => "task_complete",
            StopReason::PlanSubmitted => "plan_submitted",
            StopReason::MaxTurns => "max_turns",
            StopReason::RepetitionGuard => "repetition_guard",
            StopReason::NoProgress => "no_progress",
            StopReason::RepeatedFailure => "repeated_failure",
            StopReason::ProviderError => "provider_error",
            StopReason::EmptyCompletion => "empty_completion",
            StopReason::TruncatedAction => "truncated_action",
            StopReason::Interrupted => "interrupted",
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
