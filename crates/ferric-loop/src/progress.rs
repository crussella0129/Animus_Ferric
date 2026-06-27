//! Same-tool-NAME streak guard for "semantic flailing" (ADR-031).
//!
//! The repetition guard (`repetition.rs`) canonicalizes the COMPLETE action
//! signature — names **and** args — so it never fires when a stuck model calls
//! the same tool with DIFFERENT args every turn (`make_dir` ×15 with new paths
//! → it grinds to `max_turns`). This guard is the complement: it canonicalizes
//! only the SORTED-UNIQUE tool NAMES of a turn, so a same-tool/different-args
//! streak is detected.
//!
//! The threshold is deliberately looser than the repetition guard's 2-strike
//! identical-signature stop, and sits well under every tier's `max_turns`
//! (Nano 15 … Large 40), so it bounds wasted compute without encroaching on the
//! repetition guard's territory. Scope (ADR-031): this does NOT make a weak
//! model complete a task — that is a capability limit. It bounds the cost of a
//! stuck model and emits a precise `no_progress` diagnostic distinct from
//! `max_turns`.

use std::collections::BTreeSet;

use ferric_core::ToolCall;

use crate::repetition::Verdict;

/// Warn at this many consecutive same-name turns — one course-correction
/// chance before the stop.
const WARN_AT: u8 = 4;
/// Stop at this many — the same tool-name set has been the sole operation for
/// `STOP_AT + 1` turns with no completion. Well under every tier's `max_turns`.
const STOP_AT: u8 = 5;

pub struct ProgressGuard {
    last_names: Option<String>,
    streak: u8,
}

impl ProgressGuard {
    pub fn new() -> Self {
        Self {
            last_names: None,
            streak: 0,
        }
    }

    /// Observe a turn's tool-call set (NAMES only, arg-insensitive) and decide.
    /// `Proceed` until the same name set repeats `WARN_AT` times in a row
    /// (`Warn`), then `Stop` at `STOP_AT`. Any change in the name set resets.
    pub fn observe(&mut self, calls: &[ToolCall]) -> Verdict {
        let names = names_of(calls);
        let same = self.last_names.as_deref() == Some(names.as_str());
        if !same {
            self.last_names = Some(names);
            self.streak = 0;
            return Verdict::Proceed;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak >= STOP_AT {
            Verdict::Stop
        } else if self.streak >= WARN_AT {
            Verdict::Warn
        } else {
            Verdict::Proceed
        }
    }
}

impl Default for ProgressGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonical name set: sorted, de-duplicated tool names. A `BTreeSet` makes it
/// order-independent (`[a,b]` == `[b,a]`) and dedups (`[a,a]` == `[a]`) — the
/// streak is about *which tools*, not how many calls or in what order.
fn names_of(calls: &[ToolCall]) -> String {
    calls
        .iter()
        .map(|c| c.name.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repetition::RepetitionGuard;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "x".to_string(),
            name: name.to_string(),
            args,
        }
    }

    #[test]
    fn warns_then_stops_on_a_same_name_streak() {
        let mut g = ProgressGuard::new();
        let turn = vec![call("make_dir", json!({"path": "a"}))];
        // First observation seeds; the next WARN_AT-1 are Proceed.
        assert_eq!(g.observe(&turn), Verdict::Proceed); // seed
        assert_eq!(g.observe(&turn), Verdict::Proceed); // streak 1
        assert_eq!(g.observe(&turn), Verdict::Proceed); // streak 2
        assert_eq!(g.observe(&turn), Verdict::Proceed); // streak 3
        assert_eq!(g.observe(&turn), Verdict::Warn); // streak 4 == WARN_AT
        assert_eq!(g.observe(&turn), Verdict::Stop); // streak 5 == STOP_AT
    }

    #[test]
    fn the_defining_contrast_same_tool_different_args() {
        // The exact ADR-031 gap: the same tool, DIFFERENT args each turn.
        // ProgressGuard reaches Stop; RepetitionGuard never leaves Proceed.
        let mut progress = ProgressGuard::new();
        let mut repetition = RepetitionGuard::new();
        let mut progress_stopped = false;
        for i in 0..8 {
            let turn = vec![call("make_dir", json!({ "path": format!("dir{i}") }))];
            // RepetitionGuard sees a different signature every turn → Proceed.
            assert_eq!(
                repetition.observe(&turn),
                Verdict::Proceed,
                "repetition guard must NOT fire on different args (turn {i})"
            );
            if progress.observe(&turn) == Verdict::Stop {
                progress_stopped = true;
                break;
            }
        }
        assert!(
            progress_stopped,
            "progress guard must Stop the same-tool/different-args flail"
        );
    }

    #[test]
    fn a_tool_name_change_resets_the_streak() {
        let mut g = ProgressGuard::new();
        let a = vec![call("make_dir", json!({"path": "a"}))];
        let b = vec![call("write_file", json!({"path": "b"}))];
        // Build the streak to one short of Warn, then change tools.
        for _ in 0..4 {
            g.observe(&a);
        }
        assert_eq!(g.observe(&b), Verdict::Proceed, "a name change resets");
        // After the reset it takes the full run again to warn.
        assert_eq!(g.observe(&b), Verdict::Proceed);
        assert_eq!(g.observe(&b), Verdict::Proceed);
        assert_eq!(g.observe(&b), Verdict::Proceed);
        assert_eq!(g.observe(&b), Verdict::Warn);
    }

    #[test]
    fn name_set_is_order_independent() {
        let mut g = ProgressGuard::new();
        let ab = vec![
            call("make_dir", json!({"path": "a"})),
            call("write_file", json!({"path": "b"})),
        ];
        let ba = vec![
            call("write_file", json!({"path": "c"})),
            call("make_dir", json!({"path": "d"})),
        ];
        assert_eq!(g.observe(&ab), Verdict::Proceed); // seed {make_dir, write_file}
        // Same name set, reversed order + different args → still a match (streak builds).
        assert_eq!(g.observe(&ba), Verdict::Proceed); // streak 1, not a reset
        assert_eq!(g.observe(&ab), Verdict::Proceed); // streak 2
        assert_eq!(g.observe(&ba), Verdict::Proceed); // streak 3
        assert_eq!(g.observe(&ab), Verdict::Warn); // streak 4
    }
}
