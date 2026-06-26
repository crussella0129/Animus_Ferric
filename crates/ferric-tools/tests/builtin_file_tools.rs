//! Unit tests for the builtin file tools, driven through the registry
//! chokepoint exactly as the agent loop will drive them.

use ferric_guard::Workspace;
use ferric_tools::{ExecuteOutcome, Registry, register_builtin_tools};
use serde_json::json;

fn setup() -> (tempfile::TempDir, Workspace, Registry) {
    let dir = tempfile::tempdir().unwrap();
    let ws = Workspace::new(dir.path()).unwrap();
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    (dir, ws, registry)
}

fn expect_completed(outcome: ExecuteOutcome) -> (String, bool) {
    match outcome {
        ExecuteOutcome::Completed { output, .. } => (output.full, output.is_error),
        other => panic!("expected Completed, got {other:?}"),
    }
}

#[test]
fn write_then_read_roundtrip() {
    let (_dir, ws, registry) = setup();
    let content = "alpha\nbeta\ngamma\n";

    let (write_out, write_err) = expect_completed(registry.execute(
        &ws,
        "write_file",
        &json!({"path": "notes/today.md", "content": content}),
    ));
    assert!(!write_err, "write failed: {write_out}");

    let (read_out, read_err) =
        expect_completed(registry.execute(&ws, "read_file", &json!({"path": "notes/today.md"})));
    assert!(!read_err, "read failed: {read_out}");
    assert_eq!(read_out, content);
}

#[test]
fn tools_refuse_outside_workspace() {
    let (dir, ws, registry) = setup();
    let outside = dir.path().parent().unwrap().join("escape.txt");

    for (tool, args) in [
        (
            "write_file",
            json!({"path": outside.to_string_lossy(), "content": "x"}),
        ),
        ("read_file", json!({"path": "../escape.txt"})),
        ("list_dir", json!({"path": ".."})),
    ] {
        let outcome = registry.execute(&ws, tool, &args);
        assert!(
            matches!(outcome, ExecuteOutcome::Denied { .. }),
            "{tool} must be denied outside the workspace, got {outcome:?}"
        );
    }
    assert!(!outside.exists(), "denied write must create nothing");
}

#[test]
fn list_dir_deterministic_order() {
    let (dir, ws, registry) = setup();
    for name in ["zeta.txt", "alpha.txt", "mike.txt", "echo.txt", "kilo.txt"] {
        std::fs::write(dir.path().join(name), "x").unwrap();
    }
    std::fs::create_dir(dir.path().join("subdir")).unwrap();

    let (first, _) = expect_completed(registry.execute(&ws, "list_dir", &json!({"path": "."})));
    let (second, _) = expect_completed(registry.execute(&ws, "list_dir", &json!({"path": "."})));
    assert_eq!(first, second, "two listings must be identical");

    let lines: Vec<&str> = first.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted, "listing must be sorted");
    assert!(
        lines.contains(&"subdir/"),
        "directories get a trailing slash"
    );
}

#[test]
fn read_missing_file_is_error_not_panic() {
    let (_dir, ws, registry) = setup();
    let (output, is_error) =
        expect_completed(registry.execute(&ws, "read_file", &json!({"path": "nope.txt"})));
    assert!(is_error);
    assert!(output.contains("nope.txt"));
}

#[test]
fn move_path_renames_file() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("old.py"), "x = 1").unwrap();
    let (out, err) = expect_completed(registry.execute(
        &ws,
        "move_path",
        &json!({"from": "old.py", "to": "greet.py"}),
    ));
    assert!(!err, "{out}");
    assert!(!dir.path().join("old.py").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("greet.py")).unwrap(),
        "x = 1"
    );
}

#[test]
fn move_path_renames_dir() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir(dir.path().join("a")).unwrap();
    std::fs::write(dir.path().join("a").join("f"), "y").unwrap();
    let (_out, err) =
        expect_completed(registry.execute(&ws, "move_path", &json!({"from": "a", "to": "b"})));
    assert!(!err);
    assert!(dir.path().join("b").join("f").exists());
    assert!(!dir.path().join("a").exists());
}

#[test]
fn move_path_missing_source_is_error() {
    let (_dir, ws, registry) = setup();
    let (output, is_error) = expect_completed(registry.execute(
        &ws,
        "move_path",
        &json!({"from": "ghost.txt", "to": "x.txt"}),
    ));
    assert!(is_error);
    assert!(output.contains("ghost.txt"));
}

#[test]
fn move_path_outside_to_denied() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("secret.txt"), "data").unwrap();
    // `to` escapes the workspace — must be denied, source left intact.
    let outcome = registry.execute(
        &ws,
        "move_path",
        &json!({"from": "secret.txt", "to": "../escaped.txt"}),
    );
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "cross-boundary move must be denied, got {outcome:?}"
    );
    assert!(
        dir.path().join("secret.txt").exists(),
        "source untouched on deny"
    );
    assert!(!dir.path().parent().unwrap().join("escaped.txt").exists());
}

#[test]
fn make_dir_creates_parents_and_is_idempotent() {
    let (dir, ws, registry) = setup();
    let (_o, err) = expect_completed(registry.execute(&ws, "make_dir", &json!({"path": "a/b/c"})));
    assert!(!err);
    assert!(dir.path().join("a").join("b").join("c").is_dir());
    // Idempotent: a second call on the existing dir succeeds.
    let (_o2, err2) =
        expect_completed(registry.execute(&ws, "make_dir", &json!({"path": "a/b/c"})));
    assert!(!err2, "make_dir must be idempotent");
}

#[test]
fn search_files_finds_matches_with_relpath_and_lineno() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/a.rs"),
        "fn foo() {}\nlet MARKER = 1;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("b.txt"), "no hits here\nMARKER again\n").unwrap();

    let (out, err) =
        expect_completed(registry.execute(&ws, "search_files", &json!({"query": "MARKER"})));
    assert!(!err, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.iter().any(|l| l.starts_with("b.txt:2:")), "{out}");
    assert!(lines.iter().any(|l| l.starts_with("src/a.rs:2:")), "{out}");
    assert!(lines.iter().all(|l| l.contains("MARKER")));
    // Sorted (ADR-008).
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted, "results must be sorted");
}

#[test]
fn search_files_miss_is_empty_not_error() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("a.txt"), "nothing relevant").unwrap();
    let (out, err) =
        expect_completed(registry.execute(&ws, "search_files", &json!({"query": "ZZZ_absent"})));
    assert!(!err, "a miss must not be an error: {out}");
    assert!(out.is_empty(), "no matches → empty output, got {out:?}");
}

#[test]
fn search_files_caps_results() {
    let (dir, ws, registry) = setup();
    let body: String = (0..20).map(|_| "HIT\n").collect();
    std::fs::write(dir.path().join("many.txt"), body).unwrap();
    let (out, _err) = expect_completed(registry.execute(
        &ws,
        "search_files",
        &json!({"query": "HIT", "max_results": 5}),
    ));
    assert_eq!(out.lines().count(), 5, "must cap at max_results");
}

#[test]
fn search_files_skips_binary_and_noise_dirs() {
    let (dir, ws, registry) = setup();
    // Non-UTF-8 file (read_to_string fails → skipped), even though it has the bytes.
    std::fs::write(
        dir.path().join("blob.bin"),
        [0xff, 0xfe, b'N', b'E', b'E', b'D', b'L', b'E'],
    )
    .unwrap();
    // Match under a noise dir.
    std::fs::create_dir_all(dir.path().join("target/sub")).unwrap();
    std::fs::write(dir.path().join("target/sub/gen.txt"), "NEEDLE").unwrap();
    // A real hit.
    std::fs::write(dir.path().join("real.txt"), "NEEDLE here").unwrap();

    let (out, err) =
        expect_completed(registry.execute(&ws, "search_files", &json!({"query": "NEEDLE"})));
    assert!(!err, "{out}");
    assert!(out.contains("real.txt:1:"), "real hit present: {out}");
    assert!(!out.contains("blob.bin"), "binary file must be skipped");
    assert!(!out.contains("target/"), "noise dir must be skipped");
}

#[test]
fn search_files_refuses_outside_workspace() {
    let (_dir, ws, registry) = setup();
    let outcome = registry.execute(&ws, "search_files", &json!({"query": "x", "path": ".."}));
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "search outside the workspace must be denied, got {outcome:?}"
    );
}

#[test]
fn search_files_deterministic() {
    let (dir, ws, registry) = setup();
    for n in ["c.txt", "a.txt", "b.txt"] {
        std::fs::write(dir.path().join(n), "TOKEN\n").unwrap();
    }
    let (first, _) =
        expect_completed(registry.execute(&ws, "search_files", &json!({"query": "TOKEN"})));
    let (second, _) =
        expect_completed(registry.execute(&ws, "search_files", &json!({"query": "TOKEN"})));
    assert_eq!(
        first, second,
        "two identical searches must match byte-for-byte"
    );
}

#[test]
fn edit_file_replaces_first_occurrence() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.rs"), "let x = 1;\nlet x = 2;\n").unwrap();
    let (out, err) = expect_completed(registry.execute(
        &ws,
        "edit_file",
        &json!({"path": "f.rs", "old_string": "let x = 1;", "new_string": "let y = 9;"}),
    ));
    assert!(!err, "{out}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.rs")).unwrap(),
        "let y = 9;\nlet x = 2;\n",
        "only the first occurrence is replaced"
    );
}

#[test]
fn edit_file_absent_old_string_is_error_and_no_write() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.txt"), "original").unwrap();
    let (output, is_error) = expect_completed(registry.execute(
        &ws,
        "edit_file",
        &json!({"path": "f.txt", "old_string": "absent", "new_string": "x"}),
    ));
    assert!(is_error);
    assert!(output.contains("f.txt"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "original",
        "file untouched when old_string is absent"
    );
}

#[test]
fn edit_file_empty_old_string_is_error() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.txt"), "x").unwrap();
    let (_o, is_error) = expect_completed(registry.execute(
        &ws,
        "edit_file",
        &json!({"path": "f.txt", "old_string": "", "new_string": "y"}),
    ));
    assert!(is_error, "empty old_string must error");
}

#[test]
fn edit_file_outside_workspace_denied() {
    let (_dir, ws, registry) = setup();
    let outcome = registry.execute(
        &ws,
        "edit_file",
        &json!({"path": "../escape.txt", "old_string": "a", "new_string": "b"}),
    );
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "edit outside the workspace must be denied, got {outcome:?}"
    );
}

#[test]
fn delete_path_removes_file() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("gone.txt"), "x").unwrap();
    let (out, err) =
        expect_completed(registry.execute(&ws, "delete_path", &json!({"path": "gone.txt"})));
    assert!(!err, "{out}");
    assert!(!dir.path().join("gone.txt").exists());
}

#[test]
fn delete_path_removes_empty_dir() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    let (_o, err) =
        expect_completed(registry.execute(&ws, "delete_path", &json!({"path": "empty"})));
    assert!(!err);
    assert!(!dir.path().join("empty").exists());
}

#[test]
fn delete_path_nonempty_dir_needs_recursive() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    std::fs::write(dir.path().join("d").join("f"), "x").unwrap();
    // Without recursive → error, tree intact.
    let (output, is_error) =
        expect_completed(registry.execute(&ws, "delete_path", &json!({"path": "d"})));
    assert!(is_error);
    assert!(output.contains("recursive"));
    assert!(
        dir.path().join("d").join("f").exists(),
        "tree intact without recursive"
    );
    // With recursive → gone.
    let (_o, err) = expect_completed(registry.execute(
        &ws,
        "delete_path",
        &json!({"path": "d", "recursive": true}),
    ));
    assert!(!err);
    assert!(!dir.path().join("d").exists());
}

#[test]
fn delete_path_missing_is_error() {
    let (_dir, ws, registry) = setup();
    let (output, is_error) =
        expect_completed(registry.execute(&ws, "delete_path", &json!({"path": "ghost.txt"})));
    assert!(is_error);
    assert!(output.contains("ghost.txt"));
}

#[test]
fn delete_path_outside_workspace_denied() {
    let (dir, ws, registry) = setup();
    let outside = dir.path().parent().unwrap().join("victim.txt");
    std::fs::write(&outside, "data").unwrap();
    let outcome = registry.execute(&ws, "delete_path", &json!({"path": "../victim.txt"}));
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "delete outside the workspace must be denied, got {outcome:?}"
    );
    assert!(outside.exists(), "denied delete must remove nothing");
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn delete_path_into_ferric_denied() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir_all(dir.path().join(".ferric")).unwrap();
    std::fs::write(dir.path().join(".ferric").join("server.json"), "{}").unwrap();
    let outcome = registry.execute(&ws, "delete_path", &json!({"path": ".ferric/server.json"}));
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "deleting under .ferric must be denied, got {outcome:?}"
    );
    assert!(dir.path().join(".ferric").join("server.json").exists());
}

#[test]
fn move_path_into_ferric_denied() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("x.txt"), "data").unwrap();
    let outcome = registry.execute(
        &ws,
        "move_path",
        &json!({"from": "x.txt", "to": ".ferric/stash.txt"}),
    );
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "move into .ferric must be denied, got {outcome:?}"
    );
}

#[test]
fn rings_gate_builtins_by_tier() {
    use ferric_core::{ModelProfile, policy_for};
    let mut registry = Registry::new();
    register_builtin_tools(&mut registry);
    let profile = |params_b: f32| ModelProfile {
        params_b,
        quant: "Q4_K_M".to_string(),
        ctx: 4096,
        family: "t".to_string(),
        measured_level: None,
    };

    // Nano (params < 4 → ring ceiling 0) → exactly the 6 Ring-0 core tools.
    let nano = registry.tools_for_policy(&policy_for(&profile(1.0)));
    let nano_names: Vec<&str> = nano.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(nano.len(), 6, "Nano gets the 6-tool core: {nano_names:?}");
    assert!(
        nano_names.contains(&"write_file"),
        "the core (e.g. write_file) is never dropped by the cap"
    );
    assert!(
        !nano_names.contains(&"search_files") && !nano_names.contains(&"move_path"),
        "Ring-1 tools are not in a Nano grammar"
    );

    // Small (4..13 → ring ceiling 1) → the full 10 (Ring 0 + the 4-tool Ring 1).
    let small = registry.tools_for_policy(&policy_for(&profile(8.0)));
    let small_names: Vec<&str> = small.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        small.len(),
        10,
        "Small adds the 4-tool Ring 1: {small_names:?}"
    );
    for ring1 in ["search_files", "move_path", "find_files", "copy_file"] {
        assert!(small_names.contains(&ring1), "Ring 1 includes {ring1}");
    }
    // The Ring-1 round-out stays out of a Nano grammar.
    assert!(
        !nano_names.contains(&"find_files") && !nano_names.contains(&"copy_file"),
        "Ring-1 round-out absent at Nano"
    );
    // Ring 2 (`multi_edit`) is still above the Small ceiling.
    assert!(
        !small_names.contains(&"multi_edit"),
        "Ring-2 multi_edit absent at Small"
    );

    // Medium (13..30 → ring ceiling 2) → 11: Ring 0 + Ring 1 + `multi_edit`.
    let medium = registry.tools_for_policy(&policy_for(&profile(20.0)));
    let medium_names: Vec<&str> = medium.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        medium.len(),
        11,
        "Medium adds Ring 2 (multi_edit): {medium_names:?}"
    );
    assert!(
        medium_names.contains(&"multi_edit"),
        "Ring 2 includes multi_edit: {medium_names:?}"
    );
}

#[test]
fn find_files_matches_names_sorted_scoped_and_skips_noise() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("config.toml"), "a").unwrap();
    std::fs::write(dir.path().join("notes.md"), "b").unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("config.rs"), "c").unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git").join("config"), "noise").unwrap();

    // From the root: both `config` files, name-sorted, no `notes.md`, no `.git`.
    let (out, err) =
        expect_completed(registry.execute(&ws, "find_files", &json!({"pattern": "config"})));
    assert!(!err);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, vec!["config.toml", "src/config.rs"], "got {lines:?}");

    // `path` scopes the walk.
    let (scoped, _) = expect_completed(registry.execute(
        &ws,
        "find_files",
        &json!({"pattern": "config", "path": "src"}),
    ));
    assert_eq!(scoped, "src/config.rs");

    // Cap honoured.
    let (capped, _) = expect_completed(registry.execute(
        &ws,
        "find_files",
        &json!({"pattern": "config", "max_results": 1}),
    ));
    assert_eq!(capped.lines().count(), 1);

    // Empty pattern errors.
    let (_, empty_err) =
        expect_completed(registry.execute(&ws, "find_files", &json!({"pattern": ""})));
    assert!(empty_err, "empty pattern must error");
}

#[test]
fn copy_file_copies_keeps_original_and_creates_parent() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("a.txt"), "payload").unwrap();
    let (out, err) = expect_completed(registry.execute(
        &ws,
        "copy_file",
        &json!({"from": "a.txt", "to": "b/a.txt"}),
    ));
    assert!(!err && out.contains("copied"), "copy failed: {out}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b").join("a.txt")).unwrap(),
        "payload"
    );
    assert!(dir.path().join("a.txt").exists(), "original kept");
}

#[test]
fn copy_file_into_ferric_denied() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("x.txt"), "data").unwrap();
    let outcome = registry.execute(
        &ws,
        "copy_file",
        &json!({"from": "x.txt", "to": ".ferric/stash.txt"}),
    );
    assert!(
        matches!(outcome, ExecuteOutcome::Denied { .. }),
        "copy into .ferric must be denied, got {outcome:?}"
    );
}

#[test]
fn copy_file_directory_source_errors() {
    let (dir, ws, registry) = setup();
    std::fs::create_dir_all(dir.path().join("adir")).unwrap();
    let (out, is_error) = expect_completed(registry.execute(
        &ws,
        "copy_file",
        &json!({"from": "adir", "to": "bdir"}),
    ));
    assert!(is_error, "a directory source must error, got: {out}");
}

#[test]
fn multi_edit_applies_ordered_batch_atomically() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.txt"), "a b").unwrap();
    // Sequential: "a b" -[a→X]-> "X b" -[X b→DONE]-> "DONE" (2nd edits the 1st's output).
    let (out, err) = expect_completed(registry.execute(
        &ws,
        "multi_edit",
        &json!({"path": "f.txt", "edits": [
            {"old_string": "a", "new_string": "X"},
            {"old_string": "X b", "new_string": "DONE"}
        ]}),
    ));
    assert!(!err, "{out}");
    assert!(out.contains("applied 2 edits"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "DONE"
    );
}

#[test]
fn multi_edit_missing_old_leaves_file_unchanged() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.txt"), "hello world").unwrap();
    // The first edit would apply, but the second's old_string is absent → abort,
    // nothing written.
    let (_out, err) = expect_completed(registry.execute(
        &ws,
        "multi_edit",
        &json!({"path": "f.txt", "edits": [
            {"old_string": "hello", "new_string": "hi"},
            {"old_string": "ABSENT", "new_string": "z"}
        ]}),
    ));
    assert!(err, "a missing old_string must error");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "hello world",
        "file must be byte-identical when any edit fails"
    );
}

#[test]
fn multi_edit_empty_edits_and_empty_old_error() {
    let (dir, ws, registry) = setup();
    std::fs::write(dir.path().join("f.txt"), "data").unwrap();
    let (_o1, e1) = expect_completed(registry.execute(
        &ws,
        "multi_edit",
        &json!({"path": "f.txt", "edits": []}),
    ));
    assert!(e1, "empty edits must error");
    let (_o2, e2) = expect_completed(registry.execute(
        &ws,
        "multi_edit",
        &json!({"path": "f.txt", "edits": [{"old_string": "", "new_string": "x"}]}),
    ));
    assert!(e2, "empty old_string must error");
}
