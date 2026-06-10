use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::denylist::{DENIED_WRITE_FILES, DENIED_WRITE_SEGMENTS};

/// What a tool is allowed to do. Ordering is meaningful:
/// `Read < Write < Execute`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    Read,
    Write,
    Execute,
}

/// The outcome of a permission check, with a machine-readable denial reason.
/// (Serialize-only: decisions flow *into* traces, never back out.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DenyReason {
    /// Which hardcoded rule fired, e.g. `"denied_write_segment"`.
    pub rule: &'static str,
    /// What it matched, e.g. `".ssh"`.
    pub matched: String,
}

impl Decision {
    pub fn is_allow(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// Check `level` access to `path` (which should already have passed
/// [`crate::Workspace::resolve`] — this layer screens *what* is touched, the
/// boundary screens *where*).
pub fn check(level: PermissionLevel, path: &Path) -> Decision {
    match level {
        // Reads are gated by the workspace boundary alone.
        PermissionLevel::Read => Decision::Allow,
        PermissionLevel::Write | PermissionLevel::Execute => check_write_target(path),
    }
}

fn check_write_target(path: &Path) -> Decision {
    for component in path.components() {
        if let Component::Normal(name) = component
            && let Some(name) = name.to_str()
        {
            let lowered = name.to_ascii_lowercase();
            if DENIED_WRITE_SEGMENTS.contains(&lowered.as_str()) {
                return Decision::Deny(DenyReason {
                    rule: "denied_write_segment",
                    matched: lowered,
                });
            }
        }
    }
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        let lowered = file_name.to_ascii_lowercase();
        if DENIED_WRITE_FILES.contains(&lowered.as_str()) {
            return Decision::Deny(DenyReason {
                rule: "denied_write_file",
                matched: lowered,
            });
        }
    }
    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn denies_sensitive_paths() {
        let git_config = PathBuf::from("ws").join(".git").join("config");
        match check(PermissionLevel::Write, &git_config) {
            Decision::Deny(reason) => {
                assert_eq!(reason.rule, "denied_write_segment");
                assert_eq!(reason.matched, ".git");
            }
            Decision::Allow => panic!(".git/config write must be denied"),
        }

        let ssh_key = PathBuf::from("ws").join(".ssh").join("id_rsa");
        assert!(!check(PermissionLevel::Write, &ssh_key).is_allow());

        // Denied file name outside a denied segment still trips the file rule.
        let stray_key = PathBuf::from("ws").join("backup").join("id_ed25519");
        match check(PermissionLevel::Write, &stray_key) {
            Decision::Deny(reason) => assert_eq!(reason.rule, "denied_write_file"),
            Decision::Allow => panic!("stray private key write must be denied"),
        }
    }

    #[test]
    fn allows_plain_read() {
        let path = PathBuf::from("ws").join("src").join("main.rs");
        assert!(check(PermissionLevel::Read, &path).is_allow());
        // Reads are allowed even of sensitive names — the boundary gates location.
        let git_config = PathBuf::from("ws").join(".git").join("config");
        assert!(check(PermissionLevel::Read, &git_config).is_allow());
    }

    #[test]
    fn allows_ordinary_write() {
        let path = PathBuf::from("ws").join("src").join("lib.rs");
        assert!(check(PermissionLevel::Write, &path).is_allow());
    }

    #[test]
    fn denylist_is_const() {
        // The lists are compile-time constants — this test pins their
        // non-emptiness; no setter API exists to mutate them at runtime.
        assert!(!crate::denylist::DENIED_WRITE_SEGMENTS.is_empty());
        assert!(!crate::denylist::DENIED_WRITE_FILES.is_empty());
        assert!(!crate::denylist::DENIED_COMMAND_PATTERNS.is_empty());
    }
}
