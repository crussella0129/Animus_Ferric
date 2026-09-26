//! Real Hermes through the constrained valve: bring-up, readiness,
//! cancellation and the native-vs-valve pilot (T-12608/T-12609).
//!
//! ```text
//! cargo run -p ferric-valve --example hermes_pilot -- \
//!   --lab <dir outside both repos> --hermes <Animus_Amalgam checkout> \
//!   --python <Amalgam venv python> --server <llama-server> \
//!   --model <gguf> --model-layers <n> --gpu-layers <n> --plan smoke|cancel|pilot
//! ```
//!
//! Every child (llama-server, each Hermes driver) is owned by a
//! `ferric_process::ProcessTree` and proven reaped; a cleanup failure fails the
//! run. The valve runs in-process. Session deadlines derive from the rates
//! measured on this host. Hermes gets the `file` toolset only, so no
//! model-authored shell command runs. Everything a run writes stays under the
//! lab root, which must lie outside both repositories.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use ferric_process::ProcessTree;
use ferric_valve::pilot::{
    DriverResult, Rates, Task, admit, check, check_lab_root, prefixes_extend, receipt_hash_lists,
    session_deadline, tool_calls_valid,
};
use ferric_valve::probe::probe_enforcement;
use ferric_valve::receipt::ReceiptSink;
use ferric_valve::server::upstream_client;
use ferric_valve::{ValveConfig, ValveMode, router, serve};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Plan {
    /// One valve session each for `lookup` and `no_tool`.
    Smoke,
    /// A long reply interrupted mid-generation through the valve.
    Cancel,
    /// Every task × native/valve × repetitions, counterbalanced.
    Pilot,
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    lab: PathBuf,
    #[arg(long)]
    hermes: PathBuf,
    #[arg(long)]
    python: PathBuf,
    #[arg(long)]
    server: PathBuf,
    #[arg(long)]
    model: PathBuf,
    /// Transformer layers in the model (for the resident-memory estimate).
    #[arg(long)]
    model_layers: u32,
    #[arg(long, default_value_t = 24)]
    gpu_layers: u32,
    #[arg(long, value_enum)]
    plan: Plan,
    #[arg(long, default_value_t = 16_384)]
    ctx: u32,
    #[arg(long, default_value_t = 8181)]
    port: u16,
    #[arg(long, default_value_t = 3)]
    reps: u32,
    /// Main-action output cap Hermes requests per call.
    #[arg(long, default_value_t = 512)]
    max_tokens: u32,
    /// Hermes tool-loop turns per user message.
    #[arg(long, default_value_t = 12)]
    max_turns: u32,
    /// Deadline margin over the measured-rate worst case.
    #[arg(long, default_value_t = 3.0)]
    margin: f64,
    /// Comma-separated task ids to run (default: the plan's set).
    #[arg(long)]
    tasks: Option<String>,
    /// Skip schedule entries before this index: continue an interrupted run with
    /// the same schedule (the session index is preserved in the ledger).
    #[arg(long, default_value_t = 0)]
    start_index: usize,
}

const TASKS_JSON: &str = include_str!("../e2e/tasks.json");
const DRIVER: &str = "e2e/hermes_driver.py";
/// Environment variables the Hermes driver inherits (Amalgam lab parity).
const ENV_ALLOWED: &[&str] = &[
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATH",
    "TEMP",
    "TMP",
    "SYSTEMDRIVE",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMDATA",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate sits two levels below the workspace root")
        .to_path_buf()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn git_head(dir: &Path) -> String {
    let outcome = ferric_process::run_bounded(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"]),
        Duration::from_secs(30),
        ferric_process::CapturePlan::head(256, 256),
    );
    match outcome {
        Ok(outcome) if outcome.exit_code == Some(0) => {
            String::from_utf8_lossy(&outcome.stdout).trim().to_string()
        }
        _ => "unavailable".to_string(),
    }
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        serde_json::to_string_pretty(value).unwrap_or_default(),
    )
    .expect("write lab file");
}

fn append_jsonl(path: &Path, value: &Value) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open jsonl");
    writeln!(file, "{value}").expect("append jsonl");
}

fn read_receipts(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

struct Upstream {
    base: String,
    tree: ProcessTree,
}

fn main() {
    let args = Args::parse();
    let code = run(&args);
    std::process::exit(code);
}

fn run(args: &Args) -> i32 {
    let repos = [repo_root(), args.hermes.clone()];
    let lab = match check_lab_root(&args.lab, &repos) {
        Ok(lab) => lab,
        Err(message) => {
            eprintln!("hermes_pilot: {message}");
            return 2;
        }
    };
    let run_dir = lab
        .join("runs")
        .join(format!("{:?}-{}", args.plan, now_ms()).to_lowercase());
    fs::create_dir_all(&run_dir).expect("create run dir");
    eprintln!("hermes_pilot: run directory {}", run_dir.display());

    let all_tasks: Vec<Task> = serde_json::from_str(TASKS_JSON).expect("tasks.json parses");
    let model_bytes = fs::metadata(&args.model)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let memory = ferric_process::memory::system_memory();
    if let Err(message) = admit(
        model_bytes,
        args.model_layers,
        args.gpu_layers,
        args.ctx,
        memory.map(|memory| memory.available_bytes),
    ) {
        eprintln!("hermes_pilot: {message}");
        write_json(&run_dir.join("not-run.json"), &json!({"reason": message}));
        return 4;
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let client = upstream_client().expect("http client");

    eprintln!("hermes_pilot: hashing model and server binaries");
    let manifest_base = json!({
        "plan": format!("{:?}", args.plan),
        "model_file": args.model.file_name().map(|name| name.to_string_lossy().to_string()),
        "model_bytes": model_bytes,
        "model_sha256": sha256_file(&args.model).unwrap_or_else(|error| format!("unavailable: {error}")),
        "server_sha256": sha256_file(&args.server).unwrap_or_else(|error| format!("unavailable: {error}")),
        "ferric_commit": git_head(&repo_root()),
        "amalgam_commit": git_head(&args.hermes),
        "corpus_sha256": hex::encode(Sha256::digest(TASKS_JSON.as_bytes())),
        "ctx": args.ctx,
        "gpu_layers": args.gpu_layers,
        "model_layers": args.model_layers,
        "max_tokens": args.max_tokens,
        "max_turns": args.max_turns,
        "margin": args.margin,
        "reps": args.reps,
        "host": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "total_memory_bytes": memory.map(|m| m.total_bytes),
            "available_memory_bytes_at_start": memory.map(|m| m.available_bytes),
        },
    });

    let mut upstream = match launch_upstream(args, &run_dir, &rt, &client, model_bytes) {
        Ok(upstream) => upstream,
        Err(message) => {
            eprintln!("hermes_pilot: {message}");
            write_json(
                &run_dir.join("failed.json"),
                &json!({"stage": "launch", "reason": message}),
            );
            return 5;
        }
    };
    let outcome = run_sessions(
        args,
        &run_dir,
        &rt,
        &client,
        &upstream,
        &all_tasks,
        manifest_base,
    );
    let cleanup = upstream.tree.terminate_and_reap();
    append_jsonl(
        &run_dir.join("cleanup.jsonl"),
        &json!({"child": "llama-server", "reaped": cleanup.is_ok(), "error": cleanup.as_ref().err().map(|e| e.to_string())}),
    );
    if let Err(error) = cleanup {
        eprintln!("hermes_pilot: llama-server cleanup failed: {error}");
        return 125;
    }
    match outcome {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("hermes_pilot: {message}");
            6
        }
    }
}

fn launch_upstream(
    args: &Args,
    run_dir: &Path,
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    model_bytes: u64,
) -> Result<Upstream, String> {
    let argv: Vec<String> = vec![
        "-m".into(),
        args.model.display().to_string(),
        "-c".into(),
        args.ctx.to_string(),
        "-ngl".into(),
        args.gpu_layers.to_string(),
        "-fa".into(),
        "on".into(),
        "-ctk".into(),
        "q8_0".into(),
        "-ctv".into(),
        "q8_0".into(),
        "-np".into(),
        "1".into(),
        "--jinja".into(),
        "--cache-ram".into(),
        "0".into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        args.port.to_string(),
        "--no-webui".into(),
    ];
    let log = fs::File::create(run_dir.join("llama-server.log")).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(&args.server);
    command
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err);
    let mut tree =
        ProcessTree::spawn(&mut command).map_err(|error| format!("spawn llama-server: {error}"))?;
    let base = format!("http://127.0.0.1:{}", args.port);
    // Loading is bounded by the bytes to read, not a fixed constant.
    let load_deadline =
        Duration::from_secs(60) + Duration::from_secs(model_bytes / (40 * 1024 * 1024));
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = tree.try_wait_leader() {
            let _ = tree.terminate_and_reap();
            return Err(format!(
                "llama-server exited during load with {status}; see llama-server.log"
            ));
        }
        let healthy = rt.block_on(async {
            client
                .get(format!("{base}/health"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        });
        if healthy {
            break;
        }
        if started.elapsed() > load_deadline {
            let _ = tree.terminate_and_reap();
            return Err(format!("llama-server not healthy within {load_deadline:?}"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let props = rt.block_on(async {
        match client.get(format!("{base}/props")).send().await {
            Ok(response) => response.json::<Value>().await.unwrap_or(Value::Null),
            Err(_) => Value::Null,
        }
    });
    write_json(
        &run_dir.join("upstream.json"),
        &json!({
            "argv": argv.iter().map(|a| if a == &args.model.display().to_string() { "<model>".to_string() } else { a.clone() }).collect::<Vec<_>>(),
            "load_seconds": started.elapsed().as_secs_f64(),
            "build_info": props.get("build_info"),
            "total_slots": props.get("total_slots"),
            "n_ctx": props.pointer("/default_generation_settings/n_ctx"),
        }),
    );
    Ok(Upstream { base, tree })
}

fn measure_rates(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    base: &str,
) -> Result<Rates, String> {
    let filler: String = (0..240)
        .map(|i| format!("Line {i}: the orchard keeps apples, pears and plums in separate rows. "))
        .collect();
    let body = json!({
        "model": "calibration",
        "messages": [{"role": "user", "content": format!("{filler}\nSummarize the text above in one short paragraph.")}],
        "max_tokens": 64,
        "temperature": 0,
        "stream": false,
        "chat_template_kwargs": {"enable_thinking": false},
    });
    let reply: Value = rt
        .block_on(async {
            client
                .post(format!("{base}/v1/chat/completions"))
                .json(&body)
                .send()
                .await?
                .json::<Value>()
                .await
        })
        .map_err(|error| format!("calibration request failed: {error}"))?;
    let timings = reply
        .get("timings")
        .ok_or("calibration reply has no timings")?;
    let rate = |n: &str, ms: &str| -> Option<f64> {
        let n = timings.get(n)?.as_f64()?;
        let ms = timings.get(ms)?.as_f64()?;
        (n > 0.0 && ms > 0.0).then(|| n / (ms / 1_000.0))
    };
    Ok(Rates {
        prefill_tokens_per_second: rate("prompt_n", "prompt_ms").ok_or("no prefill timing")?,
        decode_tokens_per_second: rate("predicted_n", "predicted_ms").ok_or("no decode timing")?,
    })
}

fn erase_slot(rt: &tokio::runtime::Runtime, client: &reqwest::Client, base: &str) -> bool {
    rt.block_on(async {
        client
            .post(format!("{base}/slots/0?action=erase"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    })
}

fn slot_processing(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    base: &str,
) -> Option<bool> {
    rt.block_on(async {
        let slots: Value = client
            .get(format!("{base}/slots"))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        slots.get(0)?.get("is_processing")?.as_bool()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Native,
    Valve,
}

impl Arm {
    fn label(self) -> &'static str {
        match self {
            Arm::Native => "native",
            Arm::Valve => "valve",
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_sessions(
    args: &Args,
    run_dir: &Path,
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    upstream: &Upstream,
    all_tasks: &[Task],
    mut manifest: Value,
) -> Result<(), String> {
    let rates = measure_rates(rt, client, &upstream.base)?;
    eprintln!(
        "hermes_pilot: measured prefill {:.1} tok/s, decode {:.2} tok/s",
        rates.prefill_tokens_per_second, rates.decode_tokens_per_second
    );
    let probe = rt.block_on(probe_enforcement(client, &upstream.base));
    manifest["rates"] = serde_json::to_value(rates).unwrap_or(Value::Null);
    manifest["enforcement_probe"] = json!(
        probe
            .as_ref()
            .map(|()| "enforced")
            .map_err(|e| e.to_string())
    );
    if let Err(error) = probe {
        write_json(&run_dir.join("manifest.json"), &manifest);
        return Err(format!("upstream failed the enforcement probe: {error}"));
    }

    let select = |ids: &[&str]| -> Vec<Task> {
        let wanted: Vec<String> = match &args.tasks {
            Some(list) => list.split(',').map(str::to_string).collect(),
            None => ids.iter().map(|id| id.to_string()).collect(),
        };
        all_tasks
            .iter()
            .filter(|task| wanted.contains(&task.id))
            .cloned()
            .collect()
    };
    let schedule: Vec<(Task, Arm, u32)> = match args.plan {
        Plan::Smoke => select(&["lookup", "no_tool"]).into_iter().map(|t| (t, Arm::Valve, 1)).collect(),
        Plan::Cancel => vec![(
            Task {
                id: "cancel".to_string(),
                prompt: "Write a numbered list of 100 distinct prime numbers, one per line, then a one-sentence note about primes. Do not use tools.".to_string(),
                files: BTreeMap::new(),
                check: ferric_valve::pilot::Check::AnswerContains { expected: "never-expected".to_string() },
                planned_requests: 1,
            },
            Arm::Valve,
            1,
        )],
        Plan::Pilot => {
            let mut schedule = Vec::new();
            for rep in 1..=args.reps {
                for task in select(&["lookup", "edit", "create", "no_tool"]) {
                    // ABBA across repetitions: A B | B A | A B …
                    let order = if rep % 2 == 1 { [Arm::Native, Arm::Valve] } else { [Arm::Valve, Arm::Native] };
                    for arm in order {
                        schedule.push((task.clone(), arm, rep));
                    }
                }
            }
            schedule
        }
    };
    let deadlines: BTreeMap<String, f64> = schedule
        .iter()
        .map(|(task, _, _)| {
            let deadline = session_deadline(
                rates,
                args.ctx,
                args.max_tokens,
                task.planned_requests,
                args.margin,
            );
            (
                task.id.clone(),
                deadline.map(|d| d.as_secs_f64()).unwrap_or(f64::NAN),
            )
        })
        .collect();
    manifest["deadlines_seconds"] = json!(deadlines);
    manifest["schedule"] = json!(
        schedule
            .iter()
            .map(|(t, a, r)| format!("{}:{}:r{r}", t.id, a.label()))
            .collect::<Vec<_>>()
    );
    write_json(&run_dir.join("manifest.json"), &manifest);

    let sessions_path = run_dir.join("sessions.jsonl");
    manifest_note_start(run_dir, args.start_index);
    for (index, (task, arm, rep)) in schedule.iter().enumerate().skip(args.start_index) {
        let record = run_session(
            args, run_dir, rt, client, upstream, rates, index, task, *arm, *rep,
        )?;
        eprintln!(
            "hermes_pilot: [{}/{}] {} {} r{} -> complete={} wall={:.1}s",
            index + 1,
            schedule.len(),
            task.id,
            arm.label(),
            rep,
            record["complete"],
            record["wall_seconds"].as_f64().unwrap_or(0.0)
        );
        append_jsonl(&sessions_path, &record);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_session(
    args: &Args,
    run_dir: &Path,
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    upstream: &Upstream,
    rates: Rates,
    index: usize,
    task: &Task,
    arm: Arm,
    rep: u32,
) -> Result<Value, String> {
    let dir = run_dir.join(format!("{index:02}-{}-{}-r{rep}", task.id, arm.label()));
    let fixture = dir.join("fixture");
    let home = dir.join("hermes_home");
    fs::create_dir_all(&fixture).map_err(|e| e.to_string())?;
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    for isolated in ["appdata", "local"] {
        fs::create_dir_all(home.join(isolated)).map_err(|e| e.to_string())?;
    }
    for (path, content) in &task.files {
        let target = fixture.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&target, content).map_err(|e| e.to_string())?;
    }
    fs::write(dir.join("prompts.json"), json!([task.prompt]).to_string())
        .map_err(|e| e.to_string())?;
    let receipts = dir.join("receipts.jsonl");
    let interrupt = dir.join("interrupt");

    // A fresh in-process valve per session, with its own receipts file.
    let mode = match arm {
        Arm::Native => ValveMode::RecordOnly,
        Arm::Valve => ValveMode::Constrained,
    };
    let app = router(
        ValveConfig {
            upstream: upstream.base.clone(),
            mode,
            heartbeat: Duration::from_secs(2),
        },
        ReceiptSink::open(&receipts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let listener = rt
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .map_err(|e| e.to_string())?;
    let valve_base = format!(
        "http://{}",
        listener.local_addr().map_err(|e| e.to_string())?
    );
    let valve = rt.spawn(async move {
        let _ = serve(listener, app).await;
    });

    let slot_erased = erase_slot(rt, client, &upstream.base);
    let deadline = session_deadline(
        rates,
        args.ctx,
        args.max_tokens,
        task.planned_requests,
        args.margin,
    )?;
    // Mirrors Animus Amalgam's proven lab session config (evals/local_qualification/run.py):
    // an explicit context pin with compression off is Hermes's sanctioned way to run a
    // local window below its 64K floor, and every timeout is the measured-rate deadline.
    // JSON is valid YAML, so the dict is written exactly.
    let timer = deadline.as_secs().max(1);
    let config = json!({
        "model": {
            "default": "amalgam-pilot",
            "provider": "custom",
            "base_url": format!("{valve_base}/v1"),
            "context_length": args.ctx,
            "reasoning_echo": true,
        },
        "providers": {"custom": {"request_timeout_seconds": timer, "stale_timeout_seconds": timer}},
        "agent": {
            "environment_probe": false,
            "local_stream_stale_timeout": timer,
            "turn_liveness": {"timeout_s": timer},
        },
        "local_runtime": {"enabled": false},
        "compression": {"enabled": false},
        "fallback_models": [],
        "display": {"streaming": true},
        "tools": {"tool_search": {"enabled": "off"}},
        "auxiliary": {
            "title_generation": {"enabled": false, "model_upgrade_enabled": false},
        },
    });
    fs::write(
        home.join("config.yaml"),
        serde_json::to_string_pretty(&config).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let result_path = dir.join("result.json");
    let stdout = fs::File::create(dir.join("driver.log")).map_err(|e| e.to_string())?;
    let stderr = stdout.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(&args.python);
    command
        .arg(repo_root().join("crates/ferric-valve").join(DRIVER))
        .arg("--hermes")
        .arg(&args.hermes)
        .arg("--fixture")
        .arg(&fixture)
        .arg("--url")
        .arg(format!("{valve_base}/v1"))
        .arg("--prompts")
        .arg(dir.join("prompts.json"))
        .args(["--toolsets", "file"])
        .arg("--max-turns")
        .arg(args.max_turns.to_string())
        .arg("--max-tokens")
        .arg(args.max_tokens.to_string())
        .arg("--result")
        .arg(&result_path)
        .arg("--interrupt-file")
        .arg(&interrupt)
        .env_clear()
        .envs(
            std::env::vars().filter(|(key, _)| ENV_ALLOWED.contains(&key.to_uppercase().as_str())),
        )
        // Everything Hermes might treat as home points into the disposable session,
        // so the owner's live profile is unreachable (as in Amalgam's lab).
        .env("HERMES_HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", home.join("appdata"))
        .env("LOCALAPPDATA", home.join("local"))
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .current_dir(&fixture)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);

    let started = Instant::now();
    let mut tree =
        ProcessTree::spawn(&mut command).map_err(|error| format!("spawn driver: {error}"))?;
    let mut cancel = Value::Null;
    if args.plan == Plan::Cancel {
        cancel =
            interrupt_mid_generation(rt, client, &upstream.base, &interrupt, &mut tree, deadline);
    }
    let remaining = deadline.saturating_sub(started.elapsed());
    let waited = tree.wait_for_exit(remaining);
    let wall = started.elapsed();
    let (exit, deadline_hit) = match &waited {
        Ok(status) => (json!(status.code()), false),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => (Value::Null, true),
        Err(error) => (json!(error.to_string()), false),
    };
    let reaped = tree.terminate_and_reap();
    valve.abort();
    append_jsonl(
        &run_dir.join("cleanup.jsonl"),
        &json!({"child": format!("driver {}", dir.display()), "reaped": reaped.is_ok()}),
    );
    reaped.map_err(|error| format!("driver cleanup failed: {error}"))?;
    // Let the valve task finish writing any trailing receipt.
    std::thread::sleep(Duration::from_millis(200));

    let result: DriverResult = fs::read_to_string(&result_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let receipts = read_receipts(&receipts);
    let chat: Vec<&Value> = receipts
        .iter()
        .filter(|r| r["mode"] == "constrained" || r["mode"] == "record-only")
        .collect();
    let sum = |key: &str| -> Value {
        let values: Vec<f64> = chat.iter().filter_map(|r| r[key].as_f64()).collect();
        if values.len() == chat.len() && !values.is_empty() {
            json!(values.iter().sum::<f64>())
        } else {
            Value::Null
        }
    };
    let check_outcome = if args.plan == Plan::Cancel {
        Ok(false)
    } else {
        check(task, &result, &fixture)
    };
    let hash_lists = receipt_hash_lists(
        &receipts,
        if arm == Arm::Valve {
            "constrained"
        } else {
            "record-only"
        },
    );
    Ok(json!({
        "index": index,
        "task": task.id,
        "arm": arm.label(),
        "rep": rep,
        "slot_erased": slot_erased,
        "deadline_seconds": deadline.as_secs_f64(),
        "deadline_hit": deadline_hit,
        "exit": exit,
        "wall_seconds": wall.as_secs_f64(),
        "complete": check_outcome.as_ref().ok(),
        "check_error": check_outcome.as_ref().err(),
        "driver_error": result.error,
        "responses": result.responses.len(),
        "tool_calls": result.tool_calls.len(),
        "tool_calls_valid": tool_calls_valid(&result),
        "tool_names": result.tool_calls.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
        "chat_requests": chat.len(),
        "passthrough_requests": receipts.iter().filter(|r| r["mode"] == "pass-through").count(),
        "request_outcomes": chat.iter().map(|r| r["outcome"].clone()).collect::<Vec<_>>(),
        "finish_reasons": chat.iter().map(|r| r["finish_reason"].clone()).collect::<Vec<_>>(),
        "error_classes": chat.iter().map(|r| r["error_class"].clone()).collect::<Vec<_>>(),
        "prompt_tokens": sum("prompt_tokens"),
        "prompt_eval_tokens": sum("prompt_eval_tokens"),
        "cached_tokens": sum("cached_tokens"),
        "predicted_tokens": sum("predicted_tokens"),
        "prompt_ms": sum("prompt_ms"),
        "predicted_ms": sum("predicted_ms"),
        "per_request_cached": chat.iter().map(|r| r["cached_tokens"].clone()).collect::<Vec<_>>(),
        "per_request_prompt_eval": chat.iter().map(|r| r["prompt_eval_tokens"].clone()).collect::<Vec<_>>(),
        "per_request_predicted": chat.iter().map(|r| r["predicted_tokens"].clone()).collect::<Vec<_>>(),
        "prefixes_extend": prefixes_extend(&hash_lists),
        "tools_offered": chat.first().map(|r| r["tools_offered"].clone()),
        "cancel": cancel,
    }))
}

/// Wait until the upstream is generating, interrupt Hermes, and measure how
/// long the slot takes to go idle.
fn interrupt_mid_generation(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    base: &str,
    interrupt: &Path,
    tree: &mut ProcessTree,
    deadline: Duration,
) -> Value {
    let started = Instant::now();
    let mut generating_since = None;
    while started.elapsed() < deadline {
        if tree.try_wait_leader().ok().flatten().is_some() {
            return json!({"interrupted": false, "reason": "driver exited before generation was observed"});
        }
        if slot_processing(rt, client, base) == Some(true) {
            let since = *generating_since.get_or_insert_with(Instant::now);
            // Let it decode for a few seconds so the interrupt lands mid-reply.
            if since.elapsed() >= Duration::from_secs(5) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = fs::write(interrupt, "stop");
    let interrupted_at = Instant::now();
    // The bound for going idle is the same measured-rate deadline, never a
    // fixed constant; record the observed latency.
    while interrupted_at.elapsed() < deadline {
        if slot_processing(rt, client, base) == Some(false) {
            return json!({
                "interrupted": true,
                "slot_idle_after_seconds": interrupted_at.elapsed().as_secs_f64(),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    json!({"interrupted": true, "slot_idle_after_seconds": Value::Null, "reason": "slot still processing at deadline"})
}

/// Record a continuation's starting index next to its manifest.
fn manifest_note_start(run_dir: &Path, start_index: usize) {
    if start_index > 0 {
        write_json(
            &run_dir.join("continuation.json"),
            &json!({"start_index": start_index}),
        );
    }
}
