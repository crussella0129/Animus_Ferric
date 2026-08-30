//! Pure lifecycle resolution for captured server registrations.
//!
//! File precedence is deliberately absent here. Callers first capture every
//! registration path and bind each parsed record to live OS facts. Resolution
//! then chooses at most one exact process identity, or fails closed.

use crate::server_process::ProcessIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CandidateState {
    Verified {
        identity: ProcessIdentity,
        /// Canonical serialization of the parsed registration. Raw captured
        /// bytes remain separate compare/delete tokens: two formatting-only
        /// aliases may resolve together without making their bytes fungible.
        registration_key: Vec<u8>,
        http_healthy: bool,
        listener_present: bool,
        listener_loopback_only: bool,
    },
    Stale {
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub label: String,
    pub state: CandidateState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    Empty,
    StaleOnly {
        stale: Vec<usize>,
    },
    One {
        target: usize,
        aliases: Vec<usize>,
        stale: Vec<usize>,
        http_healthy: bool,
        listener_present: bool,
        listener_loopback_only: bool,
    },
    Blocked {
        reasons: Vec<String>,
    },
}

pub(crate) fn resolve(candidates: &[Candidate]) -> Resolution {
    if candidates.is_empty() {
        return Resolution::Empty;
    }

    let mut blockers = candidates
        .iter()
        .filter_map(|candidate| match &candidate.state {
            CandidateState::Blocked { reason } => Some(format!("{}: {reason}", candidate.label)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        return Resolution::Blocked { reasons: blockers };
    }

    let stale = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            matches!(candidate.state, CandidateState::Stale { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    let verified = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            matches!(candidate.state, CandidateState::Verified { .. }).then_some(index)
        })
        .collect::<Vec<_>>();

    let Some((&target, alias_indices)) = verified.split_first() else {
        return Resolution::StaleOnly { stale };
    };
    let CandidateState::Verified {
        identity,
        registration_key,
        http_healthy,
        listener_present,
        listener_loopback_only,
    } = &candidates[target].state
    else {
        unreachable!("verified index must identify a verified candidate");
    };
    let mut resolved_http_healthy = *http_healthy;
    let mut resolved_listener_present = *listener_present;
    let mut resolved_listener_loopback_only = *listener_loopback_only;

    for index in alias_indices {
        let CandidateState::Verified {
            identity: alias_identity,
            registration_key: alias_key,
            http_healthy: alias_http_healthy,
            listener_present: alias_listener_present,
            listener_loopback_only: alias_listener_loopback_only,
        } = &candidates[*index].state
        else {
            unreachable!("verified index must identify a verified candidate");
        };
        if alias_identity != identity || alias_key != registration_key {
            blockers.push(format!(
                "{} and {} bind to different live registrations",
                candidates[target].label, candidates[*index].label
            ));
        }
        // Alias observations are sequential snapshots of the same process.
        // Never discard a degraded/public observation merely because the
        // first scope happened to see a healthier moment.
        resolved_http_healthy &= *alias_http_healthy;
        resolved_listener_present &= *alias_listener_present;
        resolved_listener_loopback_only &= *alias_listener_loopback_only;
    }
    if !blockers.is_empty() {
        return Resolution::Blocked { reasons: blockers };
    }

    Resolution::One {
        target,
        aliases: alias_indices.to_vec(),
        stale,
        http_healthy: resolved_http_healthy,
        listener_present: resolved_listener_present,
        listener_loopback_only: resolved_listener_loopback_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn identity(token: &str) -> ProcessIdentity {
        ProcessIdentity {
            start_token: token.to_string(),
            executable: PathBuf::from("example-engine"),
            argv: vec!["example-engine".to_string(), "--serve".to_string()],
        }
    }

    fn verified(label: &str, token: &str, key: &[u8]) -> Candidate {
        Candidate {
            label: label.to_string(),
            state: CandidateState::Verified {
                identity: identity(token),
                registration_key: key.to_vec(),
                http_healthy: true,
                listener_present: true,
                listener_loopback_only: true,
            },
        }
    }

    fn stale(label: &str) -> Candidate {
        Candidate {
            label: label.to_string(),
            state: CandidateState::Stale {
                reason: "process creation token changed".to_string(),
            },
        }
    }

    #[test]
    fn stale_local_does_not_shadow_unique_live_global() {
        let candidates = [stale("local"), verified("global", "b", b"B")];
        assert_eq!(
            resolve(&candidates),
            Resolution::One {
                target: 1,
                aliases: Vec::new(),
                stale: vec![0],
                http_healthy: true,
                listener_present: true,
                listener_loopback_only: true,
            }
        );
    }

    #[test]
    fn exact_process_and_registration_group_as_aliases() {
        let candidates = [
            verified("local", "same", b"same-record"),
            verified("global", "same", b"same-record"),
        ];
        assert!(matches!(
            resolve(&candidates),
            Resolution::One {
                target: 0,
                aliases,
                ..
            } if aliases == [1]
        ));
    }

    #[test]
    fn alias_health_and_listener_facts_are_aggregated_conservatively() {
        let healthy = verified("local", "same", b"same-record");
        let mut degraded = verified("global", "same", b"same-record");
        degraded.state = CandidateState::Verified {
            identity: identity("same"),
            registration_key: b"same-record".to_vec(),
            http_healthy: false,
            listener_present: true,
            listener_loopback_only: false,
        };

        assert!(matches!(
            resolve(&[healthy, degraded]),
            Resolution::One {
                http_healthy: false,
                listener_present: true,
                listener_loopback_only: false,
                ..
            }
        ));
    }

    #[test]
    fn distinct_live_processes_fail_closed() {
        let candidates = [verified("local", "a", b"A"), verified("global", "b", b"B")];
        assert!(matches!(
            resolve(&candidates),
            Resolution::Blocked { reasons } if reasons.len() == 1
        ));
    }

    #[test]
    fn same_pid_identity_with_different_registration_fails_closed() {
        let candidates = [
            verified("local", "same", b"A"),
            verified("global", "same", b"B"),
        ];
        assert!(matches!(resolve(&candidates), Resolution::Blocked { .. }));
    }

    #[test]
    fn any_uninspectable_peer_blocks_destructive_resolution() {
        let candidates = [
            verified("global", "b", b"B"),
            Candidate {
                label: "local".to_string(),
                state: CandidateState::Blocked {
                    reason: "malformed JSON".to_string(),
                },
            },
        ];
        assert!(matches!(resolve(&candidates), Resolution::Blocked { .. }));
    }

    #[test]
    fn stale_only_is_distinct_from_stopped() {
        assert_eq!(
            resolve(&[stale("local"), stale("global")]),
            Resolution::StaleOnly { stale: vec![0, 1] }
        );
    }
}
