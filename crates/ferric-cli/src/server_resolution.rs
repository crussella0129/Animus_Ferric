//! Pure, typed lifecycle resolution for captured server registrations.
//!
//! File precedence is deliberately absent. Callers capture every configured
//! registration coordinate, bind captured records to runtime facts, and pass
//! those observations here. Resolution then returns one typed managed state or
//! fails closed without choosing a local/global/origin scope by convention.

use crate::server::ServerRunfile;
use crate::server_process::{ListenerState, ProcessIdentity};
use crate::server_registration::RegistrationCoordinate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HealthState {
    NotProbed,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionIssueKind {
    Conflict,
    Unverifiable,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolutionIssue {
    pub coordinates: Vec<RegistrationCoordinate>,
    pub kind: ResolutionIssueKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CandidateState {
    Verified {
        identity: ProcessIdentity,
        listener: ListenerState,
        health: HealthState,
    },
    Stale {
        reason: String,
        observed_identity: Option<ProcessIdentity>,
        listener: ListenerState,
    },
    Unverifiable {
        reason: String,
        observed_identity: Option<ProcessIdentity>,
        listener: Option<ListenerState>,
        health: HealthState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub coordinate: RegistrationCoordinate,
    /// Complete parsed metadata; aliases require structural equality across
    /// every runfile field. Raw bytes remain separate store revision tokens.
    pub runfile: Option<ServerRunfile>,
    pub state: CandidateState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    Empty,
    Ready {
        target: usize,
        aliases: Vec<usize>,
        stale: Vec<usize>,
    },
    Degraded {
        target: usize,
        aliases: Vec<usize>,
        stale: Vec<usize>,
        listener: ListenerState,
        health: HealthState,
        issues: Vec<ResolutionIssue>,
    },
    StaleOnly {
        stale: Vec<usize>,
    },
    Conflict {
        issues: Vec<ResolutionIssue>,
    },
    Unverifiable {
        issues: Vec<ResolutionIssue>,
    },
}

fn issue(
    kind: ResolutionIssueKind,
    coordinates: impl IntoIterator<Item = RegistrationCoordinate>,
    detail: impl Into<String>,
) -> ResolutionIssue {
    ResolutionIssue {
        coordinates: coordinates.into_iter().collect(),
        kind,
        detail: detail.into(),
    }
}

fn coordinate_label(coordinate: &RegistrationCoordinate) -> String {
    format!(
        "{} registration {}",
        coordinate.scope,
        coordinate.path.display()
    )
}

fn listener_and_health(
    candidates: &[Candidate],
    verified: &[usize],
) -> Result<(ListenerState, HealthState), ResolutionIssue> {
    let target = verified[0];
    let CandidateState::Verified {
        listener, health, ..
    } = &candidates[target].state
    else {
        unreachable!("verified index must name a verified candidate");
    };
    let mut health = *health;
    for index in &verified[1..] {
        let CandidateState::Verified {
            listener: alias_listener,
            health: alias_health,
            ..
        } = &candidates[*index].state
        else {
            unreachable!("verified index must name a verified candidate");
        };
        if alias_listener != listener {
            return Err(issue(
                ResolutionIssueKind::Unverifiable,
                [
                    candidates[target].coordinate.clone(),
                    candidates[*index].coordinate.clone(),
                ],
                "alias listener observations changed during discovery",
            ));
        }
        health = match (health, *alias_health) {
            (HealthState::Unhealthy, _) | (_, HealthState::Unhealthy) => HealthState::Unhealthy,
            (HealthState::NotProbed, _) | (_, HealthState::NotProbed) => HealthState::NotProbed,
            (HealthState::Healthy, HealthState::Healthy) => HealthState::Healthy,
        };
    }
    Ok((listener.clone(), health))
}

fn stale_listener_issues(
    candidates: &[Candidate],
    stale: &[usize],
    selected: Option<usize>,
) -> (Vec<ResolutionIssue>, Vec<ResolutionIssue>) {
    let selected_runfile = selected.and_then(|index| candidates[index].runfile.as_ref());
    let mut conflicts = Vec::new();
    let mut unverifiable = Vec::new();
    for index in stale {
        let candidate = &candidates[*index];
        let CandidateState::Stale { listener, .. } = &candidate.state else {
            unreachable!("stale index must name a stale candidate");
        };
        match listener {
            ListenerState::Absent => {}
            ListenerState::OwnedByTarget => {
                let accounted = candidate
                    .runfile
                    .as_ref()
                    .zip(selected_runfile)
                    .is_some_and(|(stale_record, selected_record)| {
                        stale_record.pid == selected_record.pid
                            && stale_record.port == selected_record.port
                    });
                if !accounted {
                    conflicts.push(issue(
                        ResolutionIssueKind::Conflict,
                        [candidate.coordinate.clone()],
                        "stale registration still has an unreconciled listener owner",
                    ));
                }
            }
            ListenerState::OwnedByOther(owners) => {
                let accounted = candidate
                    .runfile
                    .as_ref()
                    .zip(selected_runfile)
                    .is_some_and(|(stale_record, selected_record)| {
                        stale_record.port == selected_record.port
                            && !owners.is_empty()
                            && owners.iter().all(|owner| *owner == selected_record.pid)
                    });
                if !accounted {
                    conflicts.push(issue(
                        ResolutionIssueKind::Conflict,
                        [candidate.coordinate.clone()],
                        format!(
                            "stale registration listener ownership by PIDs {owners:?} is not accounted for by the selected process"
                        ),
                    ));
                }
            }
            ListenerState::OwnedByTargetWildcard => conflicts.push(issue(
                ResolutionIssueKind::Conflict,
                [candidate.coordinate.clone()],
                "stale registration still has a wildcard/dual-stack listener",
            )),
            ListenerState::Uninspectable(detail) => unverifiable.push(issue(
                ResolutionIssueKind::Unverifiable,
                [candidate.coordinate.clone()],
                format!("stale listener ownership is uninspectable: {detail}"),
            )),
        }
    }
    (conflicts, unverifiable)
}

pub(crate) fn resolve(candidates: &[Candidate]) -> Resolution {
    if candidates.is_empty() {
        return Resolution::Empty;
    }

    // Validate every candidate before selecting a target so reversing
    // local/global/origin input order cannot change Conflict into
    // Unverifiable or permit a malformed authority coordinate.
    let mut coherence_issues = Vec::new();
    for candidate in candidates {
        match (&candidate.runfile, &candidate.state) {
            (None, CandidateState::Verified { .. } | CandidateState::Stale { .. }) => {
                coherence_issues.push(issue(
                    ResolutionIssueKind::Unverifiable,
                    [candidate.coordinate.clone()],
                    "runtime observation has no captured registration metadata",
                ));
            }
            (Some(record), _) if record.tailscale && record.tailscale_serve.is_none() => coherence_issues.push(issue(
                ResolutionIssueKind::Unverifiable,
                [candidate.coordinate.clone()],
                "registration owns durable Tailscale Serve state without endpoint-scoped ownership metadata",
            )),
            (
                Some(record),
                CandidateState::Verified {
                    identity: observed, ..
                },
            ) => {
                if record.schema_version != 2 {
                    coherence_issues.push(issue(
                        ResolutionIssueKind::Unverifiable,
                        [candidate.coordinate.clone()],
                        "a live legacy registration has no authoritative process generation",
                    ));
                } else if record.process_identity.as_ref() != Some(observed) {
                    coherence_issues.push(issue(
                        ResolutionIssueKind::Unverifiable,
                        [candidate.coordinate.clone()],
                        "observed process identity does not match captured registration authority",
                    ));
                }
            }
            _ => {}
        }
    }
    if !coherence_issues.is_empty() {
        return Resolution::Unverifiable {
            issues: coherence_issues,
        };
    }

    let unverifiable = candidates
        .iter()
        .filter_map(|candidate| match &candidate.state {
            CandidateState::Unverifiable { reason, .. } => Some(issue(
                ResolutionIssueKind::Unverifiable,
                [candidate.coordinate.clone()],
                format!("{}: {reason}", coordinate_label(&candidate.coordinate)),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !unverifiable.is_empty() {
        return Resolution::Unverifiable {
            issues: unverifiable,
        };
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

    let Some((&target, aliases)) = verified.split_first() else {
        let (conflicts, unverifiable) = stale_listener_issues(candidates, &stale, None);
        if !unverifiable.is_empty() {
            return Resolution::Unverifiable {
                issues: unverifiable,
            };
        }
        if !conflicts.is_empty() {
            return Resolution::Conflict { issues: conflicts };
        }
        return Resolution::StaleOnly { stale };
    };

    let CandidateState::Verified {
        identity: target_identity,
        ..
    } = &candidates[target].state
    else {
        unreachable!("verified target must have verified state");
    };
    let Some(target_runfile) = candidates[target].runfile.as_ref() else {
        return Resolution::Unverifiable {
            issues: vec![issue(
                ResolutionIssueKind::Unverifiable,
                [candidates[target].coordinate.clone()],
                "verified process observation has no captured registration metadata",
            )],
        };
    };

    let coherent_live_group = aliases.iter().all(|index| {
        let candidate = &candidates[*index];
        let CandidateState::Verified {
            identity: alias_identity,
            ..
        } = &candidate.state
        else {
            unreachable!("verified alias must have verified state");
        };
        alias_identity == target_identity
            && candidate
                .runfile
                .as_ref()
                .is_some_and(|runfile| runfile.same_lifecycle_authority(target_runfile))
    });
    if !coherent_live_group {
        let mut coordinates = verified
            .iter()
            .map(|index| candidates[*index].coordinate.clone())
            .collect::<Vec<_>>();
        coordinates.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.scope.to_string().cmp(&right.scope.to_string()))
        });
        return Resolution::Conflict {
            issues: vec![issue(
                ResolutionIssueKind::Conflict,
                coordinates,
                "captured registrations do not form one coherent live process-key and metadata group",
            )],
        };
    }

    let mut conflicts = Vec::new();
    let (stale_conflicts, stale_unverifiable) =
        stale_listener_issues(candidates, &stale, Some(target));
    conflicts.extend(stale_conflicts);
    if !stale_unverifiable.is_empty() {
        return Resolution::Unverifiable {
            issues: stale_unverifiable,
        };
    }
    if !conflicts.is_empty() {
        return Resolution::Conflict { issues: conflicts };
    }

    let (listener, health) = match listener_and_health(candidates, &verified) {
        Ok(facts) => facts,
        Err(issue) => {
            return Resolution::Unverifiable {
                issues: vec![issue],
            };
        }
    };
    let alias_indices = aliases.to_vec();
    match &listener {
        ListenerState::OwnedByOther(owners) => Resolution::Conflict {
            issues: vec![issue(
                ResolutionIssueKind::Conflict,
                verified
                    .iter()
                    .map(|index| candidates[*index].coordinate.clone()),
                format!("registered listener is shared with or owned by PIDs {owners:?}"),
            )],
        },
        ListenerState::Uninspectable(detail) => Resolution::Unverifiable {
            issues: vec![issue(
                ResolutionIssueKind::Unverifiable,
                verified
                    .iter()
                    .map(|index| candidates[*index].coordinate.clone()),
                format!("registered listener ownership is uninspectable: {detail}"),
            )],
        },
        ListenerState::OwnedByTarget if health == HealthState::Healthy => Resolution::Ready {
            target,
            aliases: alias_indices,
            stale,
        },
        ListenerState::OwnedByTarget
        | ListenerState::OwnedByTargetWildcard
        | ListenerState::Absent => {
            let detail = match &listener {
                ListenerState::OwnedByTarget if health == HealthState::Unhealthy => {
                    "the exact loopback listener is not HTTP-healthy"
                }
                ListenerState::OwnedByTarget => "HTTP health has not been probed",
                ListenerState::OwnedByTargetWildcard => {
                    "the selected process owns a wildcard/dual-stack listener"
                }
                ListenerState::Absent => "the selected process has no registered listener",
                _ => unreachable!(),
            };
            Resolution::Degraded {
                target,
                aliases: alias_indices,
                stale,
                listener,
                health,
                issues: vec![issue(
                    ResolutionIssueKind::Degraded,
                    verified
                        .iter()
                        .map(|index| candidates[*index].coordinate.clone()),
                    detail,
                )],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Engine, RUNFILE_SCHEMA_V2};
    use crate::server_process::canonical_test_start_token;
    use crate::server_registration::RegistrationScope;
    use std::path::PathBuf;

    fn coordinate(scope: RegistrationScope, name: &str) -> RegistrationCoordinate {
        RegistrationCoordinate {
            scope,
            path: PathBuf::from(format!("/fixture/{name}/server.json")),
        }
    }

    fn identity(value: u64) -> ProcessIdentity {
        ProcessIdentity {
            start_token: canonical_test_start_token(value),
            executable: PathBuf::from("/fixture/llama-server"),
            argv: vec!["llama-server".to_string(), "--serve".to_string()],
        }
    }

    fn runfile(value: u64) -> ServerRunfile {
        ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: u32::try_from(4000 + value).unwrap(),
            port: u16::try_from(8000 + value).unwrap(),
            base_url: format!("http://127.0.0.1:{}/v1", 8000 + value),
            tailscale: false,
            tailscale_serve: None,
            model: Some("model.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(identity(value)),
            origin_local_runfile: Some(PathBuf::from("/fixture/workspace/.ferric/server.json")),
        }
    }

    fn verified(scope: RegistrationScope, name: &str, record: ServerRunfile) -> Candidate {
        Candidate {
            coordinate: coordinate(scope, name),
            state: CandidateState::Verified {
                identity: record.process_identity.clone().unwrap(),
                listener: ListenerState::OwnedByTarget,
                health: HealthState::Healthy,
            },
            runfile: Some(record),
        }
    }

    fn stale(
        scope: RegistrationScope,
        name: &str,
        record: ServerRunfile,
        listener: ListenerState,
    ) -> Candidate {
        Candidate {
            coordinate: coordinate(scope, name),
            runfile: Some(record),
            state: CandidateState::Stale {
                reason: "process creation token changed".to_string(),
                observed_identity: None,
                listener,
            },
        }
    }

    fn unverifiable(scope: RegistrationScope, name: &str, reason: &str) -> Candidate {
        Candidate {
            coordinate: coordinate(scope, name),
            runfile: None,
            state: CandidateState::Unverifiable {
                reason: reason.to_string(),
                observed_identity: None,
                listener: None,
                health: HealthState::NotProbed,
            },
        }
    }

    #[test]
    fn registration_resolution_cross_workspace_matrix() {
        assert_eq!(resolve(&[]), Resolution::Empty);

        let record = runfile(1);
        assert!(matches!(
            resolve(&[verified(
                RegistrationScope::Global,
                "global",
                record.clone()
            )]),
            Resolution::Ready { target: 0, .. }
        ));
        assert!(matches!(
            resolve(&[
                verified(RegistrationScope::Local, "local", record.clone()),
                verified(RegistrationScope::Global, "global", record.clone()),
            ]),
            Resolution::Ready { aliases, .. } if aliases == [1]
        ));

        for candidates in [
            vec![
                stale(
                    RegistrationScope::Local,
                    "local",
                    runfile(2),
                    ListenerState::Absent,
                ),
                verified(RegistrationScope::Global, "global", record.clone()),
            ],
            vec![
                verified(RegistrationScope::Local, "local", record.clone()),
                stale(
                    RegistrationScope::Global,
                    "global",
                    runfile(2),
                    ListenerState::Absent,
                ),
            ],
        ] {
            assert!(matches!(
                resolve(&candidates),
                Resolution::Ready { stale, .. } if stale.len() == 1
            ));
        }

        assert!(matches!(
            resolve(&[
                stale(
                    RegistrationScope::Local,
                    "local",
                    runfile(1),
                    ListenerState::Absent,
                ),
                stale(
                    RegistrationScope::Global,
                    "global",
                    runfile(2),
                    ListenerState::Absent,
                ),
            ]),
            Resolution::StaleOnly { stale } if stale == [0, 1]
        ));

        assert!(matches!(
            resolve(&[
                verified(RegistrationScope::Local, "local", runfile(1)),
                verified(RegistrationScope::Global, "global", runfile(2)),
            ]),
            Resolution::Conflict { .. }
        ));

        let baseline = runfile(1);
        let mut metadata_variants = Vec::new();
        let mut changed = baseline.clone();
        changed.engine = Engine::Ollama;
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.pid += 1;
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.port += 1;
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.base_url.push_str("/changed");
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.origin_local_runfile = Some(PathBuf::from("/fixture/other/server.json"));
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.model = Some("other.gguf".to_string());
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.context_size = Some(4096);
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.sampling_seed = Some(7);
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.parallel_slots = Some(2);
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed.process_identity.as_mut().unwrap().executable =
            PathBuf::from("/fixture/other-llama-server");
        metadata_variants.push(changed);
        let mut changed = baseline.clone();
        changed
            .process_identity
            .as_mut()
            .unwrap()
            .argv
            .push("--different-coordinate".to_string());
        metadata_variants.push(changed);
        for changed in metadata_variants {
            assert!(matches!(
                resolve(&[
                    verified(RegistrationScope::Local, "local", baseline.clone()),
                    verified(RegistrationScope::Global, "global", changed),
                ]),
                Resolution::Conflict { .. }
            ));
        }

        let mut legacy_live = baseline.clone();
        legacy_live.schema_version = 1;
        assert!(matches!(
            resolve(&[verified(
                RegistrationScope::Local,
                "legacy-live",
                legacy_live
            )]),
            Resolution::Unverifiable { .. }
        ));
        let mut tailscale = baseline.clone();
        tailscale.tailscale = true;
        assert!(matches!(
            resolve(&[verified(
                RegistrationScope::Global,
                "tailscale",
                tailscale.clone()
            )]),
            Resolution::Unverifiable { .. }
        ));
        let token = "00112233445566778899aabbccddeeff";
        let fqdn = "example-host.tailnet-example.ts.net";
        tailscale.tailscale_serve = Some(crate::tailscale_serve::TailscaleServeOwnership {
            version: crate::tailscale_serve::OWNERSHIP_VERSION,
            token: token.to_string(),
            stable_node_id: "node-fixture".to_string(),
            fqdn: fqdn.to_string(),
            https_port: crate::tailscale_serve::HTTPS_PORT,
            mount_path: format!("/_ferric/{token}"),
            proxy_target: format!("http://127.0.0.1:{}", tailscale.port),
            remote_base_url: format!("https://{fqdn}/_ferric/{token}/v1"),
            before_status_sha256: "a".repeat(64),
            tcp_map_preexisting: false,
            tcp_https_preexisting: false,
            web_map_preexisting: false,
            web_host_preexisting: false,
            apply_confirmed: true,
        });
        assert!(matches!(
            resolve(&[verified(
                RegistrationScope::Global,
                "owned-tailscale",
                tailscale
            )]),
            Resolution::Ready { .. }
        ));

        for reason in [
            "malformed registration",
            "unreadable registration",
            "live schema-1 registration",
            "missing promised origin",
            "durable Tailscale state",
        ] {
            assert!(matches!(
                resolve(&[
                    verified(RegistrationScope::Local, "local", baseline.clone()),
                    unverifiable(RegistrationScope::Global, "global", reason),
                ]),
                Resolution::Unverifiable { .. }
            ));
        }

        let mut changed_origin = baseline.clone();
        changed_origin.context_size = Some(4096);
        assert!(matches!(
            resolve(&[
                verified(RegistrationScope::Global, "global", baseline.clone()),
                verified(RegistrationScope::Origin, "origin", changed_origin),
            ]),
            Resolution::Conflict { .. }
        ));

        let mut selected = baseline.clone();
        selected.pid = 4100;
        selected.port = 8100;
        let live = verified(RegistrationScope::Global, "global", selected.clone());
        let mut accounted_stale = runfile(3);
        accounted_stale.pid = selected.pid;
        accounted_stale.port = selected.port;
        for listener in [
            ListenerState::OwnedByTarget,
            ListenerState::OwnedByOther(vec![selected.pid]),
        ] {
            assert!(matches!(
                resolve(&[
                    stale(
                        RegistrationScope::Local,
                        "local",
                        accounted_stale.clone(),
                        listener,
                    ),
                    live.clone(),
                ]),
                Resolution::Ready { .. }
            ));
        }
        for listener in [
            ListenerState::OwnedByOther(vec![9999]),
            ListenerState::OwnedByOther(vec![selected.pid, 9999]),
            ListenerState::OwnedByTargetWildcard,
        ] {
            assert!(matches!(
                resolve(&[
                    stale(RegistrationScope::Local, "local", runfile(3), listener,),
                    live.clone(),
                ]),
                Resolution::Conflict { .. }
            ));
        }
        assert!(matches!(
            resolve(&[
                stale(
                    RegistrationScope::Local,
                    "local",
                    runfile(3),
                    ListenerState::OwnedByTarget,
                ),
                live.clone(),
            ]),
            Resolution::Conflict { .. }
        ));
        assert!(matches!(
            resolve(&[
                stale(
                    RegistrationScope::Local,
                    "local",
                    runfile(3),
                    ListenerState::OwnedByOther(vec![selected.pid]),
                ),
                live.clone(),
            ]),
            Resolution::Conflict { .. }
        ));
        assert!(matches!(
            resolve(&[
                stale(
                    RegistrationScope::Local,
                    "local",
                    accounted_stale.clone(),
                    ListenerState::OwnedByOther(Vec::new()),
                ),
                live.clone(),
            ]),
            Resolution::Conflict { .. }
        ));
        assert!(matches!(
            resolve(&[
                stale(
                    RegistrationScope::Local,
                    "local",
                    runfile(3),
                    ListenerState::Uninspectable("permission denied".to_string()),
                ),
                live,
            ]),
            Resolution::Unverifiable { .. }
        ));

        assert!(matches!(
            resolve(&[stale(
                RegistrationScope::Local,
                "stale-owned",
                runfile(3),
                ListenerState::OwnedByTarget,
            )]),
            Resolution::Conflict { .. }
        ));

        for listener in [
            ListenerState::OwnedByOther(vec![9999]),
            ListenerState::OwnedByTargetWildcard,
        ] {
            assert!(matches!(
                resolve(&[stale(
                    RegistrationScope::Local,
                    "stale-blocked",
                    runfile(3),
                    listener,
                )]),
                Resolution::Conflict { .. }
            ));
        }
        assert!(matches!(
            resolve(&[stale(
                RegistrationScope::Local,
                "stale-uninspectable",
                runfile(3),
                ListenerState::Uninspectable("permission denied".to_string()),
            )]),
            Resolution::Unverifiable { .. }
        ));

        let live_a = verified(RegistrationScope::Local, "a", runfile(10));
        let live_b = verified(RegistrationScope::Global, "b", runfile(11));
        let stale_accounted_only_by_b = stale(
            RegistrationScope::Origin,
            "stale",
            runfile(12),
            ListenerState::OwnedByOther(vec![live_b.runfile.as_ref().unwrap().pid]),
        );
        let first = resolve(&[
            stale_accounted_only_by_b.clone(),
            live_a.clone(),
            live_b.clone(),
        ]);
        let second = resolve(&[stale_accounted_only_by_b, live_b, live_a]);
        assert_eq!(
            first, second,
            "live conflicts must precede target selection"
        );

        // These inventory-level blockers are part of E03-A as well. Calling
        // their shared rows here keeps the frozen exact-name command honest:
        // it must prove both present/absent Tailscale PIDs make zero process
        // calls and missing/changed origins block before acquisition.
        crate::server::tests::legacy_tailscale_registration_remains_unowned();
        crate::server::tests::promised_origin_static_matrix_precedes_process_inspection();
    }

    #[test]
    fn degraded_listener_and_health_states_remain_typed() {
        let mut candidate = verified(RegistrationScope::Local, "local", runfile(1));
        candidate.state = CandidateState::Verified {
            identity: candidate
                .runfile
                .as_ref()
                .unwrap()
                .process_identity
                .clone()
                .unwrap(),
            listener: ListenerState::OwnedByTargetWildcard,
            health: HealthState::NotProbed,
        };
        assert!(matches!(resolve(&[candidate]), Resolution::Degraded { .. }));
    }
}
