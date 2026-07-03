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
fn query_file_text_folds_into_prompt() {
    // A text/code --file is read and folded into the prompt (any model). It
    // shows up in the assembled prompt's char count.
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.md");
    let body = "MARKER ".repeat(100); // ~700 distinctive chars
    std::fs::write(&notes, &body).unwrap();
    let out = ferric()
        .args(["query", "--mock", "summarize the notes"])
        .arg("--workspace")
        .arg(dir.path())
        .arg("--file")
        .arg(&notes)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let trace_dir = dir.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace");
    let content = std::fs::read_to_string(trace.path()).unwrap();
    let max_chars = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"]["type"] == "prompt_assembled")
        .filter_map(|v| v["event"]["chars"].as_u64())
        .max()
        .unwrap_or(0);
    assert!(
        max_chars >= body.len() as u64,
        "assembled prompt ({max_chars} chars) should include the {}-char file",
        body.len()
    );
}

#[test]
fn query_file_media_skipped_with_reason() {
    // A media --file with no multimodal-capable backend is skipped, non-fatally,
    // with the reason surfaced on stderr (never silent).
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, [0u8; 16]).unwrap();
    let out = ferric()
        .args(["query", "--mock", "describe the photo"])
        .arg("--workspace")
        .arg(dir.path())
        .arg("--file")
        .arg(&photo)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "skip is non-fatal; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("skip") && stderr.contains("photo.png"),
        "expected a surfaced skip reason; stderr: {stderr}"
    );
}

#[test]
fn max_ring_caps_the_offered_tools() {
    // Run a `--mock` query at Small tier (params-b 8 → ring ceiling 1) and read
    // the tools actually offered to the grammar from the trace's PromptAssembled.
    fn offered(extra: &[&str], dir: &std::path::Path) -> Vec<String> {
        let out = ferric()
            .args(["query", "--mock", "do a task", "--params-b", "8"])
            .args(extra)
            .arg("--workspace")
            .arg(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let trace_dir = dir.join(".ferric").join("trace");
        let trace = std::fs::read_dir(&trace_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
            .expect("a q-*.jsonl trace");
        let content = std::fs::read_to_string(trace.path()).unwrap();
        content
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["event"]["type"] == "prompt_assembled")
            .and_then(|v| {
                v["event"]["offered_tools"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    // Without the cap: Small admits Ring 0 + Ring 1.
    let d1 = tempfile::tempdir().unwrap();
    let full = offered(&[], d1.path());
    assert!(
        full.contains(&"search_files".to_string()) && full.contains(&"move_path".to_string()),
        "Small offers Ring 1 too: {full:?}"
    );

    // --max-ring 0: only the Ring-0 core, even at Small tier.
    let d2 = tempfile::tempdir().unwrap();
    let core = offered(&["--max-ring", "0"], d2.path());
    assert!(
        !core.contains(&"search_files".to_string()) && !core.contains(&"move_path".to_string()),
        "--max-ring 0 drops Ring 1: {core:?}"
    );
    assert!(
        core.contains(&"write_file".to_string()),
        "the core is still offered: {core:?}"
    );
}

#[test]
fn persisted_calibrated_ring_caps_the_offered_tools() {
    // A persisted `calibrated_ring: 0` makes a query run at the core grammar
    // automatically — no `--max-ring` flag (ADR-029, the durable promotion). And
    // with no profile file the offered set is unchanged (no-op safety). Reads the
    // offered tools from the trace, like `max_ring_caps_the_offered_tools`.
    fn offered(model: &str, profile_dir: &std::path::Path, ws: &std::path::Path) -> Vec<String> {
        let out = ferric()
            .args([
                "query",
                "--mock",
                "do a task",
                "--params-b",
                "8",
                "--protocol",
                "grammar",
            ])
            .args(["--model", model])
            .arg("--profile-dir")
            .arg(profile_dir)
            .arg("--workspace")
            .arg(ws)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let trace_dir = ws.join(".ferric").join("trace");
        let trace = std::fs::read_dir(&trace_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
            .expect("a q-*.jsonl trace");
        let content = std::fs::read_to_string(trace.path()).unwrap();
        content
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["event"]["type"] == "prompt_assembled")
            .and_then(|v| {
                v["event"]["offered_tools"].as_array().map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    // A profile dir carrying calibrated_ring 0 for ConstrainedJson + this model.
    let pdir = tempfile::tempdir().unwrap();
    std::fs::write(
        pdir.path().join("model_profiles.json"),
        r#"[{"model":"mockmodel","params_b":8.0,"protocol":"ConstrainedJson","measured_level":null,"tier_from_params":"Small","tier_from_measured":null,"calibrated_ring":0}]"#,
    )
    .unwrap();

    // With the persisted ring 0: only the Ring-0 core, even at Small tier — no flag.
    let ws1 = tempfile::tempdir().unwrap();
    let capped = offered("mockmodel", pdir.path(), ws1.path());
    assert!(
        !capped.contains(&"search_files".to_string()) && capped.contains(&"write_file".to_string()),
        "persisted calibrated_ring 0 caps to the core: {capped:?}"
    );

    // No-op safety: an empty profile dir leaves Small's Ring 1 intact.
    let empty = tempfile::tempdir().unwrap();
    let ws2 = tempfile::tempdir().unwrap();
    let full = offered("mockmodel", empty.path(), ws2.path());
    assert!(
        full.contains(&"search_files".to_string()),
        "no profile ⇒ unchanged (Ring 1 still offered): {full:?}"
    );
}

#[test]
fn unknown_args_fail_with_usage() {
    let out = ferric().arg("frobnicate").output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.to_lowercase().contains("usage") || stderr.contains("unrecognized"));
}

/// T-3606 E2E (sprint 36, ADR-046): a real `ferric mcp --mock` child process,
/// driven over its actual stdin/stdout pipes — proves the real stdio framing
/// (line delimiting, stdout purity), not just the in-process dispatch logic
/// `mcp::tests` already covers.
#[test]
fn mcp_stdio_e2e() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let mut child = ferric()
        .args(["mcp", "--mock"])
        .arg("--workspace")
        .arg(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
    )
    .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert!(
        resp["result"]["protocolVersion"].is_string(),
        "initialize response: {line}"
    );

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
    )
    .unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["tools"][0]["name"], "ferric_query");

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"ferric_query","arguments":{{"prompt":"do a mock task"}}}}}}"#
    )
    .unwrap();
    line.clear();
    reader.read_line(&mut line).unwrap();
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["content"][0]["text"], "mock run complete");
    assert_eq!(resp["result"]["isError"], false);

    drop(stdin); // EOF: the server should exit cleanly on its own.
    let status = child.wait().unwrap();
    assert!(status.success(), "ferric mcp did not exit cleanly on EOF");
}
