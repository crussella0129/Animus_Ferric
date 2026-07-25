//! Integration tests: the windowed oscillation guard (ADR-077).
//!
//! The scenario is not invented — it is the sprint-86 live run reproduced.
//! qwen2.5-coder-7b alternated `search_files` / `find_files` for the entire
//! 20-turn budget: 20 calls, 2 distinct `(name, args)` pairs, and **zero** guard
//! events, because all three existing guards are streak-based and alternation
//! resets each of them every turn.

mod common;

use common::*;
use ferric_core::ActionProtocol;
use ferric_loop::StopReason;
use serde_json::json;

/// One turn of the live 2-cycle.
fn cycle_turn(i: usize) -> ferric_provider::Completion {
    if i.is_multiple_of(2) {
        json_completion(json!({
            "thought": "search",
            "tool": "search_files",
            "args": {"path": "big.txt", "query": "line"}
        }))
    } else {
        json_completion(json!({
            "thought": "find",
            "tool": "find_files",
            "args": {"max_results": 1, "path": ".", "pattern": "big.txt"}
        }))
    }
}

#[test]
fn the_live_two_cycle_is_stopped_before_max_turns() {
    let script: Vec<_> = (0..20).map(cycle_turn).collect();
    let result = run_scripted_protocol(
        script,
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |_| {},
    );

    assert_eq!(
        result.outcome.stop,
        StopReason::Oscillation,
        "an A-B-A-B cycle must stop with `oscillation`, not grind to max_turns"
    );
    assert_eq!(session_end_reason(&result.records), "oscillation");
}

/// The diagnostic matters as much as the stop: `max_turns` says only "it ran
/// out", `oscillation` says why.
#[test]
fn it_stops_well_inside_the_turn_budget() {
    let script: Vec<_> = (0..20).map(cycle_turn).collect();
    let result = run_scripted_protocol(
        script,
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            // nano's max_turns is 15; the guard should stop long before that.
            assert!(
                provider.requests().len() <= 10,
                "expected a stop by ~8 turns, saw {} requests",
                provider.requests().len()
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::Oscillation);
}

/// The model gets one warning — a chance to break the cycle itself — before the
/// stop, and that warning has to actually reach it.
#[test]
fn the_model_is_warned_before_being_stopped() {
    let script: Vec<_> = (0..20).map(cycle_turn).collect();
    let result = run_scripted_protocol(
        script,
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            let saw_warning = provider.requests().iter().any(|r| {
                r.messages
                    .iter()
                    .filter_map(|m| m.text.as_deref())
                    .any(|t| t.contains("cycling between the same few tool calls"))
            });
            assert!(saw_warning, "the oscillation warning must reach the model");
        },
    );
    assert_eq!(result.outcome.stop, StopReason::Oscillation);

    assert!(
        kinds(&result.records).contains(&"oscillation_guard"),
        "the guard decision must be in the trace"
    );
}

/// A model doing real work — new arguments each turn — must never be stopped,
/// however long it takes. This is the guard's false-positive boundary.
#[test]
fn genuine_progress_is_never_stopped() {
    // Deliberately the hardest shape for this guard to get right: the tool
    // NAMES alternate (so a name-based window would call it a 2-cycle) while
    // every call carries fresh arguments and SUCCEEDS. That is real work, and
    // all four guards must let it run.
    let mut script: Vec<_> = (0..8usize)
        .map(|i| {
            if i.is_multiple_of(2) {
                json_completion(json!({
                    "thought": "make a dir",
                    "tool": "make_dir",
                    "args": { "path": format!("d{i}") }
                }))
            } else {
                json_completion(json!({
                    "thought": "write a file",
                    "tool": "write_file",
                    "args": { "path": format!("f{i}.txt"), "content": format!("file {i}") }
                }))
            }
        })
        .collect();
    script.push(json_completion(json!({
        "thought": "done", "tool": "task_complete", "args": {"summary": "done"}
    })));

    let result = run_scripted_protocol(
        script,
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |_| {},
    );
    assert_eq!(
        result.outcome.stop,
        StopReason::TaskComplete,
        "distinct arguments are progress, not a cycle"
    );
}

/// The sharper guards still own their cases — this one must not steal them,
/// or the `repetition` / `no_progress` diagnostics degrade into `oscillation`.
#[test]
fn identical_repeats_still_report_as_repetition() {
    let script: Vec<_> = (0..20).map(|_| cycle_turn(0)).collect();
    let result = run_scripted_protocol(
        script,
        &nano_policy(),
        ActionProtocol::ConstrainedJson,
        |_| {},
    );
    assert_eq!(
        result.outcome.stop,
        StopReason::RepetitionGuard,
        "an identical repeat is the repetition guard's case, at threshold 2"
    );
}
