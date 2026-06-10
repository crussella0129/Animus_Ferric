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
