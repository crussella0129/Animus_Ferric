use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_tools::{
    ControlFailureKind, ControlFailureWitness, ControlledOutcome, MutationKind, NoEffectKind,
    PathState, PrepareError, PrepareErrorKind, PrepareFailureWitness, PrepareOutcome, PreparedCall,
    PreparedIntent, Registry, SyntaxState, SyntaxUncheckedReason, WorkspaceEffectKind,
    WorkspaceEffectReport, register_builtin_tools,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn setup() -> (tempfile::TempDir, Workspace, Registry) {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(directory.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    (directory, workspace, registry)
}

fn prepare<'a>(
    registry: &'a Registry,
    workspace: &'a Workspace,
    tool: &str,
    args: &Value,
) -> PreparedCall<'a> {
    match registry.prepare_controlled(workspace, tool, args) {
        PrepareOutcome::Prepared(prepared) => prepared,
        other => panic!("expected {tool} preparation, got {other:?}"),
    }
}

fn rejected(registry: &Registry, workspace: &Workspace, tool: &str, args: &Value) -> PrepareError {
    match registry.prepare_controlled(workspace, tool, args) {
        PrepareOutcome::Rejected { error, .. } => error,
        other => panic!("expected {tool} rejection, got {other:?}"),
    }
}

fn commit(registry: &Registry, prepared: PreparedCall<'_>) -> ControlledOutcome {
    registry.commit_admitted(prepared, Provenance::Clean, &SinkPolicy::deny(), None)
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn all_content_tools_prepare_then_commit_exact_candidates_after_a_clean_reset() {
    let (directory, workspace, registry) = setup();
    let path = directory.path().join("sample.txt");
    let cases = [
        (
            "write_file",
            json!({"path": "sample.txt", "content": "beta\n"}),
            b"beta\n".as_slice(),
        ),
        (
            "edit_file",
            json!({"path": "sample.txt", "old_string": "alpha", "new_string": "beta"}),
            b"beta\n".as_slice(),
        ),
        (
            "multi_edit",
            json!({"path": "sample.txt", "edits": [{"old_string": "alpha", "new_string": "beta"}]}),
            b"beta\n".as_slice(),
        ),
        (
            "apply_patch",
            json!({"path": "sample.txt", "patch": "@@\n-alpha\n+beta"}),
            b"beta\n".as_slice(),
        ),
    ];

    for (tool, args, expected) in cases {
        std::fs::write(&path, b"alpha\n").unwrap();
        let prepared = prepare(&registry, &workspace, tool, &args);
        assert!(matches!(
            prepared.intent(),
            PreparedIntent::Mutation(intent)
                if matches!(
                    intent.states.as_slice(),
                    [state]
                        if matches!(
                            state.candidate,
                            PathState::File { sha256: ref digest_value, bytes, lines: 1 }
                                if digest_value == &sha256(expected)
                                    && bytes == expected.len() as u64
                        )
                )
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"alpha\n");
        match commit(&registry, prepared) {
            ControlledOutcome::Completed {
                output, metadata, ..
            } => {
                assert!(!output.is_error, "{tool}: {}", output.full);
                assert!(matches!(
                    metadata.effects,
                    WorkspaceEffectReport::Measured(ref effects)
                        if matches!(
                            effects.as_slice(),
                            [effect]
                                if effect.kind == WorkspaceEffectKind::Modified
                                    && effect.after
                                        == PathState::File {
                                            sha256: sha256(expected),
                                            bytes: expected.len() as u64,
                                            lines: 1,
                                        }
                        )
                ));
            }
            other => panic!("expected {tool} commit, got {other:?}"),
        }
        assert_eq!(std::fs::read(&path).unwrap(), expected, "{tool}");
    }

    let absent = directory.path().join("new.txt");
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "new.txt", "content": "created\n"}),
    );
    assert!(!absent.exists());
    drop(prepared);
}

#[test]
fn create_and_modify_effects_preserve_exact_crlf_bytes_and_line_counts() {
    let (directory, workspace, registry) = setup();
    let raw = b"alpha\r\nbeta\r\n";
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "created.txt", "content": "alpha\r\nbeta\r\n"}),
    );
    match prepared.intent() {
        PreparedIntent::Mutation(intent) => {
            assert_eq!(intent.kind, MutationKind::CreateFile);
            assert_eq!(intent.states.len(), 1);
            assert_eq!(intent.states[0].before, PathState::Absent);
            assert!(matches!(
                intent.states[0].candidate,
                PathState::File { sha256: ref digest_value, bytes, lines }
                    if digest_value == &sha256(raw) && bytes == raw.len() as u64 && lines == 2
            ));
        }
        other => panic!("expected mutation intent, got {other:?}"),
    }
    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(!output.is_error);
            let WorkspaceEffectReport::Measured(effects) = metadata.effects else {
                panic!("expected measured effects")
            };
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0].kind, WorkspaceEffectKind::Created);
            assert_eq!(effects[0].before, PathState::Absent);
            assert!(matches!(
                effects[0].after,
                PathState::File { sha256: ref digest_value, bytes, lines }
                    if digest_value == &sha256(raw) && bytes == raw.len() as u64 && lines == 2
            ));
        }
        other => panic!("expected commit, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(directory.path().join("created.txt")).unwrap(),
        raw
    );

    let prepared = prepare(
        &registry,
        &workspace,
        "edit_file",
        &json!({"path": "created.txt", "old_string": "beta", "new_string": "gamma"}),
    );
    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(!output.is_error);
            let WorkspaceEffectReport::Measured(effects) = metadata.effects else {
                panic!("expected measured effects")
            };
            assert_eq!(effects.len(), 1);
            assert_eq!(effects[0].kind, WorkspaceEffectKind::Modified);
            assert!(matches!(effects[0].after, PathState::File { lines: 2, .. }));
        }
        other => panic!("expected commit, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(directory.path().join("created.txt")).unwrap(),
        b"alpha\r\ngamma\r\n"
    );

    let lf = b"one\ntwo";
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "created.txt", "content": "one\ntwo"}),
    );
    match commit(&registry, prepared) {
        ControlledOutcome::Completed { metadata, .. } => {
            let WorkspaceEffectReport::Measured(effects) = metadata.effects else {
                panic!("expected measured effects")
            };
            assert!(matches!(
                effects.as_slice(),
                [effect]
                    if effect.kind == WorkspaceEffectKind::Modified
                        && matches!(effect.after, PathState::File { sha256: ref digest_value, bytes: 7, lines: 2 } if digest_value == &sha256(lf))
            ));
        }
        other => panic!("expected LF commit, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(directory.path().join("created.txt")).unwrap(),
        lf
    );
}

#[test]
fn cas_race_returns_typed_expected_and_observed_identities_without_effects() {
    let (directory, workspace, registry) = setup();
    let path = directory.path().join("race.txt");
    std::fs::write(&path, b"alpha\n").unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "edit_file",
        &json!({"path": "race.txt", "old_string": "alpha", "new_string": "beta"}),
    );
    std::fs::write(&path, b"external race\n").unwrap();

    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            let failure = metadata.failure.expect("typed stale failure");
            assert_eq!(failure.kind, ControlFailureKind::StalePrecondition);
            let Some(ControlFailureWitness::StaleObservation(witness)) = failure.witness else {
                panic!("expected stale witness")
            };
            assert_eq!(witness.path, "race.txt");
            assert!(matches!(
                witness.expected,
                PathState::File { sha256: ref digest_value, .. }
                    if digest_value == &sha256(b"alpha\n")
            ));
            assert!(matches!(
                witness.observed,
                PathState::File { sha256: ref digest_value, .. }
                    if digest_value == &sha256(b"external race\n")
            ));
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
            ));
        }
        other => panic!("expected controlled stale result, got {other:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"external race\n");
}

#[test]
fn cas_race_from_absent_to_directory_reports_the_non_file_shape() {
    let (directory, workspace, registry) = setup();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "shape-race", "content": "candidate\n"}),
    );
    std::fs::create_dir(directory.path().join("shape-race")).unwrap();

    match commit(&registry, prepared) {
        ControlledOutcome::Completed { metadata, .. } => {
            let failure = metadata.failure.expect("stale failure");
            let Some(ControlFailureWitness::StaleObservation(witness)) = failure.witness else {
                panic!("expected stale witness")
            };
            assert_eq!(witness.expected, PathState::Absent);
            assert_eq!(witness.observed, PathState::Directory);
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
            ));
        }
        other => panic!("expected controlled stale result, got {other:?}"),
    }
}

#[test]
fn cas_race_from_existing_file_to_directory_is_typed_and_has_zero_effects() {
    let (directory, workspace, registry) = setup();
    let path = directory.path().join("shape-race.txt");
    std::fs::write(&path, b"before\n").unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "shape-race.txt", "content": "candidate\n"}),
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();

    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            let failure = metadata.failure.expect("stale failure");
            assert_eq!(failure.kind, ControlFailureKind::StalePrecondition);
            let Some(ControlFailureWitness::StaleObservation(witness)) = failure.witness else {
                panic!("expected stale witness")
            };
            assert!(matches!(
                witness.expected,
                PathState::File { sha256: ref digest_value, .. }
                    if digest_value == &sha256(b"before\n")
            ));
            assert_eq!(witness.observed, PathState::Directory);
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
            ));
        }
        other => panic!("expected controlled stale result, got {other:?}"),
    }
    assert!(path.is_dir());
}

#[test]
fn identity_missing_match_and_net_zero_candidates_are_typed_no_effects() {
    let (directory, workspace, registry) = setup();
    std::fs::write(directory.path().join("same.txt"), b"alpha\n").unwrap();
    let cases = [
        (
            "write_file",
            json!({"path": "same.txt", "content": "alpha\n"}),
            NoEffectKind::Identity,
        ),
        (
            "edit_file",
            json!({"path": "same.txt", "old_string": "alpha", "new_string": "alpha"}),
            NoEffectKind::Identity,
        ),
        (
            "edit_file",
            json!({"path": "same.txt", "old_string": "missing", "new_string": "beta"}),
            NoEffectKind::MatchNotFound,
        ),
        (
            "multi_edit",
            json!({"path": "same.txt", "edits": [
                {"old_string": "alpha", "new_string": "beta"},
                {"old_string": "missing", "new_string": "value"}
            ]}),
            NoEffectKind::MatchNotFound,
        ),
        (
            "apply_patch",
            json!({"path": "same.txt", "patch": "@@\n-alpha\n+beta\n@@\n-missing\n+value"}),
            NoEffectKind::MatchNotFound,
        ),
        (
            "multi_edit",
            json!({"path": "same.txt", "edits": [
                {"old_string": "alpha", "new_string": "beta"},
                {"old_string": "beta", "new_string": "alpha"}
            ]}),
            NoEffectKind::NetZeroBatch,
        ),
        (
            "apply_patch",
            json!({"path": "same.txt", "patch": "@@\n alpha"}),
            NoEffectKind::NetZeroPatch,
        ),
    ];

    for (tool, args, expected_kind) in cases {
        let error = rejected(&registry, &workspace, tool, &args);
        assert_eq!(error.kind, PrepareErrorKind::NoEffect);
        assert!(matches!(
            error.witness,
            Some(PrepareFailureWitness::NoEffect { kind, ref states })
                if kind == expected_kind && states.len() == 1 && states[0].before == states[0].candidate
        ));
        assert_eq!(
            std::fs::read(directory.path().join("same.txt")).unwrap(),
            b"alpha\n"
        );
    }
}

#[test]
fn malformed_empty_and_unsupported_shapes_fail_during_preparation() {
    let (directory, workspace, registry) = setup();
    std::fs::write(directory.path().join("file.txt"), b"alpha\n").unwrap();
    std::fs::create_dir(directory.path().join("directory.txt")).unwrap();

    for (tool, args) in [
        ("multi_edit", json!({"path": "file.txt", "edits": []})),
        ("apply_patch", json!({"path": "file.txt", "patch": ""})),
        (
            "apply_patch",
            json!({"path": "file.txt", "patch": "not a hunk"}),
        ),
    ] {
        assert_eq!(
            rejected(&registry, &workspace, tool, &args).kind,
            PrepareErrorKind::InvalidArguments
        );
    }

    for args in [
        json!({"path": "directory.txt", "content": "x"}),
        json!({"path": "missing/child.txt", "content": "x"}),
        json!({"path": "file.txt/child.txt", "content": "x"}),
    ] {
        let error = rejected(&registry, &workspace, "write_file", &args);
        assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
        assert!(matches!(
            error.witness,
            Some(PrepareFailureWitness::UnsupportedMutation(_))
        ));
    }

    assert!(!directory.path().join("missing").exists());
}

#[test]
fn hardlinked_targets_are_rejected_at_prepare_and_if_linked_after_prepare() {
    let outer = tempfile::tempdir().unwrap();
    let workspace_path = outer.path().join("workspace");
    std::fs::create_dir(&workspace_path).unwrap();
    let workspace = Workspace::new(&workspace_path).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);

    let outside = outer.path().join("outside.txt");
    std::fs::write(&outside, b"outside-before\n").unwrap();
    std::fs::hard_link(&outside, workspace_path.join("inside-alias.txt")).unwrap();
    let error = rejected(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "inside-alias.txt", "content": "replacement\n"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside-before\n");

    let target = workspace_path.join("commit-race.txt");
    std::fs::write(&target, b"target-before\n").unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "commit-race.txt", "content": "candidate\n"}),
    );
    let outside_alias = outer.path().join("outside-alias.txt");
    std::fs::hard_link(&target, &outside_alias).unwrap();

    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            assert_eq!(
                metadata.failure.expect("typed failure").kind,
                ControlFailureKind::StalePrecondition
            );
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
            ));
        }
        other => panic!("expected controlled hardlink refusal, got {other:?}"),
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"target-before\n");
    assert_eq!(std::fs::read(&outside_alias).unwrap(), b"target-before\n");
    assert!(std::fs::read_dir(&workspace_path).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".ferric-candidate-")
    }));
}

#[cfg(any(unix, windows))]
#[test]
fn raced_link_ancestor_fails_closed_without_touching_either_directory() {
    let outer = tempfile::tempdir().unwrap();
    let workspace_path = outer.path().join("workspace");
    let original_parent = workspace_path.join("nested");
    let held_parent = workspace_path.join("held-original");
    let outside_parent = outer.path().join("outside");
    std::fs::create_dir_all(&original_parent).unwrap();
    std::fs::create_dir(&outside_parent).unwrap();
    std::fs::write(original_parent.join("target.txt"), b"inside-before\n").unwrap();
    std::fs::write(outside_parent.join("target.txt"), b"outside-before\n").unwrap();
    let workspace = Workspace::new(&workspace_path).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "nested/target.txt", "content": "candidate\n"}),
    );

    std::fs::rename(&original_parent, &held_parent).unwrap();
    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(&outside_parent, &original_parent);
    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_dir(&outside_parent, &original_parent);
    if let Err(error) = link_result {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create raced ancestor link: {error}");
    }

    match commit(&registry, prepared) {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            assert_eq!(
                metadata.failure.expect("typed stale failure").kind,
                ControlFailureKind::StalePrecondition
            );
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
            ));
        }
        other => panic!("expected controlled ancestor refusal, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(held_parent.join("target.txt")).unwrap(),
        b"inside-before\n"
    );
    assert_eq!(
        std::fs::read(outside_parent.join("target.txt")).unwrap(),
        b"outside-before\n"
    );
    assert!(std::fs::read_dir(&held_parent).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".ferric-candidate-")
    }));
}

#[cfg(any(unix, windows))]
#[test]
fn symlink_or_reparse_targets_are_rejected_during_preparation() {
    let (directory, workspace, registry) = setup();
    let target = directory.path().join("target.txt");
    let link = directory.path().join("link.txt");
    std::fs::write(&target, b"target\n").unwrap();

    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(&target, &link);
    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_file(&target, &link);
    if let Err(error) = link_result {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create test symlink: {error}");
    }

    let error = rejected(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "link.txt", "content": "replacement\n"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
    assert_eq!(std::fs::read(&target).unwrap(), b"target\n");

    std::fs::remove_file(&link).unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "link.txt", "content": "candidate\n"}),
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link).unwrap();
    match commit(&registry, prepared) {
        ControlledOutcome::Completed { metadata, .. } => {
            let failure = metadata.failure.expect("stale failure");
            let Some(ControlFailureWitness::StaleObservation(witness)) = failure.witness else {
                panic!("expected stale shape witness")
            };
            assert_eq!(witness.expected, PathState::Absent);
            assert_eq!(witness.observed, PathState::Other);
        }
        other => panic!("expected controlled stale result, got {other:?}"),
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"target\n");
}

#[cfg(any(unix, windows))]
#[test]
fn symlink_or_reparse_ancestors_are_rejected_without_touching_the_target() {
    let (directory, workspace, registry) = setup();
    let real = directory.path().join("real");
    let link = directory.path().join("linked");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("target.txt"), b"target\n").unwrap();

    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(&real, &link);
    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_dir(&real, &link);
    if let Err(error) = link_result {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create test directory symlink: {error}");
    }

    let error = rejected(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "linked/target.txt", "content": "replacement\n"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
    assert!(matches!(
        error.witness,
        Some(PrepareFailureWitness::UnsupportedMutation(_))
    ));
    assert_eq!(std::fs::read(real.join("target.txt")).unwrap(), b"target\n");
}

#[test]
fn python_syntax_matrix_blocks_regressions_and_warns_on_invalid_repairs() {
    let (directory, workspace, registry) = setup();
    let valid_path = directory.path().join("valid.py");
    std::fs::write(&valid_path, b"value = 1\n").unwrap();
    let invalid_candidate = json!({"path": "valid.py", "content": "return\n"});

    let error = rejected(&registry, &workspace, "write_file", &invalid_candidate);
    assert_eq!(error.kind, PrepareErrorKind::SyntaxRejected);
    assert!(matches!(
        error.witness,
        Some(PrepareFailureWitness::SyntaxRegression(ref syntax))
            if syntax.before == SyntaxState::Valid && syntax.candidate == SyntaxState::Invalid
    ));
    assert_eq!(std::fs::read(&valid_path).unwrap(), b"value = 1\n");

    let error = rejected(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "absent.py", "content": "return\n"}),
    );
    assert!(matches!(
        error.witness,
        Some(PrepareFailureWitness::SyntaxRegression(ref syntax))
            if syntax.before == SyntaxState::Absent && syntax.candidate == SyntaxState::Invalid
    ));
    assert!(!directory.path().join("absent.py").exists());

    let invalid_path = directory.path().join("repair.py");
    std::fs::write(&invalid_path, b"def old(:\n    pass\n").unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "repair.py", "content": "def still_bad(:\n    pass\n"}),
    );
    let PreparedIntent::Mutation(intent) = prepared.intent() else {
        panic!("expected mutation")
    };
    let syntax = intent.syntax.as_ref().expect("syntax transition");
    assert_eq!(syntax.before, SyntaxState::Invalid);
    assert_eq!(syntax.candidate, SyntaxState::Invalid);
    assert!(syntax.warning.is_some());
    match commit(&registry, prepared) {
        ControlledOutcome::Completed { output, .. } => {
            assert!(!output.is_error);
            assert!(output.full.contains("candidate remains invalid"));
        }
        other => panic!("expected invalid repair commit, got {other:?}"),
    }

    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "repair.py", "content": "def repaired():\n    pass\n"}),
    );
    assert!(matches!(
        prepared.intent(),
        PreparedIntent::Mutation(intent)
            if matches!(intent.syntax.as_ref(), Some(syntax) if syntax.before == SyntaxState::Invalid && syntax.candidate == SyntaxState::Valid)
    ));
    let _ = commit(&registry, prepared);

    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "repair.py", "content": "def repaired():\n    return 1\n"}),
    );
    assert!(matches!(
        prepared.intent(),
        PreparedIntent::Mutation(intent)
            if matches!(intent.syntax.as_ref(), Some(syntax) if syntax.before == SyntaxState::Valid && syntax.candidate == SyntaxState::Valid)
    ));
    drop(prepared);

    assert!(!directory.path().join("__pycache__").exists());
    assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("ferric-candidate")
    }));
}

#[test]
fn controlled_mutation_python_05_transition_matrix() {
    let (directory, workspace, registry) = setup();
    let path = directory.path().join("compiler-limited.py");
    let compiler_unsupported = b"type Alias[T] = list[T]\n";
    std::fs::write(&path, compiler_unsupported).unwrap();

    let error = rejected(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "compiler-limited.py", "content": "return\n"}),
    );
    assert_eq!(error.kind, PrepareErrorKind::SyntaxRejected);
    assert!(matches!(
        error.witness,
        Some(PrepareFailureWitness::SyntaxRegression(ref syntax))
            if syntax.before
                == SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
                && syntax.candidate == SyntaxState::Invalid
                && syntax.blocks_mutation()
    ));
    assert_eq!(std::fs::read(&path).unwrap(), compiler_unsupported);

    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({
            "path": "compiler-limited.py",
            "content": "type Alias[T] = tuple[T]\n"
        }),
    );
    assert!(matches!(
        prepared.intent(),
        PreparedIntent::Mutation(intent)
            if matches!(
                intent.syntax.as_ref(),
                Some(syntax)
                    if syntax.before
                        == SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
                        && syntax.candidate
                            == SyntaxState::Unchecked(SyntaxUncheckedReason::CompilerFailure)
                        && !syntax.blocks_mutation()
            )
    ));
    drop(prepared);
}

#[test]
fn unsupported_syntax_is_explicitly_unchecked_and_legacy_write_still_creates_parents() {
    let (directory, workspace, registry) = setup();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "notes.txt", "content": "not source code\n"}),
    );
    assert!(matches!(
        prepared.intent(),
        PreparedIntent::Mutation(intent)
            if matches!(intent.syntax.as_ref(), Some(syntax) if syntax.candidate == SyntaxState::Unchecked(SyntaxUncheckedReason::UnsupportedExtension))
    ));
    drop(prepared);

    let args = json!({"path": "legacy/nested.txt", "content": "legacy\n"});
    assert_eq!(
        rejected(&registry, &workspace, "write_file", &args).kind,
        PrepareErrorKind::UnsupportedOperation
    );
    let outcome = registry.execute(
        &workspace,
        "write_file",
        &args,
        Provenance::Clean,
        &SinkPolicy::deny(),
        None,
    );
    assert!(matches!(
        outcome,
        ferric_tools::ExecuteOutcome::Completed { .. }
    ));
    assert_eq!(
        std::fs::read(directory.path().join("legacy/nested.txt")).unwrap(),
        b"legacy\n"
    );
}

#[cfg(windows)]
#[test]
fn commit_io_error_still_reports_measured_effects_separately() {
    let (directory, workspace, registry) = setup();
    let path = directory.path().join("readonly.txt");
    std::fs::write(&path, b"before\n").unwrap();
    let prepared = prepare(
        &registry,
        &workspace,
        "write_file",
        &json!({"path": "readonly.txt", "content": "after\n"}),
    );
    let original_permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly_permissions = original_permissions.clone();
    readonly_permissions.set_readonly(true);
    std::fs::set_permissions(&path, readonly_permissions).unwrap();

    let outcome = commit(&registry, prepared);

    std::fs::set_permissions(&path, original_permissions).unwrap();
    match outcome {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => {
            assert!(output.is_error);
            assert_eq!(metadata.failure.unwrap().kind, ControlFailureKind::Io);
            assert!(matches!(
                metadata.effects,
                WorkspaceEffectReport::Measured(_)
            ));
        }
        other => panic!("expected controlled I/O result, got {other:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"before\n");
    assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".ferric-candidate-")
    }));
}
