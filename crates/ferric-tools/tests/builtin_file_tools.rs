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
