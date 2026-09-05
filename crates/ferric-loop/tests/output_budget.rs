//! T-12101: main-action output budgets do not grant authority or change the
//! independent compactor. The 24 KiB provider below is deliberately scripted:
//! its 4096-token threshold is NOT a tokenizer measurement or real-model claim.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ferric_core::{
    ActionProtocol, HarnessPolicy, Message, ModelProfile, OutputBudget, OutputBudgetSource, Role,
    RunPolicy, policy_for, resolve_output_budget,
};
use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_loop::{EditApprover, EditPreview, LoopOutcome, RunArgs, Sleeper, StopReason, run};
use ferric_provider::{
    Capabilities, Completion, CompletionRequest, MockProvider, Provider, ProviderError,
    SamplingParams,
};
use ferric_tools::{Registry, register_builtin_tools};
use ferric_trace::{Event, JsonlSink, ParsedEvent, TraceReader, TraceRecord};
use serde_json::json;

const DECLARED_CONTEXT: u32 = 32_768;
const LARGE_ACTION_CAP: u32 = 4096;
const PAYLOAD_BYTES: usize = 24 * 1024;
const TARGET: &str = "budget-output.txt";

fn base_policy() -> RunPolicy {
    policy_for(&ModelProfile {
        params_b: 1.0,
        quant: "Q4_K_M".to_owned(),
        ctx: DECLARED_CONTEXT,
        family: "scripted-budget-fixture".to_owned(),
        measured_level: None,
    })
}

fn with_budget(mut policy: RunPolicy, requested: Option<u32>) -> RunPolicy {
    let budget = resolve_output_budget(&policy, DECLARED_CONTEXT, requested).unwrap();
    policy.max_output_tokens = budget.effective;
    policy.output_budget = Some(budget);
    policy
}

fn sampling(cap: u32) -> SamplingParams {
    SamplingParams {
        temperature: 0.0,
        max_tokens: cap,
        ..SamplingParams::default()
    }
}

fn payload() -> String {
    let alphabet = b"0123456789abcdef\n\"\\\t";
    String::from_utf8(
        alphabet
            .iter()
            .copied()
            .cycle()
            .take(PAYLOAD_BYTES)
            .collect(),
    )
    .unwrap()
}

fn action(tool: &str, args: serde_json::Value, input_tokens: u32) -> Completion {
    Completion {
        message: Message::assistant(json!({"tool": tool, "args": args}).to_string()),
        input_tokens: Some(input_tokens),
        output_tokens: Some(20),
        truncated: false,
    }
}

struct NoRetrySleeper;

impl Sleeper for NoRetrySleeper {
    fn sleep(&self, _duration: Duration) {
        panic!("these deterministic fixtures must not enter provider-error backoff");
    }
}

struct Capture {
    outcome: LoopOutcome,
    records: Vec<TraceRecord>,
}

fn run_fixture(
    directory: &Path,
    provider: &dyn Provider,
    policy: &RunPolicy,
    sampling: SamplingParams,
    harness: HarnessPolicy,
    approver: Option<EditApprover<'_>>,
) -> Capture {
    let workspace = Workspace::new(directory).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let trace = directory.join("budget-trace.jsonl");
    let mut sink = JsonlSink::open(&trace, "output-budget-fixture").unwrap();
    // No Git initialization or executable fixture is introduced here. The real
    // loop's existing non-repository snapshot failure remains a traced note.
    let outcome = futures_executor::block_on(run(
        RunArgs {
            provider,
            registry: &registry,
            workspace: &workspace,
            policy,
            protocol: ActionProtocol::ConstrainedJson,
            harness_policy: Some(harness),
            sampling,
            sleeper: &NoRetrySleeper,
            system_prompt: None,
            prompt_lineage: None,
            media: Vec::new(),
            stream_sink: None,
            resume: None,
            answer: None,
            cancel_flag: None,
            provenance: Provenance::Clean,
            sink_policy: SinkPolicy::deny(),
            hooks: None,
            edit_approver: approver,
        },
        &mut sink,
        Some("perform the scripted workspace action"),
    ))
    .unwrap();
    drop(sink);
    let records = TraceReader::open(trace)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    Capture { outcome, records }
}

/// At most two requests. Below the scripted threshold, return a cut JSON
/// action. `force_truncated` also tests a syntactically complete first action
/// marked truncated: admission must honor the flag before parsing or dispatch.
struct BudgetSensitiveProvider {
    target: PathBuf,
    full_action: String,
    force_truncated: bool,
    requests: Mutex<Vec<CompletionRequest>>,
}

impl BudgetSensitiveProvider {
    fn new(directory: &Path, force_truncated: bool) -> Self {
        Self {
            target: directory.join(TARGET),
            full_action: json!({
                "tool": "write_file",
                "args": {"path": TARGET, "content": payload()},
            })
            .to_string(),
            force_truncated,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for BudgetSensitiveProvider {
    fn id(&self) -> &str {
        "scripted-budget-sensitive-not-a-tokenizer"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: false,
            supports_constraint: true,
            exposes_logits: false,
            supports_media: false,
        }
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        _cancel_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        request.validate()?;
        assert!(request.constraint.is_some());
        assert!(request.tools.is_empty());
        let mut requests = self.requests.lock().unwrap();
        assert!(requests.len() < 2, "the one-retry truncation bound changed");
        assert!(
            !self.target.exists(),
            "no action may be published before a complete response is returned"
        );
        let truncated = self.force_truncated || request.sampling.max_tokens < LARGE_ACTION_CAP;
        let text = if truncated && !(self.force_truncated && requests.is_empty()) {
            self.full_action[..self.full_action.len() / 2].to_owned()
        } else {
            self.full_action.clone()
        };
        let output_tokens = if truncated {
            request.sampling.max_tokens
        } else {
            LARGE_ACTION_CAP
        };
        requests.push(request);
        Ok(Completion {
            message: Message::assistant(text),
            input_tokens: Some(60),
            output_tokens: Some(output_tokens),
            truncated,
        })
    }
}

fn successful_writes(records: &[TraceRecord]) -> usize {
    records
        .iter()
        .filter(|record| {
            matches!(&record.event, ParsedEvent::Known(Event::ToolResult {
                name, is_error: false, ..
            }) if name == "write_file")
        })
        .count()
}

fn main_budgets(records: &[TraceRecord]) -> Vec<(u32, OutputBudget)> {
    records
        .iter()
        .filter_map(|record| match &record.event {
            ParsedEvent::Known(Event::MainActionBudget { turn, budget }) => {
                Some((*turn, budget.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn large_action_budget_preserves_exact_bytes() {
    for harness in [HarnessPolicy::Legacy, HarnessPolicy::Evidence] {
        let directory = tempfile::tempdir().unwrap();
        let mut policy = with_budget(base_policy(), Some(LARGE_ACTION_CAP));
        policy.max_turns = 1;
        let provider = BudgetSensitiveProvider::new(directory.path(), false);
        let approvals = AtomicUsize::new(0);
        let approve = |_preview: &EditPreview| {
            approvals.fetch_add(1, Ordering::SeqCst);
            true
        };
        let capture = run_fixture(
            directory.path(),
            &provider,
            &policy,
            sampling(policy.max_output_tokens),
            harness,
            Some(&approve),
        );
        // This is one admitted action, not a claim of task verification.
        assert_eq!(capture.outcome.stop, StopReason::MaxTurns);
        assert_eq!(capture.outcome.turns, 1);
        let expected = payload();
        assert_eq!(expected.len(), PAYLOAD_BYTES);
        assert_eq!(
            std::fs::read(directory.path().join(TARGET)).unwrap(),
            expected.as_bytes()
        );
        assert_eq!(approvals.load(Ordering::SeqCst), 1);
        assert_eq!(successful_writes(&capture.records), 1);
        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].sampling.max_tokens, LARGE_ACTION_CAP);
        assert_eq!(
            main_budgets(&capture.records),
            [(0, policy.output_budget.clone().unwrap())]
        );
    }
}

#[test]
fn truncated_large_action_never_dispatches() {
    for harness in [HarnessPolicy::Legacy, HarnessPolicy::Evidence] {
        for force_truncated in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            assert!(base_policy().max_output_tokens < LARGE_ACTION_CAP);
            let policy = with_budget(base_policy(), force_truncated.then_some(LARGE_ACTION_CAP));
            let provider = BudgetSensitiveProvider::new(directory.path(), force_truncated);
            let approvals = AtomicUsize::new(0);
            let approve = |_preview: &EditPreview| {
                approvals.fetch_add(1, Ordering::SeqCst);
                true
            };
            let capture = run_fixture(
                directory.path(),
                &provider,
                &policy,
                sampling(policy.max_output_tokens),
                harness,
                Some(&approve),
            );
            assert_eq!(capture.outcome.stop, StopReason::TruncatedAction);
            assert_eq!(capture.outcome.turns, 2);
            assert!(!directory.path().join(TARGET).exists());
            assert_eq!(approvals.load(Ordering::SeqCst), 0);
            assert!(!capture.records.iter().any(|record| matches!(
                &record.event,
                ParsedEvent::Known(Event::ToolCall { .. } | Event::ToolResult { .. })
            )));
            let proposed: Vec<_> = capture
                .records
                .iter()
                .filter_map(|record| match &record.event {
                    ParsedEvent::Known(Event::ActionsProposed { calls, .. }) => Some(calls),
                    _ => None,
                })
                .collect();
            assert_eq!(proposed.len(), 2);
            assert!(proposed.iter().all(|calls| calls.is_empty()));
            assert!(capture.records.iter().any(|record| matches!(
                &record.event,
                ParsedEvent::Known(Event::SessionEnd { reason }) if reason == "truncated_action"
            )));
            let requests = provider.requests();
            assert_eq!(requests.len(), 2);
            assert!(
                requests
                    .iter()
                    .all(|request| request.sampling.max_tokens == policy.max_output_tokens)
            );
            assert_eq!(
                main_budgets(&capture.records),
                [
                    (0, policy.output_budget.clone().unwrap()),
                    (1, policy.output_budget.clone().unwrap()),
                ]
            );
            assert!(requests[1].messages.iter().any(|message| {
                message.role == Role::User
                    && message
                        .text
                        .as_deref()
                        .is_some_and(|text| text.contains("cut off"))
            }));
            assert!(!requests[1].messages.iter().any(|message| {
                message.role == Role::Assistant
                    && message
                        .text
                        .as_deref()
                        .is_some_and(|text| text.contains(TARGET))
            }));
        }
    }
}

#[test]
fn output_override_preserves_authority() {
    let mut before = base_policy();
    before.max_turns = 1;
    before.max_ring = Some(0);
    let before = with_budget(before, None);
    let after = with_budget(before.clone(), Some(LARGE_ACTION_CAP));
    let mut normalized = after.clone();
    normalized.max_output_tokens = before.max_output_tokens;
    normalized.output_budget = before.output_budget.clone();
    assert_eq!(normalized, before, "only the main-action cap may change");
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    assert_eq!(
        registry.tools_for_policy(&before),
        registry.tools_for_policy(&after)
    );
    assert_eq!(
        registry.tools_for_controlled_policy(&before),
        registry.tools_for_controlled_policy(&after)
    );

    for harness in [HarnessPolicy::Legacy, HarnessPolicy::Evidence] {
        for existing in [false, true] {
            let mut offered_baseline = None;
            for policy in [&before, &after] {
                let directory = tempfile::tempdir().unwrap();
                if existing {
                    std::fs::write(directory.path().join(TARGET), b"existing sentinel").unwrap();
                }
                // A small complete response isolates authority from the default
                // cap's deliberate truncation in the large-action fixture.
                let provider = MockProvider::new(vec![action(
                    "write_file",
                    json!({"path": TARGET, "content": "must be refused"}),
                    60,
                )]);
                let approvals = AtomicUsize::new(0);
                let reject = |_preview: &EditPreview| {
                    approvals.fetch_add(1, Ordering::SeqCst);
                    false
                };
                let capture = run_fixture(
                    directory.path(),
                    &provider,
                    policy,
                    sampling(policy.max_output_tokens),
                    harness,
                    Some(&reject),
                );
                assert_eq!(capture.outcome.stop, StopReason::MaxTurns);
                assert_eq!(successful_writes(&capture.records), 0);
                if existing {
                    assert_eq!(
                        std::fs::read(directory.path().join(TARGET)).unwrap(),
                        b"existing sentinel"
                    );
                } else {
                    assert!(!directory.path().join(TARGET).exists());
                }
                let blind_existing = harness == HarnessPolicy::Evidence && existing;
                assert_eq!(
                    approvals.load(Ordering::SeqCst),
                    usize::from(!blind_existing)
                );
                let blocks: Vec<_> = capture
                    .records
                    .iter()
                    .filter_map(|record| match &record.event {
                        ParsedEvent::Known(Event::ControllerBlocked { block, .. }) => {
                            Some(block.reason)
                        }
                        _ => None,
                    })
                    .collect();
                if blind_existing {
                    assert_eq!(blocks, [ferric_trace::ControllerBlockReason::BlindMutation]);
                } else {
                    assert!(blocks.is_empty());
                    assert!(capture.records.iter().any(|record| matches!(
                        &record.event,
                        ParsedEvent::Known(Event::ToolResult { is_error: true, output, .. }) if output.contains("rejected")
                    )));
                }
                let offered = capture
                    .records
                    .iter()
                    .find_map(|record| match &record.event {
                        ParsedEvent::Known(Event::PromptAssembled { offered_tools, .. }) => {
                            Some(offered_tools.clone())
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(offered.iter().any(|name| name == "write_file"));
                assert!(!offered.iter().any(|name| name == "shell_exec"));
                if let Some(baseline) = &offered_baseline {
                    assert_eq!(&offered, baseline);
                } else {
                    offered_baseline = Some(offered);
                }
                assert!(capture.records.iter().any(|record| matches!(
                    &record.event,
                    ParsedEvent::Known(Event::PolicySelected {
                        tier, harness_policy, max_turns, max_tools,
                        prompt_budget_tokens, tier_source, ..
                    }) if *tier == before.tier && *harness_policy == harness
                        && *max_turns == u32::from(before.max_turns)
                        && *max_tools == u32::from(before.max_tools)
                        && *prompt_budget_tokens == before.prompt_budget_tokens
                        && tier_source == before.tier_source.label()
                )));
            }
        }
    }
}

#[test]
fn action_budget_does_not_retune_compaction() {
    for harness in [HarnessPolicy::Legacy, HarnessPolicy::Evidence] {
        for explicit in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            for name in ["a.txt", "b.txt", "c.txt"] {
                std::fs::write(directory.path().join(name), name).unwrap();
            }
            let mut policy = base_policy();
            policy.max_turns = 4;
            policy.compact_trigger_fraction = 0.85;
            policy.compact_keep_last_turns = 2;
            let original = policy.clone();
            let policy = with_budget(policy, explicit.then_some(LARGE_ACTION_CAP));
            assert_eq!(policy.prompt_budget_tokens, original.prompt_budget_tokens);
            assert_eq!(
                policy.compact_trigger_fraction,
                original.compact_trigger_fraction
            );
            assert_eq!(
                policy.compact_keep_last_turns,
                original.compact_keep_last_turns
            );
            let provider = MockProvider::new(vec![
                action("read_file", json!({"path": "a.txt"}), 100),
                action("read_file", json!({"path": "b.txt"}), 100),
                action(
                    "read_file",
                    json!({"path": "c.txt"}),
                    policy.prompt_budget_tokens,
                ),
                Completion {
                    message: Message::assistant(
                        "Read a.txt, b.txt and c.txt; no changes were made.",
                    ),
                    input_tokens: Some(40),
                    output_tokens: Some(15),
                    truncated: false,
                },
                action(
                    "task_complete",
                    json!({"summary": "inspection complete"}),
                    100,
                ),
            ]);
            let capture = run_fixture(
                directory.path(),
                &provider,
                &policy,
                sampling(policy.max_output_tokens),
                harness,
                None,
            );
            assert_eq!(capture.outcome.stop, StopReason::TaskComplete);
            assert_eq!(capture.outcome.turns, 4);
            let requests = provider.requests();
            assert_eq!(requests.len(), 5);
            let main: Vec<_> = requests
                .iter()
                .filter(|request| request.constraint.is_some())
                .collect();
            let compaction: Vec<_> = requests
                .iter()
                .filter(|request| request.constraint.is_none())
                .collect();
            assert_eq!(main.len(), 4);
            assert!(
                main.iter()
                    .all(|request| request.sampling == sampling(policy.max_output_tokens))
            );
            assert_eq!(compaction.len(), 1);
            assert!(compaction[0].tools.is_empty());
            assert_eq!(compaction[0].sampling, SamplingParams::default());
            assert!(
                compaction[0].messages[0]
                    .text
                    .as_deref()
                    .unwrap()
                    .contains("summarizing")
            );
            let folds: Vec<_> = capture
                .records
                .iter()
                .filter_map(|record| match &record.event {
                    ParsedEvent::Known(Event::HistoryCompacted {
                        through_turn,
                        dropped_turns,
                        ..
                    }) => Some((*through_turn, *dropped_turns)),
                    _ => None,
                })
                .collect();
            assert_eq!(folds, [(0, 1)]);
            assert_eq!(
                main_budgets(&capture.records),
                (0..4)
                    .map(|turn| { (turn, policy.output_budget.clone().unwrap()) })
                    .collect::<Vec<_>>(),
                "the independent summary request is not a main-action budget event"
            );
        }
    }
}

#[test]
fn direct_request_budget_provenance_is_actual() {
    for harness in [HarnessPolicy::Legacy, HarnessPolicy::Evidence] {
        for (has_declared_metadata, requested) in
            [(false, None), (true, None), (true, Some(LARGE_ACTION_CAP))]
        {
            let directory = tempfile::tempdir().unwrap();
            let policy = if has_declared_metadata {
                let policy = with_budget(base_policy(), requested);
                assert_eq!(policy.output_budget.as_ref().unwrap().requested, requested);
                assert_eq!(
                    policy.output_budget.as_ref().unwrap().source,
                    if requested.is_some() {
                        OutputBudgetSource::Explicit
                    } else {
                        OutputBudgetSource::Policy
                    }
                );
                policy
            } else {
                let policy = base_policy();
                assert!(policy.output_budget.is_none());
                policy
            };
            let actual_cap = 777;
            assert_ne!(actual_cap, policy.max_output_tokens);
            let provider = MockProvider::new(vec![action(
                "task_complete",
                json!({"summary": "no mutation requested"}),
                60,
            )]);
            let capture = run_fixture(
                directory.path(),
                &provider,
                &policy,
                sampling(actual_cap),
                harness,
                None,
            );
            assert_eq!(capture.outcome.stop, StopReason::TaskComplete);
            let requests = provider.requests();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].sampling.max_tokens, actual_cap);
            let budgets = main_budgets(&capture.records);
            assert_eq!(budgets.len(), 1);
            assert_eq!(budgets[0].0, 0);
            assert_eq!(budgets[0].1.requested, None);
            assert_eq!(budgets[0].1.effective, actual_cap);
            assert_eq!(
                budgets[0].1.declared_ctx,
                has_declared_metadata.then_some(DECLARED_CONTEXT)
            );
            assert_eq!(budgets[0].1.source, OutputBudgetSource::Caller);
            assert!(
                capture.records.iter().any(|record| matches!(
                    &record.event,
                    ParsedEvent::Known(Event::PolicySelected { max_output_tokens, tier_source, .. })
                        if *max_output_tokens == policy.max_output_tokens
                            && tier_source == policy.tier_source.label()
                )),
                "nominal policy and actual request must remain separately attributable"
            );
        }
    }
}
