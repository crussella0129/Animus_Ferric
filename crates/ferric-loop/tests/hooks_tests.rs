use ferric_core::hooks::HooksConfig;
use ferric_core::policy_for;
use ferric_core::{ActionProtocol, Message, ModelProfile, Role};
use ferric_guard::{Provenance, SinkAction, SinkPolicy, Workspace};
use ferric_loop::{RunArgs, StopReason, run};
use ferric_provider::Completion;
use ferric_provider::MockProvider;
use ferric_provider::SamplingParams;
use ferric_tools::Registry;
use ferric_trace::JsonlSink;
use tempfile::tempdir;

#[test]
fn test_hooks_success() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    // Initialize git repo to prevent git from walking up the tree
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&root)
        .output()
        .unwrap();

    let pre_turn_file = root.join("pre.txt");
    let post_turn_file = root.join("post.txt");

    let pre_turn_cmd = format!("echo pre > {}", pre_turn_file.display());
    let post_turn_cmd = format!("echo post > {}", post_turn_file.display());

    let hooks = HooksConfig {
        pre_turn: Some(pre_turn_cmd),
        post_turn: Some(post_turn_cmd),
        on_error: None,
    };

    let workspace = Workspace::new(&root).unwrap();
    let registry = Registry::new();

    let completion = Completion {
        message: Message {
            role: Role::Assistant,
            text: Some("test".to_string()),
            tool_calls: vec![],
            tool_call_id: None,
            media: vec![],
        },
        input_tokens: Some(10),
        output_tokens: Some(10),
        truncated: false,
    };
    let provider = MockProvider::new(vec![completion]);

    let profile = ModelProfile {
        params_b: 7.0,
        quant: "Q4".to_string(),
        ctx: 4096,
        family: "test".to_string(),
        measured_level: None,
    };
    let mut policy = policy_for(&profile);
    policy.max_turns = 1;

    let trace_path = root.join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "session1").unwrap();

    let args = RunArgs {
        edit_approver: None,
        cancel_flag: None,
        provider: &provider,
        registry: &registry,
        workspace: &workspace,
        policy: &policy,
        protocol: ActionProtocol::NativeTools,
        sampling: SamplingParams::default(),
        sleeper: &ferric_loop::ThreadSleeper,
        system_prompt: Some("system"),
        prompt_lineage: None,
        media: vec![],
        stream_sink: None,
        resume: None,
        answer: None,
        provenance: Provenance::Clean,
        sink_policy: SinkPolicy::new(SinkAction::Deny),
        hooks: Some(hooks),
    };

    let outcome = futures_executor::block_on(run(args, &mut sink, Some("prompt"))).unwrap();

    // Empty completion results in StopReason::FinalText for NativeTools
    assert_eq!(outcome.stop, StopReason::FinalText);

    assert!(pre_turn_file.exists());
    assert!(post_turn_file.exists());
}

#[test]
fn test_hooks_error() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();

    let on_error_file = root.join("error.txt");
    let on_error_cmd = format!("echo err > {}", on_error_file.display());

    let hooks = HooksConfig {
        pre_turn: None,
        post_turn: None,
        on_error: Some(on_error_cmd),
    };

    let workspace = Workspace::new(&root).unwrap();
    let registry = Registry::new();
    let provider = MockProvider::new(vec![]); // Will return ProviderError

    let profile = ModelProfile {
        params_b: 7.0,
        quant: "Q4".to_string(),
        ctx: 4096,
        family: "test".to_string(),
        measured_level: None,
    };
    let policy = policy_for(&profile);

    let trace_path = root.join("trace.jsonl");
    let mut sink = JsonlSink::open(&trace_path, "session2").unwrap();

    let args = RunArgs {
        edit_approver: None,
        cancel_flag: None,
        provider: &provider,
        registry: &registry,
        workspace: &workspace,
        policy: &policy,
        protocol: ActionProtocol::NativeTools,
        sampling: SamplingParams::default(),
        sleeper: &ferric_loop::ThreadSleeper,
        system_prompt: Some("system"),
        prompt_lineage: None,
        media: vec![],
        stream_sink: None,
        resume: None,
        answer: None,
        provenance: Provenance::Clean,
        sink_policy: SinkPolicy::new(SinkAction::Deny),
        hooks: Some(hooks),
    };

    let outcome = futures_executor::block_on(run(args, &mut sink, Some("prompt"))).unwrap();

    assert_eq!(outcome.stop, StopReason::ProviderError);
    assert!(on_error_file.exists());
}
