use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ferric_guard::{PermissionLevel, Provenance, SinkPolicy, Workspace};
use ferric_tools::builtin::FetchReference;
use ferric_tools::{
    ControlCapability, ControlFailureKind, ControlledOutcome, ExecuteOutcome, NamedCheck,
    PrepareCtx, PrepareError, PrepareErrorKind, PrepareOutcome, PreparedIntent, Registry, Tool,
    ToolCtx, ToolPreparation, ToolSpec, WorkspaceEffectReport, register_builtin_tools,
    register_human_tools, register_run_checks,
};
use serde_json::{Value, json};

struct OpaqueWrite {
    ran: Arc<AtomicBool>,
}

impl Tool for OpaqueWrite {
    fn spec(&self) -> ToolSpec {
        test_spec("opaque_write", PermissionLevel::Write)
    }

    fn run(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> Result<String, String> {
        self.ran.store(true, Ordering::SeqCst);
        Ok("legacy write ran".to_string())
    }
}

struct OpaqueExecute {
    ran: Arc<AtomicBool>,
}

impl Tool for OpaqueExecute {
    fn spec(&self) -> ToolSpec {
        test_spec("opaque_execute", PermissionLevel::Execute)
    }

    fn run(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> Result<String, String> {
        self.ran.store(true, Ordering::SeqCst);
        Ok("legacy execute ran".to_string())
    }
}

struct OpaqueReadProbe {
    prepared: Arc<AtomicBool>,
    ran: Arc<AtomicBool>,
}

impl Tool for OpaqueReadProbe {
    fn spec(&self) -> ToolSpec {
        test_spec("opaque_read_probe", PermissionLevel::Read)
    }

    fn prepare(
        &self,
        _ctx: &PrepareCtx<'_>,
        _args: &Value,
    ) -> Result<ToolPreparation, PrepareError> {
        self.prepared.store(true, Ordering::SeqCst);
        Ok(ToolPreparation::deferred_read_only())
    }

    fn run(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> Result<String, String> {
        self.ran.store(true, Ordering::SeqCst);
        Ok("opaque read ran".to_string())
    }
}

struct PreparationProbe {
    prepared: Arc<AtomicBool>,
    ran: Arc<AtomicBool>,
}

impl Tool for PreparationProbe {
    fn spec(&self) -> ToolSpec {
        test_spec("preparation_probe", PermissionLevel::Write)
    }

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ContentMutation
    }

    fn prepare(
        &self,
        _ctx: &PrepareCtx<'_>,
        _args: &Value,
    ) -> Result<ToolPreparation, PrepareError> {
        self.prepared.store(true, Ordering::SeqCst);
        Ok(ToolPreparation::deferred_read_only())
    }

    fn run(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> Result<String, String> {
        self.ran.store(true, Ordering::SeqCst);
        Ok("ran".to_string())
    }
}

struct FailingRead;

impl Tool for FailingRead {
    fn spec(&self) -> ToolSpec {
        test_spec("failing_read", PermissionLevel::Read)
    }

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ReadOnly
    }

    fn run(&self, _ctx: &ToolCtx<'_>, _args: &Value) -> Result<String, String> {
        Err("intentional tool failure".to_string())
    }
}

fn test_spec(name: &str, permission: PermissionLevel) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        description: "test tool".to_string(),
        input_schema: json!({"type": "object"}),
        permission,
        ring: 0,
    }
}

fn workspace() -> (tempfile::TempDir, Workspace) {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(directory.path()).unwrap();
    (directory, workspace)
}

#[test]
fn controlled_write_fails_closed_without_typed_preparation() {
    let (_directory, workspace) = workspace();
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(OpaqueWrite { ran: ran.clone() }));

    match registry.prepare_controlled(&workspace, "opaque_write", &json!({})) {
        PrepareOutcome::Rejected { error, checks, .. } => {
            assert_eq!(error.kind, PrepareErrorKind::OpaqueMutation);
            assert!(checks.is_empty());
        }
        other => panic!("expected typed rejection, got {other:?}"),
    }
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn controlled_execute_fails_closed_without_typed_preparation() {
    let (_directory, workspace) = workspace();
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(OpaqueExecute { ran: ran.clone() }));

    match registry.prepare_controlled(&workspace, "opaque_execute", &json!({})) {
        PrepareOutcome::Rejected { error, .. } => {
            assert_eq!(error.kind, PrepareErrorKind::OpaqueMutation);
        }
        other => panic!("expected typed rejection, got {other:?}"),
    }
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn controlled_read_fails_closed_before_prepare_without_an_explicit_capability() {
    let (_directory, workspace) = workspace();
    let prepared = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(OpaqueReadProbe {
        prepared: prepared.clone(),
        ran: ran.clone(),
    }));

    match registry.prepare_controlled(&workspace, "opaque_read_probe", &json!({})) {
        PrepareOutcome::Rejected { error, .. } => {
            assert_eq!(error.kind, PrepareErrorKind::OpaqueMutation);
        }
        other => panic!("expected typed rejection, got {other:?}"),
    }
    assert!(!prepared.load(Ordering::SeqCst));
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn guard_denial_happens_before_tool_preparation() {
    let (_directory, workspace) = workspace();
    let prepared = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(PreparationProbe {
        prepared: prepared.clone(),
        ran: ran.clone(),
    }));

    match registry.prepare_controlled(
        &workspace,
        "preparation_probe",
        &json!({"path": ".git/config"}),
    ) {
        PrepareOutcome::Denied { checks, .. } => {
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].decision, "deny");
        }
        other => panic!("expected guard denial, got {other:?}"),
    }
    assert!(!prepared.load(Ordering::SeqCst));
    assert!(!ran.load(Ordering::SeqCst));
}

#[test]
fn controlled_failure_keeps_error_and_effect_channels_separate() {
    let (_directory, workspace) = workspace();
    let mut registry = Registry::new();
    registry.register(Box::new(FailingRead));

    let prepared = match registry.prepare_controlled(&workspace, "failing_read", &json!({})) {
        PrepareOutcome::Prepared(prepared) => prepared,
        other => panic!("expected preparation, got {other:?}"),
    };
    assert_eq!(prepared.intent(), &PreparedIntent::ReadOnly);

    match registry.commit_admitted(prepared, Provenance::Clean, &SinkPolicy::deny(), None) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            assert_eq!(output.full, "intentional tool failure");
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::UnmeasuredReadOnly
            ));
            assert!(matches!(
                metadata.failure,
                Some(ref failure) if failure.kind == ControlFailureKind::ToolError
            ));
            assert!(metadata.observation.is_none());
        }
        other => panic!("expected controlled completion, got {other:?}"),
    }
}

#[test]
fn permission_intent_mismatch_fails_closed_before_run() {
    let (_directory, workspace) = workspace();
    let prepared_flag = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(PreparationProbe {
        prepared: prepared_flag.clone(),
        ran: ran.clone(),
    }));

    match registry.prepare_controlled(&workspace, "preparation_probe", &json!({})) {
        PrepareOutcome::Rejected { error, .. } => {
            assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
            assert!(error.message.contains("permission/intent mismatch"));
        }
        other => panic!("expected mismatch rejection, got {other:?}"),
    }
    assert!(prepared_flag.load(Ordering::SeqCst));
    assert!(
        !ran.load(Ordering::SeqCst),
        "mismatched preparation must not run the tool"
    );
}

#[test]
fn legacy_execute_remains_available_for_opaque_tools() {
    let (_directory, workspace) = workspace();
    let ran = Arc::new(AtomicBool::new(false));
    let mut registry = Registry::new();
    registry.register(Box::new(OpaqueWrite { ran: ran.clone() }));

    let outcome = registry.execute(
        &workspace,
        "opaque_write",
        &json!({}),
        Provenance::Clean,
        &SinkPolicy::deny(),
        None,
    );
    assert!(matches!(outcome, ExecuteOutcome::Completed { .. }));
    assert!(ran.load(Ordering::SeqCst));
}

#[test]
fn controlled_enumeration_and_preparation_use_the_exact_explicit_allowlist() {
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    registry.register(Box::new(FetchReference));
    register_human_tools(&mut registry);
    register_run_checks(
        &mut registry,
        vec![NamedCheck {
            name: "test-check".to_string(),
            program: std::env::current_exe().unwrap(),
            args: Vec::new(),
            timeout_s: 1,
            output_limit: 32,
        }],
    )
    .unwrap();
    let policy = ferric_core::policy_for(&ferric_core::ModelProfile {
        params_b: 70.0,
        quant: "Q4_K_M".to_string(),
        ctx: 32_768,
        family: "test".to_string(),
        measured_level: None,
    });
    let controlled: Vec<String> = registry
        .tools_for_controlled_policy(&policy)
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    assert_eq!(
        controlled,
        [
            "apply_patch",
            "edit_file",
            "find_files",
            "list_dir",
            "multi_edit",
            "read_file",
            "search_files",
            "write_file",
        ]
    );

    let opaque = [
        "copy_file",
        "delete_path",
        "fetch_reference",
        "git_read",
        "git_write",
        "make_dir",
        "manage_task",
        "move_path",
        "run_check",
        "shell_exec",
    ];
    let (_directory, workspace) = workspace();
    for name in opaque {
        assert!(!controlled.iter().any(|offered| offered == name));
        let args = if name == "git_read" {
            json!({"subcommand": "branch", "args": ["would-mutate"]})
        } else {
            json!({})
        };
        match registry.prepare_controlled(&workspace, name, &args) {
            PrepareOutcome::Rejected { error, .. } => {
                assert_eq!(error.kind, PrepareErrorKind::OpaqueMutation, "{name}");
            }
            other => panic!("expected opaque rejection for {name}, got {other:?}"),
        }
    }

    let legacy: Vec<String> = registry
        .tools_for_policy(&policy)
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    assert!(
        opaque
            .iter()
            .all(|name| legacy.iter().any(|item| item == name))
    );
}
