//! CaMeL-lite flow control (Ornstein): taint tracking + a configurable sink
//! policy. The quarantine marks research digests UNTRUSTED; this module decides
//! whether tainted (untrusted-derived) data may reach a side-effecting **sink**
//! (a `Write`/`Execute` tool). It is the pure decision function the eventual
//! dispatch-chokepoint gate will call (ADR-044) — not yet wired into the loop.
//!
//! "CaMeL-lite" (per the s1 research): tainted-string tracking + a sink-policy
//! table, no interpreter. Conservative by design — over-gating a *write* is the
//! safe direction; the three modes (Deny/RequireApproval/Warn) tune strictness.

use crate::PermissionLevel;

/// What to do when tainted-derived data would reach a `Write`/`Execute` sink.
/// The caller picks: `Deny` (autonomous), `RequireApproval` (human-gated), or
/// `Warn` (observability-first rollout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkAction {
    Deny,
    RequireApproval,
    Warn,
}

/// The sink policy's decision for a candidate tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkDecision {
    /// Proceed — not a tainted sink.
    Allow,
    /// Block the call.
    Deny,
    /// Pause for human approval.
    RequireApproval,
    /// Proceed, but flag it.
    Warn,
}

/// CaMeL-lite sink policy: how tainted data is treated at a `Write`/`Execute`
/// sink. Keyed off the existing `PermissionLevel` axis (so the eventual wiring
/// passes each tool's real `spec.permission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkPolicy {
    tainted_sink: SinkAction,
}

impl SinkPolicy {
    pub fn new(tainted_sink: SinkAction) -> Self {
        Self { tainted_sink }
    }

    /// The autonomous-harness default: block tainted data from reaching a sink.
    pub fn deny() -> Self {
        Self::new(SinkAction::Deny)
    }

    /// Decide whether a tool call may proceed. `permission` is the tool's level;
    /// `tainted` is whether any of its args derive from tainted data.
    pub fn decide(&self, permission: PermissionLevel, tainted: bool) -> SinkDecision {
        if !tainted {
            return SinkDecision::Allow; // trusted data always flows
        }
        match permission {
            // Reading isn't a dangerous sink — the workspace boundary confines it.
            PermissionLevel::Read => SinkDecision::Allow,
            // Side-effecting sinks: apply the configured action.
            PermissionLevel::Write | PermissionLevel::Execute => match self.tainted_sink {
                SinkAction::Deny => SinkDecision::Deny,
                SinkAction::RequireApproval => SinkDecision::RequireApproval,
                SinkAction::Warn => SinkDecision::Warn,
            },
        }
    }
}

/// A set of tainted strings — text that came from untrusted sources (a
/// [`ResearchDigest`]). CaMeL-lite substring tracking.
#[derive(Debug, Clone, Default)]
pub struct TaintSet {
    tainted: Vec<String>,
}

impl TaintSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a string as tainted. Empty / whitespace-only strings are ignored —
    /// an empty needle would otherwise match every value.
    pub fn taint_str(&mut self, s: &str) {
        if !s.trim().is_empty() {
            self.tainted.push(s.to_string());
        }
    }

    /// Does `value` derive from tainted data — i.e. contain any tainted substring?
    pub fn is_tainted(&self, value: &str) -> bool {
        self.tainted.iter().any(|t| value.contains(t.as_str()))
    }

    /// Are any of a tool call's args tainted-derived? Walks the args JSON and
    /// checks every string within (values, recursively).
    pub fn args_tainted(&self, args: &serde_json::Value) -> bool {
        if self.tainted.is_empty() {
            return false;
        }
        any_tainted_string(args, self)
    }
}

/// Recursively check whether any string within `value` is tainted-derived.
fn any_tainted_string(value: &serde_json::Value, taint: &TaintSet) -> bool {
    match value {
        serde_json::Value::String(s) => taint.is_tainted(s),
        serde_json::Value::Array(items) => items.iter().any(|v| any_tainted_string(v, taint)),
        serde_json::Value::Object(map) => map.values().any(|v| any_tainted_string(v, taint)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn untainted_always_allows() {
        let p = SinkPolicy::deny();
        for lvl in [
            PermissionLevel::Read,
            PermissionLevel::Write,
            PermissionLevel::Execute,
        ] {
            assert_eq!(p.decide(lvl, false), SinkDecision::Allow);
        }
    }

    #[test]
    fn read_sink_allows_even_tainted() {
        assert_eq!(
            SinkPolicy::deny().decide(PermissionLevel::Read, true),
            SinkDecision::Allow
        );
    }

    #[test]
    fn write_execute_tainted_follow_the_mode() {
        assert_eq!(
            SinkPolicy::new(SinkAction::Deny).decide(PermissionLevel::Write, true),
            SinkDecision::Deny
        );
        assert_eq!(
            SinkPolicy::new(SinkAction::RequireApproval).decide(PermissionLevel::Write, true),
            SinkDecision::RequireApproval
        );
        assert_eq!(
            SinkPolicy::new(SinkAction::Warn).decide(PermissionLevel::Write, true),
            SinkDecision::Warn
        );
        // Execute behaves like Write.
        assert_eq!(
            SinkPolicy::deny().decide(PermissionLevel::Execute, true),
            SinkDecision::Deny
        );
    }

    #[test]
    fn taint_str_and_is_tainted() {
        let mut t = TaintSet::new();
        t.taint_str("secret-token");
        assert!(t.is_tainted("here is the secret-token value"));
        assert!(!t.is_tainted("nothing relevant"));
    }

    #[test]
    fn empty_taint_set_taints_nothing() {
        let t = TaintSet::new();
        assert!(!t.is_tainted("anything"));
        assert!(!t.args_tainted(&json!({"path": "x"})));
        // Empty / whitespace strings are ignored on insert (no match-everything).
        let mut t2 = TaintSet::new();
        t2.taint_str("");
        t2.taint_str("   ");
        assert!(!t2.is_tainted("anything"));
    }

    #[test]
    fn args_tainted_walks_nested_json() {
        let mut t = TaintSet::new();
        t.taint_str("rm -rf /");
        // nested in an object value
        assert!(t.args_tainted(&json!({"path": "a.txt", "content": "do rm -rf / now"})));
        // nested in an array element
        assert!(t.args_tainted(&json!({"edits": [{"old": "x", "new": "rm -rf /"}]})));
        // clean args
        assert!(!t.args_tainted(&json!({"path": "a.txt", "content": "hello"})));
    }
}
