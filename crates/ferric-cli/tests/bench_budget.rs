//! T-12102/03: compose the benchmark CLI, request/evidence budgets, and the
//! diagnostic publication boundary. Scripted outcomes are not model claims.

use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[path = "../src/test_process_containment.rs"]
mod test_process_containment;

fn bench(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
    command
        .current_dir(root)
        .args(["bench", "full", "--protocol", "grammar"])
        .arg("--results-dir")
        .arg(root.join("results"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("FERRIC_PROMPTS_DIR")
        .env_remove("FERRIC_LOG")
        .env_remove("RUST_LOG");
    command
}

fn output(command: &mut Command) -> Output {
    test_process_containment::output_bounded(command, Duration::from_secs(45))
        .expect("benchmark finished within its fixture budget and every child was reaped")
}

fn read_rows(root: &Path, output: &Output) -> Vec<Value> {
    let text =
        std::fs::read_to_string(root.join("results/results.jsonl")).unwrap_or_else(|error| {
            panic!(
                "missing result rows: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn summary(root: &Path, row: &Value) -> Value {
    serde_json::from_slice(
        &std::fs::read(
            root.join("results")
                .join(format!("summary-{}.json", row["run_id"].as_str().unwrap())),
        )
        .unwrap(),
    )
    .unwrap()
}

/// Verify independent persisted row, summary, trace and sidecar reads. The
/// source-to-retained byte equality and publication-fault matrix live in the
/// library tests; this composed route deliberately does not keep a workspace.
fn assert_retained_pair(root: &Path, row: &Value, summary: &Value) -> Vec<Value> {
    let budget = &row["budget"];
    let retained = &budget["retained"];
    let identity = json!({
        "run_id":row["run_id"], "trial_id":row["trial_id"], "level":row["level"]
    });
    assert_eq!(retained["identity"], identity);
    assert_eq!(retained["trace_path"], row["trace_path"]);
    let bytes = std::fs::read(
        root.join("results")
            .join(retained["trace_path"].as_str().unwrap()),
    )
    .unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    assert_eq!(retained["trace_sha256"], digest);
    let sidecar: Value = serde_json::from_slice(
        &std::fs::read(
            root.join("results")
                .join(retained["sidecar_path"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(sidecar["schema_version"], 1);
    assert_eq!(sidecar["identity"], identity);
    assert_eq!(sidecar["trace_path"], row["trace_path"]);
    assert_eq!(sidecar["trace_sha256"], digest);
    assert_eq!(sidecar["evidence"], *budget);
    assert_eq!(summary["budget"]["controls"], budget["controls"]);
    let attempts = summary["budget"]["attempts"].as_array().unwrap();
    assert_eq!(
        attempts.len() as u64,
        summary["observed_rows"].as_u64().unwrap()
    );
    let matching: Vec<_> = attempts
        .iter()
        .filter(|attempt| attempt["identity"] == identity)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "one exact summary reference per row identity"
    );
    assert_eq!(
        *matching[0],
        json!({"identity":identity,"retained":retained})
    );
    std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["event"].clone())
        .collect()
}

#[test]
fn bench_budget_rejects_before_effects() {
    // L3 would invoke Python preflight. The absent command must not win over
    // admission, create a result directory, or start the selected query child.
    for (flag, value) in [
        ("--timeout-scale", "NaN"),
        ("--timeout-scale", "inf"),
        ("--timeout-scale", "-inf"),
        ("--timeout-scale", "0"),
        ("--timeout-scale", "-0"),
        ("--timeout-scale", "-1"),
        ("--timeout-scale", "1e308"),
        ("--timeout-scale", "1e-320"),
        ("--max-output-tokens", "0"),
        ("--max-output-tokens", "-1"),
        ("--max-output-tokens", "4294967296"),
        ("--max-output-tokens", "1230"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let result = output(
            bench(root.path())
                .args(["--mock", "--level", "3", "--params-b", "7", "--ctx", "4096"])
                .arg("--python-bin")
                .arg(root.path().join("absent-preflight-command"))
                .arg("--ferric-bin")
                .arg(root.path().join("absent-query-child"))
                .arg(format!("{flag}={value}")),
        );
        assert!(!result.status.success(), "admitted {flag}={value}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains(flag), "budget diagnostic absent: {stderr}");
        assert!(
            !stderr.contains("benchmark check infrastructure")
                && !stderr.contains("cannot execute benchmark child"),
            "admission must precede command execution: {stderr}"
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "invalid {flag}={value} created files or directories"
        );
    }
}

#[test]
fn bench_mock_effective_context_matches_child() {
    let root = tempfile::tempdir().unwrap();
    let result = output(bench(root.path()).args([
        "--mock",
        "--level",
        "0",
        "--params-b",
        "7",
        "--ctx",
        "4096",
        "--max-output-tokens",
        "1024",
        "--timeout-scale",
        "0.625",
    ]));
    let rows = read_rows(root.path(), &result);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    // The mock writes the wrong file for L0. Recording its honest failure is
    // the operational assertion, not a fabricated benchmark capability pass.
    assert_eq!(row["completed"], false);
    assert_eq!(row["terminator"], "task_complete");
    assert_eq!(row["infrastructure_error"], Value::Null);
    assert_eq!(row["budget"]["base_timeout_s"], 60);
    assert_eq!(
        row["budget"]["enforced_duration"],
        json!({"secs":37,"nanos":500_000_000})
    );
    assert_eq!(row["budget"]["warmup"], "not_performed");
    assert_eq!(
        row["budget"]["controls"],
        json!({
            "timeout_scale":0.625,"max_output_tokens":1024,"params_b":7.0,"ctx":4096
        })
    );
    let summary = summary(root.path(), row);
    let events = assert_retained_pair(root.path(), row, &summary);
    let policy = events
        .iter()
        .find(|event| event["type"] == "policy_selected")
        .unwrap();
    assert_eq!(policy["tier"], "small");
    assert_eq!(policy["tier_source"], "params");
    let main: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "main_action_budget")
        .collect();
    assert!(!main.is_empty(), "child must actually request an action");
    for event in main {
        assert_eq!(
            event["budget"],
            json!({
                "requested":1024,"effective":1024,"declared_ctx":4096,"source":"explicit"
            })
        );
    }
    assert_eq!(summary["provenance"]["model"]["ctx"], 4096);
    assert_eq!(summary["provenance"]["model"]["params_b"], 7.0);
}

#[test]
fn bench_no_controls_preserve_legacy_mock_defaults() {
    for explicit_one in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut command = bench(root.path());
        command.args(["--mock", "--level", "0", "--params-b", "7", "--ctx", "8192"]);
        if explicit_one {
            command.args(["--timeout-scale", "1"]);
        }
        let result = output(&mut command);
        let rows = read_rows(root.path(), &result);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["terminator"], "task_complete");
        assert_eq!(row["infrastructure_error"], Value::Null);
        assert_eq!(
            row["budget"]["enforced_duration"],
            json!({"secs":60,"nanos":0})
        );
        let events = assert_retained_pair(root.path(), row, &summary(root.path(), row));
        let observed: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "main_action_budget")
            .collect();
        assert!(!observed.is_empty());
        for event in observed {
            assert_eq!(
                event["budget"],
                json!({
                    "requested":null,"effective":512,"declared_ctx":4096,"source":"policy"
                })
            );
        }
    }
}

#[cfg(feature = "backend-openai")]
mod http {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread::JoinHandle;
    use std::time::Instant;

    #[derive(Clone, Copy)]
    enum Reply {
        Complete,
        ProviderError,
        OutputLimit,
        Stall,
    }

    struct Server {
        endpoint: String,
        requests: Arc<Mutex<Vec<Value>>>,
        stop: Option<mpsc::Sender<()>>,
        worker: Option<JoinHandle<()>>,
    }

    impl Server {
        fn new(reply: Reply) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = requests.clone();
            let (stop, stopped) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(40);
                while Instant::now() < deadline {
                    if stopped.try_recv().is_ok() {
                        return;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Winsock accepts inherit the listener's mode.
                            stream.set_nonblocking(false).unwrap();
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let request = read_request(&mut stream);
                            let index = {
                                let mut requests = captured.lock().unwrap();
                                let index = requests.len();
                                requests.push(request);
                                index
                            };
                            if matches!(reply, Reply::Stall) {
                                // A finite request fixture, released promptly by
                                // its owner after the benchmark checked reaping.
                                let _ = stopped.recv_timeout(Duration::from_secs(20));
                                return;
                            }
                            let (status, body) = if matches!(reply, Reply::ProviderError) {
                                ("500 Internal Server Error", json!({"error":{"message":"scripted provider failure","type":"fixture"}}).to_string())
                            } else {
                                let action = if matches!(reply, Reply::Complete) && index == 0 {
                                    json!({"thought":"inspect","tool":"list_dir","args":{"path":"."}})
                                } else {
                                    json!({"thought":"complete","tool":"task_complete","args":{"summary":"listed the fixture files"}})
                                };
                                let finish = if matches!(reply, Reply::OutputLimit) {
                                    "length"
                                } else {
                                    "stop"
                                };
                                ("200 OK", json!({
                                    "choices":[{"index":0,"message":{"role":"assistant","content":action.to_string()},"finish_reason":finish}],
                                    "usage":{"prompt_tokens":20,"completion_tokens":10}
                                }).to_string())
                            };
                            let response = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            stream.write_all(response.as_bytes()).unwrap();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if stopped.recv_timeout(Duration::from_millis(5)).is_ok() {
                                return;
                            }
                        }
                        Err(error) => panic!("benchmark fixture accept: {error}"),
                    }
                }
            });
            Self {
                endpoint,
                requests,
                stop: Some(stop),
                worker: Some(worker),
            }
        }

        fn finish(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(worker) = self.worker.take() {
                let deadline = Instant::now() + Duration::from_secs(6);
                while !worker.is_finished() {
                    if Instant::now() >= deadline {
                        test_process_containment::abort_on_cleanup_failure(
                            "benchmark HTTP fixture",
                            "join deadline exceeded",
                        );
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                if let Err(payload) = worker.join()
                    && !std::thread::panicking()
                {
                    std::panic::resume_unwind(payload);
                }
            }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.finish();
        }
    }

    fn read_request(stream: &mut TcpStream) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            assert!(Instant::now() < deadline, "bounded HTTP framing deadline");
            assert!(bytes.len() <= 512 * 1024, "bounded HTTP request size");
            let read = stream
                .read(&mut buffer)
                .expect("HTTP fixture request bytes");
            assert!(read > 0, "request ended before body");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..split]).unwrap();
                assert!(header.starts_with("POST /v1/chat/completions "));
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .expect("content length");
                assert!(length <= 512 * 1024);
                if bytes.len() >= split + 4 + length {
                    return serde_json::from_slice(&bytes[split + 4..split + 4 + length]).unwrap();
                }
            }
        }
    }

    fn real_bench(root: &Path, server: &Server) -> Command {
        let mut command = bench(root);
        command.args([
            "--level",
            "0",
            "--params-b",
            "7",
            "--ctx",
            "4096",
            "--max-output-tokens",
            "1024",
            "--model",
            "budget-fixture",
            "--api-base",
            &server.endpoint,
        ]);
        command
    }

    fn assert_operator_evidence(output: &Output, root: &Path, row: &Value) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let sidecar = row["budget"]["retained"]["sidecar_path"].as_str().unwrap();
        let destination = root.join("results").join(sidecar);
        assert!(
            stdout.contains(&format!("evidence destination: {}", destination.display())),
            "operator must see this attempt's actual evidence destination: {stdout}"
        );
    }

    #[test]
    fn diagnostic_single_fleet_preserve_profile_bytes() {
        fn profile(model: &str) -> Value {
            json!({
                "model": model, "params_b":7.0, "protocol":"ConstrainedJson",
                "measured_level":6, "tier_from_params":"Small",
                "tier_from_measured":"Large", "calibrated_ring":1,
                "calibration_evidence":null
            })
        }
        let states = [
            ("absent", None, vec!["--max-output-tokens", "1024"]),
            (
                "valid-target",
                Some(
                    serde_json::to_vec_pretty(&json!([
                        profile("budget-fixture-a"),
                        profile("budget-fixture-b")
                    ]))
                    .unwrap(),
                ),
                vec!["--timeout-scale", "0.5"],
            ),
            (
                "unrelated-multi-model",
                Some(
                    serde_json::to_vec_pretty(&json!([
                        profile("unrelated-fixture-one"),
                        profile("unrelated-fixture-two")
                    ]))
                    .unwrap(),
                ),
                vec!["--timeout-scale", "2"],
            ),
            (
                "malformed",
                Some(b"[ deliberately incomplete profile data\r\n".to_vec()),
                vec!["--timeout-scale", "0.5", "--max-output-tokens", "1024"],
            ),
        ];
        for (state, before, controls) in states {
            for fleet in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let results_dir = root.path().join("results");
                let profile_path = results_dir.join("model_profiles.json");
                if let Some(bytes) = &before {
                    std::fs::create_dir(&results_dir).unwrap();
                    std::fs::write(&profile_path, bytes).unwrap();
                }
                let mut server = Server::new(Reply::ProviderError);
                let mut command = bench(root.path());
                command
                    .args([
                        "--params-b",
                        "7",
                        "--ctx",
                        "4096",
                        "--api-base",
                        &server.endpoint,
                    ])
                    .args(&controls)
                    .arg("--python-bin")
                    .arg(env!("CARGO_BIN_EXE_ferric"));
                if fleet {
                    command.args(["--models", "budget-fixture-a,budget-fixture-b"]);
                } else {
                    command.args(["--model", "budget-fixture-a"]);
                }
                let result = output(&mut command);
                server.finish();
                // Fleet compatibility: a complete, infrastructure-clean
                // measurement returns success even when its raw tasks fail.
                assert_eq!(
                    result.status.success(),
                    fleet,
                    "state={state} fleet={fleet}\n{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let after = match std::fs::read(&profile_path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => panic!("profile readback: {error}"),
                };
                assert_eq!(
                    after, before,
                    "profile bytes changed: state={state} fleet={fleet}"
                );
                let rows = read_rows(root.path(), &result);
                let count = if fleet { 14 } else { 7 };
                assert_eq!(rows.len(), count);
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), count);
                for model in if fleet {
                    vec!["budget-fixture-a", "budget-fixture-b"]
                } else {
                    vec!["budget-fixture-a"]
                } {
                    assert_eq!(
                        requests
                            .iter()
                            .filter(|request| request["model"] == model)
                            .count(),
                        7
                    );
                }
                for row in &rows {
                    assert_eq!(
                        row["completed"], false,
                        "scripted provider failure is not a successful ladder"
                    );
                    assert_eq!(row["terminator"], "provider_error");
                    assert_eq!(row["timed_out"], false);
                    assert_eq!(row["infrastructure_error"], Value::Null);
                    let summary = summary(root.path(), row);
                    assert_eq!(summary["complete"], true);
                    assert_eq!(summary["infrastructure_clean"], true);
                    assert_eq!(summary["observed_rows"], 7);
                    assert_eq!(summary["calibration"]["full_ladder"], true);
                    assert_eq!(summary["calibration"]["diagnostic"], true);
                    assert_eq!(summary["calibration"]["eligible"], false);
                    assert_eq!(summary["calibration"]["measured_level"], Value::Null);
                    assert!(
                        summary["calibration"]["ineligible_reason"]
                            .as_str()
                            .unwrap()
                            .contains("diagnostic benchmark budgets")
                    );
                    assert!(
                        summary["levels"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|level| level["failures"] == 1)
                    );
                    assert_retained_pair(root.path(), row, &summary);
                    assert_operator_evidence(&result, root.path(), row);
                }
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);
                assert!(stdout.contains("diagnostic budgets — profile left unchanged"));
                assert!(stdout.contains("observations only"));
                assert!(stdout.contains("PROVIDER ERROR (provider_error)"));
                assert!(stdout.contains("DIAGNOSTIC — PROVIDER ERROR (provider_error)"));
                assert!(!stdout.contains("Agentic Capability Leaderboard"));
                assert!(!stdout.contains("calibrated budget-fixture"));
                assert!(!stdout.contains("INFRASTRUCTURE FAILURE"));
                assert!(!stderr.contains("cannot write model profile"));
                assert!(!stderr.contains("calibration evidence is not eligible"));
            }
        }
    }

    #[test]
    fn diagnostic_budget_operator_output() {
        let mut server = Server::new(Reply::Complete);
        let root = tempfile::tempdir().unwrap();
        let result = output(&mut real_bench(root.path(), &server));
        server.finish();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let rows = read_rows(root.path(), &result);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["completed"], true);
        assert_operator_evidence(&result, root.path(), row);
        let summary = summary(root.path(), row);
        assert_eq!(summary["calibration"]["diagnostic"], true);
        assert_eq!(summary["calibration"]["eligible"], false);
        assert_eq!(summary["calibration"]["measured_level"], Value::Null);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("diagnostic budgets — profile left unchanged"));
        assert!(stdout.contains("observations only"));
        assert!(stdout.contains("DIAGNOSTIC — PASS"));
        assert!(!stdout.contains("calibrated budget-fixture"));
        assert!(!root.path().join("results/model_profiles.json").exists());
    }

    #[test]
    fn bench_budget_trace_sidecar_roundtrip() {
        let mut server = Server::new(Reply::Complete);
        let root = tempfile::tempdir().unwrap();
        let result = output(real_bench(root.path(), &server).args(["--timeout-scale", "0.625"]));
        server.finish();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let rows = read_rows(root.path(), &result);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["completed"], true);
        assert_eq!(row["infrastructure_error"], Value::Null);
        assert_eq!(
            row["budget"]["parent_termination"],
            json!({"kind":"exited","exit_code":0})
        );
        assert_eq!(row["budget"]["trace"]["child_terminal"], "task_complete");
        let summary = summary(root.path(), row);
        let events = assert_retained_pair(root.path(), row, &summary);
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request["max_tokens"], 1024);
            assert_eq!(request["temperature"], 0.0);
            assert_eq!(request["model"], "budget-fixture");
            assert!(!request["stream"].as_bool().unwrap_or(false));
        }
        let observed: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "main_action_budget")
            .map(|event| json!({"turn":event["turn"],"budget":event["budget"]}))
            .collect();
        assert_eq!(observed.len(), requests.len());
        assert_eq!(
            row["budget"]["trace"]["main_action_budgets"],
            json!(observed)
        );
        assert_eq!(summary["provenance"]["model"]["api_base"], server.endpoint);
        assert!(
            !root.path().join("results/model_profiles.json").exists(),
            "a partial L0 sweep must not create a profile"
        );
    }

    #[test]
    fn benchmark_termination_causes_remain_distinct() {
        for (reply, terminal, expected_requests) in [
            (Reply::ProviderError, "provider_error", 1),
            (Reply::OutputLimit, "truncated_action", 2),
        ] {
            let mut server = Server::new(reply);
            let root = tempfile::tempdir().unwrap();
            let result = output(&mut real_bench(root.path(), &server));
            server.finish();
            let rows = read_rows(root.path(), &result);
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            assert!(!result.status.success());
            assert_eq!(row["completed"], false);
            assert_eq!(row["timed_out"], false);
            assert_eq!(row["budget"]["parent_termination"]["kind"], "exited");
            assert_eq!(row["budget"]["trace"]["child_terminal"], terminal);
            assert_eq!(row["terminator"], terminal);
            assert_eq!(row["infrastructure_error"], Value::Null);
            assert_eq!(server.requests.lock().unwrap().len(), expected_requests);
            assert_retained_pair(root.path(), row, &summary(root.path(), row));
            assert_operator_evidence(&result, root.path(), row);
            let expected_label = match reply {
                Reply::ProviderError => "PROVIDER ERROR (provider_error)",
                Reply::OutputLimit => "OUTPUT LIMIT (truncated_action)",
                _ => unreachable!(),
            };
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                stdout.contains(expected_label),
                "observed cause must remain visible: {stdout}"
            );
            assert!(!stdout.contains("PARENT TIMEOUT"));
        }
    }

    #[test]
    fn bench_parent_timeout_retains_partial_request_evidence() {
        let mut server = Server::new(Reply::Stall);
        let root = tempfile::tempdir().unwrap();
        let result = output(real_bench(root.path(), &server).args(["--timeout-scale", "0.1"]));
        server.finish();
        let rows = read_rows(root.path(), &result);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(!result.status.success());
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "must reach the actual request phase before parent timeout"
        );
        assert_eq!(row["timed_out"], true);
        assert_eq!(row["exit_code"], Value::Null);
        assert_eq!(row["terminator"], Value::Null);
        assert_eq!(
            row["budget"]["parent_termination"],
            json!({"kind":"execution_timeout"})
        );
        assert_eq!(
            row["budget"]["enforced_duration"],
            json!({"secs":6,"nanos":0})
        );
        assert_eq!(row["budget"]["trace"]["child_terminal"], Value::Null);
        assert_eq!(row["budget"]["trace"]["state"]["kind"], "readable");
        assert_eq!(
            row["budget"]["trace"]["main_action_budgets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let events = assert_retained_pair(root.path(), row, &summary(root.path(), row));
        assert_operator_evidence(&result, root.path(), row);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("PARENT TIMEOUT"));
        assert!(!stdout.contains("PROVIDER ERROR"));
        assert!(
            !events.iter().any(|event| event["type"] == "session_end"),
            "parent must not synthesize a child completion"
        );
    }

    #[test]
    fn bench_budget_recording_failure_is_infrastructure() {
        let mut server = Server::new(Reply::Complete);
        let root = tempfile::tempdir().unwrap();
        let results = root.path().join("results");
        std::fs::create_dir(&results).unwrap();
        let sentinel = b"existing artifact must not be replaced by trace retention";
        std::fs::write(results.join("traces"), sentinel).unwrap();
        let result = output(&mut real_bench(root.path(), &server));
        server.finish();
        let rows = read_rows(root.path(), &result);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(!result.status.success());
        assert_eq!(
            row["completed"], true,
            "the scripted model passed L0; only evidence publication failed"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(
            row["infrastructure_error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
        assert_eq!(row["trace_path"], Value::Null);
        assert_eq!(row["budget"]["retained"], Value::Null);
        assert_eq!(row["budget"]["trace"]["child_terminal"], "task_complete");
        assert_eq!(summary(root.path(), row)["infrastructure_clean"], false);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            stdout.contains("INFRASTRUCTURE FAILURE"),
            "recording failure must not print an unqualified PASS: {stdout}"
        );
        assert!(stdout.contains("INFRASTRUCTURE FAILURE; observed PASS"));
        assert!(stdout.contains(&format!(
            "evidence destination: {}",
            results.join("results.jsonl").display()
        )));
        assert!(!stdout.contains("— PASS"));
        assert_eq!(std::fs::read(results.join("traces")).unwrap(), sentinel);
        assert!(!results.join("model_profiles.json").exists());
    }

    #[test]
    fn bench_budget_row_append_failure_is_infrastructure() {
        let mut server = Server::new(Reply::Complete);
        let root = tempfile::tempdir().unwrap();
        let results = root.path().join("results");
        std::fs::create_dir_all(results.join("results.jsonl")).unwrap();
        let sentinel = b"unrelated contents of the pre-existing directory";
        std::fs::write(results.join("results.jsonl/sentinel"), sentinel).unwrap();
        let result = output(&mut real_bench(root.path(), &server));
        server.finish();
        assert!(!result.status.success());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let summaries: Vec<_> = std::fs::read_dir(&results)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("summary-") && name.ends_with(".json"))
            })
            .collect();
        assert_eq!(summaries.len(), 1);
        let summary: Value =
            serde_json::from_slice(&std::fs::read(&summaries[0]).unwrap()).unwrap();
        assert_eq!(summary["infrastructure_clean"], false);
        assert_eq!(summary["calibration"]["eligible"], false);
        assert_eq!(
            summary["levels"][0]["passes"], 1,
            "retain the actual scripted task outcome"
        );
        assert!(summary["issues"].as_array().unwrap().iter().any(|issue| {
            issue["message"]
                .as_str()
                .is_some_and(|message| message.contains("cannot append results row"))
        }));
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            stdout.contains("INFRASTRUCTURE FAILURE"),
            "append failure must not print an unqualified PASS: {stdout}"
        );
        assert!(stdout.contains("INFRASTRUCTURE FAILURE; observed PASS"));
        // The trace pair survived this append failure, so it remains the
        // precise evidence destination even though the row could not publish.
        let sidecar = summary["budget"]["attempts"][0]["retained"]["sidecar_path"]
            .as_str()
            .unwrap();
        assert!(stdout.contains(&format!(
            "evidence destination: {}",
            results.join(sidecar).display()
        )));
        assert!(!stdout.contains("— PASS"));
        assert_eq!(
            std::fs::read(results.join("results.jsonl/sentinel")).unwrap(),
            sentinel
        );
        assert_eq!(
            std::fs::read_dir(results.join("results.jsonl"))
                .unwrap()
                .count(),
            1
        );
        assert!(!results.join("model_profiles.json").exists());
    }
}
