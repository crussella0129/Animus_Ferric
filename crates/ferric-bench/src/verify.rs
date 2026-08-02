//! Trace-derived verification (T-213).
//!
//! Parse the child's JSONL trace into metrics, check the workspace against the
//! spec's expectations, check the tool-call set (required ∧ any_of ∧
//! ¬forbidden), execute trusted fixed-argv artifact checks, and compute the
//! `completed` verdict. Metrics Ferric cannot yet source (plan_steps — no
//! planner) are left `None`, flagged not faked.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ferric_trace::{Event, ParsedEvent, TraceReader};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::spec::{BenchSpec, CommandCheck, ExpectKind};

const CHECK_OUTPUT_LIMIT: usize = 16 * 1024;
const CHECK_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PYTHON_PLACEHOLDER: &str = "{python}";

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

/// A post-run executable check is either successful, evidence that the model's
/// artifact is wrong, or invalid evidence because the grading infrastructure
/// itself failed. Infrastructure errors must never be charged to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCheckStatus {
    Passed,
    ModelFailure,
    InfrastructureError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandCheckResult {
    pub name: String,
    pub status: CommandCheckStatus,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
    pub reason: Option<String>,
}

impl CommandCheckResult {
    pub fn passed(&self) -> bool {
        self.status == CommandCheckStatus::Passed
    }

    pub fn infrastructure_error(&self) -> bool {
        self.status == CommandCheckStatus::InfrastructureError
    }
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
        let record = record.map_err(std::io::Error::other)?;
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

/// Validate the trusted check definitions and the operator-selected Python
/// executable before spending model time. Absence of Python is infrastructure,
/// not a failed model trial. Levels without checks do not require Python.
pub fn preflight_command_checks(specs: &[BenchSpec], python_bin: &Path) -> Result<(), String> {
    let mut needs_python = false;
    for spec in specs {
        for check in &spec.checks {
            let Some(program) = check.argv.first() else {
                return Err(format!(
                    "L{} check `{}` has an empty argv",
                    spec.level, check.name
                ));
            };
            if check.timeout_s == 0 {
                return Err(format!(
                    "L{} check `{}` has a zero-second timeout",
                    spec.level, check.name
                ));
            }
            needs_python |= program == PYTHON_PLACEHOLDER;
            for (stream, pattern) in [
                ("stdout", check.stdout_regex.as_deref()),
                ("stderr", check.stderr_regex.as_deref()),
            ] {
                if let Some(pattern) = pattern {
                    Regex::new(pattern).map_err(|e| {
                        format!(
                            "L{} check `{}` has an invalid {stream} regex /{pattern}/: {e}",
                            spec.level, check.name
                        )
                    })?;
                }
            }
        }
    }

    if needs_python {
        let status = Command::new(python_bin)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("cannot launch Python `{}`: {e}", python_bin.display()))?;
        if !status.success() {
            return Err(format!(
                "Python `{}` failed its --version preflight with {status}",
                python_bin.display()
            ));
        }
    }
    Ok(())
}

/// Execute a spec's trusted checks in order, directly from the generated
/// workspace. No shell is involved. Checks may be stateful, so the first
/// failure stops the sequence and prevents misleading cascade failures.
pub fn verify_command_checks(
    workspace: &Path,
    spec: &BenchSpec,
    python_bin: &Path,
) -> Vec<CommandCheckResult> {
    verify_command_checks_inner(workspace, spec, python_bin, None)
}

/// Execute trusted checks within one shared wall-clock budget.
///
/// This is used by multi-segment autonomy episodes so final grading cannot
/// multiply the task-wide deadline by applying a fresh timeout to every check.
/// Exhausting the shared budget is harness infrastructure failure, not evidence
/// that the model produced an incorrect artifact.
pub fn verify_command_checks_with_deadline(
    workspace: &Path,
    spec: &BenchSpec,
    python_bin: &Path,
    budget: Duration,
) -> Vec<CommandCheckResult> {
    verify_command_checks_inner(
        workspace,
        spec,
        python_bin,
        Instant::now().checked_add(budget),
    )
}

fn verify_command_checks_inner(
    workspace: &Path,
    spec: &BenchSpec,
    python_bin: &Path,
    deadline: Option<Instant>,
) -> Vec<CommandCheckResult> {
    let mut results = Vec::with_capacity(spec.checks.len());
    for check in &spec.checks {
        let configured_timeout = Duration::from_secs(check.timeout_s);
        let (timeout, deadline_limited) = if let Some(deadline) = deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                results.push(check_infrastructure_error(
                    check,
                    "shared command-check deadline exhausted before launch".to_string(),
                ));
                break;
            }
            (
                configured_timeout.min(remaining),
                remaining < configured_timeout,
            )
        } else {
            (configured_timeout, false)
        };
        let result = run_command_check(workspace, check, python_bin, timeout, deadline_limited);
        let passed = result.passed();
        results.push(result);
        if !passed {
            break;
        }
    }
    results
}

fn run_command_check(
    workspace: &Path,
    check: &CommandCheck,
    python_bin: &Path,
    timeout: Duration,
    deadline_limited: bool,
) -> CommandCheckResult {
    let Some(program) = check.argv.first() else {
        return check_infrastructure_error(check, "empty argv".to_string());
    };
    let program = if program == PYTHON_PLACEHOLDER {
        python_bin.as_os_str()
    } else {
        std::ffi::OsStr::new(program)
    };

    let mut command = Command::new(program);
    command
        .args(&check.argv[1..])
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    ferric_core::configure_check_environment(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return check_infrastructure_error(
                check,
                format!("cannot launch `{}`: {e}", program.to_string_lossy()),
            );
        }
    };

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return check_infrastructure_error(check, "child stdout was not piped".to_string());
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return check_infrastructure_error(check, "child stderr was not piped".to_string());
    };
    let stdout_reader = std::thread::spawn(move || read_bounded(stdout, CHECK_OUTPUT_LIMIT));
    let stderr_reader = std::thread::spawn(move || read_bounded(stderr, CHECK_OUTPUT_LIMIT));

    let started = Instant::now();
    let mut timed_out = false;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(CHECK_POLL_INTERVAL),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout_reader, "stdout");
                let _ = join_reader(stderr_reader, "stderr");
                return check_infrastructure_error(check, format!("cannot poll child: {e}"));
            }
        }
    };

    let stdout = match join_reader(stdout_reader, "stdout") {
        Ok(output) => output,
        Err(e) => return check_infrastructure_error(check, e.to_string()),
    };
    let stderr = match join_reader(stderr_reader, "stderr") {
        Ok(output) => output,
        Err(e) => return check_infrastructure_error(check, e.to_string()),
    };
    let stdout_excerpt = String::from_utf8_lossy(&stdout).into_owned();
    let stderr_excerpt = String::from_utf8_lossy(&stderr).into_owned();

    if timed_out && deadline_limited {
        return CommandCheckResult {
            name: check.name.clone(),
            status: CommandCheckStatus::InfrastructureError,
            exit_code,
            timed_out,
            stdout_excerpt,
            stderr_excerpt,
            reason: Some(format!(
                "shared command-check deadline exhausted after {} ms",
                timeout.as_millis()
            )),
        };
    }

    let reason = if timed_out {
        Some(format!("timed out after {}s", check.timeout_s))
    } else if exit_code != Some(check.expected_exit) {
        Some(format!(
            "exit code {:?}, expected {}",
            exit_code, check.expected_exit
        ))
    } else if let Some(pattern) = &check.stdout_regex {
        match Regex::new(pattern) {
            Ok(regex) if !regex.is_match(&stdout_excerpt) => {
                Some(format!("stdout did not match /{pattern}/"))
            }
            Err(e) => {
                return check_infrastructure_error(
                    check,
                    format!("invalid stdout regex /{pattern}/: {e}"),
                );
            }
            _ => None,
        }
    } else {
        None
    };
    let reason = if reason.is_none() {
        if let Some(pattern) = &check.stderr_regex {
            match Regex::new(pattern) {
                Ok(regex) if !regex.is_match(&stderr_excerpt) => {
                    Some(format!("stderr did not match /{pattern}/"))
                }
                Err(e) => {
                    return check_infrastructure_error(
                        check,
                        format!("invalid stderr regex /{pattern}/: {e}"),
                    );
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        reason
    };

    CommandCheckResult {
        name: check.name.clone(),
        status: if reason.is_none() {
            CommandCheckStatus::Passed
        } else {
            CommandCheckStatus::ModelFailure
        },
        exit_code,
        timed_out,
        stdout_excerpt,
        stderr_excerpt,
        reason,
    }
}

fn check_infrastructure_error(check: &CommandCheck, reason: String) -> CommandCheckResult {
    CommandCheckResult {
        name: check.name.clone(),
        status: CommandCheckStatus::InfrastructureError,
        exit_code: None,
        timed_out: false,
        stdout_excerpt: String::new(),
        stderr_excerpt: String::new(),
        reason: Some(reason),
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

fn join_reader(handle: JoinHandle<io::Result<Vec<u8>>>, stream: &str) -> io::Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| io::Error::other(format!("{stream} reader thread panicked")))?
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
    checks: &[CommandCheckResult],
    terminator: Option<&str>,
) -> bool {
    !timed_out
        && exit_code == Some(0)
        && expectations.iter().all(|e| e.passed)
        && tools.ok()
        && checks.iter().all(CommandCheckResult::passed)
        && terminator == Some("task_complete")
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
            version: 1,
            level: 0,
            name: "t".to_string(),
            prompt: "p".to_string(),
            setup_files: Default::default(),
            expectations,
            expected_tools: expected.iter().map(|s| s.to_string()).collect(),
            any_of_tools: any_of.iter().map(|s| s.to_string()).collect(),
            forbidden_tools: forbidden.iter().map(|s| s.to_string()).collect(),
            checks: Vec::new(),
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
    fn parse_trace_fails_closed_on_malformed_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        std::fs::write(&path, "{not-json}\n").unwrap();
        let error = parse_trace(&path).unwrap_err();
        assert!(error.to_string().contains("expected") || error.to_string().contains("key"));
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
            &[],
            Some("task_complete")
        ));
        // timed out
        assert!(!completed(
            true,
            Some(0),
            &pass_expect,
            &ok_tools,
            &[],
            Some("task_complete")
        ));
        // nonzero exit
        assert!(!completed(
            false,
            Some(1),
            &pass_expect,
            &ok_tools,
            &[],
            Some("task_complete")
        ));
        // bad terminator
        assert!(!completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            &[],
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
            &[],
            Some("task_complete")
        ));

        // Exact structured completion is required; missing/final-text endings
        // are not valid ladder completions.
        assert!(!completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            &[],
            None
        ));
        assert!(!completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            &[],
            Some("final_text")
        ));

        let failed_check = CommandCheckResult {
            name: "grade".to_string(),
            status: CommandCheckStatus::ModelFailure,
            exit_code: Some(1),
            timed_out: false,
            stdout_excerpt: String::new(),
            stderr_excerpt: String::new(),
            reason: Some("bad artifact".to_string()),
        };
        assert!(!completed(
            false,
            Some(0),
            &pass_expect,
            &ok_tools,
            &[failed_check],
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

    fn command_check(argv: Vec<String>) -> CommandCheck {
        CommandCheck {
            name: "grade".to_string(),
            argv,
            expected_exit: 0,
            stdout_regex: None,
            stderr_regex: None,
            timeout_s: 5,
        }
    }

    #[test]
    fn fixed_argv_check_classifies_pass_model_failure_and_infrastructure() {
        let workspace = tempfile::tempdir().unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let python = current_exe.clone();

        let mut spec = spec_with(&[], &[], &[], vec![]);
        let mut passing = command_check(vec!["{python}".to_string(), "--list".to_string()]);
        passing.stdout_regex = Some("completed_truth_table".to_string());
        spec.checks = vec![passing];
        let passed = verify_command_checks(workspace.path(), &spec, &python);
        assert_eq!(passed.len(), 1);
        assert_eq!(passed[0].status, CommandCheckStatus::Passed);

        let mut wrong_exit = command_check(vec![
            current_exe.display().to_string(),
            "--list".to_string(),
        ]);
        wrong_exit.expected_exit = 99;
        spec.checks = vec![wrong_exit];
        let failed = verify_command_checks(workspace.path(), &spec, &python);
        assert_eq!(failed[0].status, CommandCheckStatus::ModelFailure);
        assert!(failed[0].reason.as_deref().unwrap().contains("exit code"));

        spec.checks = vec![command_check(vec![
            workspace
                .path()
                .join("definitely-missing-check-program")
                .display()
                .to_string(),
        ])];
        let infrastructure = verify_command_checks(workspace.path(), &spec, &python);
        assert_eq!(
            infrastructure[0].status,
            CommandCheckStatus::InfrastructureError
        );
    }

    #[test]
    fn command_check_timeout_is_a_model_failure() {
        let workspace = tempfile::tempdir().unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let mut check = command_check(vec![
            current_exe.display().to_string(),
            "--ignored".to_string(),
            "--exact".to_string(),
            "verify::tests::command_check_sleep_fixture".to_string(),
        ]);
        check.timeout_s = 1;
        let mut spec = spec_with(&[], &[], &[], vec![]);
        spec.checks = vec![check];

        let result = verify_command_checks(workspace.path(), &spec, &current_exe);
        assert_eq!(result[0].status, CommandCheckStatus::ModelFailure);
        assert!(result[0].timed_out);
    }

    #[test]
    fn shared_check_deadline_is_an_infrastructure_failure() {
        let workspace = tempfile::tempdir().unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let check = command_check(vec![
            current_exe.display().to_string(),
            "--ignored".to_string(),
            "--exact".to_string(),
            "verify::tests::command_check_sleep_fixture".to_string(),
        ]);
        let mut spec = spec_with(&[], &[], &[], vec![]);
        spec.checks = vec![check];

        let result = verify_command_checks_with_deadline(
            workspace.path(),
            &spec,
            &current_exe,
            Duration::from_millis(100),
        );
        assert_eq!(result[0].status, CommandCheckStatus::InfrastructureError);
        assert!(result[0].timed_out);
        assert!(
            result[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("shared command-check deadline")
        );
    }

    #[test]
    fn command_check_output_is_bounded_while_pipes_are_drained() {
        let workspace = tempfile::tempdir().unwrap();
        let current_exe = std::env::current_exe().unwrap();
        let check = command_check(vec![
            current_exe.display().to_string(),
            "--ignored".to_string(),
            "--exact".to_string(),
            "verify::tests::command_check_noisy_fixture".to_string(),
            "--nocapture".to_string(),
        ]);
        let mut spec = spec_with(&[], &[], &[], vec![]);
        spec.checks = vec![check];

        let result = verify_command_checks(workspace.path(), &spec, &current_exe);
        assert_eq!(result[0].status, CommandCheckStatus::Passed);
        assert!(result[0].stdout_excerpt.len() <= CHECK_OUTPUT_LIMIT);
    }

    #[test]
    fn preflight_rejects_bad_regex_and_missing_python() {
        let mut spec = spec_with(&[], &[], &[], vec![]);
        let mut bad_regex = command_check(vec!["{python}".to_string()]);
        bad_regex.stdout_regex = Some("(".to_string());
        spec.checks = vec![bad_regex];
        assert!(preflight_command_checks(&[spec.clone()], Path::new("python")).is_err());

        spec.checks[0].stdout_regex = None;
        let missing = tempfile::tempdir().unwrap().path().join("missing-python");
        assert!(preflight_command_checks(&[spec], &missing).is_err());
    }

    #[test]
    fn authoritative_l3_l6_python_checks_accept_known_good_artifacts() {
        let Some(python) = available_python() else {
            eprintln!("python is unavailable; executable spec fixture test skipped");
            return;
        };
        let specs = crate::spec::embedded_specs().unwrap();

        let l3 = tempfile::tempdir().unwrap();
        std::fs::write(
            l3.path().join("greet.py"),
            "def hello():\n    return 'world'\n",
        )
        .unwrap();
        assert_checks_pass(l3.path(), &specs[3], &python);

        let l4 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(l4.path().join("tests")).unwrap();
        std::fs::write(
            l4.path().join("greet.py"),
            "def hello():\n    return 'world'\n",
        )
        .unwrap();
        std::fs::write(
            l4.path().join("tests/test_greet.py"),
            "import unittest\nimport greet\n\nclass GreetTest(unittest.TestCase):\n    def test_hello(self):\n        self.assertEqual(greet.hello(), 'world')\n",
        )
        .unwrap();
        assert_checks_pass(l4.path(), &specs[4], &python);

        let l5 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(l5.path().join("tests")).unwrap();
        std::fs::write(
            l5.path().join("cli.py"),
            r#"import argparse

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--name", required=True)
    args = parser.parse_args()
    print(f"Hello, {args.name}!")

if __name__ == "__main__":
    main()
"#,
        )
        .unwrap();
        std::fs::write(
            l5.path().join("tests/test_cli.py"),
            r#"import subprocess
import sys
import unittest

class CliTest(unittest.TestCase):
    def test_cli(self):
        output = subprocess.check_output(
            [sys.executable, "cli.py", "--name", "Test"], text=True
        )
        self.assertEqual(output.strip(), "Hello, Test!")
"#,
        )
        .unwrap();
        assert_checks_pass(l5.path(), &specs[5], &python);

        let l6 = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(l6.path().join("tests")).unwrap();
        std::fs::write(
            l6.path().join("todo.py"),
            r#"import argparse
import json
from pathlib import Path

STORE = Path("todos.json")

def load():
    return json.loads(STORE.read_text()) if STORE.exists() else []

def save(items):
    STORE.write_text(json.dumps(items))

parser = argparse.ArgumentParser()
commands = parser.add_subparsers(dest="command", required=True)
add = commands.add_parser("add")
add.add_argument("text")
commands.add_parser("list")
done = commands.add_parser("done")
done.add_argument("id", type=int)
args = parser.parse_args()
items = load()

if args.command == "add":
    items.append({"id": len(items) + 1, "text": args.text, "done": False})
    save(items)
elif args.command == "done":
    for item in items:
        if item["id"] == args.id:
            item["done"] = True
    save(items)
else:
    for item in items:
        marker = "x" if item["done"] else " "
        print(f"{item['id']} [{marker}] {item['text']}")
"#,
        )
        .unwrap();
        std::fs::write(
            l6.path().join("tests/test_todo.py"),
            "import unittest\n\nclass TodoTest(unittest.TestCase):\n    def test_fixture(self):\n        self.assertTrue(True)\n",
        )
        .unwrap();
        assert_checks_pass(l6.path(), &specs[6], &python);
    }

    #[test]
    fn authoritative_check_rejects_semantically_wrong_python() {
        let Some(python) = available_python() else {
            eprintln!("python is unavailable; executable spec fixture test skipped");
            return;
        };
        let spec = crate::spec::embedded_specs().unwrap().remove(3);
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(
            workspace.path().join("greet.py"),
            "def hello():\n    return 'universe'\n",
        )
        .unwrap();

        let results = verify_command_checks(workspace.path(), &spec, &python);
        assert_eq!(
            results.last().unwrap().status,
            CommandCheckStatus::ModelFailure
        );
        assert!(
            results
                .last()
                .unwrap()
                .reason
                .as_deref()
                .unwrap()
                .contains("exit code")
        );
    }

    fn available_python() -> Option<std::path::PathBuf> {
        let python = std::env::var_os("FERRIC_TEST_PYTHON")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("python"));
        Command::new(&python)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .filter(|status| status.success())
            .map(|_| python)
    }

    fn assert_checks_pass(workspace: &Path, spec: &BenchSpec, python: &Path) {
        let results = verify_command_checks(workspace, spec, python);
        assert_eq!(
            results.len(),
            spec.checks.len(),
            "L{} stopped early: {results:#?}",
            spec.level
        );
        assert!(
            results.iter().all(CommandCheckResult::passed),
            "L{} executable grading failed: {results:#?}",
            spec.level
        );
    }

    #[test]
    #[ignore = "test-only child process fixture"]
    fn command_check_sleep_fixture() {
        std::thread::sleep(Duration::from_secs(5));
    }

    #[test]
    #[ignore = "test-only child process fixture"]
    fn command_check_noisy_fixture() {
        println!("{}", "x".repeat(CHECK_OUTPUT_LIMIT * 8));
    }
}
