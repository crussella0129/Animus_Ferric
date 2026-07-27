//! Trace-derived verification (T-213).
//!
//! Parse the child's JSONL trace into metrics, check the workspace against the
//! spec's expectations, check the tool-call set (required ∧ any_of ∧
//! ¬forbidden), and compute the `completed` verdict. Metrics Ferric cannot
//! yet source (plan_steps — no planner) are left `None`, flagged not faked.

use std::path::Path;

use ferric_trace::{Event, ParsedEvent, TraceReader};
use regex::Regex;

use crate::spec::{BenchSpec, ExpectKind};

/// Metrics read out of one trace.
#[derive(Debug, Default, Clone)]
pub struct TraceMetrics {
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub terminator: Option<String>,
    pub tier: Option<String>,
    pub protocol: Option<String>,
    pub repetition_guard_fires: u32,
    pub tools_called: Vec<String>,
    pub task_complete_summary: Option<String>,
    /// No planner yet (ADR-019): recorded null, not faked.
    pub plan_steps: Option<u32>,
}

/// One file/dir/missing check result.
#[derive(Debug, Clone)]
pub struct ExpectationResult {
    pub path: String,
    pub passed: bool,
    pub reason: Option<String>,
}

/// The tool-call verdict detail.
#[derive(Debug, Clone, Default)]
pub struct ToolVerdict {
    pub missing_required: Vec<String>,
    pub missing_any_of: bool,
    pub used_forbidden: Vec<String>,
}

impl ToolVerdict {
    pub fn ok(&self) -> bool {
        self.missing_required.is_empty() && !self.missing_any_of && self.used_forbidden.is_empty()
    }
}

/// Phrases that betray a "completed" run that actually failed (lineage H21).
const FAILURE_PHRASES: &[&str] = &[
    "could not",
    "couldn't",
    "unable to",
    "cannot",
    "can't",
    "failed to",
    "not found",
    "does not exist",
    "doesn't exist",
    "gave up",
    "i don't know",
];

pub fn parse_trace(path: &Path) -> std::io::Result<TraceMetrics> {
    let mut m = TraceMetrics::default();
    let reader = TraceReader::open(path).map_err(std::io::Error::other)?;
    for record in reader {
        let record = match record {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let ParsedEvent::Known(event) = record.event {
            match event {
                Event::TurnStart { .. } => m.turns += 1,
                Event::TurnEnd {
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    m.input_tokens += u64::from(input_tokens.unwrap_or(0));
                    m.output_tokens += u64::from(output_tokens.unwrap_or(0));
                }
                Event::PolicySelected { tier, protocol, .. } => {
                    m.tier = Some(format!("{tier:?}"));
                    m.protocol = Some(ferric_core::protocol_key(protocol));
                }
                Event::SessionEnd { reason } => {
                    // `task_complete` is a structured terminator (ADR-013),
                    // recorded as SessionEnd rather than a dispatched ToolCall —
                    // but the model *did* emit it, so credit it as a called tool
                    // so specs listing it in `expected_tools` verify correctly.
                    if reason == "task_complete" {
                        m.tools_called.push(reason.clone());
                    }
                    m.terminator = Some(reason);
                }
                Event::RepetitionGuard { action } if action == "stopped" => {
                    m.repetition_guard_fires += 1
                }
                Event::ToolCall { name, args, .. } => {
                    if name == "task_complete" {
                        m.task_complete_summary = args
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                    }
                    m.tools_called.push(name);
                }
                _ => {}
            }
        }
    }
    Ok(m)
}

/// Did the model admit failure in its task_complete summary?
pub fn failure_admission(metrics: &TraceMetrics) -> Option<String> {
    let summary = metrics
        .task_complete_summary
        .as_deref()?
        .to_ascii_lowercase();
    FAILURE_PHRASES
        .iter()
        .find(|p| summary.contains(**p))
        .map(|p| p.to_string())
}

pub fn verify_expectations(workspace: &Path, spec: &BenchSpec) -> Vec<ExpectationResult> {
    spec.expectations
        .iter()
        .map(|exp| {
            let full = workspace.join(&exp.path);
            let (passed, reason) = match exp.kind {
                ExpectKind::File => check_file(&full, exp.content_regex.as_deref()),
                ExpectKind::Dir => (
                    full.is_dir(),
                    (!full.is_dir()).then(|| "not a directory".to_string()),
                ),
                ExpectKind::Missing => (
                    !full.exists(),
                    full.exists().then(|| "should not exist".to_string()),
                ),
            };
            ExpectationResult {
                path: exp.path.clone(),
                passed,
                reason,
            }
        })
        .collect()
}

fn check_file(path: &Path, content_regex: Option<&str>) -> (bool, Option<String>) {
    if !path.is_file() {
        return (false, Some("file does not exist".to_string()));
    }
    let Some(pattern) = content_regex else {
        return (true, None);
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return (false, Some(format!("read error: {e}"))),
    };
    match Regex::new(pattern) {
        Ok(re) if re.is_match(&text) => (true, None),
        Ok(_) => (false, Some(format!("content did not match /{pattern}/"))),
        Err(e) => (false, Some(format!("bad regex /{pattern}/: {e}"))),
    }
}

pub fn verify_tools(metrics: &TraceMetrics, spec: &BenchSpec) -> ToolVerdict {
    let called: std::collections::BTreeSet<&str> =
        metrics.tools_called.iter().map(String::as_str).collect();
    ToolVerdict {
        missing_required: spec
            .expected_tools
            .iter()
            .filter(|t| !called.contains(t.as_str()))
            .cloned()
            .collect(),
        missing_any_of: !spec.any_of_tools.is_empty()
            && !spec
                .any_of_tools
                .iter()
                .any(|t| called.contains(t.as_str())),
        used_forbidden: spec
            .forbidden_tools
            .iter()
            .filter(|t| called.contains(t.as_str()))
            .cloned()
            .collect(),
    }
}

/// The overall pass/fail for one run.
pub fn completed(
    timed_out: bool,
    exit_code: Option<i32>,
    expectations: &[ExpectationResult],
    tools: &ToolVerdict,
    terminator: Option<&str>,
) -> bool {
    !timed_out
        && exit_code == Some(0)
        && expectations.iter().all(|e| e.passed)
        && tools.ok()
        && matches!(
            terminator,
            None | Some("task_complete") | Some("final_text")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::Expectation;

    fn spec_with(
        expected: &[&str],
        any_of: &[&str],
        forbidden: &[&str],
        expectations: Vec<Expectation>,
    ) -> BenchSpec {
        BenchSpec {
            level: 0,
            name: "t".to_string(),
            prompt: "p".to_string(),
            setup_files: Default::default(),
            expectations,
            expected_tools: expected.iter().map(|s| s.to_string()).collect(),
            any_of_tools: any_of.iter().map(|s| s.to_string()).collect(),
            forbidden_tools: forbidden.iter().map(|s| s.to_string()).collect(),
            max_turns: 5,
            timeout_s: 60,
        }
    }

    fn metrics_with(tools: &[&str], terminator: &str) -> TraceMetrics {
        TraceMetrics {
            terminator: Some(terminator.to_string()),
            tools_called: tools.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn tools_verdict_matrix() {
        let spec = spec_with(&["task_complete"], &["list_dir"], &["write_file"], vec![]);
        // all good
        let ok = verify_tools(
            &metrics_with(&["list_dir", "task_complete"], "task_complete"),
            &spec,
        );
        assert!(ok.ok());
        // missing required
        let v = verify_tools(&metrics_with(&["list_dir"], "task_complete"), &spec);
        assert_eq!(v.missing_required, vec!["task_complete"]);
        // missing any_of
        let v = verify_tools(&metrics_with(&["task_complete"], "task_complete"), &spec);
        assert!(v.missing_any_of);
        // used forbidden
        let v = verify_tools(
            &metrics_with(
                &["list_dir", "task_complete", "write_file"],
                "task_complete",
            ),
            &spec,
        );
        assert_eq!(v.used_forbidden, vec!["write_file"]);
    }

    #[test]
    fn completed_truth_table() {
        let pass_expect = vec![ExpectationResult {
            path: "x".to_string(),
            passed: true,
            reason: None,
        }];
        let ok_tools = ToolVerdict::default();
        // happy path
        assert!(completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            Some("task_complete")
        ));
        // timed out
        assert!(!completed(
            true,
            Some(0),
            &pass_expect,
            &ok_tools,
            Some("task_complete")
        ));
        // nonzero exit
        assert!(!completed(
            false,
            Some(1),
            &pass_expect,
            &ok_tools,
            Some("task_complete")
        ));
        // bad terminator
        assert!(!completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            Some("max_turns")
        ));
        // failed expectation
        let fail_expect = vec![ExpectationResult {
            path: "x".to_string(),
            passed: false,
            reason: Some("nope".to_string()),
        }];
        assert!(!completed(
            false,
            Some(0),
            &fail_expect,
            &ok_tools,
            Some("task_complete")
        ));
    }

    #[test]
    fn failure_admission_detects_phrases() {
        let with = |summary: &str| TraceMetrics {
            task_complete_summary: Some(summary.to_string()),
            ..Default::default()
        };
        assert_eq!(
            failure_admission(&with("I could not find the file")).as_deref(),
            Some("could not")
        );
        assert_eq!(
            failure_admission(&with("Wrote greet.py successfully")),
            None
        );
    }
}
