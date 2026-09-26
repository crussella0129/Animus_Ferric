//! Pure support for the real-Hermes E2E runner and pilot (T-12608/T-12609).
//!
//! The runner (`examples/hermes_pilot.rs`) owns processes. Everything it
//! decides lives here, so it is unit-tested in the default lane:
//! - where the lab may live;
//! - whether the host has room for the model;
//! - how long a session may take;
//! - whether a Hermes session completed its task;
//! - whether a session's rendered prefixes only ever grew.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ferric_core::{Fit, classify_fit, estimate_model_memory};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Refuse a lab root inside any of `repositories`. Model-authored files and
/// Hermes homes must never land in a source tree. Paths are compared after
/// canonicalization, so `..` tricks and casing do not slip through.
pub fn check_lab_root(lab: &Path, repositories: &[PathBuf]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(lab)
        .map_err(|error| format!("cannot create lab {}: {error}", lab.display()))?;
    let lab = lab
        .canonicalize()
        .map(strip_verbatim)
        .map_err(|error| format!("cannot resolve lab {}: {error}", lab.display()))?;
    for repository in repositories {
        if let Ok(repository) = repository.canonicalize().map(strip_verbatim)
            && lab.starts_with(&repository)
        {
            return Err(format!(
                "refusing lab root {}: it is inside the repository {}",
                lab.display(),
                repository.display()
            ));
        }
    }
    Ok(lab)
}

/// Windows `canonicalize` returns verbatim paths (`\\?\C:\...` and
/// `\\?\UNC\server\share\...`). Handing one to a child as its working
/// directory breaks tools that re-split the path: the Sprint 126 pilot's
/// first run lost every Hermes file write to `mkdir: cannot create directory
/// '//?'`. Return the ordinary form; other paths pass through unchanged.
pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return PathBuf::from(rest);
    }
    path
}

/// Admission: the CPU-resident share of the weights (layers not offloaded to
/// the GPU) plus a full KV allowance and headroom must fit the memory that is
/// available right now. An unmeasurable host is refused rather than guessed.
pub fn admit(
    model_bytes: u64,
    model_layers: u32,
    gpu_layers: u32,
    context: u32,
    available: Option<u64>,
) -> Result<u64, String> {
    let layers = model_layers.max(1);
    let cpu_layers = layers.saturating_sub(gpu_layers.min(layers));
    let cpu_bytes = ((model_bytes as u128 * cpu_layers as u128) / layers as u128) as u64;
    let estimate = estimate_model_memory(cpu_bytes, context);
    match classify_fit(estimate, available) {
        Fit::Fits | Fit::Tight => Ok(estimate),
        Fit::WontFit => Err(format!(
            "not-run: resource gate: estimated {estimate} bytes resident exceeds {} bytes available",
            available.unwrap_or_default()
        )),
        Fit::Unknown => {
            Err("not-run: resource gate: available memory could not be measured".to_string())
        }
    }
}

/// Measured throughput of the running host and model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    pub prefill_tokens_per_second: f64,
    pub decode_tokens_per_second: f64,
}

/// A session's backstop: `margin` × planned requests × (a full context of
/// prefill + a full output cap of decode), all at the measured rates. There is
/// no fixed time constant, so a slower host gets proportionally longer.
pub fn session_deadline(
    rates: Rates,
    context: u32,
    output_cap: u32,
    planned_requests: u32,
    margin: f64,
) -> Result<Duration, String> {
    if !(rates.prefill_tokens_per_second.is_finite()
        && rates.decode_tokens_per_second.is_finite()
        && rates.prefill_tokens_per_second > 0.0
        && rates.decode_tokens_per_second > 0.0
        && margin.is_finite()
        && margin > 0.0)
    {
        return Err(format!(
            "unusable rates or margin: {rates:?}, margin {margin}"
        ));
    }
    let per_request = context as f64 / rates.prefill_tokens_per_second
        + output_cap as f64 / rates.decode_tokens_per_second;
    Ok(Duration::from_secs_f64(
        margin * planned_requests.max(1) as f64 * per_request,
    ))
}

/// True when every consecutive pair of `message_hashes` lists extends: each
/// list is a prefix of the next. An empty or single list trivially extends.
pub fn prefixes_extend(lists: &[Vec<String>]) -> bool {
    lists
        .windows(2)
        .all(|pair| pair[1].len() >= pair[0].len() && pair[1][..pair[0].len()] == pair[0][..])
}

/// A task in the pilot corpus.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Task {
    pub id: String,
    pub prompt: String,
    /// Fixture files relative to the session fixture root.
    #[serde(default)]
    pub files: std::collections::BTreeMap<String, String>,
    pub check: Check,
    pub planned_requests: u32,
}

/// An independent completion check, judged from fixture state or the final
/// answer, never from the model's own claim of success.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Check {
    /// The final answer contains `expected`.
    AnswerContains { expected: String },
    /// The final answer contains `expected` and no tool was called.
    AnswerWithoutTools { expected: String },
    /// Each file's content, trimmed, equals the expected text.
    FilesEqual {
        files: std::collections::BTreeMap<String, String>,
    },
    /// `path` contains every `includes` and none of the `excludes`.
    FileContains {
        path: String,
        includes: Vec<String>,
        excludes: Vec<String>,
    },
}

/// What the Hermes driver reports for one session (`hermes_driver.py`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct DriverResult {
    #[serde(default)]
    pub responses: Vec<Option<String>>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct ToolCallRecord {
    pub name: Option<String>,
    pub arguments_valid: bool,
    pub known_tool: bool,
}

/// Judge a session. `Ok(true)` means complete, `Ok(false)` incomplete, and
/// `Err` means the check itself could not run (an infrastructure failure,
/// kept distinct from a model failure).
pub fn check(task: &Task, result: &DriverResult, fixture: &Path) -> Result<bool, String> {
    let answer = result
        .responses
        .iter()
        .rev()
        .find_map(|response| response.clone())
        .unwrap_or_default();
    Ok(match &task.check {
        Check::AnswerContains { expected } => answer.contains(expected.as_str()),
        Check::AnswerWithoutTools { expected } => {
            answer.contains(expected.as_str()) && result.tool_calls.is_empty()
        }
        Check::FilesEqual { files } => files.iter().all(|(path, expected)| {
            std::fs::read_to_string(fixture.join(path))
                .map(|content| normalize(&content) == normalize(expected))
                .unwrap_or(false)
        }),
        Check::FileContains {
            path,
            includes,
            excludes,
        } => {
            let target = fixture.join(path);
            if !target.is_file() {
                return Ok(false);
            }
            let content = std::fs::read_to_string(&target)
                .map_err(|error| format!("cannot read {}: {error}", target.display()))?;
            includes
                .iter()
                .all(|needle| content.contains(needle.as_str()))
                && excludes
                    .iter()
                    .all(|needle| !content.contains(needle.as_str()))
        }
    })
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

/// Per-arm aggregates over a pilot's session ledger (T-12609). Every session
/// counts in `sessions`, whatever happened to it, so no denominator is hidden.
/// A metric the upstream did not report for some request makes that session
/// `metrics_incomplete`, and its sums are left out rather than guessed.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ArmSummary {
    pub arm: String,
    pub sessions: usize,
    pub completed: usize,
    pub check_errors: usize,
    pub deadline_hits: usize,
    pub driver_errors: usize,
    pub tool_calls: usize,
    pub sessions_all_calls_valid: usize,
    pub requests: usize,
    pub truncations: usize,
    pub request_failures: usize,
    pub sessions_prefix_extends: usize,
    pub metrics_incomplete: usize,
    /// Requests in sessions whose metrics are complete (the token denominator).
    pub metric_requests: usize,
    pub predicted_tokens: f64,
    pub prompt_eval_tokens: f64,
    pub cached_tokens: f64,
    pub predicted_ms: f64,
    pub wall_seconds: f64,
}

impl ArmSummary {
    fn ratio(numerator: f64, denominator: usize) -> Option<f64> {
        (denominator > 0).then(|| numerator / denominator as f64)
    }

    /// Decoded tokens per request, over sessions with complete metrics.
    pub fn predicted_per_request(&self) -> Option<f64> {
        Self::ratio(self.predicted_tokens, self.metric_requests)
    }

    /// All decoded tokens divided by independently checked completions.
    pub fn predicted_per_completion(&self) -> Option<f64> {
        Self::ratio(self.predicted_tokens, self.completed)
    }

    pub fn wall_per_session(&self) -> Option<f64> {
        Self::ratio(self.wall_seconds, self.sessions)
    }

    /// All wall time divided by independently checked completions.
    pub fn wall_per_completion(&self) -> Option<f64> {
        Self::ratio(self.wall_seconds, self.completed)
    }

    /// Share of prompt tokens served from the backend's cache.
    pub fn cache_share(&self) -> Option<f64> {
        let total = self.cached_tokens + self.prompt_eval_tokens;
        (total > 0.0).then(|| self.cached_tokens / total)
    }
}

/// Aggregate session records (the runner's `sessions.jsonl`) by arm, in the
/// order arms first appear.
pub fn summarize(sessions: &[Value]) -> Vec<ArmSummary> {
    let mut summaries: Vec<ArmSummary> = Vec::new();
    for session in sessions {
        let arm = session["arm"].as_str().unwrap_or("unknown").to_string();
        let index = match summaries.iter().position(|s| s.arm == arm) {
            Some(index) => index,
            None => {
                summaries.push(ArmSummary {
                    arm,
                    ..ArmSummary::default()
                });
                summaries.len() - 1
            }
        };
        let summary = &mut summaries[index];
        summary.sessions += 1;
        if session["complete"] == Value::Bool(true) {
            summary.completed += 1;
        }
        if !session["check_error"].is_null() {
            summary.check_errors += 1;
        }
        if session["deadline_hit"] == Value::Bool(true) {
            summary.deadline_hits += 1;
        }
        if !session["driver_error"].is_null() {
            summary.driver_errors += 1;
        }
        summary.tool_calls += session["tool_calls"].as_u64().unwrap_or(0) as usize;
        if session["tool_calls_valid"] == Value::Bool(true) {
            summary.sessions_all_calls_valid += 1;
        }
        summary.requests += session["chat_requests"].as_u64().unwrap_or(0) as usize;
        let finishes = session["finish_reasons"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        summary.truncations += finishes
            .iter()
            .filter(|f| f.as_str() == Some("length"))
            .count();
        let outcomes = session["request_outcomes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        summary.request_failures += outcomes
            .iter()
            .filter(|o| o.as_str() != Some("completed"))
            .count();
        if session["prefixes_extend"] == Value::Bool(true) {
            summary.sessions_prefix_extends += 1;
        }
        let metrics = [
            session["predicted_tokens"].as_f64(),
            session["prompt_eval_tokens"].as_f64(),
            session["cached_tokens"].as_f64(),
            session["predicted_ms"].as_f64(),
        ];
        match metrics {
            [Some(predicted), Some(evaluated), Some(cached), Some(ms)] => {
                summary.metric_requests += session["chat_requests"].as_u64().unwrap_or(0) as usize;
                summary.predicted_tokens += predicted;
                summary.prompt_eval_tokens += evaluated;
                summary.cached_tokens += cached;
                summary.predicted_ms += ms;
            }
            _ => summary.metrics_incomplete += 1,
        }
        summary.wall_seconds += session["wall_seconds"].as_f64().unwrap_or(0.0);
    }
    summaries
}

/// Every call's arguments parsed as a JSON object and named an offered tool.
pub fn tool_calls_valid(result: &DriverResult) -> bool {
    result
        .tool_calls
        .iter()
        .all(|call| call.arguments_valid && call.known_tool)
}

/// Pull `message_hashes` lists, in order, from receipts of the given mode.
pub fn receipt_hash_lists(receipts: &[Value], mode: &str) -> Vec<Vec<String>> {
    receipts
        .iter()
        .filter(|receipt| receipt["mode"] == mode)
        .map(|receipt| {
            receipt["message_hashes"]
                .as_array()
                .map(|hashes| {
                    hashes
                        .iter()
                        .filter_map(|h| h.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn lab_root_inside_repo_is_refused() {
        let repo = tempfile::tempdir().unwrap();
        let inside = repo.path().join("lab");
        let error = check_lab_root(&inside, &[repo.path().to_path_buf()]).unwrap_err();
        assert!(error.contains("inside the repository"), "{error}");
        let dotted = repo.path().join("sub").join("..").join("lab2");
        assert!(check_lab_root(&dotted, &[repo.path().to_path_buf()]).is_err());
        let outside = tempfile::tempdir().unwrap();
        assert!(check_lab_root(&outside.path().join("lab"), &[repo.path().to_path_buf()]).is_ok());
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\lab\runs")),
            PathBuf::from(r"C:\lab\runs")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share\lab")),
            PathBuf::from(r"\\server\share\lab")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from("/tmp/lab")),
            PathBuf::from("/tmp/lab")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\Volume{abc}\x")),
            PathBuf::from(r"\\?\Volume{abc}\x"),
            "a non-drive verbatim path has no ordinary form and is kept"
        );
    }

    #[test]
    fn lab_root_is_returned_without_verbatim_prefix() {
        let outside = tempfile::tempdir().unwrap();
        let lab = check_lab_root(&outside.path().join("lab"), &[]).unwrap();
        assert!(
            !lab.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            lab.display()
        );
        assert!(lab.is_dir());
    }

    #[test]
    fn deadline_is_derived_from_measured_rates() {
        let rates = Rates {
            prefill_tokens_per_second: 100.0,
            decode_tokens_per_second: 4.0,
        };
        // 2 × 3 × (1000/100 + 400/4) = 660 s
        assert_eq!(
            session_deadline(rates, 1_000, 400, 3, 2.0).unwrap(),
            Duration::from_secs(660)
        );
        let slower = Rates {
            prefill_tokens_per_second: 50.0,
            decode_tokens_per_second: 2.0,
        };
        assert_eq!(
            session_deadline(slower, 1_000, 400, 3, 2.0).unwrap(),
            Duration::from_secs(1_320),
            "half the speed, twice the time: no fixed constant"
        );
        let broken = Rates {
            prefill_tokens_per_second: 0.0,
            decode_tokens_per_second: 4.0,
        };
        assert!(session_deadline(broken, 1_000, 400, 3, 2.0).is_err());
    }

    #[test]
    fn admission_refuses_unknown_or_insufficient_memory() {
        let gib = 1u64 << 30;
        // 16 GiB model, 24 of 66 layers offloaded, 16K context.
        assert!(admit(16 * gib, 66, 24, 16_384, Some(32 * gib)).is_ok());
        assert!(
            admit(16 * gib, 66, 24, 16_384, Some(8 * gib))
                .unwrap_err()
                .contains("resource gate")
        );
        assert!(
            admit(16 * gib, 66, 24, 16_384, None)
                .unwrap_err()
                .contains("could not be measured")
        );
        // Fully offloaded: only KV and headroom stay resident.
        let resident = admit(4 * gib, 28, 99, 8_192, Some(8 * gib)).unwrap();
        assert!(resident < 3 * gib, "{resident}");
    }

    #[test]
    fn prefix_extension_is_detected() {
        let a = vec!["1".to_string()];
        let ab = vec!["1".to_string(), "2".to_string()];
        let ac = vec!["1".to_string(), "3".to_string()];
        assert!(prefixes_extend(&[a.clone(), ab.clone(), ab.clone()]));
        assert!(!prefixes_extend(&[ab, ac.clone()]));
        assert!(!prefixes_extend(&[ac, a]));
        assert!(prefixes_extend(&[]));
    }

    fn result(answer: &str, calls: usize) -> DriverResult {
        DriverResult {
            responses: vec![Some(answer.to_string())],
            tool_calls: (0..calls)
                .map(|_| ToolCallRecord {
                    name: Some("read_file".to_string()),
                    arguments_valid: true,
                    known_tool: true,
                })
                .collect(),
            error: None,
        }
    }

    fn task(check: Check) -> Task {
        Task {
            id: "t".to_string(),
            prompt: "p".to_string(),
            files: BTreeMap::new(),
            check,
            planned_requests: 3,
        }
    }

    #[test]
    fn checker_lookup_accepts_expected_value() {
        let fixture = tempfile::tempdir().unwrap();
        let t = task(Check::AnswerContains {
            expected: "7342".to_string(),
        });
        assert!(check(&t, &result("The port is 7342.", 1), fixture.path()).unwrap());
    }

    #[test]
    fn checker_lookup_rejects_other() {
        let fixture = tempfile::tempdir().unwrap();
        let t = task(Check::AnswerContains {
            expected: "7342".to_string(),
        });
        assert!(!check(&t, &result("The port is 8080.", 1), fixture.path()).unwrap());
        assert!(!check(&t, &DriverResult::default(), fixture.path()).unwrap());
    }

    #[test]
    fn checker_edit_detects_change() {
        let fixture = tempfile::tempdir().unwrap();
        let t = task(Check::FileContains {
            path: "hello.py".to_string(),
            includes: vec!["return \"Hi, world\"".to_string()],
            excludes: vec!["Hello, world".to_string()],
        });
        std::fs::write(
            fixture.path().join("hello.py"),
            "def greet():\n    return \"Hello, world\"\n",
        )
        .unwrap();
        assert!(!check(&t, &result("done", 1), fixture.path()).unwrap());
        std::fs::write(
            fixture.path().join("hello.py"),
            "def greet():\n    return \"Hi, world\"\n",
        )
        .unwrap();
        assert!(check(&t, &result("done", 1), fixture.path()).unwrap());
    }

    #[test]
    fn checker_create_requires_both_files() {
        let fixture = tempfile::tempdir().unwrap();
        let t = task(Check::FilesEqual {
            files: BTreeMap::from([
                ("functions.txt".to_string(), "a\nb".to_string()),
                ("count.txt".to_string(), "2".to_string()),
            ]),
        });
        std::fs::write(fixture.path().join("functions.txt"), "a\r\nb\r\n").unwrap();
        assert!(
            !check(&t, &result("done", 2), fixture.path()).unwrap(),
            "count.txt missing"
        );
        std::fs::write(fixture.path().join("count.txt"), "2\n").unwrap();
        assert!(check(&t, &result("done", 2), fixture.path()).unwrap());
    }

    #[test]
    fn checker_no_tool_requires_zero_tool_calls() {
        let fixture = tempfile::tempdir().unwrap();
        let t = task(Check::AnswerWithoutTools {
            expected: "51".to_string(),
        });
        assert!(check(&t, &result("51", 0), fixture.path()).unwrap());
        assert!(!check(&t, &result("51", 1), fixture.path()).unwrap());
    }

    #[test]
    fn summarize_counts_every_session() {
        let sessions = vec![
            serde_json::json!({"arm": "native", "complete": true, "check_error": null, "deadline_hit": false,
                "driver_error": null, "tool_calls": 1, "tool_calls_valid": true, "chat_requests": 2,
                "finish_reasons": ["tool_calls", "stop"], "request_outcomes": ["completed", "completed"],
                "prefixes_extend": true, "predicted_tokens": 80.0, "prompt_eval_tokens": 2600.0,
                "cached_tokens": 2500.0, "predicted_ms": 20000.0, "wall_seconds": 40.0}),
            serde_json::json!({"arm": "valve", "complete": false, "check_error": null, "deadline_hit": true,
                "driver_error": null, "tool_calls": 0, "tool_calls_valid": true, "chat_requests": 1,
                "finish_reasons": ["length"], "request_outcomes": ["completed"], "prefixes_extend": true,
                "predicted_tokens": null, "prompt_eval_tokens": 2600.0, "cached_tokens": 0.0,
                "predicted_ms": 1.0, "wall_seconds": 90.0}),
            serde_json::json!({"arm": "native", "complete": false, "check_error": null, "deadline_hit": false,
                "driver_error": "boom", "tool_calls": 0, "tool_calls_valid": true, "chat_requests": 0,
                "finish_reasons": [], "request_outcomes": [], "prefixes_extend": true,
                "predicted_tokens": null, "prompt_eval_tokens": null, "cached_tokens": null,
                "predicted_ms": null, "wall_seconds": 5.0}),
        ];
        let summaries = summarize(&sessions);
        assert_eq!(summaries.len(), 2);
        let native = &summaries[0];
        assert_eq!(
            (native.arm.as_str(), native.sessions, native.completed),
            ("native", 2, 1)
        );
        assert_eq!(native.driver_errors, 1);
        assert_eq!(native.metrics_incomplete, 1);
        assert_eq!(native.predicted_per_request(), Some(40.0));
        assert_eq!(native.predicted_per_completion(), Some(80.0));
        assert_eq!(native.wall_per_completion(), Some(45.0));
        let valve = &summaries[1];
        assert_eq!(
            (
                valve.sessions,
                valve.completed,
                valve.deadline_hits,
                valve.truncations
            ),
            (1, 0, 1, 1)
        );
        assert_eq!(
            valve.predicted_per_completion(),
            None,
            "no completion, no ratio"
        );
        assert_eq!(valve.metrics_incomplete, 1);
    }

    #[test]
    fn tool_call_validity_requires_every_call_valid() {
        let mut r = result("x", 2);
        assert!(tool_calls_valid(&r));
        r.tool_calls[1].known_tool = false;
        assert!(!tool_calls_valid(&r));
    }
}
