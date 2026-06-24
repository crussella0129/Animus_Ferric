//! Black-box tests of the `ferric` binary.

use std::process::Command;

fn ferric() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferric"))
}

#[test]
fn version_flag() {
    let out = ferric().arg("--version").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("ferric "));
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn trace_cat_renders_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.jsonl");
    let lines = [
        r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"note","text":"hello"}}"#,
        r#"{"v":9,"ts_ms":2,"session":"s","seq":1,"event":{"type":"GRAMMAR_STATE","mask_us":50}}"#,
        r#"{"v":1,"ts_ms":3,"session":"s","seq":2,"event":{"type":"session_end","reason":"done"}}"#,
    ];
    std::fs::write(&path, lines.join("\n")).unwrap();

    let out = ferric().args(["trace", "cat"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "exit code must be 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("note: hello"));
    assert!(stdout.contains("[unknown event: GRAMMAR_STATE]"));
    assert!(stdout.contains("session end (done)"));
}

#[test]
fn no_args_fails_with_usage() {
    let out = ferric().output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.to_lowercase().contains("usage:"));
}

#[test]
fn mock_query_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["query", "--mock", "do a mock task"])
        .arg("--workspace")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("mock run complete"));

    // Exactly one parseable trace spanning session_start..session_end.
    let trace_dir = dir.path().join(".ferric").join("trace");
    let traces: Vec<_> = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .collect();
    assert_eq!(traces.len(), 1, "exactly one q-*.jsonl trace");
    let content = std::fs::read_to_string(traces[0].path()).unwrap();
    let first: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();
    let last: serde_json::Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();
    assert_eq!(first["event"]["type"], "session_start");
    assert_eq!(last["event"]["type"], "session_end");
    assert_eq!(last["event"]["reason"], "task_complete");
    // The mock's write went through the real guard + registry.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ferric-mock.txt")).unwrap(),
        "mock run"
    );
}

#[test]
fn query_without_backend_errors() {
    // Default build lacks backend-mistralrs: a non-mock query must fail with
    // a message naming the missing feature.
    #[cfg(not(feature = "backend-mistralrs"))]
    {
        let dir = tempfile::tempdir().unwrap();
        let out = ferric()
            .args(["query", "real task"])
            .arg("--workspace")
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8(out.stderr).unwrap();
        // Names the missing backend. The no-backend build's stub says
        // "...--features backend-mistralrs..."; a backend-openai-only build
        // reaches create_provider and says "built without mistralrs backend".
        // Both name `mistralrs`.
        assert!(stderr.contains("mistralrs"));
    }
}

#[test]
fn unknown_args_fail_with_usage() {
    let out = ferric().arg("frobnicate").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.to_lowercase().contains("usage") || stderr.contains("unrecognized"));
}
