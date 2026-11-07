//! Structural filesystem mutations (make_dir / delete_path / move_path /
//! copy_file) under the evidence-controlled registry path: typed intent, CAS on
//! the prepared precondition, and measured, directory-aware effects.

mod common;

use common::{commit, controlled_call, prepare, rejected, setup};
use ferric_guard::Workspace;
use ferric_tools::{
    ControlledOutcome, MutationKind, PrepareErrorKind, PreparedIntent, Registry, WorkspaceEffect,
    WorkspaceEffectKind, WorkspaceEffectReport,
};
use serde_json::{Value, json};

fn prepared(
    registry: &Registry,
    workspace: &Workspace,
    name: &str,
    args: &Value,
) -> PreparedIntent {
    let (intent, outcome) = controlled_call(registry, workspace, name, args);
    assert!(
        !errored(&outcome),
        "{name} unexpectedly errored: {outcome:?}"
    );
    intent
}

fn mutation_kind(intent: &PreparedIntent) -> MutationKind {
    match intent {
        PreparedIntent::Mutation(intent) => intent.kind,
        other => panic!("expected a mutation intent, got {other:?}"),
    }
}

fn effects(outcome: &ControlledOutcome) -> Vec<WorkspaceEffect> {
    match outcome {
        ControlledOutcome::Completed { metadata, .. } => match &metadata.effects {
            WorkspaceEffectReport::Measured(effects) => effects.clone(),
            other => panic!("expected measured effects, got {other:?}"),
        },
        other => panic!("expected completion, got {other:?}"),
    }
}

fn errored(outcome: &ControlledOutcome) -> bool {
    match outcome {
        ControlledOutcome::Completed { output, .. } => output.is_error,
        ControlledOutcome::Denied { .. } => true,
    }
}

fn is_stale(outcome: &ControlledOutcome) -> bool {
    matches!(
        outcome,
        ControlledOutcome::Completed { metadata, .. }
            if metadata.failure.as_ref().is_some_and(|failure| {
                failure.kind == ferric_tools::ControlFailureKind::StalePrecondition
            })
    )
}

#[test]
fn make_dir_creates_a_directory_with_a_measured_effect() {
    let (dir, workspace, registry) = setup();
    let (intent, outcome) =
        controlled_call(&registry, &workspace, "make_dir", &json!({"path": "proj"}));
    assert_eq!(mutation_kind(&intent), MutationKind::CreateDirectory);
    assert!(!errored(&outcome));
    assert!(dir.path().join("proj").is_dir());

    let effects = effects(&outcome);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].path, "proj");
    assert_eq!(effects[0].kind, WorkspaceEffectKind::CreatedDirectory);
}

#[test]
fn make_dir_into_a_missing_parent_is_rejected() {
    let (_dir, workspace, registry) = setup();
    let error = rejected(
        &registry,
        &workspace,
        "make_dir",
        &json!({"path": "absent/child"}),
    );
    assert!(
        error.message.contains("does not exist"),
        "message: {}",
        error.message
    );
}

#[test]
fn make_dir_on_an_existing_directory_is_a_no_effect() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("proj")).unwrap();
    let error = rejected(&registry, &workspace, "make_dir", &json!({"path": "proj"}));
    assert_eq!(error.kind, PrepareErrorKind::NoEffect);
}

#[test]
fn make_dir_over_an_existing_file_is_unsupported() {
    let (dir, workspace, registry) = setup();
    std::fs::write(dir.path().join("proj"), b"i am a file").unwrap();
    let error = rejected(&registry, &workspace, "make_dir", &json!({"path": "proj"}));
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
}

#[test]
fn delete_removes_a_file_with_a_deleted_effect() {
    let (dir, workspace, registry) = setup();
    std::fs::write(dir.path().join("junk.txt"), b"bye").unwrap();
    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "delete_path",
        &json!({"path": "junk.txt"}),
    );
    assert_eq!(mutation_kind(&intent), MutationKind::DeleteFile);
    assert!(!errored(&outcome));
    assert!(!dir.path().join("junk.txt").exists());

    let effects = effects(&outcome);
    assert_eq!(effects[0].kind, WorkspaceEffectKind::Deleted);
}

#[test]
fn delete_removes_an_empty_directory_with_a_directory_effect() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("temp")).unwrap();
    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "delete_path",
        &json!({"path": "temp"}),
    );
    assert_eq!(mutation_kind(&intent), MutationKind::DeleteDirectory);
    assert!(!errored(&outcome));
    assert!(!dir.path().join("temp").exists());

    let effects = effects(&outcome);
    assert_eq!(effects[0].kind, WorkspaceEffectKind::DeletedDirectory);
}

#[test]
fn delete_of_a_non_empty_directory_is_unsupported() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("full")).unwrap();
    std::fs::write(dir.path().join("full/keep.txt"), b"x").unwrap();
    let error = rejected(
        &registry,
        &workspace,
        "delete_path",
        &json!({"path": "full"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
    assert!(dir.path().join("full/keep.txt").exists());
}

#[test]
fn delete_of_an_absent_path_is_a_no_effect() {
    let (_dir, workspace, registry) = setup();
    let error = rejected(
        &registry,
        &workspace,
        "delete_path",
        &json!({"path": "ghost"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::NoEffect);
}

#[test]
fn move_file_renames_and_reports_a_deletion_and_a_creation() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("dest")).unwrap();
    std::fs::write(dir.path().join("a.txt"), b"payload").unwrap();
    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "move_path",
        &json!({"from": "a.txt", "to": "dest/b.txt"}),
    );
    assert_eq!(mutation_kind(&intent), MutationKind::MovePath);
    assert!(!errored(&outcome));
    assert!(!dir.path().join("a.txt").exists());
    assert_eq!(
        std::fs::read(dir.path().join("dest/b.txt")).unwrap(),
        b"payload"
    );

    let effects = effects(&outcome);
    assert_eq!(effects.len(), 2);
    let from = effects
        .iter()
        .find(|effect| effect.path == "a.txt")
        .unwrap();
    let to = effects
        .iter()
        .find(|effect| effect.path == "dest/b.txt")
        .unwrap();
    assert_eq!(from.kind, WorkspaceEffectKind::Deleted);
    assert_eq!(to.kind, WorkspaceEffectKind::Created);
}

#[test]
fn move_directory_reports_directory_effects() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("old")).unwrap();
    let (_intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "move_path",
        &json!({"from": "old", "to": "new"}),
    );
    assert!(!errored(&outcome));
    assert!(dir.path().join("new").is_dir());
    assert!(!dir.path().join("old").exists());

    let effects = effects(&outcome);
    let from = effects.iter().find(|effect| effect.path == "old").unwrap();
    let to = effects.iter().find(|effect| effect.path == "new").unwrap();
    assert_eq!(from.kind, WorkspaceEffectKind::DeletedDirectory);
    assert_eq!(to.kind, WorkspaceEffectKind::CreatedDirectory);
}

#[test]
fn move_onto_an_existing_destination_is_unsupported() {
    let (dir, workspace, registry) = setup();
    std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"b").unwrap();
    let error = rejected(
        &registry,
        &workspace,
        "move_path",
        &json!({"from": "a.txt", "to": "b.txt"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
    // Neither endpoint was touched.
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"a");
    assert_eq!(std::fs::read(dir.path().join("b.txt")).unwrap(), b"b");
}

#[test]
fn copy_file_publishes_source_bytes_at_a_new_destination() {
    let (dir, workspace, registry) = setup();
    std::fs::write(dir.path().join("src.txt"), b"copy me").unwrap();
    let intent = prepared(
        &registry,
        &workspace,
        "copy_file",
        &json!({"from": "src.txt", "to": "dst.txt"}),
    );
    // A copy to a new path is a file creation.
    assert_eq!(mutation_kind(&intent), MutationKind::CreateFile);
    assert_eq!(
        std::fs::read(dir.path().join("dst.txt")).unwrap(),
        b"copy me"
    );
    assert_eq!(
        std::fs::read(dir.path().join("src.txt")).unwrap(),
        b"copy me"
    );
}

#[test]
fn copy_of_a_directory_source_is_unsupported() {
    let (dir, workspace, registry) = setup();
    std::fs::create_dir(dir.path().join("adir")).unwrap();
    let error = rejected(
        &registry,
        &workspace,
        "copy_file",
        &json!({"from": "adir", "to": "dst"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
}

#[test]
fn a_state_change_between_preparation_and_commit_is_a_stale_precondition() {
    let (dir, workspace, registry) = setup();
    // Prepare a make_dir whose precondition is "target is absent".
    let prepared = prepare(&registry, &workspace, "make_dir", &json!({"path": "race"}));
    // The workspace changes underneath the sealed preparation.
    std::fs::create_dir(dir.path().join("race")).unwrap();
    let outcome = commit(&registry, prepared);
    assert!(errored(&outcome), "a stale commit must not report success");
    assert!(
        is_stale(&outcome),
        "expected a stale precondition, got {outcome:?}"
    );
}
