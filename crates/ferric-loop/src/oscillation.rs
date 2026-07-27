//! Windowed cycle guard for A-B-A-B oscillation (ADR-077).
//!
//! The other three guards all key on **consecutive-turn** state, so a model that
//! alternates between two actions resets every one of them, every turn:
//!
//! * `repetition.rs` compares a turn's full signature (names **and** args) to
//!   the *previous* turn's — turn N is `A`, turn N+1 is `B`, never two alike in
//!   a row, so the streak never reaches 2.
//! * `progress.rs` compares the sorted-unique tool **names** — `{A}` then `{B}`,
//!   different again, so it never reaches 5.
//! * `failure.rs` counts all-errored turns — and an oscillating model's calls
//!   typically **succeed**, so it never engages at all.
//!
//! Found live in sprint 86: qwen2.5-coder-7b alternated `search_files` /
//! `find_files` for the **entire 20-turn budget** — 20 calls, 2 distinct
//! `(name, args)` pairs, zero guard events. Bounding wasted compute is this
//! family's whole stated purpose (ADR-037/038), and a 2-cycle of successful
//! calls walked straight through it.
//!
//! So this guard is **windowed** rather than streak-based: it asks "over the
//! last N turns, how many *distinct* things has the model actually done?" A
//! sustained 2-cycle answers "two", however the turns are interleaved.
//!
//! Scope, same as its siblings (ADR-031): this does not make a weak model finish
//! a task. It bounds the cost of a looping one and emits a precise `oscillation`
//! diagnostic instead of an uninformative `max_turns`.

use std::collections::BTreeSet;

use ferric_core::ToolCall;

use crate::repetition::Verdict;

/// Turns of history to consider. Long enough that a 2-cycle has visibly
/// *repeated* (A-B-A-B-A-B) rather than merely alternated once.
const WARN_AT: usize = 6;
/// Stop once the window has held this many turns while still drawing on no more
/// than [`MAX_DISTINCT`] actions. Sits above the no-progress guard's 5 — this is
/// the last-resort catcher for what the sharper guards miss — and well under
/// every tier's `max_turns` (Nano 15 … Ultra 80).
const STOP_AT: usize = 8;
/// How few distinct actions counts as a cycle. Deliberately tight: 2 is the
/// unambiguous pathological case (a model ping-ponging between exactly two
/// calls). Raising it would start catching legitimate short workflows that
/// happen to repeat with identical arguments.
const MAX_DISTINCT: usize = 2;

pub struct OscillationGuard {
    /// Canonical signature per turn, most recent last, capped at `STOP_AT`.
    window: Vec<String>,
}

impl OscillationGuard {
    pub fn new() -> Self {
        Self {
            window: Vec::with_capacity(STOP_AT),
        }
    }

    /// Observe a turn's tool calls and decide.
    ///
    /// Unlike the streak guards, nothing here "resets" — the window simply
    /// slides. That is the point: an interleaving that breaks a streak still
    /// leaves the same small set of distinct actions inside the window.
    pub fn observe(&mut self, calls: &[ToolCall]) -> Verdict {
        if calls.is_empty() {
            return Verdict::Proceed;
        }
        self.window.push(signature(calls));
        if self.window.len() > STOP_AT {
            self.window.remove(0);
        }

        if self.window.len() >= STOP_AT && self.distinct() <= MAX_DISTINCT {
            Verdict::Stop
        } else if self.window.len() >= WARN_AT && self.distinct_in_last(WARN_AT) <= MAX_DISTINCT {
            Verdict::Warn
        } else {
            Verdict::Proceed
        }
    }

    fn distinct(&self) -> usize {
        self.window.iter().collect::<BTreeSet<_>>().len()
    }

    fn distinct_in_last(&self, n: usize) -> usize {
        let start = self.window.len().saturating_sub(n);
        self.window[start..].iter().collect::<BTreeSet<_>>().len()
    }
}

impl Default for OscillationGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// A turn's full canonical signature — names **and** args, so `read_file(a)` and
/// `read_file(b)` are different actions. Sorted so call order within a turn does
/// not matter.
fn signature(calls: &[ToolCall]) -> String {
    let mut parts: Vec<String> = calls
        .iter()
        .map(|c| format!("{}({})", c.name, canonical_args(&c.args)))
        .collect();
    parts.sort();
    parts.join("|")
}

/// Key-sorted JSON so `{"a":1,"b":2}` and `{"b":2,"a":1}` are one action.
fn canonical_args(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .collect::<std::collections::BTreeMap<_, _>>()
                .iter()
                .map(|(k, v)| format!("{k}:{}", canonical_args(v)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonical_args).collect();
            format!("[{}]", inner.join(","))
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "x".to_string(),
            name: name.to_string(),
            args,
        }
    }

    fn a() -> Vec<ToolCall> {
        vec![call(
            "search_files",
            json!({"path": "big.txt", "query": "line"}),
        )]
    }
    fn b() -> Vec<ToolCall> {
        vec![call(
            "find_files",
            json!({"max_results": 1, "path": ".", "pattern": "big.txt"}),
        )]
    }

    /// The live sprint-86 failure, reproduced: alternate two successful calls
    /// and every other guard stays silent while this one stops it.
    #[test]
    fn a_two_cycle_is_stopped() {
        let mut g = OscillationGuard::new();
        let (turn_a, turn_b) = (a(), b());
        let mut verdicts = Vec::new();
        for i in 0..STOP_AT {
            verdicts.push(g.observe(if i.is_multiple_of(2) {
                &turn_a
            } else {
                &turn_b
            }));
        }
        assert_eq!(verdicts[STOP_AT - 1], Verdict::Stop, "got: {verdicts:?}");
        assert!(
            verdicts.contains(&Verdict::Warn),
            "must warn before stopping: {verdicts:?}"
        );
    }

    /// It must not fire before the cycle has actually repeated — a single
    /// there-and-back is ordinary.
    #[test]
    fn one_alternation_is_fine() {
        let mut g = OscillationGuard::new();
        assert_eq!(g.observe(&a()), Verdict::Proceed);
        assert_eq!(g.observe(&b()), Verdict::Proceed);
        assert_eq!(g.observe(&a()), Verdict::Proceed);
    }

    /// Real progress means new arguments. A model working through distinct
    /// files must never be stopped, however many turns it takes.
    #[test]
    fn genuine_progress_is_never_stopped() {
        let mut g = OscillationGuard::new();
        for i in 0..20 {
            let calls = vec![call("read_file", json!({ "path": format!("f{i}.txt") }))];
            assert_eq!(
                g.observe(&calls),
                Verdict::Proceed,
                "distinct args must always proceed (turn {i})"
            );
        }
    }

    /// A 3-cycle stays under the tighter threshold deliberately — see
    /// `MAX_DISTINCT`. Documented so the choice is a decision, not an accident.
    #[test]
    fn a_three_cycle_is_deliberately_not_caught() {
        let mut g = OscillationGuard::new();
        let c = vec![call("list_dir", json!({"path": "."}))];
        for i in 0..STOP_AT {
            let calls = match i % 3 {
                0 => a(),
                1 => b(),
                _ => c.clone(),
            };
            assert_ne!(g.observe(&calls), Verdict::Stop);
        }
    }

    /// Arg order and key order must not create false distinctness.
    #[test]
    fn argument_order_does_not_matter() {
        let mut g = OscillationGuard::new();
        for i in 0..STOP_AT {
            let calls = if i.is_multiple_of(2) {
                vec![call("t", json!({"a": 1, "b": 2}))]
            } else {
                vec![call("t", json!({"b": 2, "a": 1}))]
            };
            let v = g.observe(&calls);
            if i == STOP_AT - 1 {
                assert_eq!(v, Verdict::Stop, "these are the SAME action");
            }
        }
    }

    /// Turns with no tool calls are the no-action-nudge path; they are not
    /// actions and must not fill the window.
    #[test]
    fn empty_turns_are_ignored() {
        let mut g = OscillationGuard::new();
        for _ in 0..STOP_AT {
            assert_eq!(g.observe(&[]), Verdict::Proceed);
        }
    }
}
