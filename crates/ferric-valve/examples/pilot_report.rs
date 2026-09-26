//! Render a pilot run's report (T-12609) from the runner's own ledger.
//!
//! ```text
//! cargo run -p ferric-valve --example pilot_report -- <out-dir> <run-dir> [<run-dir>...]
//! ```
//!
//! Writes `<out-dir>/report.md` plus sanitized copies of the manifest, upstream
//! identity, session ledger and cleanup proofs. Every session in the ledger
//! appears in the report; nothing is dropped.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ferric_valve::pilot::{ArmSummary, summarize};
use serde_json::Value;

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn fmt(value: Option<f64>, digits: usize) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{v:.digits$}"))
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--exclusions <file>`: a JSON list of {"run", "tasks", "reason"}. Excluded
    // sessions stay in the ledger and an exclusions table, with their reason,
    // and are left out of the per-arm aggregates. No session disappears.
    let exclusions: Vec<Value> = match args.iter().position(|a| a == "--exclusions") {
        Some(at) if at + 1 < args.len() => {
            let path = args.remove(at + 1);
            args.remove(at);
            serde_json::from_str(&fs::read_to_string(&path).expect("exclusions file"))
                .expect("exclusions json")
        }
        _ => Vec::new(),
    };
    if args.len() < 2 {
        eprintln!("usage: pilot_report <out-dir> [--exclusions <file>] <run-dir> [<run-dir>...]");
        std::process::exit(2);
    }
    let out = Path::new(&args[0]);
    let runs: Vec<&Path> = args[1..].iter().map(Path::new).collect();
    fs::create_dir_all(out).expect("create out dir");
    let excluded_reason = |run: &str, task: &str| -> Option<String> {
        exclusions.iter().find_map(|rule| {
            let tasks = rule["tasks"].as_array()?;
            (rule["run"].as_str() == Some(run) && tasks.iter().any(|t| t.as_str() == Some(task)))
                .then(|| rule["reason"].as_str().unwrap_or("excluded").to_string())
        })
    };

    // A continuation run (`--start-index`) resumes the same schedule; its
    // sessions carry their original indices, so the ledgers merge by index.
    let read_json = |path: &Path| -> Value {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null)
    };
    let manifest = read_json(&runs[0].join("manifest.json"));
    let upstream = read_json(&runs[0].join("upstream.json"));
    let run_notes: Vec<Value> = runs
        .iter()
        .map(|run| {
            let manifest = read_json(&run.join("manifest.json"));
            serde_json::json!({
                "run": run.file_name().map(|name| name.to_string_lossy().to_string()),
                "start_index": read_json(&run.join("continuation.json"))["start_index"],
                "rates": manifest["rates"],
                "ferric_commit": manifest["ferric_commit"],
                "sessions": read_jsonl(&run.join("sessions.jsonl")).len(),
            })
        })
        .collect();
    // Tag every session with its run and any exclusion, keeping run order.
    let sessions: Vec<Value> = runs
        .iter()
        .flat_map(|run| {
            let name = run
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut rows = read_jsonl(&run.join("sessions.jsonl"));
            rows.sort_by_key(|session| session["index"].as_u64().unwrap_or(u64::MAX));
            rows.into_iter().map(move |mut session| {
                session["run"] = Value::String(name.clone());
                session
            })
        })
        .map(|mut session| {
            let reason = excluded_reason(
                session["run"].as_str().unwrap_or(""),
                session["task"].as_str().unwrap_or(""),
            );
            session["excluded"] = reason.map_or(Value::Null, Value::String);
            session
        })
        .collect();
    let cleanup: Vec<Value> = runs
        .iter()
        .flat_map(|run| read_jsonl(&run.join("cleanup.jsonl")))
        .collect();
    let included: Vec<Value> = sessions
        .iter()
        .filter(|session| session["excluded"].is_null())
        .cloned()
        .collect();
    let excluded: Vec<&Value> = sessions
        .iter()
        .filter(|session| !session["excluded"].is_null())
        .collect();
    let summaries = summarize(&included);
    fs::write(
        out.join("runs.json"),
        serde_json::to_string_pretty(&run_notes).unwrap(),
    )
    .unwrap();

    // Sanitized copies: the ledger records carry no paths; cleanup rows do.
    fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        out.join("upstream.json"),
        serde_json::to_string_pretty(&upstream).unwrap(),
    )
    .unwrap();
    fs::write(
        out.join("sessions.jsonl"),
        sessions
            .iter()
            .map(|s| format!("{s}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let cleanup_rows: Vec<String> = cleanup
        .iter()
        .map(|row| {
            let child = row["child"].as_str().unwrap_or("");
            let label = child.rsplit(['\\', '/']).next().unwrap_or(child);
            let label = if child.starts_with("driver") {
                format!("driver {label}")
            } else {
                label.to_string()
            };
            format!(
                "{}\n",
                serde_json::json!({"child": label, "reaped": row["reaped"]})
            )
        })
        .collect();
    fs::write(out.join("cleanup.jsonl"), cleanup_rows.concat()).unwrap();
    let all_reaped = cleanup.iter().all(|row| row["reaped"] == Value::Bool(true));

    let mut md = String::new();
    let rates = &manifest["rates"];
    writeln!(md, "# Sprint 126 pilot — native vs valve on the 27B\n").unwrap();
    writeln!(
        md,
        "Real Hermes (Animus Amalgam `{}`) ran the same four tasks through two arms on one \
         llama.cpp process:\n\n\
         - **native** — Hermes's ordinary request with native `tools`; the valve in \
         **record-only** mode forwards it byte-for-byte.\n\
         - **valve** — the same request through Ferric's constrained valve, where one \
         harness-authored JSON-Schema action is enforced by llama.cpp (the `thought`, `tool`, \
         `args` grammar, today's defaults).\n\n\
         Both arms pass through the same valve process and receipt code, so the constraint \
         transform is the only difference. Arm order is counterbalanced (A B | B A | A B). \
         Every session starts from an erased slot.\n\n\
         **This is a three-repetition pilot. It estimates feasibility and variance; it supports \
         no advancement or general-capability claim.** The arm-B advancement decision belongs \
         to Amalgam (INT-0004, T-202).\n",
        manifest["amalgam_commit"]
            .as_str()
            .unwrap_or("?")
            .get(..10)
            .unwrap_or("?"),
    )
    .unwrap();

    writeln!(md, "## Manifest\n").unwrap();
    writeln!(md, "| Coordinate | Value |\n|---|---|").unwrap();
    for (label, value) in [
        ("Model", manifest["model_file"].to_string()),
        ("Model SHA-256", manifest["model_sha256"].to_string()),
        (
            "llama-server SHA-256",
            manifest["server_sha256"].to_string(),
        ),
        ("llama.cpp build", upstream["build_info"].to_string()),
        ("Server argv", upstream["argv"].to_string()),
        (
            "Context / GPU layers / model layers",
            format!(
                "{} / {} / {}",
                manifest["ctx"], manifest["gpu_layers"], manifest["model_layers"]
            ),
        ),
        (
            "Output cap / Hermes max turns",
            format!("{} / {}", manifest["max_tokens"], manifest["max_turns"]),
        ),
        (
            "Measured prefill / decode (tok/s)",
            format!(
                "{} / {}",
                fmt(rates["prefill_tokens_per_second"].as_f64(), 1),
                fmt(rates["decode_tokens_per_second"].as_f64(), 2)
            ),
        ),
        (
            "Enforcement probe",
            manifest["enforcement_probe"].to_string(),
        ),
        (
            "Derived session deadlines (s)",
            manifest["deadlines_seconds"].to_string(),
        ),
        ("Deadline margin", manifest["margin"].to_string()),
        ("Repetitions", manifest["reps"].to_string()),
        ("Ferric commit", manifest["ferric_commit"].to_string()),
        (
            "Amalgam (Hermes) commit",
            manifest["amalgam_commit"].to_string(),
        ),
        (
            "Corpus SHA-256 (`e2e/tasks.json`)",
            manifest["corpus_sha256"].to_string(),
        ),
        ("Host", manifest["host"].to_string()),
        (
            "Runs merged (continuations resume the same schedule)",
            serde_json::to_string(&run_notes).unwrap_or_default(),
        ),
    ] {
        writeln!(md, "| {label} | `{}` |", value.replace('|', "\\|")).unwrap();
    }

    writeln!(
        md,
        "\n## Per-arm results ({} included sessions; {} excluded, listed below)\n",
        included.len(),
        excluded.len()
    )
    .unwrap();
    writeln!(
        md,
        "| Metric | {} |",
        summaries
            .iter()
            .map(|s| s.arm.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    )
    .unwrap();
    writeln!(
        md,
        "|---|{}|",
        summaries
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join("|")
    )
    .unwrap();
    let row = |label: &str, f: &dyn Fn(&ArmSummary) -> String| -> String {
        format!(
            "| {label} | {} |",
            summaries.iter().map(f).collect::<Vec<_>>().join(" | ")
        )
    };
    for line in [
        row("Sessions", &|s| s.sessions.to_string()),
        row("Independently checked completions", &|s| {
            format!("{} / {}", s.completed, s.sessions)
        }),
        row("Tool calls (sessions with all calls valid)", &|s| {
            format!(
                "{} ({}/{})",
                s.tool_calls, s.sessions_all_calls_valid, s.sessions
            )
        }),
        row("Model requests", &|s| s.requests.to_string()),
        row("Truncated requests (`length`)", &|s| {
            s.truncations.to_string()
        }),
        row("Requests not completed (failed or cancelled)", &|s| {
            s.request_failures.to_string()
        }),
        row("Sessions stopped by the derived deadline", &|s| {
            s.deadline_hits.to_string()
        }),
        row("Sessions with a driver error", &|s| {
            s.driver_errors.to_string()
        }),
        row("Check infrastructure errors", &|s| {
            s.check_errors.to_string()
        }),
        row("Decoded tokens per request", &|s| {
            fmt(s.predicted_per_request(), 1)
        }),
        row("Decoded tokens per checked completion", &|s| {
            fmt(s.predicted_per_completion(), 1)
        }),
        row("Decode time per session (s)", &|s| {
            fmt(
                (s.sessions > s.metrics_incomplete)
                    .then(|| s.predicted_ms / 1000.0 / (s.sessions - s.metrics_incomplete) as f64),
                1,
            )
        }),
        row("Wall time per session (s)", &|s| {
            fmt(s.wall_per_session(), 1)
        }),
        row("Wall time per checked completion (s)", &|s| {
            fmt(s.wall_per_completion(), 1)
        }),
        row("Prompt tokens served from cache", &|s| {
            fmt(s.cache_share().map(|v| v * 100.0), 1) + "%"
        }),
        row("Sessions whose rendered prefixes only grew", &|s| {
            format!("{}/{}", s.sessions_prefix_extends, s.sessions)
        }),
        row("Sessions with incomplete upstream metrics", &|s| {
            s.metrics_incomplete.to_string()
        }),
    ] {
        writeln!(md, "{line}").unwrap();
    }

    writeln!(md, "\n## Per task\n").unwrap();
    writeln!(md, "| Task | Arm | Completed | Mean wall (s) | Mean decoded tokens | Mean requests |\n|---|---|---|---|---|---|").unwrap();
    let mut by_task: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for session in &included {
        by_task
            .entry((
                session["task"].as_str().unwrap_or("?").to_string(),
                session["arm"].as_str().unwrap_or("?").to_string(),
            ))
            .or_default()
            .push(session);
    }
    for ((task, arm), rows) in &by_task {
        let n = rows.len() as f64;
        let completed = rows
            .iter()
            .filter(|r| r["complete"] == Value::Bool(true))
            .count();
        let mean = |key: &str| {
            let values: Vec<f64> = rows.iter().filter_map(|r| r[key].as_f64()).collect();
            (values.len() == rows.len()).then(|| values.iter().sum::<f64>() / n)
        };
        writeln!(
            md,
            "| {task} | {arm} | {completed}/{} | {} | {} | {} |",
            rows.len(),
            fmt(mean("wall_seconds"), 1),
            fmt(mean("predicted_tokens"), 1),
            fmt(mean("chat_requests"), 1),
        )
        .unwrap();
    }

    if !excluded.is_empty() {
        writeln!(md, "\n## Excluded sessions\n").unwrap();
        writeln!(
            md,
            "| Run | # | Task | Arm | Rep | Checker said | Reason |\n|---|---|---|---|---|---|---|"
        )
        .unwrap();
        for session in &excluded {
            writeln!(
                md,
                "| {} | {} | {} | {} | {} | {} | {} |",
                session["run"].as_str().unwrap_or("?"),
                session["index"],
                session["task"].as_str().unwrap_or("?"),
                session["arm"].as_str().unwrap_or("?"),
                session["rep"],
                session["complete"],
                session["excluded"].as_str().unwrap_or("").replace('|', "/"),
            )
            .unwrap();
        }
    }

    writeln!(
        md,
        "\n## Session ledger (every session, including excluded)\n"
    )
    .unwrap();
    writeln!(md, "| Run | # | Task | Arm | Rep | Complete | Tools called | Requests | Finish reasons | Decoded | Cached / evaluated prompt | Wall (s) | Notes |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|").unwrap();
    for session in &sessions {
        let mut notes = Vec::new();
        if session["deadline_hit"] == Value::Bool(true) {
            notes.push("deadline".to_string());
        }
        if let Some(error) = session["driver_error"].as_str() {
            notes.push(format!(
                "driver: {}",
                error.chars().take(80).collect::<String>()
            ));
        }
        if let Some(error) = session["check_error"].as_str() {
            notes.push(format!("check: {error}"));
        }
        if session["tool_calls_valid"] == Value::Bool(false) {
            notes.push("invalid tool call".to_string());
        }
        if !session["excluded"].is_null() {
            notes.push("EXCLUDED".to_string());
        }
        writeln!(
            md,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} / {} | {} | {} |",
            session["run"].as_str().unwrap_or("?"),
            session["index"],
            session["task"].as_str().unwrap_or("?"),
            session["arm"].as_str().unwrap_or("?"),
            session["rep"],
            session["complete"],
            session["tool_names"],
            session["chat_requests"],
            session["finish_reasons"],
            session["predicted_tokens"],
            session["cached_tokens"],
            session["prompt_eval_tokens"],
            fmt(session["wall_seconds"].as_f64(), 1),
            notes.join("; ").replace('|', "/"),
        )
        .unwrap();
    }

    let offered: Vec<String> = sessions
        .iter()
        .filter_map(|s| s["tools_offered"].as_u64())
        .map(|n| n.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    writeln!(md, "\n## Limits and open measurements\n").unwrap();
    writeln!(
        md,
        "- **Tool catalog size:** {} tools offered (Hermes's `file` toolset). The Hermes-sized \
         catalog measurement (INT-0013 AC-2) remains open.\n\
         - **Arms:** only native (template tool grammar, lazy) versus today's `F-thought` \
         schema. `N-forced`, `F-bounded`, `F-none` and the reasoning axis (INT-0013 AC-4) are \
         not measured here. Thinking was off in both arms.\n\
         - **Sample:** three repetitions per task and arm. Differences smaller than \
         session-to-session variation are not evidence either way.\n\
         - **History rendering:** the valve re-renders prior actions canonically. The cached \
         and evaluated prompt columns show what that costs per session.\n\
         - **Cleanup:** every child was reaped: **{all_reaped}** ({} cleanup records).",
        if offered.is_empty() {
            "unknown".to_string()
        } else {
            offered.join(", ")
        },
        cleanup.len(),
    )
    .unwrap();

    fs::write(out.join("report.md"), md).expect("write report");
    println!("wrote {}", out.join("report.md").display());
}
