mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::{RecordingSleeper, nano_policy, text_completion, tool_completion};
use ferric_core::{ActionProtocol, FerricError, HarnessPolicy};
use ferric_guard::Workspace;
use ferric_loop::{EditApprover, RunArgs, StopReason, TraceStructure, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{NamedCheck, Registry, register_builtin_tools, register_run_checks};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader, TraceRecord};
use serde_json::json;

/// The operator check re-enters this test binary in the temporary workspace.
/// During the ordinary parent test run there is no fixture, so this is a no-op.
#[test]
fn evidence_check_process() {
    let target = std::path::Path::new("evidence_target.txt");
    if !target.exists() {
        return;
    }
    let content = std::fs::read_to_string(target).unwrap();
    assert!(
        content.contains("value = 3"),
        "verification failed in {}: expected value = 3, got {content:?}",
        std::env::current_dir().unwrap().display()
    );
}

fn evidence_check(marker: &std::path::Path) -> NamedCheck {
    let executable = std::env::current_exe().unwrap();
    #[cfg(windows)]
    let (program, args) = {
        let marker = marker.display().to_string().replace('\'', "''");
        let executable = executable.display().to_string().replace('\'', "''");
        (
            std::path::PathBuf::from("powershell"),
            vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                format!(
                    "Add-Content -LiteralPath '{marker}' -Value spawn; & '{executable}' --exact evidence_check_process --nocapture; exit $LASTEXITCODE"
                ),
            ],
        )
    };
    #[cfg(not(windows))]
    let (program, args) = {
        let quote = |value: &std::path::Path| value.display().to_string().replace('\'', "'\\''");
        (
            std::path::PathBuf::from("sh"),
            vec![
                "-c".to_string(),
                format!(
                    "printf 'spawn\\n' >> '{}'; '{}' --exact evidence_check_process --nocapture",
                    quote(marker),
                    quote(&executable)
                ),
            ],
        )
    };
    NamedCheck {
        name: "unit".to_string(),
        program,
        args,
        timeout_s: 10,
        output_limit: 16_000,
    }
}

struct Capture {
    result: Result<ferric_loop::LoopOutcome, FerricError>,
    records: Vec<TraceRecord>,
}

fn init_workspace(directory: &tempfile::TempDir, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = directory.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }
    for args in [
        vec!["init"],
        vec!["config", "user.name", "Test User"],
        vec!["config", "user.email", "test@example.com"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn run_policy(
    directory: &tempfile::TempDir,
    script: Vec<ferric_provider::Completion>,
    harness_policy: HarnessPolicy,
    checks: Vec<NamedCheck>,
    max_turns: u8,
    custom_prompt: Option<&str>,
    edit_approver: Option<EditApprover<'_>>,
) -> Capture {
    run_policy_with_safety(
        directory,
        script,
        harness_policy,
        checks,
        max_turns,
        custom_prompt,
        edit_approver,
        ferric_guard::Provenance::Clean,
        ferric_guard::SinkPolicy::deny(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_policy_with_safety(
    directory: &tempfile::TempDir,
    script: Vec<ferric_provider::Completion>,
    harness_policy: HarnessPolicy,
    checks: Vec<NamedCheck>,
    max_turns: u8,
    custom_prompt: Option<&str>,
    edit_approver: Option<EditApprover<'_>>,
    provenance: ferric_guard::Provenance,
    sink_policy: ferric_guard::SinkPolicy,
) -> Capture {
    let workspace = Workspace::new(directory.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    if !checks.is_empty() {
        register_run_checks(&mut registry, checks).unwrap();
    }
    let provider = MockProvider::new(script);
    let sleeper = RecordingSleeper::new();
    let trace = directory.path().join(format!("{harness_policy}.jsonl"));
    let mut sink = JsonlSink::open(&trace, format!("{harness_policy}")).unwrap();
    let mut policy = nano_policy();
    policy.max_tools = 32;
    policy.max_turns = max_turns;
    let result = futures_executor::block_on(run(
        RunArgs {
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &policy,
            protocol: ActionProtocol::NativeTools,
            harness_policy: Some(harness_policy),
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: custom_prompt,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance,
            sink_policy,
            hooks: None,
            edit_approver,
        },
        &mut sink,
        Some("complete the requested repository change"),
    ));
    drop(sink);
    let records = TraceReader::open(trace)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    Capture { result, records }
}

fn validate_trace(records: &[TraceRecord]) {
    let mut structure = TraceStructure::new();
    for record in records {
        let ParsedEvent::Known(event) = &record.event else {
            panic!("test trace contains an unknown event")
        };
        structure.observe(event).unwrap_or_else(|error| {
            panic!(
                "event {} failed structure validation: {error}\n{event:#?}",
                record.seq
            )
        });
    }
    structure.finish().unwrap();
}

fn blocks(records: &[TraceRecord]) -> Vec<ferric_trace::ControllerBlockReason> {
    records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::ControllerBlocked { block, .. }) => Some(block.reason),
            _ => None,
        })
        .collect()
}

#[test]
fn full_evidence_path_reads_edits_repairs_verifies_and_completes() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![(
                "read-1",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
            tool_completion(vec![(
                "edit-1",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "value = 1", "new_string": "value = 2"}),
            )]),
            tool_completion(vec![("check-1", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "read-2",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
            tool_completion(vec![(
                "edit-2",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "value = 2", "new_string": "value = 3"}),
            )]),
            tool_completion(vec![("check-2", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "done",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "verified"}),
            )]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        12,
        None,
        None,
    );
    let outcome = capture.result.unwrap();
    assert_eq!(outcome.stop, StopReason::TaskComplete);
    assert_eq!(outcome.final_text.as_deref(), Some("verified"));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("evidence_target.txt")).unwrap(),
        "value = 3\n"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 2);
    assert_eq!(
        capture
            .records
            .iter()
            .filter(|record| matches!(
                record.event,
                ParsedEvent::Known(Event::ObservationRecorded { .. })
            ))
            .count(),
        2
    );
    assert_eq!(
        capture
            .records
            .iter()
            .filter(|record| matches!(
                record.event,
                ParsedEvent::Known(Event::WorkspaceEffectRecorded { .. })
            ))
            .count(),
        2
    );
    let checks: Vec<_> = capture
        .records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::VerificationCheckRecorded { check, .. }) => Some((
                check.outcome,
                check.mutation_epoch,
                check.diagnostic_sha256.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0].0, ferric_trace::VerificationOutcome::Failed);
    assert_eq!(checks[0].1, 1);
    assert!(
        checks[0]
            .2
            .as_ref()
            .is_some_and(|digest| digest.len() == 64)
    );
    assert_eq!(checks[1].0, ferric_trace::VerificationOutcome::Passed);
    assert_eq!(checks[1].1, 2);
    assert_eq!(checks[1].2, None);
    let offered = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::PromptAssembled { offered_tools, .. }) => Some(offered_tools),
            _ => None,
        })
        .unwrap();
    assert!(offered.iter().any(|name| name == "run_check"));
    // Structural mutations (make_dir/delete_path/move_path/copy_file) are now
    // typed and offered under evidence control; only genuinely opaque Write/
    // Execute tools stay excluded.
    for opaque in ["shell_exec", "git_write"] {
        assert!(!offered.iter().any(|name| name == opaque), "{offered:?}");
    }
    validate_trace(&capture.records);
}

#[test]
fn multi_path_effect_advances_one_epoch_and_trace_verifies() {
    let directory = tempfile::tempdir().unwrap();
    init_workspace(&directory, &[("source.txt", "payload\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![("read", "read_file", json!({"path": "source.txt"}))]),
            tool_completion(vec![(
                "move",
                "move_path",
                json!({"from": "source.txt", "to": "destination.txt"}),
            )]),
        ],
        HarnessPolicy::Evidence,
        Vec::new(),
        2,
        None,
        None,
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    assert!(!directory.path().join("source.txt").exists());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("destination.txt")).unwrap(),
        "payload\n"
    );

    let recorded_effects: Vec<_> = capture
        .records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::WorkspaceEffectRecorded {
                call_id,
                tool,
                effect,
                ..
            }) => Some((call_id, tool, effect)),
            _ => None,
        })
        .collect();
    assert_eq!(recorded_effects.len(), 1);
    let (call_id, tool, effect) = recorded_effects[0];
    assert_eq!(call_id, "move");
    assert_eq!(tool, "move_path");
    assert_eq!(effect.mutation_epoch, 1);
    assert_eq!(effect.effects.len(), 2);
    assert!(effect.effects.iter().any(|path_effect| {
        path_effect.path == "source.txt"
            && path_effect.kind == ferric_trace::PathEffectKind::Deleted
    }));
    assert!(effect.effects.iter().any(|path_effect| {
        path_effect.path == "destination.txt"
            && path_effect.kind == ferric_trace::PathEffectKind::Created
    }));

    let final_controller_epoch = capture
        .records
        .iter()
        .rev()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::ControllerCheckpoint { state }) => Some(state.mutation_epoch),
            _ => None,
        })
        .unwrap();
    assert_eq!(final_controller_epoch, 1);
    validate_trace(&capture.records);
}

#[test]
fn blind_and_same_turn_mutations_are_rejected_before_the_callback() {
    for (same_turn, expected) in [
        (false, ferric_trace::ControllerBlockReason::BlindMutation),
        (
            true,
            ferric_trace::ControllerBlockReason::SameTurnObservation,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
        let callbacks = AtomicUsize::new(0);
        let approve = |_preview: &ferric_loop::EditPreview| {
            callbacks.fetch_add(1, Ordering::SeqCst);
            true
        };
        let first = if same_turn {
            tool_completion(vec![
                ("read", "read_file", json!({"path": "evidence_target.txt"})),
                (
                    "edit",
                    "edit_file",
                    json!({"path": "evidence_target.txt", "old_string": "1", "new_string": "2"}),
                ),
            ])
        } else {
            tool_completion(vec![(
                "edit",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "1", "new_string": "2"}),
            )])
        };
        let capture = run_policy(
            &directory,
            vec![first, text_completion("done")],
            HarnessPolicy::Evidence,
            Vec::new(),
            4,
            None,
            Some(&approve),
        );
        assert_eq!(capture.result.unwrap().stop, StopReason::FinalText);
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        assert_eq!(blocks(&capture.records), [expected]);
        assert_eq!(
            std::fs::read_to_string(directory.path().join("evidence_target.txt")).unwrap(),
            "value = 1\n"
        );
        validate_trace(&capture.records);
    }
}

#[test]
fn stale_commit_is_typed_and_an_admitted_call_prompts_once() {
    let directory = tempfile::tempdir().unwrap();
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let callbacks = AtomicUsize::new(0);
    let target = directory.path().join("evidence_target.txt");
    let race = |_preview: &ferric_loop::EditPreview| {
        callbacks.fetch_add(1, Ordering::SeqCst);
        std::fs::write(&target, "external change\n").unwrap();
        true
    };
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![(
                "read",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
            tool_completion(vec![(
                "edit",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "1", "new_string": "2"}),
            )]),
            text_completion("done"),
        ],
        HarnessPolicy::Evidence,
        Vec::new(),
        5,
        None,
        Some(&race),
    );
    assert_eq!(capture.result.unwrap().stop, StopReason::FinalText);
    assert_eq!(callbacks.load(Ordering::SeqCst), 1);
    assert_eq!(
        blocks(&capture.records),
        [ferric_trace::ControllerBlockReason::StaleObservation]
    );
    assert!(!capture.records.iter().any(|record| matches!(
        record.event,
        ParsedEvent::Known(Event::WorkspaceEffectRecorded { .. })
    )));
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "external change\n"
    );
    validate_trace(&capture.records);
}

#[test]
fn no_effect_and_syntax_regression_are_typed_before_approval() {
    for (path, original, replacement, expected) in [
        (
            "same.txt",
            "alpha\n",
            "alpha\n",
            ferric_trace::ControllerBlockReason::NoEffect,
        ),
        (
            "valid.py",
            "value = 1\n",
            "def broken(:\n    pass\n",
            ferric_trace::ControllerBlockReason::SyntaxRegression,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        init_workspace(&directory, &[(path, original)]);
        let callbacks = AtomicUsize::new(0);
        let approve = |_preview: &ferric_loop::EditPreview| {
            callbacks.fetch_add(1, Ordering::SeqCst);
            true
        };
        let capture = run_policy(
            &directory,
            vec![
                tool_completion(vec![("read", "read_file", json!({"path": path}))]),
                tool_completion(vec![(
                    "write",
                    "write_file",
                    json!({"path": path, "content": replacement}),
                )]),
                text_completion("done"),
            ],
            HarnessPolicy::Evidence,
            Vec::new(),
            5,
            None,
            Some(&approve),
        );
        assert_eq!(capture.result.unwrap().stop, StopReason::FinalText);
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        assert_eq!(blocks(&capture.records), [expected]);
        let feedback = capture
            .records
            .iter()
            .find_map(|record| match &record.event {
                ParsedEvent::Known(Event::ToolResult {
                    id,
                    output,
                    is_error: true,
                    ..
                }) if id == "write" => Some(output),
                _ => None,
            })
            .expect("typed preparation block should have model-facing feedback");
        let reason = match expected {
            ferric_trace::ControllerBlockReason::NoEffect => "reason=no_effect",
            ferric_trace::ControllerBlockReason::SyntaxRegression => "reason=syntax_regression",
            _ => unreachable!(),
        };
        assert!(feedback.contains(reason), "{feedback}");
        if expected == ferric_trace::ControllerBlockReason::NoEffect {
            assert!(
                feedback.contains("requested result already equals the current bytes"),
                "{feedback}"
            );
            assert!(
                feedback
                    .contains("changing unrelated or unchanged files does not repair the check"),
                "{feedback}"
            );
        }
        assert!(
            feedback.contains("No human approval is required"),
            "{feedback}"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join(path)).unwrap(),
            original
        );
        validate_trace(&capture.records);
    }
}

#[test]
fn content_and_structural_no_effect_feedback_stays_truthful() {
    let cases = [
        (
            "edit_file",
            json!({"path": "same.txt", "old_string": "missing", "new_string": "beta"}),
            Some(("same.txt", "alpha\n")),
            "the exact requested match was absent",
        ),
        (
            "delete_path",
            json!({"path": "absent.txt"}),
            None,
            "the requested path or source was absent",
        ),
        (
            "move_path",
            json!({"from": "absent.txt", "to": "moved.txt"}),
            None,
            "the requested path or source was absent",
        ),
        (
            "make_dir",
            json!({"path": "present"}),
            None,
            "requested result already equals the current path state",
        ),
    ];

    for (tool, args, file, expected) in cases {
        let directory = tempfile::tempdir().unwrap();
        init_workspace(&directory, &[("init.txt", "init\n")]);
        if let Some((path, content)) = file {
            std::fs::write(directory.path().join(path), content).unwrap();
        }
        if tool == "make_dir" {
            std::fs::create_dir(directory.path().join("present")).unwrap();
        }
        let mut script = Vec::new();
        if tool == "edit_file" {
            script.push(tool_completion(vec![(
                "read",
                "read_file",
                json!({"path": "same.txt"}),
            )]));
        }
        script.push(tool_completion(vec![("no-effect", tool, args)]));
        let max_turns = script.len() as u8;
        let capture = run_policy(
            &directory,
            script,
            HarnessPolicy::Evidence,
            Vec::new(),
            max_turns,
            None,
            None,
        );
        assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
        let feedback = capture
            .records
            .iter()
            .find_map(|record| match &record.event {
                ParsedEvent::Known(Event::ToolResult {
                    id,
                    output,
                    is_error: true,
                    ..
                }) if id == "no-effect" => Some(output),
                _ => None,
            })
            .unwrap();
        assert!(feedback.contains(expected), "{tool}: {feedback}");
        assert!(
            feedback.contains("No human approval is required"),
            "{feedback}"
        );
        validate_trace(&capture.records);
    }
}

#[test]
fn repeated_check_is_blocked_before_a_second_process_starts() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![("check-1", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![("check-2", "run_check", json!({"name": "unit"}))]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        2,
        None,
        None,
    );
    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);
    assert_eq!(
        blocks(&capture.records),
        [ferric_trace::ControllerBlockReason::RepeatedCheck]
    );
    assert_eq!(
        capture
            .records
            .iter()
            .filter(|record| matches!(
                record.event,
                ParsedEvent::Known(Event::VerificationCheckRecorded { .. })
            ))
            .count(),
        1
    );
    let feedback = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                id,
                output,
                is_error: true,
                ..
            }) if id == "check-2" => Some(output),
            _ => None,
        })
        .expect("repeated check should have model-facing feedback");
    assert!(feedback.contains("reason=repeated_check"), "{feedback}");
    assert!(feedback.contains("mutation_epoch=0"), "{feedback}");
    assert!(feedback.contains("check_name=\"unit\""), "{feedback}");
    assert!(
        feedback.contains("If it failed, read the current relevant path in a later turn"),
        "{feedback}"
    );
    assert!(feedback.contains("make a material repair"), "{feedback}");
    assert!(
        feedback.contains("If it passed and all required checks are current, call task_complete"),
        "{feedback}"
    );
    assert!(
        feedback.contains("No human approval is required"),
        "{feedback}"
    );
    validate_trace(&capture.records);
}

#[test]
fn repeated_passed_check_points_to_completion_instead_of_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 3\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![("check-1", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![("check-2", "run_check", json!({"name": "unit"}))]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        2,
        None,
        None,
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);
    let feedback = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                id,
                output,
                is_error: true,
                ..
            }) if id == "check-2" => Some(output),
            _ => None,
        })
        .expect("repeated passed check should have model-facing feedback");
    assert!(feedback.contains("reason=repeated_check"), "{feedback}");
    assert!(
        feedback.contains("If it passed and all required checks are current, call task_complete"),
        "{feedback}"
    );
    assert!(
        !feedback.contains("Next action: make a material mutation"),
        "{feedback}"
    );
    validate_trace(&capture.records);
}

#[test]
fn failed_check_no_effect_and_repair_blocks_allow_the_recovery_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![(
                "read",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
            tool_completion(vec![(
                "edit",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "1", "new_string": "2"}),
            )]),
            tool_completion(vec![("check", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "no-effect",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "2", "new_string": "2"}),
            )]),
            tool_completion(vec![(
                "premature-repair",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "2", "new_string": "3"}),
            )]),
            tool_completion(vec![(
                "repair-inspection",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        6,
        None,
        None,
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    let no_effect = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                id,
                output,
                is_error: true,
                ..
            }) if id == "no-effect" => Some(output),
            _ => None,
        })
        .expect("identity mutation should have model-facing feedback");
    assert!(
        no_effect.contains("requested result already equals the current bytes"),
        "{no_effect}"
    );
    assert!(
        no_effect.contains("inspect the relevant implementation implicated by its diagnostic"),
        "{no_effect}"
    );
    let feedback = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                id,
                output,
                is_error: true,
                ..
            }) if id == "premature-repair" => Some(output),
            _ => None,
        })
        .expect("premature repair should have model-facing feedback");
    assert!(
        feedback.contains("reason=repair_inspection_required"),
        "{feedback}"
    );
    assert!(
        feedback.contains("paths=[\"evidence_target.txt\"]"),
        "{feedback}"
    );
    assert!(
        feedback.contains("read the current relevant path in a later turn"),
        "{feedback}"
    );
    assert!(feedback.contains("make a material repair"), "{feedback}");
    assert!(
        feedback.contains("No human approval is required"),
        "{feedback}"
    );
    assert!(capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::ToolResult {
            id,
            is_error: false,
            ..
        }) if id == "repair-inspection"
    )));
    assert!(!capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::FailureGuard { action }) if action == "stopped"
    )));
    validate_trace(&capture.records);
}

#[test]
fn blocked_task_complete_breaks_execution_failure_streak_before_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![(
                "missing",
                "read_file",
                json!({"path": "missing.txt"}),
            )]),
            tool_completion(vec![("failed-check", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "premature-completion",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "not verified"}),
            )]),
            tool_completion(vec![(
                "blocked-repair",
                "edit_file",
                json!({"path": "evidence_target.txt", "old_string": "1", "new_string": "3"}),
            )]),
            tool_completion(vec![(
                "recovery-read",
                "read_file",
                json!({"path": "evidence_target.txt"}),
            )]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        5,
        None,
        None,
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    assert!(capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::ToolResult {
            id,
            is_error: false,
            ..
        }) if id == "recovery-read"
    )));
    assert!(!capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::FailureGuard { action }) if action == "stopped"
    )));
    let guard_history = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::RecoveryCheckpoint { state }) => Some(&state.guard_history),
            _ => None,
        })
        .expect("max-turns stop should record guard reconstruction state");
    let completion_turn = guard_history
        .iter()
        .find(|guarded| {
            guarded
                .calls
                .iter()
                .any(|call| call.id == "premature-completion")
        })
        .expect("blocked completion turn should remain in guard history");
    assert_eq!(
        (completion_turn.dispatched, completion_turn.errored),
        (1, 1),
        "GuardTurn must preserve the raw synthetic result counts; restoration derives execution-only counts from the task_complete call"
    );
    validate_trace(&capture.records);
}

#[test]
fn real_errors_still_stop_when_a_turn_also_has_a_block_and_blocked_completion() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 1\n")]);
    let capture = run_policy(
        &directory,
        vec![
            tool_completion(vec![(
                "failure-1",
                "read_file",
                json!({"path": "missing-1.txt"}),
            )]),
            tool_completion(vec![(
                "failure-2",
                "list_dir",
                json!({"path": "missing-2"}),
            )]),
            tool_completion(vec![
                ("failure-3", "read_file", json!({"path": "missing-3.txt"})),
                (
                    "blind-block",
                    "write_file",
                    json!({"path": "evidence_target.txt", "content": "value = 2\n"}),
                ),
                (
                    "blocked-completion",
                    ferric_loop::TASK_COMPLETE,
                    json!({"summary": "not verified"}),
                ),
            ]),
        ],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        4,
        None,
        None,
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::RepeatedFailure);
    let checkpoint = capture
        .records
        .iter()
        .find_map(|record| match &record.event {
            ParsedEvent::Known(Event::RecoveryCheckpoint { state }) => Some(state),
            _ => None,
        })
        .unwrap();
    let mixed = checkpoint
        .guard_history
        .iter()
        .find(|guarded| guarded.calls.iter().any(|call| call.id == "blind-block"))
        .unwrap();
    assert_eq!(
        (mixed.dispatched, mixed.errored, mixed.controller_blocks),
        (3, 3, 1),
        "raw results remain durable while one typed block and one completion-gate error are excluded from the one real execution failure"
    );
    validate_trace(&capture.records);
}

#[test]
fn sink_denial_happens_before_a_verification_process_starts() {
    let directory = tempfile::tempdir().unwrap();
    let marker_directory = tempfile::tempdir().unwrap();
    let marker = marker_directory.path().join("check-spawns.txt");
    init_workspace(&directory, &[("evidence_target.txt", "value = 3\n")]);
    let capture = run_policy_with_safety(
        &directory,
        vec![tool_completion(vec![(
            "check",
            "run_check",
            json!({"name": "unit"}),
        )])],
        HarnessPolicy::Evidence,
        vec![evidence_check(&marker)],
        1,
        None,
        None,
        ferric_guard::Provenance::UntrustedIngested,
        ferric_guard::SinkPolicy::deny(),
    );

    assert_eq!(capture.result.unwrap().stop, StopReason::MaxTurns);
    assert!(
        !marker.exists(),
        "a denied check must not start its process"
    );
    assert!(capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::ToolResult {
            output,
            is_error: true,
            ..
        }) if output.contains("sink policy: mutation denied")
    )));
    assert!(!capture.records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::VerificationCheckRecorded { .. })
    )));
    validate_trace(&capture.records);
}

#[test]
fn evidence_guidance_is_added_to_custom_prompts_and_legacy_is_literal() {
    for (policy, guided) in [
        (HarnessPolicy::Evidence, true),
        (HarnessPolicy::Legacy, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        init_workspace(&directory, &[("init.txt", "init\n")]);
        let capture = run_policy(
            &directory,
            vec![tool_completion(vec![(
                "done",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "done"}),
            )])],
            policy,
            Vec::new(),
            2,
            Some("custom system prompt"),
            None,
        );
        assert_eq!(capture.result.unwrap().stop, StopReason::TaskComplete);
        let system = capture
            .records
            .iter()
            .find_map(|record| match &record.event {
                ParsedEvent::Known(Event::SessionPrompt { system, .. }) => Some(system),
                _ => None,
            })
            .unwrap();
        if guided {
            assert!(
                system.starts_with("custom system prompt\n\n[Ferric general evidence guidance v2]")
            );
            assert!(system.contains("paginate incomplete reads"));
            assert!(system.contains("every existing task-scoped workspace file explicitly named"));
            assert!(system.contains("implementation implicated by its diagnostic"));
            assert!(system.contains("do not change unrelated tests or files"));
            assert_eq!(
                system
                    .matches("[Ferric general evidence guidance v2]")
                    .count(),
                1
            );
            assert!(capture.records.iter().any(|record| matches!(
                record.event,
                ParsedEvent::Known(Event::ControllerCheckpoint { .. })
            )));
            validate_trace(&capture.records);
        } else {
            assert_eq!(system, "custom system prompt");
            assert!(!capture.records.iter().any(|record| matches!(
                record.event,
                ParsedEvent::Known(
                    Event::ControllerCheckpoint { .. }
                        | Event::ObservationRecorded { .. }
                        | Event::ControllerBlocked { .. }
                        | Event::WorkspaceEffectRecorded { .. }
                        | Event::VerificationCheckRecorded { .. }
                )
            )));
        }
    }
}

#[test]
fn planner_rejection_writes_no_trace_or_workspace_effect() {
    let directory = tempfile::tempdir().unwrap();
    init_workspace(&directory, &[("init.txt", "init\n")]);
    let capture = run_policy(
        &directory,
        vec![tool_completion(vec![(
            "write",
            "write_file",
            json!({"path": "never.txt", "content": "never"}),
        )])],
        HarnessPolicy::EvidencePlanner,
        Vec::new(),
        2,
        None,
        None,
    );
    assert!(matches!(capture.result, Err(FerricError::InvalidInput(_))));
    assert!(capture.records.is_empty());
    assert!(!directory.path().join("never.txt").exists());
}
