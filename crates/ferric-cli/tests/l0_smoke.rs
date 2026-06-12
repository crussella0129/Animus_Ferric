//! L0 smoke E2E — the ADR-009 real-GGUF gate, first runnable in s1.
//!
//! One real model run must produce a correct file edit and a valid trace.
//! Deliberately narrow (critique C-004 disposition): a max_turns or
//! repetition_guard ending is a FAILING run — L0 is the capability gate, and
//! a false green here is exactly what the lineage's three missed bugs were.
//!
//! Run locally (CI is model-free). `--release` is MANDATORY: candle's CPU
//! matmul in a debug build runs at ~1 tok/s (a single turn took >37 min in
//! the first s1 attempt); release runs at usable speed.
//!   FERRIC_SMOKE_MODEL_DIR=C:\Users\charl\.animus\models \
//!   FERRIC_SMOKE_MODEL_FILE=Llama-3.2-1B-Instruct-Q4_K_M.gguf \
//!   cargo test -p ferric-cli --release --features backend-mistralrs --test l0_smoke -- --ignored --nocapture
#![cfg(feature = "backend-mistralrs")]

use std::process::Command;
use std::time::Instant;

#[test]
#[ignore = "requires a local GGUF; run with FERRIC_SMOKE_MODEL_DIR/FILE set"]
fn l0_smoke() {
    let model_dir = std::env::var("FERRIC_SMOKE_MODEL_DIR").expect(
        "FERRIC_SMOKE_MODEL_DIR must point at the GGUF directory (e.g. ~/.animus/models)",
    );
    let model_file = std::env::var("FERRIC_SMOKE_MODEL_FILE")
        .expect("FERRIC_SMOKE_MODEL_FILE must name the GGUF (e.g. Llama-3.2-1B-Instruct-Q4_K_M.gguf)");

    let ws = tempfile::tempdir().unwrap();
    let prompt = "Use write_file to create hello.txt containing exactly: hello ferric. \
                  Then call task_complete.";

    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ferric"))
        .args(["query", prompt])
        .arg("--workspace")
        .arg(ws.path())
        .args(["--model-dir", &model_dir])
        .args(["--model-file", &model_file])
        .args(["--params-b", "1.2", "--ctx", "4096", "--temperature", "0"])
        .args(["--family", "llama-3.2"])
        .output()
        .unwrap();
    let wall = started.elapsed();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("--- L0 smoke: wall={wall:?}\nstdout: {stdout}\nstderr: {stderr}");

    // 1. exits 0
    assert!(out.status.success(), "process must exit 0; stderr: {stderr}");

    // 2. correct file edit (trailing-newline tolerant)
    let hello = std::fs::read_to_string(ws.path().join("hello.txt"))
        .expect("hello.txt must exist in the workspace");
    assert_eq!(hello.trim_end(), "hello ferric");

    // 3. exactly one parseable trace, v==1, seq monotonic from 0
    let trace_dir = ws.path().join(".ferric").join("trace");
    let traces: Vec<_> = std::fs::read_dir(&trace_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("q-"))
        .collect();
    assert_eq!(traces.len(), 1, "exactly one q-*.jsonl trace");
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(traces[0].path())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("every trace line parses"))
        .collect();
    assert!(!lines.is_empty());
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(line["v"], 1, "schema version");
        assert_eq!(line["seq"], i as u64, "seq strictly monotonic from 0");
    }

    // 4. session frame + clean terminator (narrow by design)
    assert_eq!(lines.first().unwrap()["event"]["type"], "session_start");
    let last = lines.last().unwrap();
    assert_eq!(last["event"]["type"], "session_end");
    let reason = last["event"]["reason"].as_str().unwrap();
    assert!(
        reason == "task_complete" || reason == "final_text",
        "clean termination required, got: {reason}"
    );

    // 5. turn pairs with real token counts
    let turn_ends: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|l| l["event"]["type"] == "turn_end")
        .collect();
    assert!(!turn_ends.is_empty(), "at least one turn");
    assert!(
        turn_ends
            .iter()
            .any(|t| t["event"]["output_tokens"].as_u64().unwrap_or(0) > 0),
        "usage plumbing must report output tokens"
    );

    // 6. the write_file call, its successful result, and its allow check
    let wrote = lines.iter().any(|l| {
        l["event"]["type"] == "tool_call"
            && l["event"]["name"] == "write_file"
            && l["event"]["args"]["path"]
                .as_str()
                .is_some_and(|p| p.contains("hello.txt"))
    });
    assert!(wrote, "write_file(hello.txt) must be traced");
    assert!(
        lines.iter().any(|l| l["event"]["type"] == "tool_result"
            && l["event"]["name"] == "write_file"
            && l["event"]["is_error"] == false),
        "successful write_file result traced"
    );
    assert!(
        lines.iter().any(|l| l["event"]["type"] == "permission_check"
            && l["event"]["decision"] == "allow"),
        "allow permission_check traced"
    );

    // 7. offered tools include write_file and task_complete
    assert!(
        lines.iter().any(|l| {
            l["event"]["type"] == "prompt_assembled"
                && l["event"]["offered_tools"]
                    .as_array()
                    .is_some_and(|tools| {
                        let names: Vec<&str> =
                            tools.iter().filter_map(|t| t.as_str()).collect();
                        names.contains(&"write_file") && names.contains(&"task_complete")
                    })
        }),
        "prompt_assembled must list write_file and task_complete"
    );

    // 8. within the NANO budget; report actuals for the test report
    let turns = turn_ends.len();
    assert!(turns <= 15, "NANO max_turns budget");
    let total_output: u64 = turn_ends
        .iter()
        .map(|t| t["event"]["output_tokens"].as_u64().unwrap_or(0))
        .sum();
    println!(
        "--- L0 smoke PASS: {turns} turn(s), {total_output} output tokens, wall {wall:?}, reason {reason}"
    );
}
