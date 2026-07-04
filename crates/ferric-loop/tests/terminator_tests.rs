//! T-105 integration tests: the task_complete structured terminator.

mod common;

use common::*;
use ferric_loop::{StopReason, TASK_COMPLETE};
use serde_json::json;

#[test]
fn task_complete_terminates() {
    let result = run_scripted(
        vec![tool_completion(vec![(
            "tc-0",
            TASK_COMPLETE,
            json!({"summary": "created the file"}),
        )])],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(
        result.outcome.final_text.as_deref(),
        Some("created the file")
    );
    assert_eq!(session_end_reason(&result.records), "task_complete");
    // Never DISPATCHED (no tool_result for the terminator), even though it IS
    // now traced as a ToolCall (T-3902, sprint 39 — closes the NativeTools
    // summary-visibility gap needed for replay).
    assert!(kinds(&result.records).contains(&"tool_call"));
    assert!(
        !kinds(&result.records).contains(&"tool_result"),
        "task_complete must not be dispatched through the registry"
    );
}

#[test]
fn task_complete_mixed_turn() {
    // write_file and task_complete in ONE turn: the write must execute first.
    let result = run_scripted(
        vec![tool_completion(vec![
            (
                "tc-0",
                "write_file",
                json!({"path": "out.txt", "content": "data"}),
            ),
            ("tc-1", TASK_COMPLETE, json!({"summary": "wrote out.txt"})),
        ])],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(result.outcome.final_text.as_deref(), Some("wrote out.txt"));
    let kind_list = kinds(&result.records);
    assert!(
        kind_list.contains(&"tool_call") && kind_list.contains(&"tool_result"),
        "the non-terminator call must have executed: {kind_list:?}"
    );
}

#[test]
fn task_complete_mid_turn_preserves_emission_order() {
    // The terminator's ToolCall event must be traced at the exact position it
    // was emitted in `actions` — not after the dispatch loop finishes — so a
    // trace with the terminator BETWEEN two other calls preserves that order.
    // (Sprint 39, test-critic C-001: replay() itself has no terminator-aware
    // logic, so this must be caught in run.rs's dispatch loop, not replay.)
    let result = run_scripted(
        vec![tool_completion(vec![
            (
                "tc-0",
                "write_file",
                json!({"path": "a.txt", "content": "a"}),
            ),
            ("tc-1", TASK_COMPLETE, json!({"summary": "done"})),
            (
                "tc-2",
                "write_file",
                json!({"path": "b.txt", "content": "b"}),
            ),
        ])],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(
        tool_call_names(&result.records),
        vec!["write_file", TASK_COMPLETE, "write_file"],
        "traced tool_call order must match original emission order exactly"
    );
    // Only the two non-terminator calls actually dispatched.
    let result_count = result
        .records
        .iter()
        .filter(|r| {
            matches!(
                &r.event,
                ferric_trace::ParsedEvent::Known(ferric_trace::Event::ToolResult { .. })
            )
        })
        .count();
    assert_eq!(result_count, 2, "the terminator must not be dispatched");
}

#[test]
fn task_complete_always_offered() {
    // NANO max_tools is 6; builtins (3) + task_complete must both appear even
    // when the registry is overfull.
    let result = run_scripted(
        vec![tool_completion(vec![(
            "tc-0",
            TASK_COMPLETE,
            json!({"summary": "ok"}),
        )])],
        &nano_policy(),
        |provider| {
            let request = &provider.requests()[0];
            assert!(
                request.tools.iter().any(|t| t.name == TASK_COMPLETE),
                "task_complete must always be offered"
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
}

#[test]
fn malformed_summary_still_terminates() {
    let result = run_scripted(
        vec![tool_completion(vec![("tc-0", TASK_COMPLETE, json!({}))])],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::TaskComplete);
    assert_eq!(result.outcome.final_text.as_deref(), Some(""));
}

#[test]
fn terminator_result_not_fed_back() {
    // task_complete produces no tool-result message (the loop ends instead).
    let result = run_scripted(
        vec![tool_completion(vec![(
            "tc-0",
            TASK_COMPLETE,
            json!({"summary": "done"}),
        )])],
        &nano_policy(),
        |provider| {
            assert_eq!(
                provider.requests().len(),
                1,
                "loop must end after the terminator"
            );
        },
    );
    assert!(
        !result.records.iter().any(|r| matches!(
            &r.event,
            ferric_trace::ParsedEvent::Known(ferric_trace::Event::ToolResult { .. })
        )),
        "no tool_result for an intercepted terminator"
    );
}
