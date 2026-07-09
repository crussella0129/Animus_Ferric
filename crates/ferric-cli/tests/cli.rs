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

/// T-3705: `--stream` must not duplicate output. The `--mock` script's
/// completions are native tool calls with no `message.text` (`text: None`),
/// so the default `complete_streaming` impl fires zero deltas for either
/// turn — the final echo is the ONLY place the text appears, exactly as
/// without `--stream` (the EARS "no duplication, no missing text" clause).
#[test]
fn stream_flag_mock_no_duplication() {
    let dir = tempfile::tempdir().unwrap();
    let out = ferric()
        .args(["query", "--mock", "--stream", "do a mock task"])
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
        !offered.contains(&"search_files".to_string())
            && offered.contains(&"write_file".to_string()),
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
        !offered.contains(&"search_files".to_string())
            && offered.contains(&"write_file".to_string()),
        "a config-only `max_ring = 0` must cap to the core, same as `--max-ring 0`: {offered:?}"
    );
}

/// Test-critic C-002: same gap for `stream`. `--mock`'s NativeTools script has
/// `text: None` (no observable streaming difference — see
/// `stream_flag_mock_no_duplication`'s doc comment), so this uses
/// `--protocol grammar`, where the mock's completion text IS the raw
/// `{"tool":...}` JSON. With streaming active, the default `complete_
/// streaming` fires ONE `Text` delta of that raw JSON (printed live), and the
/// final echo is skipped (already-streamed) — so the raw JSON appears on
/// stdout instead of the clean `"mock run complete"` line. That only happens
/// if `resolved_stream` (config-only, no `--stream` flag) was actually true.
#[test]
fn config_only_stream_enables_live_output() {
    let ws = tempfile::tempdir().unwrap();
    write_project_config(ws.path(), "stream = true\n");

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
    assert!(
        stdout.contains("\"tool\":\"task_complete\""),
        "expected the raw streamed JSON (proving config-only `stream` took \
         effect), got: {stdout:?}"
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
            r#"{{"v":1,"ts_ms":2,"session":"{session}","seq":1,"event":{{"type":"policy_selected","tier":"nano","protocol":"native_tools","max_turns":15,"max_tools":6,"prompt_budget_tokens":2800,"max_output_tokens":512}}}}"#
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
        r#"{"v":1,"ts_ms":2,"session":"stopped-1","seq":1,"event":{"type":"policy_selected","tier":"nano","protocol":"native_tools","max_turns":15,"max_tools":6,"prompt_budget_tokens":2800,"max_output_tokens":512}}"#.to_string(),
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
