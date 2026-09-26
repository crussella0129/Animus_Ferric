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
        .map_err(|error| format!("cannot resolve lab {}: {error}", lab.display()))?;
    for repository in repositories {
        if let Ok(repository) = repository.canonicalize()
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
    fn tool_call_validity_requires_every_call_valid() {
        let mut r = result("x", 2);
        assert!(tool_calls_valid(&r));
        r.tool_calls[1].known_tool = false;
        assert!(!tool_calls_valid(&r));
    }
}
