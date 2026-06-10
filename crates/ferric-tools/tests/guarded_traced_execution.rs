//! s0 capstone integration: guard + tools + trace in one path.
//!
//! Executes real file tools through the registry chokepoint while tracing
//! every call, then asserts the trace tells the whole story — including a
//! denial, which must be traced AND leave the target untouched.

use ferric_guard::Workspace;
use ferric_tools::{ExecuteOutcome, Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};
use serde_json::json;

#[test]
fn guarded_traced_execution() {
    let ws_dir = tempfile::tempdir().unwrap();
    let trace_dir = tempfile::tempdir().unwrap();
    let trace_path = trace_dir.path().join("trace.jsonl");

    let workspace = Workspace::new(ws_dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let mut sink = JsonlSink::open(&trace_path, "it-1").unwrap();

    sink.write_event(Event::SessionStart {
        workspace: ws_dir.path().display().to_string(),
    })
    .unwrap();

    // Tool pair: write then read, tracing call + result around each execute.
    let calls = [
        (
            "write_file",
            json!({"path": "story.md", "content": "ferric"}),
        ),
        ("read_file", json!({"path": "story.md"})),
    ];
    for (i, (name, args)) in calls.iter().enumerate() {
        let id = format!("tc-{i}");
        sink.write_event(Event::ToolCall {
            id: id.clone(),
            name: name.to_string(),
            args: args.clone(),
        })
        .unwrap();
        match registry.execute(&workspace, name, args) {
            ExecuteOutcome::Completed {
                output,
                duration_ms,
            } => {
                sink.write_event(Event::ToolResult {
                    id,
                    name: name.to_string(),
                    output: output.full,
                    is_error: output.is_error,
                    duration_ms,
                })
                .unwrap();
            }
            other => panic!("{name} should complete, got {other:?}"),
        }
    }

    // Denied call: traced, handler never runs, file untouched.
    let denied_args = json!({"path": ".git/config", "content": "evil"});
    sink.write_event(Event::ToolCall {
        id: "tc-deny".to_string(),
        name: "write_file".to_string(),
        args: denied_args.clone(),
    })
    .unwrap();
    match registry.execute(&workspace, "write_file", &denied_args) {
        ExecuteOutcome::Denied { reason } => {
            sink.write_event(Event::ToolResult {
                id: "tc-deny".to_string(),
                name: "write_file".to_string(),
                output: format!("DENIED: {reason}"),
                is_error: true,
                duration_ms: 0,
            })
            .unwrap();
        }
        other => panic!("expected Denied, got {other:?}"),
    }
    assert!(
        !ws_dir.path().join(".git").exists(),
        "denied write must create nothing"
    );

    sink.write_event(Event::SessionEnd {
        reason: "test complete".to_string(),
    })
    .unwrap();

    // The trace tells the whole story, in order, with full outputs.
    let records: Vec<_> = TraceReader::open(&trace_path)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (0..records.len() as u64).collect::<Vec<_>>());

    let mut tool_results = records.iter().filter_map(|r| match &r.event {
        ParsedEvent::Known(Event::ToolResult {
            name,
            output,
            is_error,
            ..
        }) => Some((name.clone(), output.clone(), *is_error)),
        _ => None,
    });
    let (name, output, is_error) = tool_results.next().unwrap();
    assert_eq!(name, "write_file");
    assert!(output.contains("wrote 6 bytes"));
    assert!(!is_error);
    let (name, output, is_error) = tool_results.next().unwrap();
    assert_eq!(name, "read_file");
    assert_eq!(output, "ferric");
    assert!(!is_error);
    let (name, output, is_error) = tool_results.next().unwrap();
    assert_eq!(name, "write_file");
    assert!(output.contains("DENIED"));
    assert!(is_error);
}
