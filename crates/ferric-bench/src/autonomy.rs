//! Versioned internal autonomy-evaluation corpus.
//!
//! This is an internal regression baseline, not an external benchmark and not
//! evidence of general coding-agent reliability. The schema deliberately keeps
//! repository setup, runner-driven recovery segmentation, terminal outcomes,
//! and trusted fixed-argv grading contracts in one reviewable artifact.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::spec::{BenchSpec, CommandCheck};

pub const AUTONOMY_SCHEMA_VERSION: u32 = 1;
pub const AUTONOMY_TASK_COUNT: usize = 24;
pub const TASKS_PER_CATEGORY: usize = 8;

/// The frozen version-one corpus. Changing any task or grading contract
/// requires a new schema/corpus version rather than editing historical results.
pub const EMBEDDED_AUTONOMY_V1: &str = include_str!("../autonomy/v1.toml");

/// Convert a frozen autonomy task into the ordinary fixed-argv grading spec
/// used both in-loop and after the final trace.
pub fn autonomy_bench_spec(task: &AutonomyTask, version: u32) -> BenchSpec {
    BenchSpec {
        version,
        level: 0,
        name: task.name.clone(),
        prompt: task.prompt.clone(),
        setup_files: task.setup_files.clone(),
        expectations: Vec::new(),
        expected_tools: vec!["task_complete".to_string()],
        any_of_tools: Vec::new(),
        forbidden_tools: Vec::new(),
        checks: task
            .checks
            .iter()
            .map(|check| CommandCheck {
                name: check.name.clone(),
                argv: check.argv.clone(),
                expected_exit: check.expected_exit,
                stdout_regex: check.stdout_regex.clone(),
                stderr_regex: check.stderr_regex.clone(),
                timeout_s: check.timeout_s,
            })
            .collect(),
        max_turns: task.max_turns,
        timeout_s: task.timeout_s,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutonomySuite {
    pub schema_version: u32,
    pub suite_id: String,
    pub name: String,
    pub scope: BenchmarkScope,
    pub description: String,
    pub policy_variants: Vec<PolicyVariant>,
    pub tasks: Vec<AutonomyTask>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkScope {
    InternalBaseline,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVariant {
    Current,
    Recovery,
    RepositoryBrief,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyTask {
    pub id: String,
    pub name: String,
    pub category: AutonomyCategory,
    pub prompt: String,
    pub max_turns: u32,
    pub timeout_s: u64,
    #[serde(default)]
    pub setup_files: BTreeMap<String, String>,
    pub terminal: TerminalExpectation,
    #[serde(default)]
    pub clarification: Option<ClarificationContract>,
    #[serde(default)]
    pub recovery: Option<RecoveryContract>,
    pub checks: Vec<AutonomyCheck>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyCategory {
    Ambiguity,
    Recovery,
    LongHorizon,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerminalExpectation {
    /// One-shot expectation for the `current` policy variant.
    pub current: TerminalOutcome,
    /// One expected outcome per process segment for both the `recovery` and
    /// `repository_brief` variants. Keeping the sequence identical isolates
    /// the repository brief as the only A/B difference.
    pub resumable_outcomes: Vec<TerminalOutcome>,
    /// Expected outcome when a paused trace is resumed from another workspace.
    #[serde(default)]
    pub workspace_mismatch: Option<TerminalOutcome>,
    /// Expected outcome when a completed trace is resumed.
    #[serde(default)]
    pub completed_session_resume: Option<TerminalOutcome>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Completed,
    NeedsInput,
    Paused,
    ResumeRejected,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClarificationContract {
    /// Whether a safe agent must pause rather than infer the missing decision.
    pub required: bool,
    /// Deterministic answer supplied by the evaluation runner after a pause.
    #[serde(default)]
    pub answer: Option<String>,
    /// Case-insensitive terms of which at least one must occur in the question.
    #[serde(default)]
    pub expected_question_terms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryContract {
    /// Turn allowance for each injected pre-resume process segment. The final
    /// completing segment receives the task's full `max_turns` budget.
    pub segment_turns: u32,
    /// Maximum number of process segments, including the initial segment.
    pub max_segments: u32,
    /// Number of successful continuation operations expected before completion.
    pub expected_resume_count: u32,
    /// Optional answer used when an injected segment ends in `needs_input`.
    #[serde(default)]
    pub answer: Option<String>,
    /// Case-insensitive terms of which at least one must occur in an injected
    /// clarification question before the frozen answer may be supplied.
    #[serde(default)]
    pub expected_question_terms: Vec<String>,
    #[serde(default)]
    pub refusal_modes: Vec<ResumeRefusalMode>,
    #[serde(default)]
    pub injections: Vec<RecoveryInjection>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ResumeRefusalMode {
    WorkspaceMismatch,
    CompletedSession,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInjection {
    /// One-based process segment in which the harness injects the stop.
    pub segment: u32,
    pub kind: RecoveryInjectionKind,
    /// Required only for `process_crash`; absent for logical pause injection.
    #[serde(default)]
    pub point: Option<CrashPoint>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryInjectionKind {
    ProcessCrash,
    ProviderFailure,
    BudgetExhaustion,
    GuardPause,
    ClarificationPause,
}

impl RecoveryInjectionKind {
    /// Exact trace stop reason required for a logical injection. A true
    /// process crash has no terminal trace event, so its reason is `None`.
    pub const fn expected_stop_reason(self) -> Option<&'static str> {
        match self {
            Self::ProcessCrash => None,
            Self::ProviderFailure => Some("provider_error"),
            Self::BudgetExhaustion => Some("max_turns"),
            Self::GuardPause => Some("repetition_guard"),
            Self::ClarificationPause => Some("needs_input"),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrashPoint {
    BeforeDispatch,
    AfterMutation,
    AfterToolResult,
    AfterTurnCommitted,
}

/// A trusted check launched directly without a shell. `argv[0]` is a literal
/// executable or the sole supported executable placeholder, `{python}`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutonomyCheck {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub expected_exit: i32,
    #[serde(default)]
    pub stdout_regex: Option<String>,
    #[serde(default)]
    pub stderr_regex: Option<String>,
    #[serde(default = "default_check_timeout_s")]
    pub timeout_s: u64,
}

const fn default_check_timeout_s() -> u64 {
    30
}

#[derive(Debug)]
pub enum AutonomySuiteError {
    Parse(toml::de::Error),
    Validation(String),
}

impl fmt::Display for AutonomySuiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "cannot parse autonomy suite: {error}"),
            Self::Validation(error) => write!(formatter, "invalid autonomy suite: {error}"),
        }
    }
}

impl std::error::Error for AutonomySuiteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

/// Parse and validate a suite. Callers cannot accidentally run a syntactically
/// valid but statistically incomplete matrix.
pub fn parse_autonomy_suite(source: &str) -> Result<AutonomySuite, AutonomySuiteError> {
    let suite = toml::from_str(source).map_err(AutonomySuiteError::Parse)?;
    validate_autonomy_suite(&suite)?;
    Ok(suite)
}

pub fn embedded_autonomy_suite() -> Result<AutonomySuite, AutonomySuiteError> {
    parse_autonomy_suite(EMBEDDED_AUTONOMY_V1)
}

pub fn validate_autonomy_suite(suite: &AutonomySuite) -> Result<(), AutonomySuiteError> {
    if suite.schema_version != AUTONOMY_SCHEMA_VERSION {
        return invalid(format!(
            "schema_version must be {AUTONOMY_SCHEMA_VERSION}, got {}",
            suite.schema_version
        ));
    }
    require_text("suite_id", &suite.suite_id)?;
    require_text("name", &suite.name)?;
    require_text("description", &suite.description)?;
    if suite.scope != BenchmarkScope::InternalBaseline {
        return invalid("scope must remain internal_baseline");
    }

    let variants: BTreeSet<_> = suite.policy_variants.iter().copied().collect();
    let required_variants = BTreeSet::from([
        PolicyVariant::Current,
        PolicyVariant::Recovery,
        PolicyVariant::RepositoryBrief,
    ]);
    if variants != required_variants || suite.policy_variants.len() != required_variants.len() {
        return invalid(
            "policy_variants must contain current, recovery, and repository_brief once",
        );
    }
    if suite.tasks.len() != AUTONOMY_TASK_COUNT {
        return invalid(format!(
            "suite must contain exactly {AUTONOMY_TASK_COUNT} tasks, got {}",
            suite.tasks.len()
        ));
    }

    let mut ids = BTreeSet::new();
    let mut counts = BTreeMap::new();
    for task in &suite.tasks {
        validate_task(task)?;
        if !ids.insert(task.id.as_str()) {
            return invalid(format!("duplicate task id `{}`", task.id));
        }
        *counts.entry(task.category).or_insert(0_usize) += 1;
    }
    for category in [
        AutonomyCategory::Ambiguity,
        AutonomyCategory::Recovery,
        AutonomyCategory::LongHorizon,
    ] {
        let count = counts.get(&category).copied().unwrap_or_default();
        if count != TASKS_PER_CATEGORY {
            return invalid(format!(
                "category {category:?} must contain exactly {TASKS_PER_CATEGORY} tasks, got {count}"
            ));
        }
    }
    Ok(())
}

fn validate_task(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    require_text("task id", &task.id)?;
    require_text(&format!("{} name", task.id), &task.name)?;
    require_text(&format!("{} prompt", task.id), &task.prompt)?;
    validate_task_id(task)?;
    if task.max_turns == 0 || task.max_turns > u32::from(u8::MAX) {
        return invalid(format!(
            "{} max_turns must fit the child CLI range 1..=255",
            task.id
        ));
    }
    if task.timeout_s == 0 {
        return invalid(format!("{} timeout_s must be positive", task.id));
    }
    if task.setup_files.is_empty() {
        return invalid(format!(
            "{} must materialize at least one setup file",
            task.id
        ));
    }
    for path in task.setup_files.keys() {
        validate_relative_path(&task.id, path)?;
    }
    validate_path_collisions(task)?;
    if task.terminal.resumable_outcomes.is_empty() {
        return invalid(format!(
            "{} must declare at least one resumable terminal outcome",
            task.id
        ));
    }
    if task.checks.is_empty() {
        return invalid(format!("{} must declare an authoritative check", task.id));
    }
    validate_checks(task)?;

    match task.category {
        AutonomyCategory::Ambiguity => validate_ambiguity_task(task),
        AutonomyCategory::Recovery => validate_recovery_task(task),
        AutonomyCategory::LongHorizon => validate_long_horizon_task(task),
    }
}

fn validate_task_id(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    let prefix = match task.category {
        AutonomyCategory::Ambiguity => "A",
        AutonomyCategory::Recovery => "R",
        AutonomyCategory::LongHorizon => "H",
    };
    let Some(number) = task.id.strip_prefix(prefix) else {
        return invalid(format!(
            "{} id must start with category prefix {prefix}",
            task.id
        ));
    };
    if number.len() != 2 || !number.bytes().all(|byte| byte.is_ascii_digit()) || number == "00" {
        return invalid(format!(
            "{} id must use a non-zero two-digit suffix (for example {prefix}01)",
            task.id
        ));
    }
    Ok(())
}

fn validate_ambiguity_task(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    if task.recovery.is_some() {
        return invalid(format!(
            "{} ambiguity task cannot declare recovery metadata",
            task.id
        ));
    }
    let Some(contract) = &task.clarification else {
        return invalid(format!(
            "{} ambiguity task must declare a clarification contract",
            task.id
        ));
    };
    if contract.required {
        if contract
            .answer
            .as_deref()
            .is_none_or(|answer| answer.trim().is_empty())
        {
            return invalid(format!(
                "{} required clarification must provide a deterministic answer",
                task.id
            ));
        }
        if contract.expected_question_terms.is_empty()
            || contract
                .expected_question_terms
                .iter()
                .any(|term| term.trim().is_empty())
        {
            return invalid(format!(
                "{} required clarification must provide non-empty question terms",
                task.id
            ));
        }
        if task.terminal.current != TerminalOutcome::NeedsInput
            || task.terminal.resumable_outcomes
                != [TerminalOutcome::NeedsInput, TerminalOutcome::Completed]
        {
            return invalid(format!(
                "{} required clarification must be needs_input in current and needs_input then completed when resumable",
                task.id
            ));
        }
    } else {
        if contract.answer.is_some() || !contract.expected_question_terms.is_empty() {
            return invalid(format!(
                "{} unneeded clarification cannot define an answer or question terms",
                task.id
            ));
        }
        if task.terminal.current != TerminalOutcome::Completed
            || task.terminal.resumable_outcomes != [TerminalOutcome::Completed]
        {
            return invalid(format!(
                "{} unambiguous task must complete in one segment",
                task.id
            ));
        }
    }
    reject_refusal_expectations(task)
}

fn validate_recovery_task(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    if task.clarification.is_some() {
        return invalid(format!(
            "{} recovery task stores any injected answer in recovery metadata",
            task.id
        ));
    }
    let Some(recovery) = &task.recovery else {
        return invalid(format!(
            "{} recovery task must declare recovery metadata",
            task.id
        ));
    };
    if recovery.segment_turns == 0
        || recovery.segment_turns > u32::from(u8::MAX)
        || recovery.segment_turns > task.max_turns
        || recovery.max_segments < 2
    {
        return invalid(format!(
            "{} recovery segment_turns must fit 1..=max_turns and max_segments must be at least two",
            task.id
        ));
    }
    if recovery.max_segments as usize != task.terminal.resumable_outcomes.len() {
        return invalid(format!(
            "{} max_segments must equal the number of terminal outcomes",
            task.id
        ));
    }
    if recovery.expected_resume_count != recovery.max_segments - 1 {
        return invalid(format!(
            "{} expected_resume_count must equal max_segments - 1",
            task.id
        ));
    }
    if task.terminal.resumable_outcomes.last() != Some(&TerminalOutcome::Completed)
        || task.terminal.resumable_outcomes[..task.terminal.resumable_outcomes.len() - 1]
            .contains(&TerminalOutcome::Completed)
    {
        return invalid(format!(
            "{} recovery outcomes must end in one completed segment",
            task.id
        ));
    }
    if recovery.injections.len() != recovery.expected_resume_count as usize {
        return invalid(format!(
            "{} must define one injection for every expected resume",
            task.id
        ));
    }

    let mut segments = BTreeSet::new();
    let mut needs_answer = false;
    for injection in &recovery.injections {
        if injection.segment == 0 || injection.segment >= recovery.max_segments {
            return invalid(format!(
                "{} injection segment must precede the final segment",
                task.id
            ));
        }
        if !segments.insert(injection.segment) {
            return invalid(format!(
                "{} repeats recovery injection segment {}",
                task.id, injection.segment
            ));
        }
        match injection.kind {
            RecoveryInjectionKind::ProcessCrash if injection.point.is_none() => {
                return invalid(format!(
                    "{} process_crash injection requires a crash point",
                    task.id
                ));
            }
            RecoveryInjectionKind::ProcessCrash => {}
            _ if injection.point.is_some() => {
                return invalid(format!(
                    "{} non-crash injection cannot declare a crash point",
                    task.id
                ));
            }
            RecoveryInjectionKind::ClarificationPause => needs_answer = true,
            _ => {}
        }
        let expected = match injection.kind {
            RecoveryInjectionKind::ClarificationPause => TerminalOutcome::NeedsInput,
            _ => TerminalOutcome::Paused,
        };
        if task.terminal.resumable_outcomes[(injection.segment - 1) as usize] != expected {
            return invalid(format!(
                "{} segment {} outcome does not match its injection",
                task.id, injection.segment
            ));
        }
    }
    if needs_answer
        != recovery
            .answer
            .as_deref()
            .is_some_and(|answer| !answer.trim().is_empty())
    {
        return invalid(format!(
            "{} clarification_pause and non-empty recovery answer must occur together",
            task.id
        ));
    }
    let has_question_terms = !recovery.expected_question_terms.is_empty();
    let valid_question_terms = recovery
        .expected_question_terms
        .iter()
        .all(|term| !term.trim().is_empty());
    if needs_answer != has_question_terms || !valid_question_terms {
        return invalid(format!(
            "{} clarification_pause and non-empty recovery question terms must occur together",
            task.id
        ));
    }
    if task.terminal.current != task.terminal.resumable_outcomes[0] {
        return invalid(format!(
            "{} current recovery outcome must equal its first injected stop",
            task.id
        ));
    }

    let refusal_modes: BTreeSet<_> = recovery.refusal_modes.iter().copied().collect();
    if refusal_modes.len() != recovery.refusal_modes.len() {
        return invalid(format!("{} repeats a refusal mode", task.id));
    }
    validate_refusal(
        task,
        ResumeRefusalMode::WorkspaceMismatch,
        task.terminal.workspace_mismatch,
        &refusal_modes,
    )?;
    validate_refusal(
        task,
        ResumeRefusalMode::CompletedSession,
        task.terminal.completed_session_resume,
        &refusal_modes,
    )
}

fn validate_refusal(
    task: &AutonomyTask,
    mode: ResumeRefusalMode,
    outcome: Option<TerminalOutcome>,
    modes: &BTreeSet<ResumeRefusalMode>,
) -> Result<(), AutonomySuiteError> {
    match (modes.contains(&mode), outcome) {
        (true, Some(TerminalOutcome::ResumeRejected)) | (false, None) => Ok(()),
        (true, _) => invalid(format!(
            "{} requested {mode:?} refusal must expect resume_rejected",
            task.id
        )),
        (false, Some(_)) => invalid(format!(
            "{} has a {mode:?} outcome without requesting that probe",
            task.id
        )),
    }
}

fn validate_long_horizon_task(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    if task.clarification.is_some() || task.recovery.is_some() {
        return invalid(format!(
            "{} long-horizon task cannot declare clarification or recovery metadata",
            task.id
        ));
    }
    if task.terminal.current != TerminalOutcome::Completed
        || task.terminal.resumable_outcomes != [TerminalOutcome::Completed]
    {
        return invalid(format!(
            "{} long-horizon task must complete in one process segment",
            task.id
        ));
    }
    reject_refusal_expectations(task)
}

fn reject_refusal_expectations(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    if task.terminal.workspace_mismatch.is_some()
        || task.terminal.completed_session_resume.is_some()
    {
        return invalid(format!(
            "{} non-recovery task cannot declare resume-refusal outcomes",
            task.id
        ));
    }
    Ok(())
}

fn validate_checks(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    let mut names = BTreeSet::new();
    for check in &task.checks {
        if check.name.is_empty()
            || check.name.len() > 64
            || !check
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return invalid(format!(
                "{} check name `{}` must use 1-64 ASCII letters, digits, '-' or '_'",
                task.id, check.name
            ));
        }
        if !names.insert(check.name.as_str()) {
            return invalid(format!("{} repeats check name `{}`", task.id, check.name));
        }
        let Some(program) = check.argv.first() else {
            return invalid(format!("{} check `{}` has empty argv", task.id, check.name));
        };
        if check.argv.len() > 128 || check.argv.iter().any(|argument| argument.contains('\0')) {
            return invalid(format!(
                "{} check `{}` has more than 128 argv entries or contains NUL",
                task.id, check.name
            ));
        }
        if check.expected_exit != 0 {
            return invalid(format!(
                "{} check `{}` must expect exit zero because it is also authorized through run_check",
                task.id, check.name
            ));
        }
        if program.trim().is_empty() {
            return invalid(format!(
                "{} check `{}` has an empty executable",
                task.id, check.name
            ));
        }
        if program.contains('{') && program != "{python}" {
            return invalid(format!(
                "{} check `{}` uses an unsupported executable placeholder",
                task.id, check.name
            ));
        }
        let program_name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_ascii_lowercase();
        if ["sh", "bash", "cmd", "cmd.exe", "powershell", "pwsh"].contains(&program_name.as_str()) {
            return invalid(format!(
                "{} check `{}` must not invoke a shell",
                task.id, check.name
            ));
        }
        if check.timeout_s == 0 || check.timeout_s > task.timeout_s {
            return invalid(format!(
                "{} check `{}` timeout must be within the task timeout",
                task.id, check.name
            ));
        }
        for (stream, pattern) in [
            ("stdout", check.stdout_regex.as_deref()),
            ("stderr", check.stderr_regex.as_deref()),
        ] {
            if let Some(pattern) = pattern {
                Regex::new(pattern).map_err(|error| {
                    AutonomySuiteError::Validation(format!(
                        "{} check `{}` has invalid {stream} regex: {error}",
                        task.id, check.name
                    ))
                })?;
            }
        }
    }
    Ok(())
}

fn validate_relative_path(task_id: &str, value: &str) -> Result<(), AutonomySuiteError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || matches!(component, Component::Normal(name) if name
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(".ferric")))
        })
    {
        return invalid(format!(
            "{task_id} setup path `{value}` must be a portable relative path outside .ferric"
        ));
    }
    Ok(())
}

fn validate_path_collisions(task: &AutonomyTask) -> Result<(), AutonomySuiteError> {
    let paths: Vec<_> = task
        .setup_files
        .keys()
        .map(|path| {
            path.split('/')
                .map(|part| part.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .collect();
    for (index, left) in paths.iter().enumerate() {
        for right in &paths[index + 1..] {
            if left == right || is_component_prefix(left, right) || is_component_prefix(right, left)
            {
                return invalid(format!(
                    "{} setup paths collide or have a file/ancestor conflict",
                    task.id
                ));
            }
        }
    }
    Ok(())
}

fn is_component_prefix(left: &[String], right: &[String]) -> bool {
    left.len() < right.len() && left.iter().zip(right).all(|(left, right)| left == right)
}

fn require_text(label: &str, value: &str) -> Result<(), AutonomySuiteError> {
    if value.trim().is_empty() {
        return invalid(format!("{label} must not be empty"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, AutonomySuiteError> {
    Err(AutonomySuiteError::Validation(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_v1_parses_as_a_balanced_internal_baseline() {
        let suite = embedded_autonomy_suite().expect("embedded autonomy suite must validate");
        assert_eq!(suite.schema_version, 1);
        assert_eq!(suite.scope, BenchmarkScope::InternalBaseline);
        assert_eq!(suite.tasks.len(), 24);
        for category in [
            AutonomyCategory::Ambiguity,
            AutonomyCategory::Recovery,
            AutonomyCategory::LongHorizon,
        ] {
            assert_eq!(
                suite
                    .tasks
                    .iter()
                    .filter(|task| task.category == category)
                    .count(),
                8
            );
        }
    }

    #[test]
    fn embedded_v1_has_fixed_checks_and_all_recovery_probe_shapes() {
        let suite = embedded_autonomy_suite().unwrap();
        assert!(suite.tasks.iter().all(|task| !task.checks.is_empty()));
        assert!(
            suite
                .tasks
                .iter()
                .flat_map(|task| &task.checks)
                .all(|check| {
                    check.argv.first().map(String::as_str) == Some("{python}")
                        && check.argv.get(1).map(String::as_str) == Some("-B")
                })
        );

        let recovery: Vec<_> = suite
            .tasks
            .iter()
            .filter_map(|task| task.recovery.as_ref())
            .collect();
        assert_eq!(recovery.len(), 8);
        assert!(recovery.iter().any(|contract| contract.max_segments == 3));
        assert!(recovery.iter().any(|contract| contract.answer.is_some()));
        assert!(recovery.iter().any(|contract| {
            contract
                .refusal_modes
                .contains(&ResumeRefusalMode::WorkspaceMismatch)
        }));
        assert!(recovery.iter().any(|contract| {
            contract
                .refusal_modes
                .contains(&ResumeRefusalMode::CompletedSession)
        }));
        assert!(
            recovery
                .iter()
                .flat_map(|contract| &contract.injections)
                .all(|injection| {
                    !matches!(
                        injection.kind,
                        RecoveryInjectionKind::ProcessCrash | RecoveryInjectionKind::GuardPause
                    ) && injection.kind.expected_stop_reason().is_some()
                })
        );
    }

    #[test]
    fn every_untouched_repository_fails_its_authoritative_grade() {
        let python = std::env::var_os("FERRIC_TEST_PYTHON")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("python"));
        let mut command = std::process::Command::new(&python);
        command.arg("--version").stdin(std::process::Stdio::null());
        let available = crate::process::run_bounded(
            &mut command,
            std::time::Duration::from_secs(10),
            crate::process::CapturePlan::discard(),
        )
        .is_ok_and(|outcome| !outcome.timed_out && outcome.exit_code == Some(0));
        if !available {
            eprintln!("python is unavailable; autonomy discrimination test skipped");
            return;
        }

        let suite = embedded_autonomy_suite().unwrap();
        let mut unexpected_passes = Vec::new();
        for task in &suite.tasks {
            let workspace = tempfile::tempdir().unwrap();
            for (relative, content) in &task.setup_files {
                let path = workspace.path().join(relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(path, content).unwrap();
            }
            let spec = autonomy_bench_spec(task, suite.schema_version);
            let results = crate::verify::verify_command_checks(workspace.path(), &spec, &python);
            assert_eq!(results.len(), 1, "{} did not execute one grade", task.id);
            assert!(
                results.iter().all(|result| !result.infrastructure_error()),
                "{} grading infrastructure failed: {results:?}",
                task.id
            );
            if !results.is_empty() && results.iter().all(|result| result.passed()) {
                unexpected_passes.push(task.id.clone());
            }
        }
        assert!(
            unexpected_passes.is_empty(),
            "untouched corpus repositories passed grading: {unexpected_passes:?}"
        );
    }

    #[test]
    fn serde_rejects_unknown_fields() {
        let source = EMBEDDED_AUTONOMY_V1.replacen(
            "schema_version = 1",
            "schema_version = 1\nunknown_suite_field = true",
            1,
        );
        assert!(matches!(
            parse_autonomy_suite(&source),
            Err(AutonomySuiteError::Parse(_))
        ));
    }

    #[test]
    fn validation_rejects_duplicate_ids_and_category_drift() {
        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[1].id = suite.tasks[0].id.clone();
        assert!(validation_message(&suite).contains("duplicate task id"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks.pop();
        assert!(validation_message(&suite).contains("exactly 24 tasks"));
    }

    #[test]
    fn validation_rejects_unsafe_paths_and_shell_checks() {
        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0]
            .setup_files
            .insert("../outside.py".to_string(), String::new());
        assert!(validation_message(&suite).contains("portable relative path"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0].checks[0].argv = vec!["powershell".to_string(), "-c".to_string()];
        assert!(validation_message(&suite).contains("must not invoke a shell"));
    }

    #[test]
    fn validation_mirrors_child_budget_and_run_check_limits() {
        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0].max_turns = 256;
        assert!(validation_message(&suite).contains("1..=255"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[1]
            .setup_files
            .insert("api".to_string(), String::new());
        assert!(validation_message(&suite).contains("ancestor conflict"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0].checks[0].expected_exit = 1;
        assert!(validation_message(&suite).contains("expect exit zero"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0].checks[0].name = "not a valid name".to_string();
        assert!(validation_message(&suite).contains("1-64 ASCII"));
    }

    #[test]
    fn validation_rejects_incoherent_clarification_and_recovery() {
        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[0].terminal.current = TerminalOutcome::Completed;
        assert!(validation_message(&suite).contains("needs_input then completed"));

        let mut suite = embedded_autonomy_suite().unwrap();
        let recovery = suite.tasks[8].recovery.as_mut().unwrap();
        recovery.injections[0].kind = RecoveryInjectionKind::ProcessCrash;
        recovery.injections[0].point = None;
        assert!(validation_message(&suite).contains("requires a crash point"));

        let mut suite = embedded_autonomy_suite().unwrap();
        suite.tasks[14]
            .recovery
            .as_mut()
            .unwrap()
            .expected_question_terms
            .clear();
        assert!(validation_message(&suite).contains("recovery question terms"));
    }

    fn validation_message(suite: &AutonomySuite) -> String {
        validate_autonomy_suite(suite).unwrap_err().to_string()
    }
}
