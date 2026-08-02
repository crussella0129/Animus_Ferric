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
    /// The oscillation guard stopped an A-B-A-B cycle — a small set of distinct
    /// actions repeated across a window of turns. The mode all three
    /// streak-based guards miss, because alternation resets each of them
    /// (ADR-077).
    Oscillation,
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
    /// A required hook script failed.
    HookFailed,
}

impl StopReason {
    /// Whether this stop represents a completed user request.
    ///
    /// Guard trips, exhausted budgets, interrupts, malformed output, hook
    /// failures, and provider failures all leave work incomplete and must not
    /// be surfaced as a successful CLI/MCP/API result.
    pub fn is_success(self) -> bool {
        matches!(
            self,
            StopReason::FinalText | StopReason::TaskComplete | StopReason::PlanSubmitted
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::FinalText => "final_text",
            StopReason::TaskComplete => "task_complete",
            StopReason::PlanSubmitted => "plan_submitted",
            StopReason::MaxTurns => "max_turns",
            StopReason::RepetitionGuard => "repetition_guard",
            StopReason::NoProgress => "no_progress",
            StopReason::RepeatedFailure => "repeated_failure",
            StopReason::Oscillation => "oscillation",
            StopReason::ProviderError => "provider_error",
            StopReason::EmptyCompletion => "empty_completion",
            StopReason::TruncatedAction => "truncated_action",
            StopReason::Interrupted => "interrupted",
            StopReason::HookFailed => "hook_failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StopReason;

    #[test]
    fn only_clean_completion_reasons_are_successful() {
        for stop in [
            StopReason::FinalText,
            StopReason::TaskComplete,
            StopReason::PlanSubmitted,
        ] {
            assert!(stop.is_success(), "{stop:?}");
        }

        for stop in [
            StopReason::MaxTurns,
            StopReason::RepetitionGuard,
            StopReason::NoProgress,
            StopReason::RepeatedFailure,
            StopReason::Oscillation,
            StopReason::ProviderError,
            StopReason::EmptyCompletion,
            StopReason::TruncatedAction,
            StopReason::Interrupted,
            StopReason::HookFailed,
        ] {
            assert!(!stop.is_success(), "{stop:?}");
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
