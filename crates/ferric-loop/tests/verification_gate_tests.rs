mod common;

use common::*;
use ferric_core::ActionProtocol;
use ferric_guard::Workspace;
use ferric_loop::{RunArgs, StopReason, run};
use ferric_provider::{MockProvider, SamplingParams};
use ferric_tools::{NamedCheck, Registry, register_builtin_tools, register_run_checks};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader};
use serde_json::json;

fn passing_check(name: &str) -> NamedCheck {
    #[cfg(windows)]
    let (program, args) = (
        std::path::PathBuf::from("where.exe"),
        vec!["cmd.exe".to_string()],
    );
    #[cfg(not(windows))]
    let (program, args) = (std::path::PathBuf::from("true"), Vec::new());
    NamedCheck {
        name: name.to_string(),
        program,
        args,
        timeout_s: 5,
        output_limit: 1_000,
    }
}

fn run_script(
    dir: &tempfile::TempDir,
    script: Vec<ferric_provider::Completion>,
) -> (ferric_loop::LoopOutcome, Vec<ferric_trace::TraceRecord>) {
    let workspace = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    register_run_checks(&mut registry, vec![passing_check("unit")]).unwrap();
    let provider = MockProvider::new(script);
    let sleeper = RecordingSleeper::new();
    let trace = dir.path().join("verification.jsonl");
    let mut sink = JsonlSink::open(&trace, "verification").unwrap();
    let outcome = futures_executor::block_on(run(
        RunArgs {
            provider: &provider,
            registry: &registry,
            workspace: &workspace,
            policy: &nano_policy(),
            protocol: ActionProtocol::NativeTools,
            harness_policy: None,
            sampling: SamplingParams::default(),
            sleeper: &sleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance: ferric_guard::Provenance::Clean,
            sink_policy: ferric_guard::SinkPolicy::deny(),
            hooks: None,
            edit_approver: None,
        },
        &mut sink,
        Some("make the requested change and verify it"),
    ))
    .unwrap();
    drop(sink);
    let records = TraceReader::open(trace)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (outcome, records)
}

#[test]
fn completion_is_blocked_until_every_required_check_passes() {
    let dir = tempfile::tempdir().unwrap();
    let (outcome, records) = run_script(
        &dir,
        vec![
            tool_completion(vec![(
                "done-too-soon",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "unverified"}),
            )]),
            tool_completion(vec![("check-1", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "done-verified",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "verified"}),
            )]),
        ],
    );

    let tool_results = records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                name,
                output,
                is_error,
                ..
            }) => Some((name, output, is_error)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(outcome.stop, StopReason::TaskComplete, "{tool_results:#?}");
    assert_eq!(outcome.final_text.as_deref(), Some("verified"));
    let decisions: Vec<_> = records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::CompletionGate { decision, .. }) => Some(decision.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(decisions, vec!["blocked", "passed"]);
    assert!(records.iter().any(|record| matches!(
        &record.event,
        ParsedEvent::Known(Event::ToolResult {
            id,
            is_error: true,
            output,
            ..
        }) if id == "done-too-soon" && output.contains("Completion is blocked")
    )));
}

#[test]
fn a_later_mutation_makes_prior_check_evidence_stale() {
    let dir = tempfile::tempdir().unwrap();
    let (outcome, records) = run_script(
        &dir,
        vec![
            tool_completion(vec![("check-before", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "write-after",
                "write_file",
                json!({"path": "changed.txt", "content": "changed"}),
            )]),
            tool_completion(vec![(
                "done-stale",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "stale"}),
            )]),
            tool_completion(vec![("check-after", "run_check", json!({"name": "unit"}))]),
            tool_completion(vec![(
                "done-fresh",
                ferric_loop::TASK_COMPLETE,
                json!({"summary": "fresh"}),
            )]),
        ],
    );

    let tool_results = records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::ToolResult {
                name,
                output,
                is_error,
                ..
            }) => Some((name, output, is_error)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(outcome.stop, StopReason::TaskComplete, "{tool_results:#?}");
    assert_eq!(outcome.final_text.as_deref(), Some("fresh"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("changed.txt")).unwrap(),
        "changed"
    );

    let evidence: Vec<_> = records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::VerificationCheckPassed {
                name,
                mutation_epoch,
                ..
            }) => Some(("check", name.clone(), *mutation_epoch, String::new())),
            ParsedEvent::Known(Event::WorkspaceMutation {
                tool,
                mutation_epoch,
                ..
            }) => Some(("mutation", tool.clone(), *mutation_epoch, String::new())),
            ParsedEvent::Known(Event::CompletionGate {
                mutation_epoch,
                decision,
                ..
            }) => Some(("gate", String::new(), *mutation_epoch, decision.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        evidence,
        vec![
            ("check", "unit".to_string(), 0, String::new()),
            ("mutation", "write_file".to_string(), 1, String::new()),
            ("gate", String::new(), 1, "blocked".to_string()),
            ("check", "unit".to_string(), 1, String::new()),
            ("gate", String::new(), 1, "passed".to_string()),
        ]
    );
}
