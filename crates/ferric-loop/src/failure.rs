//! Repeated-failure guard — the third loop-hardening guard (ADR-038).
//!
//! The repetition guard (`repetition.rs`) and the no-progress guard
//! (`progress.rs`) both key off the *actions* a model emits — identical
//! signatures, and same-tool-name streaks. Neither keys off whether those
//! actions *work*. A model can emit a DIFFERENT tool every turn that ALL error
//! (wrong paths, denied permissions, malformed args) and never recover: the
//! repetition guard resets (different signature), the no-progress guard resets
//! (different name), so it grinds to `max_turns`.
//!
//! This guard is the complement that keys off tool *results*: it counts
//! consecutive turns whose dispatched tools ALL errored, and stops the loop
//! after a short streak. Scope (ADR-031/037): it does NOT make a weak model
//! complete a task — it bounds wasted compute on a model stuck failing, and
//! emits a precise `repeated_failure` diagnostic distinct from `max_turns`.

use crate::repetition::Verdict;

/// Warn after this many consecutive all-error turns — one course-correction
/// chance before the stop.
const WARN_AT: u8 = 2;
/// Stop at this many. Tighter than the no-progress streak: a *failing* streak
/// rarely self-corrects past a nudge, so a faster stop saves more compute.
const STOP_AT: u8 = 3;

/// Derive the result-bearing execution observation used by [`FailureGuard`]
/// while preserving raw trace counts.
///
/// Typed controller blocks are refusals before useful execution, so a turn
/// containing only those blocks returns `None` and preserves the existing
/// streak. A blocked `task_complete` remains an explicit control transition:
/// when no real calls remain it returns `Some((0, 0))` and resets the streak.
pub(crate) fn failure_observation(
    raw_dispatched: usize,
    raw_errored: usize,
    controller_blocks: usize,
    completion_was_blocked: bool,
) -> Option<(usize, usize)> {
    let Some(excluded) = controller_blocks.checked_add(usize::from(completion_was_blocked)) else {
        let failed = raw_dispatched.max(raw_errored).max(1);
        return Some((failed, failed));
    };
    if raw_errored > raw_dispatched || excluded > raw_dispatched || excluded > raw_errored {
        // Direct `ReplayedState` callers need the same fail-closed behavior as
        // validated trace replay. Never let malformed attribution subtract
        // away failures or reset the streak.
        let failed = raw_dispatched.max(raw_errored).max(1);
        return Some((failed, failed));
    }
    let dispatched = raw_dispatched - excluded;
    let errored = raw_errored - excluded;
    if dispatched > 0 || completion_was_blocked {
        Some((dispatched, errored))
    } else {
        None
    }
}

pub struct FailureGuard {
    consecutive_failed_turns: u8,
}

impl FailureGuard {
    pub fn new() -> Self {
        Self {
            consecutive_failed_turns: 0,
        }
    }

    /// Observe one turn's dispatch outcome. `dispatched` is the number of
    /// (non-terminator) tools the turn executed; `errored` how many returned an
    /// error. A turn is a "failure turn" iff it dispatched at least one tool and
    /// EVERY one errored — any success is partial progress and resets the streak.
    /// A zero-dispatch turn is not result-bearing and never trips the guard.
    pub fn observe_turn(&mut self, dispatched: usize, errored: usize) -> Verdict {
        let all_failed = dispatched > 0 && errored == dispatched;
        if !all_failed {
            self.consecutive_failed_turns = 0;
            return Verdict::Proceed;
        }
        self.consecutive_failed_turns = self.consecutive_failed_turns.saturating_add(1);
        if self.consecutive_failed_turns >= STOP_AT {
            Verdict::Stop
        } else if self.consecutive_failed_turns >= WARN_AT {
            Verdict::Warn
        } else {
            Verdict::Proceed
        }
    }
}

impl Default for FailureGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_then_stops_on_an_all_failed_streak() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed); // streak 1
        assert_eq!(g.observe_turn(1, 1), Verdict::Warn); // streak 2 == WARN_AT
        assert_eq!(g.observe_turn(1, 1), Verdict::Stop); // streak 3 == STOP_AT
    }

    #[test]
    fn a_successful_call_resets_the_streak() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed); // streak 1
        assert_eq!(g.observe_turn(1, 1), Verdict::Warn); // streak 2
        // One of two calls succeeded → not an all-failed turn → reset.
        assert_eq!(g.observe_turn(2, 1), Verdict::Proceed);
        // Streak restarts from zero.
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed); // streak 1
        assert_eq!(g.observe_turn(1, 1), Verdict::Warn); // streak 2
    }

    #[test]
    fn a_zero_dispatch_turn_never_trips() {
        let mut g = FailureGuard::new();
        // No result-bearing tool (e.g. a terminator-only or no-action turn).
        for _ in 0..5 {
            assert_eq!(g.observe_turn(0, 0), Verdict::Proceed);
        }
    }

    #[test]
    fn multi_call_turns_count_when_all_error() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed); // streak 1
        assert_eq!(g.observe_turn(3, 3), Verdict::Warn); // streak 2 — all 3 errored
        assert_eq!(g.observe_turn(2, 2), Verdict::Stop); // streak 3 — all 2 errored
    }

    #[test]
    fn blocked_completion_is_removed_from_execution_counts() {
        assert_eq!(failure_observation(1, 1, 0, true), Some((0, 0)));
        assert_eq!(failure_observation(2, 2, 0, true), Some((1, 1)));
        assert_eq!(failure_observation(2, 1, 0, true), Some((1, 0)));
        assert_eq!(failure_observation(2, 2, 0, false), Some((2, 2)));
    }

    #[test]
    fn control_only_turn_breaks_a_consecutive_failure_streak() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed);
        assert_eq!(g.observe_turn(1, 1), Verdict::Warn);
        assert_eq!(g.observe_turn(0, 0), Verdict::Proceed);
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed);
    }

    #[test]
    fn mixed_execution_error_and_blocked_completion_keeps_the_real_failure() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed);
        let (dispatched, errored) = failure_observation(2, 2, 0, true).unwrap();
        assert_eq!((dispatched, errored), (1, 1));
        assert_eq!(g.observe_turn(dispatched, errored), Verdict::Warn);
    }

    #[test]
    fn controller_only_turn_preserves_the_existing_streak() {
        let mut g = FailureGuard::new();
        assert_eq!(g.observe_turn(1, 1), Verdict::Proceed);
        assert_eq!(failure_observation(1, 1, 1, false), None);
        let (dispatched, errored) = failure_observation(1, 1, 0, false).unwrap();
        assert_eq!(g.observe_turn(dispatched, errored), Verdict::Warn);
    }

    #[test]
    fn mixed_turns_keep_only_real_results() {
        assert_eq!(failure_observation(2, 2, 1, false), Some((1, 1)));
        assert_eq!(failure_observation(2, 1, 1, false), Some((1, 0)));
        assert_eq!(failure_observation(3, 3, 1, true), Some((1, 1)));
    }

    #[test]
    fn malformed_attribution_fails_closed_without_suppressing_the_streak() {
        assert_eq!(failure_observation(0, 0, 1, false), Some((1, 1)));
        assert_eq!(failure_observation(1, 0, 1, false), Some((1, 1)));
        assert_eq!(failure_observation(1, 2, 0, false), Some((2, 2)));
        assert_eq!(failure_observation(1, 1, usize::MAX, true), Some((1, 1)));
    }
}
