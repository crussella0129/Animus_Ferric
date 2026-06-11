//! Proves every s0 interface composes into the eventual ReAct loop shape —
//! with zero inference. A scripted MockProvider "decides" to call a tool; a
//! minimal test-harness loop dispatches it through the registry, feeds the
//! truncated result back as a message, and writes a complete trace.
//!
//! This is a TEST-ONLY harness: the production loop crate is an s1 decision,
//! shaped by the first real backend. This test is also the structural
//! template for the s1 L0-smoke E2E.

use ferric_core::{Message, ModelProfile, Role, ToolCall, policy_for};
use ferric_guard::Workspace;
use ferric_provider::{
    Completion, CompletionRequest, Constraint, MockProvider, Provider, SamplingParams,
    ToolDescriptor,
};
use ferric_tools::{ExecuteOutcome, Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};
use serde_json::json;

fn tool_call_completion() -> Completion {
    Completion {
        message: Message {
            role: Role::Assistant,
            text: None,
            tool_calls: vec![ToolCall {
                id: "tc-0".to_string(),
                name: "read_file".to_string(),
                args: json!({"path": "hello.txt"}),
            }],
            tool_call_id: None,
        },
        input_tokens: Some(50),
        output_tokens: Some(12),
    }
}

fn final_completion() -> Completion {
    Completion {
        message: Message::assistant("The file says: hello ferric"),
        input_tokens: Some(70),
        output_tokens: Some(8),
    }
}

#[test]
fn mock_loop_skeleton() {
    // Arrange: workspace with a file, registry, trace sink, scripted provider,
    // NANO policy (the regime Ferric is built for).
    let ws_dir = tempfile::tempdir().unwrap();
    std::fs::write(ws_dir.path().join("hello.txt"), "hello ferric").unwrap();
    let workspace = Workspace::new(ws_dir.path()).unwrap();

    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);

    let trace_path = ws_dir.path().join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "loop-1").unwrap();

    let provider = MockProvider::new(vec![tool_call_completion(), final_completion()]);
    let policy = policy_for(&ModelProfile {
        params_b: 1.0,
        quant: "Q4_K_M".to_string(),
        ctx: 4096,
        family: "test".to_string(),
        measured_level: None,
    });

    let tools: Vec<ToolDescriptor> = registry
        .tools_for_policy(&policy)
        .into_iter()
        .map(|spec| ToolDescriptor {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        })
        .collect();
    let constraint = Constraint::JsonSchema(json!({"type": "object"}));

    // Act: the minimal loop — generate, dispatch tools, feed results back,
    // stop on a text-only completion or the policy's turn budget.
    sink.write_event(Event::SessionStart {
        workspace: ws_dir.path().display().to_string(),
    })
    .unwrap();
    let mut messages = vec![Message::user("What does hello.txt say?")];
    let mut final_text = None;

    for _turn in 0..policy.max_turns {
        let completion = futures_executor::block_on(provider.complete(CompletionRequest {
            messages: messages.clone(),
            sampling: SamplingParams::default(),
            tools: tools.clone(),
            constraint: Some(constraint.clone()),
        }))
        .unwrap();
        messages.push(completion.message.clone());

        if completion.message.tool_calls.is_empty() {
            final_text = completion.message.text.clone();
            break;
        }
        for call in &completion.message.tool_calls {
            sink.write_event(Event::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
            })
            .unwrap();
            match registry.execute(&workspace, &call.name, &call.args) {
                ExecuteOutcome::Completed {
                    output,
                    duration_ms,
                    ..
                } => {
                    sink.write_event(Event::ToolResult {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        output: output.full.clone(),
                        is_error: output.is_error,
                        duration_ms,
                    })
                    .unwrap();
                    // The model gets the TRUNCATED copy; the trace got the full one.
                    messages.push(Message::tool_result(&call.id, &output.for_model));
                }
                other => panic!("tool dispatch failed: {other:?}"),
            }
        }
    }
    sink.write_event(Event::SessionEnd {
        reason: "final_text".to_string(),
    })
    .unwrap();

    // Assert: the loop terminated with the scripted answer.
    assert_eq!(final_text.as_deref(), Some("The file says: hello ferric"));

    // The provider saw two requests; the second carried the tool result and
    // both carried the constraint (end-to-end plumbing).
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.constraint.is_some()));
    let second = &requests[1];
    let fed_back = second
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("tool result fed back as a message");
    assert_eq!(fed_back.tool_call_id.as_deref(), Some("tc-0"));
    assert_eq!(fed_back.text.as_deref(), Some("hello ferric"));

    // The trace on disk tells the full story.
    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let kinds: Vec<&str> = records
        .iter()
        .map(|r| match &r.event {
            ParsedEvent::Known(Event::SessionStart { .. }) => "start",
            ParsedEvent::Known(Event::ToolCall { .. }) => "call",
            ParsedEvent::Known(Event::ToolResult { .. }) => "result",
            ParsedEvent::Known(Event::SessionEnd { .. }) => "end",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["start", "call", "result", "end"]);
}
