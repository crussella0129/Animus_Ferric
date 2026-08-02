use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Which autonomous-controller behavior a run uses.
///
/// This is deliberately orthogonal to [`crate::RunPolicy`]: model scale still
/// determines budgets and tool rings, while this value selects the harness
/// experiment applied within those fixed limits. `Legacy` is the serde/default
/// value so every trace and result written before Sprint 113 retains its exact
/// historical meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessPolicy {
    #[default]
    Legacy,
    Evidence,
    EvidencePlanner,
}

impl HarnessPolicy {
    /// Stable wire/CLI label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Evidence => "evidence",
            Self::EvidencePlanner => "evidence_planner",
        }
    }
}

impl fmt::Display for HarnessPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// A harness-policy label was not one of the closed, versioned values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown harness policy {value:?}; expected legacy, evidence, or evidence_planner")]
pub struct ParseHarnessPolicyError {
    value: String,
}

impl ParseHarnessPolicyError {
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl FromStr for HarnessPolicy {
    type Err = ParseHarnessPolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "evidence" => Ok(Self::Evidence),
            "evidence_planner" => Ok(Self::EvidencePlanner),
            _ => Err(ParseHarnessPolicyError {
                value: value.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_display_parse_and_serde_are_one_stable_vocabulary() {
        for policy in [
            HarnessPolicy::Legacy,
            HarnessPolicy::Evidence,
            HarnessPolicy::EvidencePlanner,
        ] {
            let label = policy.label();
            assert_eq!(policy.to_string(), label);
            assert_eq!(label.parse::<HarnessPolicy>().unwrap(), policy);
            assert_eq!(
                serde_json::to_string(&policy).unwrap(),
                format!(r#""{label}""#)
            );
            assert_eq!(
                serde_json::from_str::<HarnessPolicy>(&format!(r#""{label}""#)).unwrap(),
                policy
            );
        }
    }

    #[test]
    fn default_is_legacy_and_unknown_or_ambiguous_labels_fail_closed() {
        assert_eq!(HarnessPolicy::default(), HarnessPolicy::Legacy);
        for invalid in ["", "Legacy", "evidence-planner", "planner", "future"] {
            let error = invalid.parse::<HarnessPolicy>().unwrap_err();
            assert_eq!(error.value(), invalid);
        }
        assert!(serde_json::from_str::<HarnessPolicy>(r#""future""#).is_err());
    }
}
