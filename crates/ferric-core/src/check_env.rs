//! Hermetic environment contract for operator-authorized verification checks.
//!
//! Check commands are fixed argv, but inherited language-runtime variables can
//! still change their meaning. In particular, `PYTHONOPTIMIZE=1` removes
//! `assert` statements and can turn an invalid artifact into a passing grade.

use std::process::Command;

/// Host variables removed before any named or post-run check starts.
pub const CHECK_ENV_REMOVE: &[&str] = &[
    "PYTHONBREAKPOINT",
    "PYTHONHOME",
    "PYTHONINSPECT",
    "PYTHONOPTIMIZE",
    "PYTHONPATH",
    "PYTHONPYCACHEPREFIX",
    "PYTHONSAFEPATH",
    "PYTHONSTARTUP",
    "PYTHONUSERBASE",
    "PYTHONWARNINGS",
];

/// Deterministic, isolated defaults supplied to every check process.
pub const CHECK_ENV_SET: &[(&str, &str)] = &[("PYTHONHASHSEED", "0"), ("PYTHONNOUSERSITE", "1")];

/// Apply the shared hermetic environment contract to a check command.
pub fn configure_check_environment(command: &mut Command) {
    for variable in CHECK_ENV_REMOVE {
        command.env_remove(variable);
    }
    for (variable, value) in CHECK_ENV_SET {
        command.env(variable, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimization_is_removed_and_deterministic_defaults_are_explicit() {
        let mut command = Command::new("check-program");
        command.env("PYTHONOPTIMIZE", "1");
        configure_check_environment(&mut command);
        let environment: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(environment.get("PYTHONOPTIMIZE"), Some(&None));
        assert_eq!(
            environment.get("PYTHONHASHSEED"),
            Some(&Some("0".to_string()))
        );
        assert_eq!(
            environment.get("PYTHONNOUSERSITE"),
            Some(&Some("1".to_string()))
        );
    }
}
