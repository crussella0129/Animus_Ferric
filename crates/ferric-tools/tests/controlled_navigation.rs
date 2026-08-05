use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_tools::{
    ControlledOutcome, FileObservation, LineRange, NavigationKind, NavigationObservation,
    PrepareErrorKind, PrepareOutcome, PreparedIntent, Registry, ToolObservation,
    register_builtin_tools,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn setup(limit: usize) -> (tempfile::TempDir, Workspace, Registry) {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(directory.path()).unwrap();
    let mut registry = Registry::with_truncation_limit(limit);
    register_builtin_tools(&mut registry);
    (directory, workspace, registry)
}

fn controlled_call(
    registry: &Registry,
    workspace: &Workspace,
    name: &str,
    args: &Value,
) -> (PreparedIntent, ControlledOutcome) {
    let prepared = match registry.prepare_controlled(workspace, name, args) {
        PrepareOutcome::Prepared(prepared) => prepared,
        other => panic!("expected controlled preparation, got {other:?}"),
    };
    let intent = prepared.intent().clone();
    let outcome = registry.commit_admitted(prepared, Provenance::Clean, &SinkPolicy::deny(), None);
    (intent, outcome)
}

fn completed(outcome: ControlledOutcome) -> (String, String, ToolObservation) {
    match outcome {
        ControlledOutcome::Completed {
            output, metadata, ..
        } => (
            output.full,
            output.for_model,
            metadata.observation.expect("typed observation"),
        ),
        other => panic!("expected controlled completion, got {other:?}"),
    }
}

fn file_observation(intent: PreparedIntent) -> FileObservation {
    match intent {
        PreparedIntent::FileObservation(observation) => observation,
        other => panic!("expected file observation, got {other:?}"),
    }
}

fn navigation_observation(intent: PreparedIntent) -> NavigationObservation {
    match intent {
        PreparedIntent::Navigation(observation) => observation,
        other => panic!("expected navigation observation, got {other:?}"),
    }
}

fn body_after(full: &str, closing_envelope: &str) -> String {
    full.split_once(closing_envelope)
        .unwrap_or_else(|| panic!("missing envelope terminator {closing_envelope:?}"))
        .1
        .to_string()
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn assert_lowercase_sha256(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "not lowercase hex: {value}"
    );
}

#[test]
fn read_file_preserves_exact_crlf_range_and_hashes_complete_raw_file() {
    let (directory, workspace, registry) = setup(8_192);
    std::fs::create_dir_all(directory.path().join("nested")).unwrap();
    let raw = b"one\r\ntwo\r\nthree\nlast";
    std::fs::write(directory.path().join("nested/sample.txt"), raw).unwrap();

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "read_file",
        &json!({"path": "nested\\sample.txt", "start_line": 2, "end_line": 3}),
    );
    let observation = file_observation(intent);
    assert_eq!(observation.path, "nested/sample.txt");
    assert_eq!(observation.sha256, digest(raw));
    assert_eq!(observation.bytes, raw.len() as u64);
    assert_eq!(observation.total_lines, 4);
    assert_eq!(observation.returned, Some(LineRange { start: 2, end: 3 }));
    assert_eq!(observation.coverage, vec![LineRange { start: 2, end: 3 }]);
    assert!(!observation.complete);
    assert!(!observation.model_truncated);

    let (full, for_model, metadata) = completed(outcome);
    assert_eq!(full, for_model);
    assert_eq!(
        body_after(&full, "[/ferric:file_observation:v1]\n"),
        "two\r\nthree\n"
    );
    assert_eq!(metadata, ToolObservation::File(observation));
}

#[test]
fn read_file_line_semantics_cover_empty_and_trailing_newline() {
    let (directory, workspace, registry) = setup(8_192);
    std::fs::write(directory.path().join("empty.txt"), b"").unwrap();
    std::fs::write(directory.path().join("trailing.txt"), b"alpha\n").unwrap();

    let (empty_intent, empty_outcome) = controlled_call(
        &registry,
        &workspace,
        "read_file",
        &json!({"path": "empty.txt"}),
    );
    let empty = file_observation(empty_intent);
    assert_eq!(empty.total_lines, 0);
    assert_eq!(empty.returned, None);
    assert!(empty.complete);
    assert!(empty.coverage.is_empty());
    let (empty_full, _, _) = completed(empty_outcome);
    assert_eq!(
        body_after(&empty_full, "[/ferric:file_observation:v1]\n"),
        ""
    );

    let (trailing_intent, trailing_outcome) = controlled_call(
        &registry,
        &workspace,
        "read_file",
        &json!({"path": "trailing.txt"}),
    );
    let trailing = file_observation(trailing_intent);
    assert_eq!(trailing.total_lines, 1);
    assert_eq!(trailing.returned, Some(LineRange { start: 1, end: 1 }));
    assert!(trailing.complete);
    let (trailing_full, _, _) = completed(trailing_outcome);
    assert_eq!(
        body_after(&trailing_full, "[/ferric:file_observation:v1]\n"),
        "alpha\n"
    );
}

#[test]
fn read_file_past_eof_is_an_explicit_empty_uncovered_range() {
    let (directory, workspace, registry) = setup(8_192);
    std::fs::write(directory.path().join("short.txt"), b"one\ntwo").unwrap();

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "read_file",
        &json!({"path": "short.txt", "start_line": 9, "end_line": 12}),
    );
    let observation = file_observation(intent);
    assert_eq!(observation.total_lines, 2);
    assert_eq!(observation.returned, None);
    assert!(observation.coverage.is_empty());
    assert!(!observation.complete);
    let (full, _, _) = completed(outcome);
    assert!(full.contains("returned=none"));
    assert_eq!(body_after(&full, "[/ferric:file_observation:v1]\n"), "");
}

#[test]
fn truncated_read_is_honest_and_retains_full_trace_output() {
    let (directory, workspace, registry) = setup(512);
    let raw = "x".repeat(2_000);
    std::fs::write(directory.path().join("large.txt"), &raw).unwrap();

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "read_file",
        &json!({"path": "large.txt"}),
    );
    let observation = file_observation(intent);
    assert!(observation.model_truncated);
    assert!(!observation.complete);
    assert!(observation.coverage.is_empty());

    let (full, for_model, metadata) = completed(outcome);
    assert!(full.ends_with(&raw));
    assert!(full.contains("model_truncated=1"));
    assert!(for_model.contains("output truncated for model"));
    assert_ne!(full, for_model);
    assert_eq!(metadata, ToolObservation::File(observation));
}

#[test]
fn navigation_zero_results_are_explicit_and_stably_hashed() {
    let (_directory, workspace, registry) = setup(8_192);
    let expected_hash = digest(b"");

    for (name, args, expected_kind) in [
        (
            "find_files",
            json!({"pattern": "definitely-absent"}),
            NavigationKind::FindFiles,
        ),
        (
            "search_files",
            json!({"query": "definitely-absent"}),
            NavigationKind::SearchFiles,
        ),
    ] {
        let (first_intent, first_outcome) = controlled_call(&registry, &workspace, name, &args);
        let first = navigation_observation(first_intent);
        let (second_intent, _) = controlled_call(&registry, &workspace, name, &args);
        let second = navigation_observation(second_intent);

        assert_eq!(first.kind, expected_kind);
        assert_eq!(first.root, ".");
        assert_eq!(first.matches, 0);
        assert!(!first.cap_reached);
        assert!(!first.has_more);
        assert_eq!(first.result_sha256, expected_hash);
        assert_eq!(first.result_sha256, second.result_sha256);
        assert_lowercase_sha256(&first.result_sha256);
        let (full, _, metadata) = completed(first_outcome);
        assert_eq!(
            body_after(&full, "[/ferric:navigation_observation:v1]\n"),
            "(no matches)"
        );
        assert_eq!(metadata, ToolObservation::Navigation(first));
    }
}

#[test]
fn controlled_navigation_rejects_a_zero_result_cap() {
    let (_directory, workspace, registry) = setup(8_192);
    for (name, args) in [
        ("find_files", json!({"pattern": "x", "max_results": 0})),
        ("search_files", json!({"query": "x", "max_results": 0})),
    ] {
        match registry.prepare_controlled(&workspace, name, &args) {
            PrepareOutcome::Rejected { error, .. } => {
                assert_eq!(error.kind, PrepareErrorKind::InvalidArguments);
                assert!(error.message.contains("positive integer"));
            }
            other => panic!("expected zero-cap rejection, got {other:?}"),
        }
    }
}

#[test]
fn navigation_reports_cap_and_exhaustion_independently() {
    let (directory, workspace, registry) = setup(8_192);
    for name in ["a_hit.txt", "b_hit.txt", "c_hit.txt"] {
        std::fs::write(directory.path().join(name), b"needle\n").unwrap();
    }

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "find_files",
        &json!({"pattern": "hit", "max_results": 2}),
    );
    let observation = navigation_observation(intent);
    assert_eq!(observation.matches, 2);
    assert!(observation.cap_reached);
    assert!(observation.has_more, "the two-result view is not exhausted");
    let (full, _, _) = completed(outcome);
    let body = body_after(&full, "[/ferric:navigation_observation:v1]\n");
    assert_eq!(observation.result_sha256, digest(body.as_bytes()));
    assert_lowercase_sha256(&observation.result_sha256);

    std::fs::remove_file(directory.path().join("c_hit.txt")).unwrap();
    let (exact_intent, _) = controlled_call(
        &registry,
        &workspace,
        "find_files",
        &json!({"pattern": "hit", "max_results": 2}),
    );
    let exact = navigation_observation(exact_intent);
    assert!(exact.cap_reached);
    assert!(
        !exact.has_more,
        "exactly filling the cap is still exhausted"
    );

    std::fs::write(
        directory.path().join("search.txt"),
        b"needle one\nneedle two\nneedle three\n",
    )
    .unwrap();
    let (search_intent, _) = controlled_call(
        &registry,
        &workspace,
        "search_files",
        &json!({"query": "needle", "max_results": 2}),
    );
    let search = navigation_observation(search_intent);
    assert_eq!(search.matches, 2);
    assert!(search.cap_reached);
    assert!(search.has_more);
}

#[test]
fn search_navigation_normalizes_root_and_hashes_returned_bytes() {
    let (directory, workspace, registry) = setup(8_192);
    std::fs::create_dir_all(directory.path().join("src/nested")).unwrap();
    std::fs::write(
        directory.path().join("src/nested/code.rs"),
        b"first needle\nsecond needle\n",
    )
    .unwrap();
    let args = json!({"query": "needle", "path": "src\\nested"});

    let (first_intent, first_outcome) =
        controlled_call(&registry, &workspace, "search_files", &args);
    let first = navigation_observation(first_intent);
    let (second_intent, _) = controlled_call(&registry, &workspace, "search_files", &args);
    let second = navigation_observation(second_intent);
    let (full, _, _) = completed(first_outcome);
    let body = body_after(&full, "[/ferric:navigation_observation:v1]\n");

    assert_eq!(first.root, "src/nested");
    assert_eq!(first.matches, 2);
    assert_eq!(first.result_sha256, digest(body.as_bytes()));
    assert_eq!(first.result_sha256, second.result_sha256);
    assert_lowercase_sha256(&first.result_sha256);
}

#[cfg(any(unix, windows))]
#[test]
fn controlled_reads_reject_or_skip_links_outside_the_workspace() {
    let (directory, workspace, registry) = setup(8_192);
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, b"outside-secret\n").unwrap();
    let file_link = directory.path().join("linked-secret.txt");
    let directory_link = directory.path().join("linked-directory");

    #[cfg(unix)]
    let links = std::os::unix::fs::symlink(&secret, &file_link)
        .and_then(|_| std::os::unix::fs::symlink(outside.path(), &directory_link));
    #[cfg(windows)]
    let links = std::os::windows::fs::symlink_file(&secret, &file_link)
        .and_then(|_| std::os::windows::fs::symlink_dir(outside.path(), &directory_link));
    if let Err(error) = links {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create search link fixtures: {error}");
    }

    for (name, args) in [
        ("read_file", json!({"path": "linked-secret.txt"})),
        ("list_dir", json!({"path": "linked-directory"})),
    ] {
        match registry.prepare_controlled(&workspace, name, &args) {
            PrepareOutcome::Rejected { .. } | PrepareOutcome::Denied { .. } => {}
            other => panic!("expected controlled {name} to reject an outside link, got {other:?}"),
        }
    }

    let (find_intent, find_outcome) = controlled_call(
        &registry,
        &workspace,
        "find_files",
        &json!({"pattern": "secret"}),
    );
    assert_eq!(navigation_observation(find_intent).matches, 0);
    let (find_full, _, _) = completed(find_outcome);
    assert_eq!(
        body_after(&find_full, "[/ferric:navigation_observation:v1]\n"),
        "(no matches)"
    );

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "search_files",
        &json!({"query": "outside-secret"}),
    );
    let observation = navigation_observation(intent);
    assert_eq!(observation.matches, 0);
    let (full, _, _) = completed(outcome);
    assert_eq!(
        body_after(&full, "[/ferric:navigation_observation:v1]\n"),
        "(no matches)"
    );
    assert_eq!(std::fs::read(&secret).unwrap(), b"outside-secret\n");
}

#[test]
fn truncated_navigation_marks_the_model_view_but_hashes_full_results() {
    let (directory, workspace, registry) = setup(512);
    let mut content = String::new();
    for index in 0..40 {
        content.push_str(&format!(
            "needle-{index:02}-{}\n",
            "long-result-fragment".repeat(3)
        ));
    }
    std::fs::write(directory.path().join("large-search.txt"), content).unwrap();

    let (intent, outcome) = controlled_call(
        &registry,
        &workspace,
        "search_files",
        &json!({"query": "needle", "max_results": 40}),
    );
    let observation = navigation_observation(intent);
    assert!(observation.model_truncated);
    assert_eq!(observation.matches, 40);
    assert!(!observation.has_more);

    let (full, for_model, metadata) = completed(outcome);
    let body = body_after(&full, "[/ferric:navigation_observation:v1]\n");
    assert_eq!(observation.result_sha256, digest(body.as_bytes()));
    assert!(full.contains("model_truncated=1"));
    assert!(for_model.contains("output truncated for model"));
    assert_eq!(metadata, ToolObservation::Navigation(observation));
}
