//! Versioned JSONL trajectory tracing: the source of truth for every Ferric session.
//!
//! The JSONL file is canonical; any pretty rendering (CLI, future TUI) is a
//! derived view. Writers flush per event; readers tolerate unknown event
//! types so traces and binaries can evolve independently.

mod event;
mod reader;
mod sink;

pub use event::{Event, TRACE_SCHEMA_VERSION, TraceEvent};
pub use reader::{ParsedEvent, TraceReader, TraceRecord};
pub use sink::JsonlSink;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all_event_types() -> Vec<Event> {
        vec![
            Event::SessionStart {
                workspace: "/tmp/ws".to_string(),
            },
            Event::ToolCall {
                id: "tc-1".to_string(),
                name: "read_file".to_string(),
                args: json!({"path": "a.txt"}),
            },
            Event::ToolResult {
                id: "tc-1".to_string(),
                name: "read_file".to_string(),
                output: "contents".to_string(),
                is_error: false,
                duration_ms: 3,
            },
            Event::Note {
                text: "checkpoint".to_string(),
            },
            Event::SessionEnd {
                reason: "done".to_string(),
            },
        ]
    }

    #[test]
    fn jsonl_roundtrip_all_event_types() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let events = all_event_types();
        let mut sink = JsonlSink::open(&path, "s-1").unwrap();
        for event in &events {
            sink.write_event(event.clone()).unwrap();
        }
        // Read back while the sink is still alive: flush-per-event means the
        // data must already be durable.
        let records: Vec<_> = TraceReader::open(&path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), events.len());
        for (record, event) in records.iter().zip(&events) {
            assert_eq!(record.v, TRACE_SCHEMA_VERSION);
            assert_eq!(record.session, "s-1");
            assert_eq!(record.event, ParsedEvent::Known(event.clone()));
        }
    }

    #[test]
    fn reader_tolerates_unknown_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let future_line = r#"{"v":9,"ts_ms":1,"session":"s","seq":0,"event":{"type":"FUTURE_EVENT","payload":{"x":1}}}"#;
        let known_line =
            r#"{"v":1,"ts_ms":2,"session":"s","seq":1,"event":{"type":"note","text":"hi"}}"#;
        std::fs::write(&path, format!("{future_line}\n{known_line}\n")).unwrap();

        let records: Vec<_> = TraceReader::open(&path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 2);
        match &records[0].event {
            ParsedEvent::Unknown(raw) => {
                assert_eq!(raw["type"], "FUTURE_EVENT");
                assert_eq!(raw["payload"]["x"], 1);
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
        assert_eq!(
            records[1].event,
            ParsedEvent::Known(Event::Note {
                text: "hi".to_string()
            })
        );
    }

    #[test]
    fn seq_monotonic_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let mut sink = JsonlSink::open(&path, "s-1").unwrap();
        for i in 0..100 {
            let seq = sink
                .write_event(Event::Note {
                    text: format!("n{i}"),
                })
                .unwrap();
            assert_eq!(seq, i);
        }
        let seqs: Vec<u64> = TraceReader::open(&path)
            .unwrap()
            .map(|r| r.unwrap().seq)
            .collect();
        assert_eq!(seqs, (0..100).collect::<Vec<u64>>());
    }

    #[test]
    fn tool_result_full_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let big_output = "x".repeat(100_000);
        let mut sink = JsonlSink::open(&path, "s-1").unwrap();
        sink.write_event(Event::ToolResult {
            id: "tc-1".to_string(),
            name: "read_file".to_string(),
            output: big_output.clone(),
            is_error: false,
            duration_ms: 1,
        })
        .unwrap();

        let record = TraceReader::open(&path).unwrap().next().unwrap().unwrap();
        match record.event {
            ParsedEvent::Known(Event::ToolResult { output, .. }) => {
                assert_eq!(output, big_output);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
