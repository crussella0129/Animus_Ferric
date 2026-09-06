//! Explicit, checked agent execution controls. These do not scale graders,
//! startup, capture, or the process owner's checked cleanup budget.

use std::io;
use std::path::Path;
use std::time::Duration;

use ferric_core::{ModelProfile, OutputBudget, policy_for, resolve_output_budget};
use serde::{Deserialize, Serialize};

/// Validated controls shared by child argv and parent attribution. Parameters
/// and context are declared coordinates, not child observations: legacy mock
/// invocations without selected controls retain their historical defaults.
/// Construction
/// (including deserialization) must pass the same fallible admission boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawBudgetControls")]
pub struct BudgetControls {
    timeout_scale: f64,
    max_output_tokens: Option<u32>,
    params_b: f32,
    ctx: u32,
}

#[derive(Deserialize)]
struct RawBudgetControls {
    timeout_scale: f64,
    max_output_tokens: Option<u32>,
    params_b: f32,
    ctx: u32,
}

impl TryFrom<RawBudgetControls> for BudgetControls {
    type Error = String;

    fn try_from(raw: RawBudgetControls) -> Result<Self, Self::Error> {
        Self::new(
            raw.timeout_scale,
            raw.max_output_tokens,
            raw.params_b,
            raw.ctx,
        )
    }
}

impl BudgetControls {
    pub fn new(
        timeout_scale: f64,
        max_output_tokens: Option<u32>,
        params_b: f32,
        ctx: u32,
    ) -> Result<Self, String> {
        if !timeout_scale.is_finite() || timeout_scale <= 0.0 {
            return Err("--timeout-scale must be finite and strictly positive".to_string());
        }
        if !params_b.is_finite() || params_b <= 0.0 || ctx == 0 {
            return Err("benchmark budget requires positive finite parameters and context".into());
        }
        // Benchmark children use an empty profile directory, so this is the
        // same parameter-derived policy they will select, not a durable tier.
        let policy = policy_for(&ModelProfile {
            params_b,
            quant: String::new(),
            ctx,
            family: String::new(),
            measured_level: None,
        });
        resolve_output_budget(&policy, ctx, max_output_tokens)?;
        Ok(Self {
            timeout_scale,
            max_output_tokens,
            params_b,
            ctx,
        })
    }

    pub fn timeout_scale(&self) -> f64 {
        self.timeout_scale
    }
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens
    }
    pub fn params_b(&self) -> f32 {
        self.params_b
    }
    pub fn ctx(&self) -> u32 {
        self.ctx
    }

    /// Modified execution/output budgets are useful diagnostics, but the
    /// current profile key cannot represent their calibration coordinates.
    pub fn is_diagnostic(&self) -> bool {
        self.timeout_scale != 1.0 || self.max_output_tokens.is_some()
    }

    /// Resolve once before preflight, then reuse this exact duration across
    /// trials. The 1.0 branch avoids floating conversion of large integers.
    pub fn resolve_agent(&self, base_timeout_s: u64) -> Result<ResolvedAgentBudget, String> {
        let duration = if self.timeout_scale == 1.0 {
            Duration::from_secs(base_timeout_s)
        } else {
            let seconds = base_timeout_s as f64 * self.timeout_scale;
            Duration::try_from_secs_f64(seconds).map_err(|error| {
                format!("--timeout-scale produces an unrepresentable agent timeout: {error}")
            })?
        };
        if duration.is_zero() {
            return Err(
                "--timeout-scale produces a zero agent timeout (including underflow)".into(),
            );
        }
        Ok(ResolvedAgentBudget {
            controls: self.clone(),
            base_timeout_s,
            duration,
        })
    }
}

/// Kept non-deserializable and privately constructed: execution accepts only
/// a duration produced by the checked resolver, never a reconstructed guess.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAgentBudget {
    controls: BudgetControls,
    base_timeout_s: u64,
    duration: Duration,
}

impl ResolvedAgentBudget {
    pub fn controls(&self) -> &BudgetControls {
        &self.controls
    }
    pub fn base_timeout_s(&self) -> u64 {
        self.base_timeout_s
    }
    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn evidence(&self, exit_code: Option<i32>, timed_out: bool) -> AttemptBudgetEvidence {
        AttemptBudgetEvidence {
            controls: self.controls.clone(),
            base_timeout_s: self.base_timeout_s,
            enforced_duration: self.duration.into(),
            warmup: WarmupState::NotPerformed,
            parent_termination: if timed_out {
                ParentTermination::ExecutionTimeout
            } else {
                ParentTermination::Exited { exit_code }
            },
            trace: TraceBudgetObservation::missing(),
            retained: None,
        }
    }
}

/// Lossless representation of the actual enforced Duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactDuration {
    pub secs: u64,
    pub nanos: u32,
}

impl From<Duration> for ExactDuration {
    fn from(value: Duration) -> Self {
        Self {
            secs: value.as_secs(),
            nanos: value.subsec_nanos(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmupState {
    NotPerformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentTermination {
    Exited { exit_code: Option<i32> },
    ExecutionTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEvidenceState {
    Missing,
    Readable,
    Malformed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedMainActionBudget {
    pub turn: u32,
    pub budget: OutputBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceBudgetObservation {
    pub state: TraceEvidenceState,
    /// All observed main requests, in trace order. None means unavailable,
    /// including a readable legacy trace with no request budget vocabulary.
    pub main_action_budgets: Option<Vec<ObservedMainActionBudget>>,
    pub child_terminal: Option<String>,
}

impl TraceBudgetObservation {
    pub fn missing() -> Self {
        Self {
            state: TraceEvidenceState::Missing,
            main_action_budgets: None,
            child_terminal: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptBudgetEvidence {
    pub controls: BudgetControls,
    pub base_timeout_s: u64,
    pub enforced_duration: ExactDuration,
    pub warmup: WarmupState,
    pub parent_termination: ParentTermination,
    pub trace: TraceBudgetObservation,
    pub retained: Option<RetainedBudgetEvidence>,
}

impl AttemptBudgetEvidence {
    pub fn observe_trace(&mut self, path: Option<&Path>) -> io::Result<()> {
        self.trace = match path {
            Some(path) => crate::budget_trace::observe_trace(path)?,
            None => TraceBudgetObservation::missing(),
        };
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptIdentity {
    pub run_id: String,
    pub trial_id: String,
    pub level: u8,
}

impl AttemptIdentity {
    pub fn new(run_id: &str, trial_id: &str, level: u8) -> io::Result<Self> {
        let value = Self {
            run_id: run_id.into(),
            trial_id: trial_id.into(),
            level,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        for value in [&self.run_id, &self.trial_id] {
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "budget evidence identity must contain only ASCII letters, digits, '-' or '_'",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn paths(&self) -> (String, String) {
        let stem = format!("traces/{}/{}-l{}", self.run_id, self.trial_id, self.level);
        (format!("{stem}.jsonl"), format!("{stem}.budget.json"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetainedBudgetEvidence {
    pub identity: AttemptIdentity,
    pub trace_path: String,
    pub sidecar_path: String,
    pub trace_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptBudgetReference {
    pub identity: AttemptIdentity,
    pub retained: Option<RetainedBudgetEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunBudgetEvidence {
    /// Unknown for a legacy/mixed run. Never infer a default scale from absent
    /// attribution. A freshly admitted empty run may still know its controls.
    pub controls: Option<BudgetControls>,
    pub attempts: Vec<AttemptBudgetReference>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_scale_default_fractional_matrix() {
        for base in [1, 60, 90, 180, 300, 600, 900, u64::MAX] {
            let controls = BudgetControls::new(1.0, None, 7.0, 4096).unwrap();
            assert_eq!(
                controls.resolve_agent(base).unwrap().duration(),
                Duration::from_secs(base)
            );
        }
        for (scale, base, expected) in [
            (0.5, 60, Duration::from_secs(30)),
            (2.0, 90, Duration::from_secs(180)),
            (0.125, 1, Duration::new(0, 125_000_000)),
            (1.0 / 128.0, 1, Duration::new(0, 7_812_500)),
        ] {
            let resolved = BudgetControls::new(scale, None, 7.0, 4096)
                .unwrap()
                .resolve_agent(base)
                .unwrap();
            assert_eq!(resolved.duration(), expected);
            assert_eq!(
                resolved.evidence(None, true).enforced_duration,
                ExactDuration::from(expected)
            );
        }
    }

    #[test]
    fn timeout_scale_invalid_matrix() {
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -0.0, -1.0] {
            assert!(
                BudgetControls::new(scale, None, 7.0, 4096).is_err(),
                "{scale}"
            );
        }
        for (scale, base) in [
            (f64::MAX, 900),
            (f64::MIN_POSITIVE, 60),
            (f64::from_bits(1), 60),
            (1.0, 0),
            (2.0, u64::MAX),
        ] {
            assert!(
                BudgetControls::new(scale, None, 7.0, 4096)
                    .unwrap()
                    .resolve_agent(base)
                    .is_err()
            );
        }
        for cap in [0, 1230, u32::MAX] {
            assert!(BudgetControls::new(1.0, Some(cap), 7.0, 4096).is_err());
        }
        assert!(BudgetControls::new(1.0, Some(1), 7.0, 0).is_err());
        assert!(BudgetControls::new(1.0, None, f32::NAN, 4096).is_err());
    }

    #[test]
    fn benchmark_scale_leaves_grader_bounds_unchanged() {
        let specs = crate::embedded_specs().unwrap();
        let before = specs.clone();
        let controls = BudgetControls::new(0.125, Some(1024), 7.0, 4096).unwrap();
        for spec in &specs {
            let budget = controls.resolve_agent(spec.timeout_s).unwrap();
            assert_eq!(
                budget.duration(),
                Duration::from_secs_f64(spec.timeout_s as f64 * 0.125)
            );
        }
        assert_eq!(specs, before);
        assert_eq!(
            include_str!("process.rs").replace("\r\n", "\n"),
            "//! Benchmark policy adapter; native ownership is shared with source tests.\npub(crate) use ferric_process::{CapturePlan, run_bounded};\n"
        );
    }
}
