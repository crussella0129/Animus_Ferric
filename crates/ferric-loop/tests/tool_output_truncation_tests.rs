//! Integration tests: **tool-output** truncation (ADR-002 / ADR-073).
//!
//! Distinct from `truncation_tests.rs`, which is about a *model completion*
//! being cut off mid-action by the token budget. The two share a word and
//! nothing else — and that collision is part of why the defect these tests
//! cover survived a green suite for ~38 sprints.
//!
//! The contract: the trace stores the **full** tool output (durability), while
//! the context window gets a truncated view (budget). Sprint 44's event-sourced
//! refactor moved context assembly into the projector, which rebuilds from the
//! trace — so the truncation has to be applied there, or it is applied nowhere.
//! `registry.rs` already tests that the Registry *computes* the truncated view;
//! nothing tested that the loop *uses* it, which is exactly where it was lost.

mod common;

use common::*;
use ferric_core::{ActionProtocol, Message, ModelProfile, Role, RunPolicy, policy_for};
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, replay, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::JsonlSink;
use serde_json::json;

const BIG: usize = 20_000;

/// A large context so the compaction path cannot mask the result.
fn large_policy() -> RunPolicy {
    policy_for(&ModelProfile {
        params_b: 70.0,
        quant: "Q4_K_M".to_string(),
        ctx: 131_072,
        family: "test".to_string(),
        measured_level: Some(6),
    })
}

fn write_then_read_script() -> Vec<ferric_provider::Completion> {
    let big = "X".repeat(BIG);
    vec![
        json_completion(json!({
            "thought": "write", "tool": "write_file",
            "args": {"path": "big.txt", "content": big}
        })),
        json_completion(json!({
            "thought": "read it back", "tool": "read_file",
            "args": {"path": "big.txt"}
        })),
        json_completion(json!({
            "thought": "done", "tool": "task_complete",
            "args": {"summary": "done"}
        })),
    ]
}

#[test]
fn large_tool_output_reaches_the_model_truncated() {
    run_scripted_protocol(
        write_then_read_script(),
        &large_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            // Request 2 is the one issued after the read_file result landed.
            let third = &provider.requests()[2];
            let longest = third
                .messages
                .iter()
                .filter(|m| m.role == Role::User)
                .filter_map(|m| m.text.as_deref())
                .map(|t| t.chars().count())
                .max()
                .unwrap_or(0);

            assert!(
                longest <= ferric_tools::DEFAULT_TRUNCATION_LIMIT + 200,
                "a {BIG}-char tool output must reach the model truncated to \
                 ~{} chars, but the longest user message carries {longest}",
                ferric_tools::DEFAULT_TRUNCATION_LIMIT
            );
        },
    );
}

#[test]
fn the_model_is_told_the_output_was_truncated() {
    // Silently handing back a prefix is worse than not truncating: the model
    // cannot tell a short file from a clipped one.
    run_scripted_protocol(
        write_then_read_script(),
        &large_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            let third = &provider.requests()[2];
            assert!(
                third
                    .messages
                    .iter()
                    .filter_map(|m| m.text.as_deref())
                    .any(|t| t.contains("truncated")),
                "the truncated result must say so"
            );
        },
    );
}

#[test]
fn the_trace_keeps_the_full_untruncated_output() {
    // The other half of ADR-002: truncation is a context-window concern only.
    // A post-hoc reader of the trace must still see everything.
    let result = run_scripted_protocol(
        write_then_read_script(),
        &large_policy(),
        ActionProtocol::ConstrainedJson,
        |_| {},
    );

    let longest_traced = result
        .records
        .iter()
        .filter_map(|r| match &r.event {
            ferric_trace::ParsedEvent::Known(ferric_trace::Event::ToolResult {
                output, ..
            }) => Some(output.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    assert!(
        longest_traced >= BIG,
        "the trace must retain the full output; longest ToolResult is {longest_traced}"
    );
}

// ---------------------------------------------------------------------------
// ADR-093: run and replay must agree about the cap.
//
// The property is stated as an equality between two things that are built by
// different code from different inputs — the context window `run()` handed the
// model, and the one `replay()` rebuilds from the trace alone. Anything less
// (asserting a length, asserting replay alone) can pass while the two drift.
// ---------------------------------------------------------------------------

/// Runs the write/read script for real under `cap`, kills the trace the way
/// `resume_tests.rs` does, and returns (what the model saw on its third
/// request, what replay rebuilt). Inlined rather than routed through
/// `run_scripted`, which does not expose its trace path.
fn context_window_from_run_and_from_replay(cap: usize) -> (Vec<Message>, Vec<Message>) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::with_truncation_limit(cap);
    register_builtin_tools(&mut registry);
    let trace_path = dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "cap-session").unwrap();

    let provider = MockProvider::new(write_then_read_script());
    let sleeper = RecordingSleeper::new();
    futures_executor::block_on(run(
        RunArgs {
            edit_approver: None,
            cancel_flag: None,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            provenance: ferric_guard::Provenance::Clean,
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &large_policy(),
            protocol: ActionProtocol::ConstrainedJson,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            hooks: None,
        },
        &mut sink,
        Some("do the task"),
    ))
    .unwrap();

    // Request 2 is the one carrying both tool results — the widest context
    // window this run ever built.
    let from_run = provider.requests()[2].messages.clone();

    // Simulate a kill before turn 2's explicit commit barrier. Removing both
    // SessionEnd and TurnCommitted leaves the intercepted task_complete tail
    // uncommitted, so replay reconstructs exactly the request-2 context.
    let content = std::fs::read_to_string(&trace_path).unwrap();
    let mut lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.pop().map(|l| l.contains("session_end")), Some(true));
    assert_eq!(
        lines.pop().map(|l| l.contains("turn_committed")),
        Some(true)
    );
    std::fs::write(&trace_path, lines.join("\n") + "\n").unwrap();

    let from_replay = replay(&trace_path).unwrap().messages;
    (from_run, from_replay)
}

/// The contract. Fails before ADR-093: `run()` truncated the 20,000-char read
/// at 500 while `replay()` had no way to learn that and used 4,000.
#[test]
fn replay_rebuilds_the_same_context_window_the_run_used_at_a_non_default_cap() {
    let (from_run, from_replay) = context_window_from_run_and_from_replay(500);
    assert_eq!(
        from_run, from_replay,
        "replay must rebuild the window the model actually saw"
    );
    // Confirm the cap was the thing under test: at 500 the *tool result* has
    // to be far shorter than the default would have produced. Without this the
    // equality above would also hold if truncation stopped happening entirely.
    // Measured on the result message specifically — the longest message in the
    // window is the assistant's own `write_file` call echoing 20,000 chars of
    // content, which no cap applies to and which would mask this check.
    let read_result = from_replay
        .iter()
        .filter_map(|m| m.text.as_deref())
        .find(|t| t.starts_with("[tool_result for read_file]"))
        .expect("the read_file result must be in the rebuilt window");
    let len = read_result.chars().count();
    assert!(
        len < ferric_tools::DEFAULT_TRUNCATION_LIMIT,
        "a 500-char cap must bite; the read_file result carries {len} chars"
    );
}

/// The positive control: the same equality at the default cap, which held both
/// before and after ADR-093. If this one ever fails, the test above is
/// reporting a broken replay rather than a cap disagreement.
#[test]
fn replay_rebuilds_the_same_context_window_the_run_used_at_the_default_cap() {
    let (from_run, from_replay) =
        context_window_from_run_and_from_replay(ferric_tools::DEFAULT_TRUNCATION_LIMIT);
    assert_eq!(from_run, from_replay);
}

/// Small outputs must pass through byte-for-byte — no marker, no clipping.
#[test]
fn small_tool_output_is_untouched() {
    run_scripted_protocol(
        vec![
            json_completion(json!({
                "thought": "write", "tool": "write_file",
                "args": {"path": "small.txt", "content": "hello world"}
            })),
            json_completion(json!({
                "thought": "read", "tool": "read_file",
                "args": {"path": "small.txt"}
            })),
            json_completion(json!({
                "thought": "done", "tool": "task_complete",
                "args": {"summary": "done"}
            })),
        ],
        &large_policy(),
        ActionProtocol::ConstrainedJson,
        |provider| {
            let third = &provider.requests()[2];
            let joined: String = third
                .messages
                .iter()
                .filter_map(|m| m.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(joined.contains("hello world"));
            assert!(
                !joined.contains("output truncated for model"),
                "a short output must not be marked truncated"
            );
        },
    );
}
