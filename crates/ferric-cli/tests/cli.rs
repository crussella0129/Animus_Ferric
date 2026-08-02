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

/// T-4005 (sprint 40): `ferric trace cat` legibly renders `HistoryCompacted`.
#[test]
fn trace_cat_renders_history_compacted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trace.jsonl");
    let lines = [
        r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"history_compacted","through_turn":2,"dropped_turns":3,"summary":"did a, b, c"}}"#,
    ];
    std::fs::write(&path, lines.join("\n")).unwrap();

    let out = ferric().args(["trace", "cat"]).arg(&path).output().unwrap();
    assert!(out.status.success(), "exit code must be 0");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("history compacted"));
    assert!(stdout.contains("folded 3 turns"));
    assert!(stdout.contains("through turn 2"));
    assert!(stdout.contains("did a, b, c"));
}

/// End-to-end against a `run()`-produced trace: validation must understand the
/// real TurnEnd → ToolCall → ToolResult order without replaying any tool.
#[test]
fn trace_verify_finds_no_drift_in_a_real_trace() {
    let dir = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["query", "--mock", "do a mock task"])
        .arg("--workspace")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let trace_dir = dir.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("the mock run must have written a trace")
        .path();

    // The mock script calls write_file and then task_complete, so this trace
    // exercises exactly the case that used to fail.
    let content = std::fs::read_to_string(&trace).unwrap();
    assert!(content.contains(r#""type":"tool_call""#));

    let out = ferric()
        .args(["trace", "verify"])
        .arg(&trace)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("Trace verification successful"),
        "verify rejected the trace structure: {combined}"
    );
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

/// T-3705: streaming (now default-on) must not duplicate output. The `--mock`
/// script's completions are native tool calls with no `message.text`
/// (`text: None`), so the default `complete_streaming` impl fires zero deltas
/// for either turn — the final echo is the ONLY place the text appears (the
/// EARS "no duplication, no missing text" clause).
#[test]
fn stream_flag_mock_no_duplication() {
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
    let occurrences = stdout.matches("mock run complete").count();
    assert_eq!(
        occurrences, 1,
        "expected exactly one occurrence of the final text, got {occurrences}: {stdout:?}"
    );
}

#[test]
fn query_without_backend_errors() {
    // Default build lacks backend-openai: a non-mock query must fail with
    // a message naming the missing feature.
    #[cfg(not(feature = "backend-openai"))]
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
        // Names the missing backend.
        assert!(stderr.contains("backend-openai"));
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
fn query_file_outside_selected_workspace_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "must not enter the prompt").unwrap();

    let out = ferric()
        .args(["query", "--mock", "summarize"])
        .arg("--workspace")
        .arg(&workspace)
        .arg("--file")
        .arg(&outside)
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("workspace containment") && stderr.contains("escapes workspace"),
        "unexpected denial: {stderr}"
    );
    assert!(!workspace.join("ferric-mock.txt").exists());
}

#[test]
fn query_file_sensitive_path_is_rejected_by_read_guard() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".env"), "TOKEN=must-not-leak").unwrap();

    let out = ferric()
        .args(["query", "--mock", "summarize"])
        .arg("--workspace")
        .arg(dir.path())
        .args(["--file", ".env"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("denied by read guard") && stderr.contains("denied_read_file"),
        "unexpected denial: {stderr}"
    );
    assert!(!dir.path().join("ferric-mock.txt").exists());
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
        full.contains(&"find_files".to_string()),
        "Small offers Ring 1 too: {full:?}"
    );

    // --max-ring 0: only the Ring-0 core, even at Small tier.
    let d2 = tempfile::tempdir().unwrap();
    let core = offered(&["--max-ring", "0"], d2.path());
    assert!(
        !core.contains(&"find_files".to_string()),
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
        !capped.contains(&"find_files".to_string()) && capped.contains(&"write_file".to_string()),
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

/// T-3802: `params_b`/`quant`/`family`/`ctx`/`temperature`/`profile_dir` lost
/// their clap `default_value_t`/`default_value` in favor of bare `Option<T>`,
/// resolved via `.unwrap_or(today's constant)` at the call site — no config
/// file exists yet at this point (T-3803). With NO flags and NO config file,
/// the resolved values must be byte-identical to before: default `--params-b`
/// 1.2 lands at `Tier::Nano` (512 max_output_tokens, per the pinned tier
/// table) — isolates the mechanical refactor from any config-precedence
/// regression, so a failure here points at T-3802, not T-3803.
#[test]
fn query_defaults_unchanged_after_clap_type_change() {
    let dir = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["query", "--mock", "do a task"])
        .arg("--workspace")
        .arg(dir.path())
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
    let policy = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["event"]["type"] == "policy_selected")
        .expect("a policy_selected event");
    assert_eq!(policy["event"]["tier"], "nano");
    assert_eq!(policy["event"]["max_output_tokens"], 512);
}

/// Shared by the T-3803 config-precedence tests below: the `policy_selected`
/// trace event's tier, for a `--mock` run against workspace `ws`.
fn policy_tier(ws: &std::path::Path) -> String {
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
        .find(|v| v["event"]["type"] == "policy_selected")
        .expect("a policy_selected event")["event"]["tier"]
        .as_str()
        .unwrap()
        .to_string()
}

fn write_project_config(ws: &std::path::Path, toml: &str) {
    std::fs::create_dir_all(ws.join(".ferric")).unwrap();
    std::fs::write(ws.join(".ferric").join("config.toml"), toml).unwrap();
}

/// T-3803: a `.ferric/config.toml` setting `params_b` takes effect with no
/// matching CLI flag — `params_b = 8.0` lands at `Tier::Small` (the same tier
/// `max_ring_caps_the_offered_tools` pins for `--params-b 8`).
#[test]
fn config_file_sets_default_without_flag() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "params_b = 8.0\n");

    let out = ferric()
        .args(["query", "--mock", "do a task"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(policy_tier(ws.path()), "small");
}

/// T-3803: a CLI flag wins over the same field set in `.ferric/config.toml`.
#[test]
fn cli_flag_overrides_config_file() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "params_b = 8.0\n");

    let out = ferric()
        .args(["query", "--mock", "do a task", "--params-b", "1.2"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(policy_tier(ws.path()), "nano");
}

/// C-001 (plan-critic, the significant finding): `model_key` — which feeds
/// the ADR-029 persisted-profile lookup — must be derived from the
/// POST-merge, config-resolved `model` value, not the raw CLI arg. Set
/// `model` ONLY via config.toml (no `--model` flag) alongside a persisted
/// `calibrated_ring: 0` record for that model; if `model_key` were derived
/// from the raw (unset) CLI arg instead, the profile lookup would be
/// silently skipped and Ring 1 tools would still be offered.
#[test]
fn config_only_model_still_resolves_profile() {
    let pdir = tempfile::tempdir().unwrap();
    std::fs::write(
        pdir.path().join("model_profiles.json"),
        r#"[{"model":"mockmodel","params_b":8.0,"protocol":"ConstrainedJson","measured_level":null,"tier_from_params":"Small","tier_from_measured":null,"calibrated_ring":0}]"#,
    )
    .unwrap();

    let ws = tempfile::tempdir().unwrap();
    write_project_config(
        ws.path(),
        &format!(
            "model = \"mockmodel\"\nprofile_dir = '{}'\n",
            pdir.path().display()
        ),
    );

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
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let trace_dir = ws.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace");
    let content = std::fs::read_to_string(trace.path()).unwrap();
    let offered: Vec<String> = content
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
        .unwrap_or_default();
    assert!(
        !offered.contains(&"find_files".to_string()) && offered.contains(&"write_file".to_string()),
        "a config-only `model` must still hit the persisted calibrated_ring 0 profile: {offered:?}"
    );
}

/// Test-critic C-002: `max_ring` is named in-scope for config precedence
/// (build-plan.md T-3803) but had no CLI-observable test — only `params_b`/
/// `model` did. Set `max_ring = 0` ONLY via config (no `--max-ring` flag) at
/// Small tier (`--params-b 8`, which otherwise offers Ring 1 too, per
/// `max_ring_caps_the_offered_tools`); the cap must still apply.
#[test]
fn config_only_max_ring_caps_the_offered_tools() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "max_ring = 0\n");

    let out = ferric()
        .args(["query", "--mock", "do a task", "--params-b", "8"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let trace_dir = ws.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace");
    let content = std::fs::read_to_string(trace.path()).unwrap();
    let offered: Vec<String> = content
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
        .unwrap_or_default();
    assert!(
        !offered.contains(&"find_files".to_string()) && offered.contains(&"write_file".to_string()),
        "a config-only `max_ring = 0` must cap to the core, same as `--max-ring 0`: {offered:?}"
    );
}

/// Test-critic C-002: same gap for `stream`. Streaming is now default-on, so
/// the interesting config-only test is that `stream = false` in the project
/// config SUPPRESSES live output. `--mock`'s NativeTools script has
/// `text: None` (no observable streaming difference — see
/// `stream_flag_mock_no_duplication`'s doc comment), so this uses
/// `--protocol grammar`, where the mock's completion text IS the raw
/// `{"tool":...}` JSON. With streaming active (the default), the default
/// `complete_streaming` fires ONE `Text` delta of that raw JSON (printed
/// live). With `stream = false`, the final echo of the clean summary appears
/// instead — proving the config override took effect.
#[test]
fn config_only_stream_disables_live_output() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "stream = false\n");

    let out = ferric()
        .args(["query", "--mock", "--protocol", "grammar", "do a task"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    // With streaming disabled, the clean task_complete summary appears, NOT
    // the raw streamed JSON.
    assert!(
        !stdout.contains("\"tool\":\"task_complete\""),
        "expected config `stream = false` to suppress live streaming, \
         but got raw JSON on stdout: {stdout:?}"
    );
}

/// C-004 (plan-critic): a malformed `.ferric/config.toml` degrades to absent
/// AND is traced as a `Note` — testable data, not just an unasserted
/// `eprintln!`.
#[test]
fn malformed_config_traced_as_note() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "this is not [valid toml");

    let out = ferric()
        .args(["query", "--mock", "do a task"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let trace_dir = ws.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace");
    let content = std::fs::read_to_string(trace.path()).unwrap();
    let has_note = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|v| {
            v["event"]["type"] == "note"
                && v["event"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("malformed config")
        });
    assert!(
        has_note,
        "expected a Note event carrying the malformed-config diagnostic"
    );
}

/// T-3806: an `Animus.md` at the workspace root folds into the assembled
/// system prompt — checked via the trace's `prompt_assembled` char count,
/// mirroring `query_file_text_folds_into_prompt`'s technique.
#[test]
fn animus_md_folds_into_prompt() {
    let ws = tempfile::tempdir().unwrap();
    let body = "PROJECT RULE ".repeat(50);
    std::fs::write(ws.path().join("Animus.md"), &body).unwrap();

    let out = ferric()
        .args(["query", "--mock", "do a task"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let trace_dir = ws.path().join(".ferric").join("trace");
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
        "assembled prompt ({max_chars} chars) should include the {}-char Animus.md",
        body.len()
    );
}

/// C-005 (plan-critic, narrowed to presence-only): an `Animus.md`'s presence
/// is traced as a `Note`. Absence staying untraced is already proven by every
/// other CLI test (none create an `Animus.md`, none show this Note).
#[test]
fn animus_md_present_traces_note() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("Animus.md"), "project rules").unwrap();

    let out = ferric()
        .args(["query", "--mock", "do a task"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let trace_dir = ws.path().join(".ferric").join("trace");
    let trace = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace");
    let content = std::fs::read_to_string(trace.path()).unwrap();
    let has_note = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|v| {
            v["event"]["type"] == "note"
                && v["event"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Animus.md applied")
        });
    assert!(
        has_note,
        "expected a Note event confirming Animus.md was applied"
    );
    // C-003 (test-critic): the --resume-ignores-Animus.md stderr note
    // (query.rs) is gated on `resume.is_some()` — prove it stays silent on an
    // ordinary (non-resume) run, so a regression that made it unconditional
    // would be caught.
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("ignores --prompts-dir/Animus.md"),
        "the resume-ignored note must not print on a non-resume run"
    );
}

/// A hand-written, synthetic trace fixture (matching `ferric-loop::replay`'s
/// own test idiom): `SessionPrompt` + `PolicySelected` (`NativeTools`,
/// matching `--mock`'s resolved protocol) + one COMPLETED turn (a `write_file`
/// call+result), no `SessionEnd` — the realistic shape of an interrupted
/// process (crashed after turn 0, before turn 1 started).
fn write_interrupted_trace_fixture(ws: &std::path::Path, session: &str) -> std::path::PathBuf {
    let trace_dir = ws.join(".ferric").join("trace");
    std::fs::create_dir_all(&trace_dir).unwrap();
    let path = trace_dir.join(format!("{session}.jsonl"));
    let lines = [
        format!(
            r#"{{"v":1,"ts_ms":1,"session":"{session}","seq":0,"event":{{"type":"session_start","workspace":"/ws"}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":2,"session":"{session}","seq":1,"event":{{"type":"policy_selected","tier":"nano","protocol":"native_tools","max_turns":15,"max_tools":10,"prompt_budget_tokens":2800,"max_output_tokens":512}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":3,"session":"{session}","seq":2,"event":{{"type":"session_prompt","system":"You are Ferric.","user":"do a mock task"}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":4,"session":"{session}","seq":3,"event":{{"type":"turn_start","turn":0}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":5,"session":"{session}","seq":4,"event":{{"type":"turn_end","turn":0,"text":null,"tool_call_count":1,"input_tokens":50,"output_tokens":10}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":6,"session":"{session}","seq":5,"event":{{"type":"tool_call","id":"tc-0","name":"write_file","args":{{"path":"a.txt","content":"hi"}}}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":7,"session":"{session}","seq":6,"event":{{"type":"tool_result","id":"tc-0","name":"write_file","output":"wrote 2 bytes","is_error":false,"duration_ms":1}}}}"#
        ),
        format!(
            r#"{{"v":1,"ts_ms":8,"session":"{session}","seq":7,"event":{{"type":"turn_start","turn":1}}}}"#
        ),
    ];
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

fn latest_q_trace(ws: &std::path::Path) -> std::path::PathBuf {
    let trace_dir = ws.join(".ferric").join("trace");
    std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .expect("a q-*.jsonl trace")
        .path()
}

#[test]
fn resume_continues_an_interrupted_session() {
    let ws = tempfile::tempdir().unwrap();
    let fixture = write_interrupted_trace_fixture(ws.path(), "orig-1");

    let out = ferric()
        .args(["query", "--mock", "--resume"])
        .arg(&fixture)
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let content = std::fs::read_to_string(latest_q_trace(ws.path())).unwrap();
    let resumed_from = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["event"]["type"] == "session_start")
        .map(|v| v["event"]["resumed_from"].clone())
        .unwrap();
    assert_eq!(resumed_from, serde_json::json!("orig-1"));
}

/// The first (turn 1, since the fixture resumes at turn 1) `prompt_assembled`
/// event's `chars` field for a `--resume` run of the given workspace/session,
/// with or without an extra trailing prompt argument.
fn first_resumed_turn_chars(
    ws: &std::path::Path,
    session: &str,
    extra_prompt: Option<&str>,
) -> u64 {
    let fixture = write_interrupted_trace_fixture(ws, session);
    let mut cmd = ferric();
    cmd.args(["query", "--mock", "--resume"]).arg(&fixture);
    if let Some(p) = extra_prompt {
        cmd.arg(p);
    }
    cmd.arg("--workspace").arg(ws);
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let content = std::fs::read_to_string(latest_q_trace(ws)).unwrap();
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"]["type"] == "prompt_assembled")
        .filter_map(|v| v["event"]["chars"].as_u64())
        .next()
        .expect("a prompt_assembled event")
}

#[test]
fn resume_with_extra_prompt_appends_nudge() {
    // C-002 (test-critic): a loose `>=` floor on the char count passes even
    // when the extra prompt is silently dropped, since the replayed history
    // alone already clears it. Assert the EXACT delta instead.
    let ws_without = tempfile::tempdir().unwrap();
    let chars_without = first_resumed_turn_chars(ws_without.path(), "orig-2a", None);

    let ws_with = tempfile::tempdir().unwrap();
    let chars_with = first_resumed_turn_chars(ws_with.path(), "orig-2b", Some("extra instruction"));

    assert_eq!(
        chars_with,
        chars_without + "extra instruction".len() as u64,
        "the extra prompt must add exactly its own length to the assembled turn"
    );
}

#[test]
fn no_resume_and_no_prompt_is_a_usage_error() {
    let ws = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["query", "--mock"])
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.to_lowercase().contains("usage") || stderr.contains("required"));
}

#[test]
fn resume_protocol_mismatch_is_a_clear_error() {
    let ws = tempfile::tempdir().unwrap();
    // The fixture records NativeTools; force this invocation's resolved
    // protocol to ConstrainedJson (grammar) via --protocol.
    let fixture = write_interrupted_trace_fixture(ws.path(), "orig-3");

    let out = ferric()
        .args(["query", "--mock", "--resume"])
        .arg(&fixture)
        .arg("--protocol")
        .arg("grammar")
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("NativeTools") && stderr.contains("ConstrainedJson"));
}

#[test]
fn resume_already_stopped_is_a_clear_error() {
    let ws = tempfile::tempdir().unwrap();
    let trace_dir = ws.path().join(".ferric").join("trace");
    std::fs::create_dir_all(&trace_dir).unwrap();
    let path = trace_dir.join("stopped.jsonl");
    let lines = [
        r#"{"v":1,"ts_ms":1,"session":"stopped-1","seq":0,"event":{"type":"session_start","workspace":"/ws"}}"#.to_string(),
        r#"{"v":1,"ts_ms":2,"session":"stopped-1","seq":1,"event":{"type":"policy_selected","tier":"nano","protocol":"native_tools","max_turns":15,"max_tools":10,"prompt_budget_tokens":2800,"max_output_tokens":512}}"#.to_string(),
        r#"{"v":1,"ts_ms":3,"session":"stopped-1","seq":2,"event":{"type":"session_prompt","system":"You are Ferric.","user":"do a mock task"}}"#.to_string(),
        r#"{"v":1,"ts_ms":4,"session":"stopped-1","seq":3,"event":{"type":"session_end","reason":"final_text"}}"#.to_string(),
    ];
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();

    let out = ferric()
        .args(["query", "--mock", "--resume"])
        .arg(&path)
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("final_text"));
}

/// Test-critic C-009: resuming with an `Animus.md` present prints a note
/// that it's ignored (the resumed run's system message is frozen).
#[test]
fn resume_with_animus_md_prints_ignored_note() {
    let ws = tempfile::tempdir().unwrap();
    let fixture = write_interrupted_trace_fixture(ws.path(), "orig-4");
    std::fs::write(ws.path().join("Animus.md"), "project rules").unwrap();

    let out = ferric()
        .args(["query", "--mock", "--resume"])
        .arg(&fixture)
        .arg("--workspace")
        .arg(ws.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("ignores --prompts-dir/Animus.md"));
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
///
/// Test-critique C-007: reads go through a background thread + bounded
/// `recv_timeout` (not a raw blocking `read_line`) so a server that stops
/// responding fails this test instead of hanging CI; stderr is drained on its
/// own thread so a full OS pipe buffer can never deadlock the child.
#[test]
fn mcp_stdio_e2e() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

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
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    // Drain stderr on its own thread for the process's lifetime — an
    // unread pipe can fill its OS buffer and deadlock the child.
    std::thread::spawn(move || {
        let mut sink = String::new();
        let _ = stderr.read_to_string(&mut sink);
    });

    // Stream stdout lines to the test thread over a channel so reads are
    // timeout-bounded instead of an unbounded blocking `read_line`.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break, // EOF or read error: stop forwarding.
                Ok(_) => {
                    if tx.send(line.clone()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let recv_line = |rx: &mpsc::Receiver<String>| -> String {
        rx.recv_timeout(Duration::from_secs(10))
            .expect("ferric mcp did not respond within 10s")
    };

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
    )
    .unwrap();
    let line = recv_line(&rx);
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

    // Test-critique C-006: a malformed line mid-session must yield a
    // `-32700` parse-error frame WITHOUT disrupting the requests around it —
    // the property the subprocess E2E exists to prove (a unit test already
    // covers `parse_line` in isolation; this proves it through the real
    // stdin→stdout pipe, where "keeps serving after" actually lives).
    writeln!(stdin, "not valid json at all").unwrap();
    let line = recv_line(&rx);
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        resp["error"]["code"], -32700,
        "malformed-line response: {line}"
    );

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#
    )
    .unwrap();
    let line = recv_line(&rx);
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["tools"][0]["name"], "ferric_query");

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"ferric_query","arguments":{{"prompt":"do a mock task"}}}}}}"#
    )
    .unwrap();
    let line = recv_line(&rx);
    let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["result"]["content"][0]["text"], "mock run complete");
    assert_eq!(resp["result"]["isError"], false);

    drop(stdin); // EOF: the server should exit cleanly on its own.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "ferric mcp did not exit within 10s of stdin EOF"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "ferric mcp did not exit cleanly on EOF");
}

// ---- T-4203 (sprint 42): `ferric chat` subprocess tests -----------------------
// A NEW stdin-piping harness: batch-write the whole conversation, close stdin,
// and `wait_with_output`. `ferric chat` reads lines sequentially and exits on
// `/exit` (handled purely at the parse layer — no `complete()` call) or EOF, and
// the `--mock` provider is fresh-per-turn, so an off-by-one in piped lines can't
// hang the child (plan-critic C-001/C-005).

/// Run `ferric chat --mock` with `input` piped to stdin; return (stdout, stderr).
fn run_chat_mock(ws: &std::path::Path, input: &str) -> (String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = ferric()
        .args(["chat", "--mock", "--workspace"])
        .arg(ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    } // stdin dropped → EOF
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "ferric chat exited non-zero; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Trace files under the workspace matching a prefix (`chat-` for the session
/// log — but NOT `chat-esc-`; `chat-esc-` for escalations).
fn trace_files(ws: &std::path::Path, kind: &str) -> Vec<std::path::PathBuf> {
    let dir = ws.join(".ferric").join("trace");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    match kind {
                        "session" => name.starts_with("chat-") && !name.starts_with("chat-esc-"),
                        "escalation" => name.starts_with("chat-esc-"),
                        _ => false,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn trace_event_types(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            v["event"]["type"]
                .as_str()
                .map(std::string::ToString::to_string)
        })
        .collect()
}

#[test]
fn chat_talk_then_exit() {
    let ws = tempfile::tempdir().unwrap();
    let (stdout, _stderr) = run_chat_mock(ws.path(), "hello there\n/exit\n");
    assert!(
        stdout.contains("[mock chat]"),
        "the talk response should be printed; stdout: {stdout}"
    );
    // Exactly one chat-session log file, with a coherent chat envelope + a talk
    // Note. No agentic (escalation) file for a talk-only session.
    let logs = trace_files(ws.path(), "session");
    assert_eq!(logs.len(), 1, "exactly one chat-session trace file");
    let types = trace_event_types(&logs[0]);
    assert_eq!(types.first().map(String::as_str), Some("session_start"));
    assert_eq!(types.last().map(String::as_str), Some("session_end"));
    assert!(types.iter().any(|t| t == "note"), "a talk-turn Note");
    assert!(
        trace_files(ws.path(), "escalation").is_empty(),
        "a talk-only session writes no escalation trace"
    );
}

#[test]
fn chat_do_escalates_to_agentic_loop() {
    let ws = tempfile::tempdir().unwrap();
    let (_stdout, _stderr) = run_chat_mock(ws.path(), "/do write a file\n/exit\n");
    // The escalation opened its OWN agentic trace file with the full constrained
    // loop — proving `/do` drove `run()`, not the talk path.
    let escs = trace_files(ws.path(), "escalation");
    assert_eq!(escs.len(), 1, "one escalation trace file");
    let types = trace_event_types(&escs[0]);
    assert!(
        types.iter().any(|t| t == "tool_call"),
        "the escalated agentic loop dispatched a tool: {types:?}"
    );
    assert!(types.iter().any(|t| t == "session_end"));
    // The mock's write actually landed in the workspace (guarded dispatch).
    assert!(
        ws.path().join("ferric-mock.txt").exists(),
        "the escalated loop's write reached the workspace"
    );
}

#[test]
fn chat_help_lists_commands() {
    let ws = tempfile::tempdir().unwrap();
    let (stdout, _stderr) = run_chat_mock(ws.path(), "/help\n/exit\n");
    assert!(stdout.contains("/do"), "help names /do: {stdout}");
    assert!(stdout.contains("/help"), "help names /help");
    assert!(stdout.contains("/exit"), "help names /exit");
}

/// Structural safety (black-box): a plain talk line whose text LOOKS like a tool
/// call is never dispatched — no escalation file, and no dispatch events anywhere.
#[test]
fn chat_talk_turn_is_not_dispatched() {
    let ws = tempfile::tempdir().unwrap();
    let (stdout, _stderr) = run_chat_mock(ws.path(), "write_file to /etc/passwd now\n/exit\n");
    assert!(
        stdout.contains("[mock chat]"),
        "it was talked, not executed"
    );
    // No escalation trace at all.
    assert!(
        trace_files(ws.path(), "escalation").is_empty(),
        "a talk turn must never open an agentic trace"
    );
    // The chat-session log carries no dispatch events.
    let logs = trace_files(ws.path(), "session");
    assert_eq!(logs.len(), 1);
    let types = trace_event_types(&logs[0]);
    for forbidden in ["tool_call", "tool_result", "permission_check"] {
        assert!(
            !types.iter().any(|t| t == forbidden),
            "talk mode must never {forbidden}: {types:?}"
        );
    }
    // And the workspace was not touched.
    assert!(
        !ws.path().join("passwd").exists(),
        "talk mode must not write to the workspace"
    );
}

// ---- T-4304 (sprint 43): `ferric launch` subprocess tests ---------------------
// A new stdin-piping helper modeled on `run_chat_mock`'s SHAPE (not the literal
// fn — it hardcodes `chat --mock`; plan-critic C-005). Returns (stdout, status).
fn run_launch(args: &[&std::ffi::OsStr], stdin_input: &str) -> (String, std::process::ExitStatus) {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = ferric();
    cmd.arg("launch");
    cmd.args(args);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(stdin_input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (String::from_utf8_lossy(&out.stdout).to_string(), out.status)
}

#[test]
fn launch_noninteractive_scaffolds() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("demo");
    let (stdout, status) = run_launch(
        &[
            "--name".as_ref(),
            "demo".as_ref(),
            "--path".as_ref(),
            target.as_os_str(),
            "--goal".as_ref(),
            "a tiny CLI".as_ref(),
        ],
        "", // no stdin needed — all fields supplied
    );
    assert!(status.success(), "launch should succeed; stdout: {stdout}");
    assert!(stdout.contains("Scaffolded"), "report on stdout: {stdout}");
    assert!(
        stdout.contains("enter the scaffolded directory and start a Ferric query with your task"),
        "next-step guidance should be descriptive and path-safe: {stdout}"
    );
    assert!(
        !stdout.contains("run ferric query"),
        "next-step guidance must not print a nonexistent command: {stdout}"
    );
    // A real repo with the skeleton.
    assert!(target.join(".git").is_dir());
    assert!(target.join("README.md").exists());
    assert!(target.join("agent-tasks").join("agent-tasks.md").exists());
    // main + dev exist.
    let branches = std::process::Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(&target)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&branches.stdout);
    assert!(
        branches.contains("main") && branches.contains("dev"),
        "branches: {branches}"
    );
}

#[test]
fn launch_interactive_scaffolds_from_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("ip");
    // --path is supplied; name + goal are piped (in that fixed order).
    let (stdout, status) = run_launch(
        &["--path".as_ref(), target.as_os_str()],
        "interproj\na piped goal\n",
    );
    assert!(
        status.success(),
        "interactive launch should succeed; stdout: {stdout}"
    );
    assert!(
        stdout.contains("interproj"),
        "report names the project: {stdout}"
    );
    assert!(target.join(".git").is_dir());
    let readme = std::fs::read_to_string(target.join("README.md")).unwrap();
    assert!(readme.contains("interproj") && readme.contains("a piped goal"));
}

#[test]
fn launch_refuses_to_clobber_nonempty() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("occupied");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "mine").unwrap();
    let (_stdout, status) = run_launch(
        &[
            "--name".as_ref(),
            "x".as_ref(),
            "--path".as_ref(),
            target.as_os_str(),
            "--goal".as_ref(),
            "g".as_ref(),
        ],
        "",
    );
    assert!(!status.success(), "must refuse to clobber a non-empty dir");
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "mine"
    );
    assert!(!target.join(".git").exists());
}

// ---- T-5501 (sprint 55): `ferric mcp --resume` subprocess tests ----------------

fn run_mcp_mock(
    ws: &std::path::Path,
    extra_args: &[&str],
    input: &str,
) -> (String, String, std::process::ExitStatus) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = ferric()
        .args(["mcp", "--mock", "--workspace"])
        .arg(ws)
        .args(extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status,
    )
}

#[test]
fn mcp_resume_continues_an_interrupted_session() {
    let ws = tempfile::tempdir().unwrap();
    let path = write_interrupted_trace_fixture(ws.path(), "res-1");

    // The MCP server expects a JSON-RPC request for `ferric_query`
    let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ferric_query","arguments":{"prompt":"finish it"}}}"#;
    let (stdout, stderr, status) =
        run_mcp_mock(ws.path(), &["--resume", path.to_str().unwrap()], input);

    assert!(status.success(), "mcp should succeed; stderr: {stderr}");
    assert!(
        stdout.contains("mock run complete"),
        "stdout should contain the mock's output, got: {stdout}"
    );

    // There should be a new trace file for this MCP session.
    // The new trace file should contain the original 2 turns PLUS the new turn.
    let trace_dir = ws.path().join(".ferric").join("trace");
    let new_traces: Vec<_> = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("mcp-"))
        .collect();
    assert_eq!(new_traces.len(), 1, "exactly one mcp- trace file expected");
    let content = std::fs::read_to_string(new_traces[0].path()).unwrap();

    let is_resumed = content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .any(|v| v["event"]["type"] == "session_start" && v["event"]["resumed_from"] == "res-1");
    assert!(is_resumed, "trace must show it was resumed from res-1");
}

#[test]
fn mcp_resume_rejects_already_stopped() {
    let ws = tempfile::tempdir().unwrap();
    let trace_dir = ws.path().join(".ferric").join("trace");
    std::fs::create_dir_all(&trace_dir).unwrap();
    let path = trace_dir.join("stopped.jsonl");
    let lines = [
        r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"session_start","workspace":"/ws"}}"#,
        r#"{"v":1,"ts_ms":2,"session":"s","seq":1,"event":{"type":"policy_selected","tier":"nano","protocol":"native_tools","max_turns":15,"max_tools":10,"prompt_budget_tokens":2800,"max_output_tokens":512}}"#,
        r#"{"v":1,"ts_ms":3,"session":"s","seq":2,"event":{"type":"session_prompt","system":"s","user":"u"}}"#,
        r#"{"v":1,"ts_ms":4,"session":"s","seq":3,"event":{"type":"session_end","reason":"done"}}"#,
    ];
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();

    let (_stdout, stderr, status) =
        run_mcp_mock(ws.path(), &["--resume", path.to_str().unwrap()], "");
    assert!(!status.success());
    assert!(stderr.contains("cannot resume"), "stderr: {stderr}");
    assert!(stderr.contains("already ended (done)"), "stderr: {stderr}");
}

// ── ICM agent delegation (sprint 73, ADR-064) ──────────────────────────────

#[test]
fn icm_init_scaffolds_and_plan_shows_the_pipeline() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");

    // init scaffolds a three-stage workspace.
    let out = ferric().args(["icm", "init"]).arg(&ws).output().unwrap();
    assert!(out.status.success(), "icm init must succeed");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Scaffolded ICM workspace"));
    assert!(ws.join("stages/01_research/CONTEXT.md").exists());
    assert!(ws.join("Animus.md").exists());

    // plan discovers the stages in numeric order and reports layers.
    let out = ferric().args(["icm", "plan"]).arg(&ws).output().unwrap();
    assert!(out.status.success(), "icm plan must succeed");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("01_research"));
    assert!(stdout.contains("02_script"));
    assert!(stdout.contains("03_production"));
    // Layer 0/1/2 are always present for a scaffolded workspace.
    assert!(stdout.contains("Layer 0 (identity)"));
    assert!(stdout.contains("Layer 2 (contract)"));
}

#[test]
fn icm_plan_wires_prior_stage_output_as_layer4() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");
    ferric().args(["icm", "init"]).arg(&ws).output().unwrap();

    // Simulate stage 1 having produced output and a shared voice file.
    std::fs::write(
        ws.join("stages/01_research/output/research.md"),
        "finding: constraint beats native.",
    )
    .unwrap();
    std::fs::write(ws.join("_config/voice.md"), "terse.").unwrap();

    let out = ferric().args(["icm", "plan"]).arg(&ws).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Stage 2 pulls stage 1's output as a present Layer 4 input.
    assert!(
        stdout.contains("01_research/output/research.md"),
        "plan must wire the upstream output as Layer 4; got:\n{stdout}"
    );
    assert!(stdout.contains("_config/voice.md"));
}

#[test]
fn icm_init_refuses_to_clobber() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("keep.txt"), "mine").unwrap();

    let out = ferric().args(["icm", "init"]).arg(&ws).output().unwrap();
    assert!(!out.status.success(), "init must refuse a non-empty dir");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("refuse to clobber"), "stderr: {stderr}");
    // The pre-existing file is untouched.
    assert_eq!(
        std::fs::read_to_string(ws.join("keep.txt")).unwrap(),
        "mine"
    );
}

// ── ICM live execution (sprint 74, ADR-065) ────────────────────────────────

#[test]
fn icm_run_executes_every_stage_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");
    ferric().args(["icm", "init"]).arg(&ws).output().unwrap();

    let out = ferric()
        .args(["icm", "run", "--auto", "--mock"])
        .arg(&ws)
        .output()
        .unwrap();
    assert!(out.status.success(), "pipeline must succeed");
    let stderr = String::from_utf8(out.stderr).unwrap();
    // All three stages ran and completed.
    assert!(stderr.contains("01_research"), "stderr: {stderr}");
    assert!(stderr.contains("02_script"));
    assert!(stderr.contains("03_production"));
    assert_eq!(stderr.matches("✔ Stage").count(), 3, "stderr: {stderr}");

    // One trace per stage landed at the ICM root.
    let traces: Vec<_> = std::fs::read_dir(ws.join(".ferric/trace"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(traces.len(), 3, "one trace per stage");

    // Containment: each stage's mock artifact stayed inside its OWN folder.
    for stage in ["01_research", "02_script", "03_production"] {
        assert!(
            ws.join("stages")
                .join(stage)
                .join("ferric-mock.txt")
                .exists(),
            "stage {stage} ran contained to its own workspace"
        );
    }
}

#[test]
fn icm_run_stops_at_review_gate_on_q() {
    use std::io::Write;
    use std::process::Stdio;

    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");
    ferric().args(["icm", "init"]).arg(&ws).output().unwrap();

    // No --auto: a review gate follows stage 1. Feeding 'q' stops the pipeline.
    let mut child = ferric()
        .args(["icm", "run", "--mock"])
        .arg(&ws)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"q\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("stopped at stage 01"), "stderr: {stderr}");
    // Stage 2 never ran.
    assert!(
        !ws.join("stages/02_script/ferric-mock.txt").exists(),
        "stage 2 must not run after the user stops at the gate"
    );
}

#[test]
fn icm_run_honors_a_stage_range() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("deck");
    ferric().args(["icm", "init"]).arg(&ws).output().unwrap();

    let out = ferric()
        .args(["icm", "run", "--auto", "--mock", "--from", "2", "--to", "2"])
        .arg(&ws)
        .output()
        .unwrap();
    assert!(out.status.success());
    // Only stage 2 ran.
    assert!(ws.join("stages/02_script/ferric-mock.txt").exists());
    assert!(!ws.join("stages/01_research/ferric-mock.txt").exists());
    assert!(!ws.join("stages/03_production/ferric-mock.txt").exists());
}

// ── Agentic cron (sprint 75, ADR-066) ──────────────────────────────────────

#[test]
fn cron_add_then_list_shows_the_job() {
    let ws = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args(["add", "nightly", "--schedule", "12h", "--command", "dream"])
        .output()
        .unwrap();
    assert!(out.status.success(), "cron add must succeed");
    assert!(ws.path().join(".ferric/cron/nightly.toml").exists());

    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .arg("list")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("nightly"), "list: {stdout}");
    assert!(stdout.contains("12h"));
    assert!(
        stdout.contains("never"),
        "an un-run job's last-run is 'never'"
    );
}

#[test]
fn cron_run_executes_a_due_mock_job_and_advances_state() {
    let ws = tempfile::tempdir().unwrap();
    // A mock query job runs offline (fast, no server) — proves execution.
    ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args([
            "add",
            "summary",
            "--schedule",
            "1h",
            "--command",
            "query",
            "--prompt",
            "do a task",
            "--mock",
        ])
        .output()
        .unwrap();

    // First run: the job is due (never run) → it executes.
    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .arg("run")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        ws.path().join("ferric-mock.txt").exists(),
        "the mock query job must actually run"
    );

    // Second run immediately after: state advanced → nothing is due.
    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .arg("run")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("No jobs due"),
        "state must advance; got: {stdout}"
    );
}

#[test]
fn cron_dry_run_reports_without_executing() {
    let ws = tempfile::tempdir().unwrap();
    ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args([
            "add",
            "summary",
            "--schedule",
            "1h",
            "--command",
            "query",
            "--prompt",
            "x",
            "--mock",
        ])
        .output()
        .unwrap();

    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args(["run", "--dry-run"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("DUE (dry-run): summary"),
        "dry-run: {stdout}"
    );
    // Nothing executed, state untouched → a subsequent dry-run still reports it.
    assert!(!ws.path().join("ferric-mock.txt").exists());
    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args(["run", "--dry-run"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("DUE (dry-run): summary")
    );
}

#[test]
fn cron_add_rejects_bad_input() {
    let ws = tempfile::tempdir().unwrap();
    let base = || {
        let mut c = ferric();
        c.args(["cron", "--workspace"]).arg(ws.path());
        c
    };
    // query with no prompt
    assert!(
        !base()
            .args(["add", "q", "--schedule", "1h", "--command", "query"])
            .output()
            .unwrap()
            .status
            .success()
    );
    // unknown command (not an arbitrary shell string)
    assert!(
        !base()
            .args(["add", "q", "--schedule", "1h", "--command", "rm -rf /"])
            .output()
            .unwrap()
            .status
            .success()
    );
    // bad schedule
    assert!(
        !base()
            .args(["add", "q", "--schedule", "soon", "--command", "dream"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn cron_add_accepts_a_cron_expression() {
    let ws = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .args([
            "add",
            "nightly",
            "--schedule",
            "0 2 * * *",
            "--command",
            "dream",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cron add must accept a cron expression"
    );
    assert!(
        std::fs::read_to_string(ws.path().join(".ferric/cron/nightly.toml"))
            .unwrap()
            .contains("0 2 * * *")
    );

    let out = ferric()
        .args(["cron", "--workspace"])
        .arg(ws.path())
        .arg("list")
        .output()
        .unwrap();
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("cron(0 2 * * *)")
    );
}

// ── Direct terminal passthrough (sprint 78, ADR-069) ───────────────────────

#[test]
fn chat_bang_runs_a_command_directly() {
    let ws = tempfile::tempdir().unwrap();
    // `!echo` executes with no LLM roundtrip; its output prints to stdout.
    let (stdout, _stderr) = run_chat_mock(ws.path(), "!echo passthrough-marker\n/exit\n");
    assert!(
        stdout.contains("passthrough-marker"),
        "the command output should print; stdout: {stdout}"
    );
    // No LLM talk response for a `!` line (it's a side-channel, not conversation).
    assert!(
        !stdout.contains("[mock chat]"),
        "a `!` passthrough must not trigger a talk completion; stdout: {stdout}"
    );
}

#[test]
fn chat_passthrough_still_enforces_the_command_denylist() {
    let ws = tempfile::tempdir().unwrap();
    let (_stdout, stderr) = run_chat_mock(ws.path(), "!rm -rf /\n/exit\n");
    assert!(
        stderr.contains("blocked"),
        "a denylisted command must be refused even in direct passthrough; stderr: {stderr}"
    );
}
