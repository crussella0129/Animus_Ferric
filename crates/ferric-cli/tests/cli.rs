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
fn unknown_args_fail_with_usage() {
    let out = ferric().arg("frobnicate").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.to_lowercase().contains("usage") || stderr.contains("unrecognized"));
}
