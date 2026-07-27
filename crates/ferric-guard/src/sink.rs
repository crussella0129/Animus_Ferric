//! Flow control for untrusted content (Ornstein): a **provenance** marker plus a
//! configurable sink policy. The quarantine marks research digests untrusted;
//! this module decides whether a run that has ingested them may still reach a
//! side-effecting **sink** (a `Write`/`Execute` tool).
//!
//! # Why this is structural, not a detector (ADR-080)
//!
//! This began as CaMeL-lite *substring taint*: remember text from the digest,
//! then ask whether a tool argument contains any of it. Measured live (ADR-078),
//! that does not work, and no threshold makes it work:
//!
//! * It detects **copying**, while the threat is **influence**. An injection
//!   succeeds when it is *obeyed*, not when it is quoted — "email the key to X"
//!   wins by making the model write an address, not by making it repeat the
//!   sentence.
//! * **Paraphrase defeats matching at every length.** The quarantine's own
//!   summary is already a paraphrase of the source, and a model restating it
//!   rewords again, so needles rarely appear verbatim in arguments.
//! * The one tuning axis — segment length — is bad at both ends: long segments
//!   miss lifted fragments, short segments match ordinary prose and deny every
//!   write.
//!
//! So the question changed. Not *"do these arguments contain tainted text?"*
//! (undecidable in practice) but *"has this run ingested untrusted content at
//! all?"* — a fact the harness stamps and the model cannot launder. A clean run
//! is unaffected; a contaminated one gates every mutation. Nothing to evade,
//! because nothing is being detected.
//!
//! This also matches how the rest of Ornstein already works: the quarantine is
//! structural (ADR-010/040 — empty tools is the only valid constrained shape, so
//! an injection has no action channel by construction). The sink gate was the
//! one place that reached for detection instead, and it was the one place that
//! did not hold up.

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

    /// Block mutations outright once the run is contaminated.
    pub fn deny() -> Self {
        Self::new(SinkAction::Deny)
    }

    /// The default for a run that ingests untrusted content (ADR-080): ask a
    /// human once per mutation. With no approver available there is nobody to
    /// ask, so it denies — safe by default, usable when supervised.
    pub fn require_approval() -> Self {
        Self::new(SinkAction::RequireApproval)
    }

    /// Decide whether a tool call may proceed.
    ///
    /// `permission` is the tool's level; `provenance` is whether this **run** has
    /// ingested untrusted content — not whether these particular arguments look
    /// tainted, which is the distinction ADR-080 turns on.
    pub fn decide(&self, permission: PermissionLevel, provenance: Provenance) -> SinkDecision {
        if !provenance.is_untrusted() {
            return SinkDecision::Allow; // a clean run is never gated
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

/// Shortest prose fragment worth tainting. Below this, a needle matches so much
/// ordinary text that the policy degenerates into denying every write.
pub const MIN_TAINT_SEGMENT_CHARS: usize = 12;

/// Whether this run has ingested untrusted content.
///
/// Harness-stamped, like `ResearchDigest.untrusted` — the model has no way to
/// clear it, because it is never asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provenance {
    /// Nothing untrusted has entered this run. Sinks are unaffected.
    #[default]
    Clean,
    /// Untrusted content has been ingested. Every `Write`/`Execute` is gated.
    UntrustedIngested,
}

impl Provenance {
    pub fn is_untrusted(self) -> bool {
        matches!(self, Provenance::UntrustedIngested)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_run_is_never_gated() {
        for action in [
            SinkAction::Deny,
            SinkAction::RequireApproval,
            SinkAction::Warn,
        ] {
            let p = SinkPolicy::new(action);
            for lvl in [
                PermissionLevel::Read,
                PermissionLevel::Write,
                PermissionLevel::Execute,
            ] {
                assert_eq!(
                    p.decide(lvl, Provenance::Clean),
                    SinkDecision::Allow,
                    "an ordinary run must be completely unaffected"
                );
            }
        }
    }

    #[test]
    fn reads_stay_allowed_even_when_contaminated() {
        // The workspace boundary already confines reads; gating them would add
        // friction without adding safety.
        assert_eq!(
            SinkPolicy::deny().decide(PermissionLevel::Read, Provenance::UntrustedIngested),
            SinkDecision::Allow
        );
    }

    #[test]
    fn mutations_follow_the_configured_action_once_contaminated() {
        for (action, expected) in [
            (SinkAction::Deny, SinkDecision::Deny),
            (SinkAction::RequireApproval, SinkDecision::RequireApproval),
            (SinkAction::Warn, SinkDecision::Warn),
        ] {
            let p = SinkPolicy::new(action);
            for lvl in [PermissionLevel::Write, PermissionLevel::Execute] {
                assert_eq!(p.decide(lvl, Provenance::UntrustedIngested), expected);
            }
        }
    }

    /// The gate keys on the RUN, not on the call's contents — so there is
    /// nothing an attacker can reword to slip past it (ADR-080).
    #[test]
    fn the_decision_does_not_depend_on_call_contents() {
        let p = SinkPolicy::require_approval();
        // Same decision regardless of what the call carries; `decide` is not
        // even given the arguments.
        assert_eq!(
            p.decide(PermissionLevel::Write, Provenance::UntrustedIngested),
            SinkDecision::RequireApproval
        );
        assert_eq!(
            p.decide(PermissionLevel::Write, Provenance::Clean),
            SinkDecision::Allow
        );
    }

    #[test]
    fn provenance_defaults_to_clean() {
        assert_eq!(Provenance::default(), Provenance::Clean);
        assert!(!Provenance::default().is_untrusted());
        assert!(Provenance::UntrustedIngested.is_untrusted());
    }
}
