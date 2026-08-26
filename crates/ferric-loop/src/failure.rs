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

/// Remove the synthetic error result produced by a blocked completion gate
/// from the result-bearing execution counts used by [`FailureGuard`]. A
/// `task_complete` gate is a control transition, not executed work. When it is
/// the only result in a turn this deliberately yields `(0, 0)`, which resets
/// the consecutive execution-failure streak.
pub(crate) fn execution_counts(
    dispatched: usize,
    errored: usize,
    completion_was_blocked: bool,
) -> (usize, usize) {
    if completion_was_blocked {
        (dispatched.saturating_sub(1), errored.saturating_sub(1))
    } else {
        (dispatched, errored)
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
        assert_eq!(execution_counts(1, 1, true), (0, 0));
        assert_eq!(execution_counts(2, 2, true), (1, 1));
        assert_eq!(execution_counts(2, 1, true), (1, 0));
        assert_eq!(execution_counts(2, 2, false), (2, 2));
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
        let (dispatched, errored) = execution_counts(2, 2, true);
        assert_eq!((dispatched, errored), (1, 1));
        assert_eq!(g.observe_turn(dispatched, errored), Verdict::Warn);
    }
}
