//! T-104 integration tests: the core turn loop.

mod common;

use common::*;
use ferric_core::Role;
use ferric_loop::StopReason;
use serde_json::json;

#[test]
fn happy_path_golden_trace_order() {
    let result = run_scripted(
        vec![
            tool_completion(vec![("tc-0", "list_dir", json!({"path": "."}))]),
            text_completion("all done"),
        ],
        &nano_policy(),
        |_| {},
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert_eq!(result.outcome.final_text.as_deref(), Some("all done"));
    assert_eq!(
        kinds(&result.records),
        vec![
            "session_start",
            "policy_selected",
            "turn_start",
            "prompt_assembled",
            "turn_end",
            "tool_call",
            "permission_check",
            "tool_result",
            "turn_start",
            "prompt_assembled",
            "turn_end",
            "session_end",
        ]
    );
    // seq strictly monotonic from 0
    let seqs: Vec<u64> = result.records.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (0..result.records.len() as u64).collect::<Vec<_>>());
}

#[test]
fn max_turns_budget() {
    let policy = nano_policy(); // max_turns = 15
    let n = policy.max_turns as usize;
    // Script n+1 tool turns; the loop must stop after n.
    let script: Vec<_> = (0..=n)
        .map(|i| tool_completion(vec![("tc", "list_dir", json!({"path": ".", "turn": i}))]))
        .collect();
    let result = run_scripted(script, &policy, |_| {});
    assert_eq!(result.outcome.stop, StopReason::MaxTurns);
    assert_eq!(result.outcome.turns, u32::from(policy.max_turns));
    let turn_starts = kinds(&result.records)
        .iter()
        .filter(|k| **k == "turn_start")
        .count();
    assert_eq!(turn_starts, n);
    assert_eq!(session_end_reason(&result.records), "max_turns");
}

#[test]
fn max_turns_still_emits_last_text() {
    // Tool turns that also carry text: on MaxTurns the last text must surface.
    let policy = nano_policy();
    let script: Vec<_> = (0..=policy.max_turns as usize)
        .map(|i| {
            let mut c = tool_completion(vec![("tc", "list_dir", json!({"path": ".", "i": i}))]);
            c.message.text = Some(format!("thinking {i}"));
            c
        })
        .collect();
    let result = run_scripted(script, &policy, |_| {});
    assert_eq!(result.outcome.stop, StopReason::MaxTurns);
    assert!(
        result
            .outcome
            .final_text
            .as_deref()
            .is_some_and(|t| t.starts_with("thinking")),
        "best-effort text must be emitted on MaxTurns"
    );
}

#[test]
fn denied_tool_feeds_back() {
    let result = run_scripted(
        vec![
            tool_completion(vec![(
                "tc-0",
                "write_file",
                json!({"path": ".git/config", "content": "evil"}),
            )]),
            text_completion("understood, stopping"),
        ],
        &nano_policy(),
        |provider| {
            // The denial must be fed back to the model as an error tool result.
            let second = &provider.requests()[1];
            let fed = second
                .messages
                .iter()
                .find(|m| m.role == Role::Tool)
                .expect("tool result fed back");
            assert!(fed.text.as_deref().unwrap_or_default().contains("DENIED"));
        },
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    // Deny traced via permission_check, and loop continued to a second turn.
    assert!(kinds(&result.records).contains(&"permission_check"));
    assert_eq!(session_end_reason(&result.records), "final_text");
}

#[test]
fn empty_completion_nudges_then_stops() {
    let result = run_scripted(
        vec![empty_completion(), empty_completion()],
        &nano_policy(),
        |provider| {
            // The nudge must be visible to the model on the second request.
            let second = &provider.requests()[1];
            let last = second.messages.last().unwrap();
            assert_eq!(last.role, Role::User);
            assert!(
                last.text
                    .as_deref()
                    .unwrap_or_default()
                    .contains("tool call")
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::EmptyCompletion);
    assert_eq!(session_end_reason(&result.records), "empty_completion");
}

#[test]
fn unknown_tool_feeds_back() {
    // Critique C-003: a hallucinated tool name must not kill the loop — the
    // error is fed back and the model gets another chance.
    let result = run_scripted(
        vec![
            tool_completion(vec![("tc-0", "frobnicate", json!({"x": 1}))]),
            text_completion("recovered"),
        ],
        &nano_policy(),
        |provider| {
            let second = &provider.requests()[1];
            let fed = second
                .messages
                .iter()
                .find(|m| m.role == Role::Tool)
                .expect("error tool result fed back");
            assert!(
                fed.text
                    .as_deref()
                    .unwrap_or_default()
                    .contains("unknown tool: frobnicate")
            );
        },
    );
    assert_eq!(result.outcome.stop, StopReason::FinalText);
    assert_eq!(result.outcome.final_text.as_deref(), Some("recovered"));
}
