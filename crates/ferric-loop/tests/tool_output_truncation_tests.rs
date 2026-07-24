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
use ferric_core::{ActionProtocol, ModelProfile, Role, RunPolicy, policy_for};
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
