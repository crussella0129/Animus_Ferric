//! Versioned JSONL trajectory tracing: the source of truth for every Ferric session.
//!
//! The JSONL file is canonical; any pretty rendering (CLI, future TUI) is a
//! derived view. Writers flush per event; readers tolerate unknown event
//! types so traces and binaries can evolve independently.

mod event;
mod reader;
mod sink;

use std::path::{Path, PathBuf};

/// The directory traces are written to, relative to a workspace root.
///
/// Five writers spelled `.join(".ferric").join("trace")` independently, and
/// `ferric dream` — the one *reader* — spelled it `.ferric/traces`. Nothing
/// caught it, because dream's only symptom was a tidy "No .ferric/traces
/// directory found." on a workspace that in fact held traces: **the feature had
/// never once located a trace, and said so in the voice of a clean no-op**
/// (sprint 99).
///
/// One definition, so the reader and the writers cannot disagree again.
pub fn trace_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".ferric").join("trace")
}

pub use event::{
    CONTROLLER_CHECKPOINT_VERSION, CONTROLLER_RECORD_VERSION, CheckExecutionV1,
    ControllerBlockReason, ControllerBlockV1, ControllerBlockWitnessV1, ControllerCheckpointV1,
    Event, FailedCheckV1, FileEvidenceOrigin, FileEvidenceV1, FileObservationV1, GuardTurn,
    LineRangeV1, NavigationObservationV1, ObservationDetailV1, ObservationV1, PathEffectKind,
    PathEffectV1, PreparedPathIdentityV1, PreparedPathStateV1, RECOVERY_CHECKPOINT_VERSION,
    RECOVERY_PACKET_VERSION, RecoveryCheckpointV1, RecoveryPacketV1, RequestedLineRangeV1,
    SyntaxStateV1, TRACE_SCHEMA_VERSION, TraceEvent, TurnBoundary, UnsupportedMutationKindV1,
    VerificationCheckV1, VerificationOutcome, WorkspaceEffectV1,
};
pub use reader::{ParsedEvent, TraceReadMode, TraceReader, TraceRecord};
pub use sink::JsonlSink;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all_event_types() -> Vec<Event> {
        vec![
            Event::MainActionBudget {
                turn: 4,
                budget: ferric_core::OutputBudget {
                    requested: Some(4096),
                    effective: 4096,
                    declared_ctx: Some(32768),
                    source: ferric_core::OutputBudgetSource::Explicit,
                },
            },
            Event::SessionStart {
                workspace: "/tmp/ws".to_string(),
                resumed_from: None,
            },
            Event::PolicySelected {
                tier: ferric_core::Tier::Nano,
                protocol: ferric_core::ActionProtocol::ConstrainedJson,
                // Non-default so this exercises the new additive field rather
                // than succeeding only because deserialization supplied it.
                harness_policy: ferric_core::HarnessPolicy::EvidencePlanner,
                max_turns: 15,
                max_tools: 6,
                prompt_budget_tokens: 2_800,
                max_output_tokens: 512,
                // Deliberately NOT the default: at 4_000 this round-trip would
                // pass even if the field were never serialized at all, because
                // the serde default would supply the same number on the way
                // back in. A value only the writer could have produced is what
                // makes the assertion mean something.
                truncation_limit: 1_234,
                // Not the serde default ("params"), for the same reason the
                // cap above is not 4_000: a round-trip that used the default
                // would pass even if the field were never serialized.
                tier_source: "measured".to_string(),
            },
            Event::PromptComposed {
                output_id: "system-prompt-nano-unified".to_string(),
                output_version: "1.0.0".to_string(),
                composed_of: vec![
                    ("role-declaration".to_string(), "1.0.0".to_string()),
                    ("terminator-teaching".to_string(), "1.0.0".to_string()),
                ],
            },
            Event::SessionPrompt {
                system: "You are Ferric.".to_string(),
                user: "read a.txt".to_string(),
                media: Vec::new(),
            },
            Event::RecoveryCheckpoint {
                state: RecoveryCheckpointV1 {
                    version: RECOVERY_CHECKPOINT_VERSION,
                    messages: vec![
                        ferric_core::Message::system("You are Ferric."),
                        ferric_core::Message::user("read a.txt"),
                    ],
                    next_turn: 1,
                    last_text: Some("working".to_string()),
                    head_len: 2,
                    committed_turn_starts: vec![TurnBoundary {
                        turn: 0,
                        message_index: 2,
                    }],
                    guard_history: vec![GuardTurn {
                        turn: 0,
                        calls: vec![ferric_core::ToolCall {
                            id: "tc-0".to_string(),
                            name: "read_file".to_string(),
                            args: json!({"path": "a.txt"}),
                        }],
                        dispatched: 1,
                        errored: 0,
                        controller_blocks: 0,
                        controller_blocks_was_present: true,
                    }],
                    nudged_for_no_action: false,
                    truncated_once: false,
                    last_input_tokens: Some(50),
                    pending_input: Some(ferric_core::UserInputRequest {
                        question: "Which branch?".to_string(),
                        context: "Two branches are available.".to_string(),
                        options: vec!["dev".to_string(), "main".to_string()],
                    }),
                    mutation_epoch: 2,
                    passed_checks: std::collections::BTreeMap::from([("unit".to_string(), 2)]),
                },
            },
            Event::ResumePrompt {
                user: "Use dev.".to_string(),
                media: Vec::new(),
            },
            Event::TurnStart { turn: 0 },
            Event::PromptAssembled {
                turn: 0,
                message_count: 2,
                chars: 512,
                offered_tools: vec!["read_file".to_string(), "task_complete".to_string()],
            },
            Event::ConstraintApplied {
                kind: "json_schema".to_string(),
            },
            Event::TurnEnd {
                turn: 0,
                text: Some("reading".to_string()),
                tool_call_count: 1,
                input_tokens: Some(50),
                output_tokens: Some(12),
                truncated: false,
            },
            Event::ActionsProposed {
                turn: 0,
                calls: vec![ferric_core::ToolCall {
                    id: "tc-1".to_string(),
                    name: "read_file".to_string(),
                    args: json!({"path": "a.txt"}),
                }],
            },
            Event::RepetitionGuard {
                action: "warned".to_string(),
            },
            Event::PermissionCheck {
                path: "src/main.rs".to_string(),
                decision: "allow".to_string(),
                rule: None,
                matched: None,
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
            Event::ObservationRecorded {
                turn: 0,
                call_id: "tc-1".to_string(),
                observation: ObservationV1 {
                    version: CONTROLLER_RECORD_VERSION,
                    detail: ObservationDetailV1::File(FileObservationV1 {
                        path: "a.txt".to_string(),
                        sha256: "a".repeat(64),
                        total_bytes: 8,
                        total_lines: 1,
                        requested_range: Some(RequestedLineRangeV1 {
                            start: Some(1),
                            end: Some(1),
                        }),
                        returned_range: Some(LineRangeV1 { start: 1, end: 1 }),
                        complete: true,
                        model_truncated: false,
                    }),
                },
            },
            Event::ControllerBlocked {
                turn: 0,
                call_id: "tc-2".to_string(),
                tool: "write_file".to_string(),
                block: ControllerBlockV1 {
                    version: CONTROLLER_RECORD_VERSION,
                    reason: ControllerBlockReason::SameTurnObservation,
                    mutation_epoch: 2,
                    paths: vec!["a.txt".to_string()],
                    check_name: None,
                    witness: None,
                },
            },
            Event::WorkspaceEffectRecorded {
                turn: 0,
                call_id: "tc-3".to_string(),
                tool: "write_file".to_string(),
                effect: WorkspaceEffectV1 {
                    version: CONTROLLER_RECORD_VERSION,
                    mutation_epoch: 3,
                    effects: vec![PathEffectV1 {
                        path: "a.txt".to_string(),
                        kind: PathEffectKind::Modified,
                        before_sha256: Some("a".repeat(64)),
                        after_sha256: Some("b".repeat(64)),
                        after_bytes: Some(9),
                        after_lines: Some(1),
                    }],
                },
            },
            Event::VerificationCheckRecorded {
                turn: 0,
                call_id: "tc-4".to_string(),
                check: VerificationCheckV1 {
                    version: CONTROLLER_RECORD_VERSION,
                    name: "unit".to_string(),
                    mutation_epoch: 3,
                    attempt: 1,
                    outcome: VerificationOutcome::Failed,
                    diagnostic_sha256: Some("c".repeat(64)),
                },
            },
            Event::WorkspaceMutation {
                turn: 0,
                tool: "write_file".to_string(),
                mutation_epoch: 2,
            },
            Event::VerificationCheckPassed {
                turn: 0,
                name: "unit".to_string(),
                mutation_epoch: 2,
            },
            Event::ControllerCheckpoint {
                state: ControllerCheckpointV1 {
                    version: CONTROLLER_CHECKPOINT_VERSION,
                    harness_policy: ferric_core::HarnessPolicy::EvidencePlanner,
                    mutation_epoch: 3,
                    required_checks: vec!["unit".to_string()],
                    passed_checks: std::collections::BTreeMap::from([("lint".to_string(), 3)]),
                    file_evidence: vec![FileEvidenceV1 {
                        path: "a.txt".to_string(),
                        sha256: "b".repeat(64),
                        total_bytes: 9,
                        total_lines: 1,
                        covered_ranges: vec![LineRangeV1 { start: 1, end: 1 }],
                        complete: true,
                        fresh: true,
                        observed_turn: 0,
                        origin: FileEvidenceOrigin::ModelRead,
                    }],
                    check_executions: vec![CheckExecutionV1 {
                        turn: 0,
                        name: "unit".to_string(),
                        mutation_epoch: 3,
                        attempt: 1,
                        outcome: VerificationOutcome::Failed,
                        diagnostic_sha256: Some("c".repeat(64)),
                    }],
                    last_failed_check: Some(FailedCheckV1 {
                        turn: 0,
                        name: "unit".to_string(),
                        mutation_epoch: 3,
                        attempt: 1,
                        diagnostic_sha256: "c".repeat(64),
                    }),
                    changed_paths: vec!["a.txt".to_string()],
                    repair_paths: vec!["a.txt".to_string()],
                    repair_observation_after_turn: Some(0),
                    inherited_pause_reason: Some("max_turns".to_string()),
                },
            },
            Event::RecoveryPacketInjected {
                packet: RecoveryPacketV1 {
                    version: RECOVERY_PACKET_VERSION,
                    pause_reason: "max_turns".to_string(),
                    mutation_epoch: 3,
                    required_checks: vec!["unit".to_string()],
                    passed_checks: std::collections::BTreeMap::from([("lint".to_string(), 3)]),
                    last_failed_check: Some(FailedCheckV1 {
                        turn: 0,
                        name: "unit".to_string(),
                        mutation_epoch: 3,
                        attempt: 1,
                        diagnostic_sha256: "c".repeat(64),
                    }),
                    changed_paths: vec!["a.txt".to_string()],
                    reread_paths: vec!["a.txt".to_string()],
                },
                message: "Resume from measured evidence.".to_string(),
            },
            Event::TurnCommitted {
                turn: 0,
                dispatched: 1,
                errored: 0,
                stop_reason: None,
                snapshot_commit: Some("0123456789abcdef".to_string()),
            },
            Event::CompletionGate {
                mutation_epoch: 2,
                required_checks: vec!["unit".to_string()],
                fresh_checks: vec!["unit".to_string()],
                decision: "passed".to_string(),
            },
            Event::Note {
                text: "checkpoint".to_string(),
            },
            Event::HistoryCompacted {
                through_turn: 4,
                dropped_turns: 5,
                summary: "created a.txt and b.txt".to_string(),
            },
            Event::SessionPaused {
                reason: "max_turns".to_string(),
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
    fn controller_block_reason_wire_labels_are_explicit_and_stable() {
        let cases = [
            (ControllerBlockReason::BlindMutation, "blind_mutation"),
            (
                ControllerBlockReason::SameTurnObservation,
                "same_turn_observation",
            ),
            (ControllerBlockReason::StaleObservation, "stale_observation"),
            (
                ControllerBlockReason::UnsupportedMutation,
                "unsupported_mutation",
            ),
            (
                ControllerBlockReason::RepairInspectionRequired,
                "repair_inspection_required",
            ),
            (ControllerBlockReason::NoEffect, "no_effect"),
            (ControllerBlockReason::SyntaxRegression, "syntax_regression"),
            (ControllerBlockReason::RepeatedCheck, "repeated_check"),
        ];

        for (reason, label) in cases {
            assert_eq!(serde_json::to_value(reason).unwrap(), json!(label));
            assert_eq!(
                serde_json::from_value::<ControllerBlockReason>(json!(label)).unwrap(),
                reason
            );
        }
    }

    #[test]
    fn requested_range_shapes_and_large_navigation_counts_roundtrip_losslessly() {
        let ranges = [
            None,
            Some(RequestedLineRangeV1 {
                start: Some(2),
                end: None,
            }),
            Some(RequestedLineRangeV1 {
                start: None,
                end: Some(7),
            }),
            Some(RequestedLineRangeV1 {
                start: Some(2),
                end: Some(7),
            }),
        ];
        for requested_range in ranges {
            let observation = ObservationV1 {
                version: CONTROLLER_RECORD_VERSION,
                detail: ObservationDetailV1::File(FileObservationV1 {
                    path: "src/lib.rs".to_string(),
                    sha256: "a".repeat(64),
                    total_bytes: 20,
                    total_lines: 10,
                    requested_range,
                    returned_range: Some(LineRangeV1 { start: 2, end: 7 }),
                    complete: false,
                    model_truncated: false,
                }),
            };
            let encoded = serde_json::to_string(&observation).unwrap();
            assert_eq!(
                serde_json::from_str::<ObservationV1>(&encoded).unwrap(),
                observation
            );
        }

        let navigation = NavigationObservationV1 {
            root: ".".to_string(),
            literal: "needle".to_string(),
            match_count: u64::MAX - 1,
            max_results: u64::MAX,
            exhausted: true,
            result_sha256: "b".repeat(64),
        };
        let encoded = serde_json::to_string(&navigation).unwrap();
        assert_eq!(
            serde_json::from_str::<NavigationObservationV1>(&encoded).unwrap(),
            navigation
        );
    }

    #[test]
    fn controller_block_witnesses_roundtrip_and_old_optional_fields_remain_readable() {
        let block = ControllerBlockV1 {
            version: CONTROLLER_RECORD_VERSION,
            reason: ControllerBlockReason::NoEffect,
            mutation_epoch: 2,
            paths: vec!["a.txt".to_string()],
            check_name: None,
            witness: Some(ControllerBlockWitnessV1::NoEffect {
                states: vec![PreparedPathStateV1 {
                    path: "a.txt".to_string(),
                    before: PreparedPathIdentityV1::File {
                        sha256: "a".repeat(64),
                        bytes: 4,
                    },
                    candidate: PreparedPathIdentityV1::File {
                        sha256: "a".repeat(64),
                        bytes: 4,
                    },
                }],
            }),
        };
        let encoded = serde_json::to_string(&block).unwrap();
        assert_eq!(
            serde_json::from_str::<ControllerBlockV1>(&encoded).unwrap(),
            block
        );

        let unsupported_check = ControllerBlockV1 {
            version: CONTROLLER_RECORD_VERSION,
            reason: ControllerBlockReason::UnsupportedMutation,
            mutation_epoch: 2,
            paths: Vec::new(),
            check_name: Some("unknown".to_string()),
            witness: Some(ControllerBlockWitnessV1::UnsupportedMutation {
                control_kind: UnsupportedMutationKindV1::UnsupportedOperation,
            }),
        };
        let encoded = serde_json::to_string(&unsupported_check).unwrap();
        assert_eq!(
            serde_json::from_str::<ControllerBlockV1>(&encoded).unwrap(),
            unsupported_check
        );

        let old_block =
            r#"{"version":1,"reason":"blind_mutation","mutation_epoch":0,"paths":["a.txt"]}"#;
        assert_eq!(
            serde_json::from_str::<ControllerBlockV1>(old_block)
                .unwrap()
                .witness,
            None
        );
        let old_effect = r#"{"path":"a.txt","kind":"modified","before_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","after_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#;
        let effect: PathEffectV1 = serde_json::from_str(old_effect).unwrap();
        assert_eq!(effect.after_bytes, None);
        assert_eq!(effect.after_lines, None);
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
    fn checkpoint_additive_collections_and_pending_input_default_when_omitted() {
        let encoded = r#"{
            "type":"recovery_checkpoint",
            "state":{
                "version":1,
                "messages":[],
                "next_turn":0,
                "last_text":null,
                "head_len":0,
                "committed_turn_starts":[],
                "nudged_for_no_action":false,
                "truncated_once":false,
                "last_input_tokens":null
            }
        }"#;
        let event: Event = serde_json::from_str(encoded).unwrap();
        let Event::RecoveryCheckpoint { state } = event else {
            panic!("expected checkpoint");
        };
        assert!(state.guard_history.is_empty());
        assert!(state.pending_input.is_none());
    }

    #[test]
    fn guard_turn_controller_blocks_are_additive_and_zero_omitted() {
        let old = r#"{
            "turn":3,
            "calls":[{"id":"c","name":"edit_file","args":{"path":"a.txt"}}],
            "dispatched":1,
            "errored":1
        }"#;
        let parsed: GuardTurn = serde_json::from_str(old).unwrap();
        assert_eq!(parsed.controller_blocks, 0);
        assert!(!parsed.controller_blocks_was_present);
        assert!(
            !serde_json::to_string(&parsed)
                .unwrap()
                .contains("controller_blocks")
        );

        let with_block = GuardTurn {
            controller_blocks: 1,
            ..parsed
        };
        let encoded = serde_json::to_string(&with_block).unwrap();
        assert!(encoded.contains("\"controller_blocks\":1"), "{encoded}");
        let reparsed = serde_json::from_str::<GuardTurn>(&encoded).unwrap();
        assert_eq!(reparsed, with_block);
        assert!(reparsed.controller_blocks_was_present);

        let explicit_zero: GuardTurn = serde_json::from_str(
            r#"{
                "turn":3,
                "calls":[{"id":"c","name":"edit_file","args":{"path":"a.txt"}}],
                "dispatched":1,
                "errored":1,
                "controller_blocks":0
            }"#,
        )
        .unwrap();
        assert!(explicit_zero.controller_blocks_was_present);

        let explicit_null = r#"{
            "turn":3,
            "calls":[{"id":"c","name":"edit_file","args":{"path":"a.txt"}}],
            "dispatched":1,
            "errored":1,
            "controller_blocks":null
        }"#;
        assert!(serde_json::from_str::<GuardTurn>(explicit_null).is_err());
    }

    /// ADR-093 added `truncation_limit`, and Sprint 113 added
    /// `harness_policy`, to `policy_selected`. A literal older line must pick
    /// up both safe historical defaults rather than fail or guess.
    #[test]
    fn an_old_policy_line_reads_back_at_the_default_cap_and_legacy_harness() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let old_line = r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"policy_selected","tier":"nano","protocol":"constrained_json","max_turns":15,"max_tools":6,"prompt_budget_tokens":2800,"max_output_tokens":512}}"#;
        std::fs::write(&path, format!("{old_line}\n")).unwrap();

        let records: Vec<_> = TraceReader::open(&path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        match &records[0].event {
            ParsedEvent::Known(Event::PolicySelected {
                harness_policy,
                truncation_limit,
                ..
            }) => {
                assert_eq!(*harness_policy, ferric_core::HarnessPolicy::Legacy);
                assert_eq!(*truncation_limit, ferric_core::DEFAULT_TRUNCATION_LIMIT);
            }
            other => panic!("expected a Known PolicySelected, got {other:?}"),
        }
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
    fn create_new_refuses_to_append_a_second_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let mut first = JsonlSink::create_new(&path, "first").unwrap();
        first
            .write_event(Event::Note {
                text: "first".to_string(),
            })
            .unwrap();
        drop(first);

        let error = match JsonlSink::create_new(&path, "second") {
            Ok(_) => panic!("create_new appended a second session"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ferric_core::FerricError::Io(ref io)
                if io.kind() == std::io::ErrorKind::AlreadyExists
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn s0_trace_still_parses() {
        // A fixture in the exact s0 wire format (pre-s1 event set only) must
        // keep parsing as Known events forever.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s0.jsonl");
        let s0_lines = [
            r#"{"v":1,"ts_ms":1,"session":"s0","seq":0,"event":{"type":"session_start","workspace":"/ws"}}"#,
            r#"{"v":1,"ts_ms":2,"session":"s0","seq":1,"event":{"type":"tool_call","id":"tc-1","name":"read_file","args":{"path":"a.txt"}}}"#,
            r#"{"v":1,"ts_ms":3,"session":"s0","seq":2,"event":{"type":"tool_result","id":"tc-1","name":"read_file","output":"x","is_error":false,"duration_ms":1}}"#,
            r#"{"v":1,"ts_ms":4,"session":"s0","seq":3,"event":{"type":"note","text":"n"}}"#,
            r#"{"v":1,"ts_ms":5,"session":"s0","seq":4,"event":{"type":"session_end","reason":"done"}}"#,
        ];
        std::fs::write(&path, s0_lines.join("\n")).unwrap();
        let records: Vec<_> = TraceReader::open(&path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(records.len(), 5);
        assert!(
            records
                .iter()
                .all(|r| matches!(r.event, ParsedEvent::Known(_))),
            "every s0 event must still parse as Known"
        );
    }

    #[test]
    fn turn_end_carries_completion() {
        let event = Event::TurnEnd {
            turn: 3,
            text: Some("the answer".to_string()),
            tool_call_count: 0,
            input_tokens: Some(120),
            output_tokens: Some(8),
            truncated: false,
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: Event = serde_json::from_str(&encoded).unwrap();
        assert_eq!(event, decoded);
        assert!(encoded.contains("the answer"));
    }

    /// T-3901 (sprint 39): `SessionStart.resumed_from` round-trips when set.
    #[test]
    fn session_start_resumed_from_roundtrip() {
        let event = Event::SessionStart {
            workspace: "/tmp/ws".to_string(),
            resumed_from: Some("q-1720000000000".to_string()),
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: Event = serde_json::from_str(&encoded).unwrap();
        assert_eq!(event, decoded);
        assert!(encoded.contains("q-1720000000000"));
    }

    /// T-3901 (sprint 39): a pre-sprint-39 `session_start` line (no
    /// `resumed_from` key at all) still parses as `Known`, defaulting to
    /// `None` — additive backward compat (ADR-002).
    #[test]
    fn old_session_start_line_parses_with_none_resumed_from() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.jsonl");
        let line = r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"session_start","workspace":"/ws"}}"#;
        std::fs::write(&path, line).unwrap();
        let record = TraceReader::open(&path).unwrap().next().unwrap().unwrap();
        assert_eq!(
            record.event,
            ParsedEvent::Known(Event::SessionStart {
                workspace: "/ws".to_string(),
                resumed_from: None,
            })
        );
    }

    /// T-3902 (sprint 39): `TurnEnd.truncated` round-trips when `true`.
    #[test]
    fn turn_end_truncated_roundtrip() {
        let event = Event::TurnEnd {
            turn: 1,
            text: Some("cut off".to_string()),
            tool_call_count: 0,
            input_tokens: Some(50),
            output_tokens: Some(512),
            truncated: true,
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: Event = serde_json::from_str(&encoded).unwrap();
        assert_eq!(event, decoded);
        assert!(encoded.contains("\"truncated\":true"));
    }

    /// T-3902 (sprint 39): a pre-sprint-39 `turn_end` line (no `truncated`
    /// key) still parses as `Known`, defaulting to `false`.
    #[test]
    fn old_turn_end_line_parses_with_truncated_false() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.jsonl");
        let line = r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"turn_end","turn":0,"text":"hi","tool_call_count":0,"input_tokens":10,"output_tokens":2}}"#;
        std::fs::write(&path, line).unwrap();
        let record = TraceReader::open(&path).unwrap().next().unwrap().unwrap();
        assert_eq!(
            record.event,
            ParsedEvent::Known(Event::TurnEnd {
                turn: 0,
                text: Some("hi".to_string()),
                tool_call_count: 0,
                input_tokens: Some(10),
                output_tokens: Some(2),
                truncated: false,
            })
        );
    }

    /// T-3901 (sprint 39): `Event::SessionPrompt` round-trips, including
    /// attached media.
    #[test]
    fn session_prompt_roundtrip_with_media() {
        let event = Event::SessionPrompt {
            system: "You are Ferric.".to_string(),
            user: "describe this image".to_string(),
            media: vec![ferric_core::MediaPart {
                mime: "image/png".to_string(),
                data: "aGVsbG8=".to_string(),
            }],
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: Event = serde_json::from_str(&encoded).unwrap();
        assert_eq!(event, decoded);
    }

    /// T-4001 (sprint 40): `Event::HistoryCompacted` round-trips exactly.
    #[test]
    fn history_compacted_roundtrip() {
        let event = Event::HistoryCompacted {
            through_turn: 6,
            dropped_turns: 7,
            summary: "wrote out.txt then read config.json".to_string(),
        };
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: Event = serde_json::from_str(&encoded).unwrap();
        assert_eq!(event, decoded);
        assert!(encoded.contains("wrote out.txt then read config.json"));
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

    /// The reader and the writers must agree on where traces live.
    ///
    /// `ferric dream` read `.ferric/traces` while every writer wrote
    /// `.ferric/trace`, so it never located a trace and reported that as a
    /// clean "no traces found" — a dead feature that looked like an idle one.
    /// Pinning the literal is the point: it is a path contract between crates,
    /// and the failure it guards against is silent by nature.
    #[test]
    fn the_trace_directory_is_ferric_trace() {
        let dir = trace_dir(Path::new("/ws"));
        assert!(
            dir.ends_with("trace"),
            "must be `trace`, not `traces`: {}",
            dir.display()
        );
        assert_eq!(
            dir,
            Path::new("/ws").join(".ferric").join("trace"),
            "the on-disk contract every writer and `ferric dream` depend on"
        );
    }

    /// A trace written through the public sink must land in `trace_dir`, so the
    /// agreement above is with the code that actually writes, not just a
    /// restatement of the constant.
    #[test]
    fn a_written_trace_lands_in_the_trace_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = trace_dir(tmp.path());
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("session.jsonl");
        let mut sink = JsonlSink::open(&path, "s-1").unwrap();
        sink.write_event(Event::SessionStart {
            workspace: "/ws".to_string(),
            resumed_from: None,
        })
        .unwrap();

        // Scan the way `ferric dream` scans: list the directory for *.jsonl.
        let found = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
            .count();
        assert_eq!(
            found,
            1,
            "a reader scanning {} must find the written trace",
            dir.display()
        );
    }
}
