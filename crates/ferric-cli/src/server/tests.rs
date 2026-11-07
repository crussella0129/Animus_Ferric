use super::*;
use crate::server::fixtures::{
    ScriptedPersistenceEffects, process_identity, retired_tailscale_serve,
};
use crate::server_process::canonical_test_start_token;
use crate::server_registration::{
    PersistencePhase, PromisedOriginRegistration, RegistrationBlock, capture_registration_path,
    publish_mirrored_with,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::net::TcpListener;
use std::rc::Rc;
use std::thread;

fn spawn_contained_test_child(command: &mut Command, label: &str) -> ferric_process::ProcessTree {
    ferric_process::testing::spawn(command).unwrap_or_else(|error| panic!("spawn {label}: {error}"))
}

impl SpawnedChild for ferric_process::ProcessTree {
    type ExitStatus = std::process::ExitStatus;

    fn pid(&self) -> u32 {
        self.child().id()
    }

    fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String> {
        self.try_wait_leader().map_err(|error| error.to_string())
    }

    fn wait(&mut self) -> Result<Self::ExitStatus, String> {
        self.wait_for_exit(Duration::from_secs(5))
            .map_err(|error| error.to_string())
    }

    fn kill(&mut self) -> Result<(), String> {
        self.terminate_leader().map_err(|error| error.to_string())
    }
}

fn cfg(engine: Engine) -> ServerUpArgs {
    ServerUpArgs {
        engine,
        model: Some("model.gguf".to_string()),
        mmproj: None,
        ctx: 4096,
        port: 8080,
        threads: None,
        gpu_layers: None,
        batch_size: None,
        seed: None,
        parallel: None,
    }
}

fn discovery_fixture_path(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"C:\fixture\{name}\.ferric\server.json"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(format!("/fixture/{name}/.ferric/server.json"))
    }
}

fn discovery_fixture_runfile(pid: u32, name: &str) -> ServerRunfile {
    let port = u16::try_from(7000 + pid % 1000).unwrap();
    ServerRunfile {
        schema_version: RUNFILE_SCHEMA_V2,
        engine: Engine::LlamaServer,
        pid,
        port,
        base_url: format!("http://127.0.0.1:{port}/v1"),
        tailscale: false,
        tailscale_serve: None,
        model: Some("model.gguf".to_string()),
        context_size: Some(8192),
        sampling_seed: Some(42),
        parallel_slots: Some(1),
        process_identity: Some(process_identity(u64::from(pid))),
        origin_local_runfile: Some(discovery_fixture_path(name)),
    }
}

fn discovery_fixture_capture(
    scope: RegistrationScope,
    pid: u32,
    name: &str,
) -> CapturedRegistration {
    let runfile = discovery_fixture_runfile(pid, name);
    CapturedRegistration {
        scope,
        path: discovery_fixture_path(name),
        raw: serde_json::to_vec_pretty(&runfile).unwrap(),
        runfile,
    }
}

fn scripted_remove_event(capture: &CapturedRegistration) -> LifecycleEvent {
    LifecycleEvent::Remove(
        capture.path.clone(),
        ferric_bench::sha256_bytes(&capture.raw),
    )
}

fn discovery_fixture_coordinate(scope: RegistrationScope, name: &str) -> RegistrationCoordinate {
    RegistrationCoordinate {
        scope,
        path: discovery_fixture_path(name),
    }
}

fn discovery_fixture_observation(
    coordinate: RegistrationCoordinate,
    runfile: ServerRunfile,
    runtime: RuntimeObservation,
) -> ManagedRegistrationObservation {
    ManagedRegistrationObservation {
        coordinate,
        promised: None,
        state: ManagedRegistrationState::Captured {
            runfile: Box::new(runfile),
            raw_sha256: "fixture-revision".to_string(),
            runtime,
        },
    }
}

fn discovery_fixture_empty() -> ManagedServerDiscovery {
    let local = discovery_fixture_coordinate(RegistrationScope::Local, "local");
    let global = discovery_fixture_coordinate(RegistrationScope::Global, "global");
    ManagedServerDiscovery {
        inventory: RegistrationInventory {
            local: RegistrationSlot::Absent {
                scope: local.scope,
                path: local.path.clone(),
            },
            global: Some(RegistrationSlot::Absent {
                scope: global.scope,
                path: global.path.clone(),
            }),
            promised_origins: Vec::new(),
        },
        observations: vec![
            ManagedRegistrationObservation {
                coordinate: local,
                promised: None,
                state: ManagedRegistrationState::Absent,
            },
            ManagedRegistrationObservation {
                coordinate: global,
                promised: None,
                state: ManagedRegistrationState::Absent,
            },
        ],
        state: ManagedServerState::Empty,
    }
}

fn discovery_fixture_ready() -> ManagedServerDiscovery {
    let coordinate = discovery_fixture_coordinate(RegistrationScope::Local, "local");
    let runfile = discovery_fixture_runfile(4101, "local");
    let identity = runfile.process_identity.clone().unwrap();
    let raw = serde_json::to_vec(&runfile).unwrap();
    let observation = ManagedRegistrationObservation {
        coordinate: coordinate.clone(),
        promised: None,
        state: ManagedRegistrationState::Captured {
            runfile: Box::new(runfile.clone()),
            raw_sha256: ferric_bench::sha256_bytes(&raw),
            runtime: RuntimeObservation::Verified {
                identity: identity.clone(),
                listener: ListenerState::OwnedByTarget,
                health: HealthState::Healthy,
            },
        },
    };
    let revision = RegistrationRevision {
        coordinate: coordinate.clone(),
        promised: None,
        state: RegistrationRevisionState::Captured(ferric_bench::sha256_bytes(&raw)),
    };
    let server = ManagedServer {
        registration: coordinate.clone(),
        runfile: runfile.clone(),
        identity: identity.clone(),
        listener: ListenerState::OwnedByTarget,
        health: HealthState::Healthy,
        aliases: Vec::new(),
        stale: Vec::new(),
        fingerprint: DiscoveryFingerprint {
            pid: runfile.pid,
            identity,
            runfile: runfile.clone(),
            revisions: vec![revision],
        },
    };
    ManagedServerDiscovery {
        inventory: RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: RegistrationScope::Local,
                path: coordinate.path.clone(),
                raw,
                runfile,
            })),
            global: None,
            promised_origins: Vec::new(),
        },
        observations: vec![observation],
        state: ManagedServerState::Ready(server),
    }
}

fn discovery_fixture_degraded(
    listener: ListenerState,
    health: HealthState,
) -> ManagedServerDiscovery {
    let mut discovery = discovery_fixture_ready();
    let mut server = match &discovery.state {
        ManagedServerState::Ready(server) => server.clone(),
        _ => unreachable!(),
    };
    server.listener = listener.clone();
    server.health = health;
    let coordinate = server.registration.clone();
    if let ManagedRegistrationState::Captured { runtime, .. } = &mut discovery.observations[0].state
    {
        *runtime = RuntimeObservation::Verified {
            identity: server.identity.clone(),
            listener: listener.clone(),
            health,
        };
    }
    discovery.state = ManagedServerState::Degraded {
        server,
        issues: vec![ResolutionIssue {
            coordinates: vec![coordinate],
            kind: ResolutionIssueKind::Degraded,
            detail: "fixture degraded state".to_string(),
        }],
    };
    discovery
}

fn discovery_fixture_stale_only() -> ManagedServerDiscovery {
    let mut discovery = discovery_fixture_ready();
    let coordinate = discovery.observations[0].coordinate.clone();
    if let ManagedRegistrationState::Captured { runtime, .. } = &mut discovery.observations[0].state
    {
        *runtime = RuntimeObservation::Stale {
            reason: "PID is absent".to_string(),
            observed_identity: None,
            listener: ListenerState::Absent,
        };
    }
    discovery.state = ManagedServerState::StaleOnly {
        stale: vec![coordinate],
    };
    discovery
}

fn discovery_fixture_blocked(conflict: bool) -> ManagedServerDiscovery {
    let mut discovery = discovery_fixture_ready();
    let coordinate = discovery.observations[0].coordinate.clone();
    let issue = ResolutionIssue {
        coordinates: vec![coordinate],
        kind: if conflict {
            ResolutionIssueKind::Conflict
        } else {
            ResolutionIssueKind::Unverifiable
        },
        detail: if conflict {
            "fixture registration conflict"
        } else {
            "fixture registration is unverifiable"
        }
        .to_string(),
    };
    if conflict {
        discovery.state = ManagedServerState::Conflict {
            issues: vec![issue],
        };
    } else {
        if let ManagedRegistrationState::Captured { runtime, .. } =
            &mut discovery.observations[0].state
        {
            *runtime = RuntimeObservation::Unverifiable {
                reason: "fixture process inspection failed".to_string(),
                observed_identity: None,
                listener: None,
                health: HealthState::NotProbed,
            };
        }
        discovery.state = ManagedServerState::Unverifiable {
            issues: vec![issue],
        };
    }
    discovery
}

fn verified_not_probed_observation(capture: CapturedRegistration) -> LifecycleObservation {
    let identity = capture.runfile.process_identity.clone().unwrap();
    LifecycleObservation {
        candidate: Candidate {
            coordinate: capture.coordinate(),
            runfile: Some(capture.runfile.clone()),
            state: CandidateState::Verified {
                identity,
                listener: ListenerState::OwnedByTarget,
                health: HealthState::NotProbed,
            },
        },
        capture: Some(capture),
        process: None,
    }
}

struct PanicHealth;

impl HealthProbe for PanicHealth {
    fn status_ok(&mut self, _host: &str, _port: u16, _path: &str) -> bool {
        panic!("static registration blockers must precede HTTP health")
    }
}

#[test]
fn retired_registrations_block_status_down_and_doctor_without_effects() {
    // Schema-1 records and every record shape the retired `server up
    // --tailscale` wrote are unverifiable with manual cleanup steps. Nothing
    // is acquired, probed, signalled or deleted, in any command.
    let schema_cleanup = "has schema version 1; only schema 2 is managed, so";
    let tailscale_cleanup = "records a Tailscale Serve publication, which Ferric no longer manages; turn that serve off by hand (for example `tailscale serve --https=443 off`), then";
    let mut schema_1 = discovery_fixture_runfile(4201, "schema-1");
    schema_1.schema_version = 1;
    schema_1.process_identity = None;
    schema_1.origin_local_runfile = None;
    let mut boolean_only = discovery_fixture_runfile(4202, "tailscale-boolean");
    boolean_only.tailscale = true;
    let mut owned = discovery_fixture_runfile(4203, "tailscale-owned");
    owned.tailscale = true;
    owned.tailscale_serve = Some(retired_tailscale_serve());

    let root = tempfile::tempdir().unwrap();
    for (case, mut runfile, cleanup) in [
        ("schema-1", schema_1, schema_cleanup),
        ("tailscale-boolean", boolean_only, tailscale_cleanup),
        ("tailscale-owned", owned, tailscale_cleanup),
    ] {
        let workspace = root.path().join(case);
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let global = root
            .path()
            .join(format!("{case}-config"))
            .join("server.json");
        if runfile.origin_local_runfile.is_some() {
            runfile.origin_local_runfile = Some(local.clone());
        }
        write_test_runfile(&local, &runfile);
        write_test_runfile(&global, &runfile);
        let originals = [
            (local.clone(), fs::read(&local).unwrap()),
            (global.clone(), fs::read(&global).unwrap()),
        ];
        let diagnostic = |scope: &str, path: &Path| {
            format!(
                "{scope} registration {}: {cleanup} stop PID {} and delete {}",
                path.display(),
                runfile.pid,
                path.display()
            )
        };
        let local_detail = diagnostic("local", &local);
        let global_detail = diagnostic("global", &global);
        let diagnostics = vec![
            format!("[diagnostic] {local_detail}"),
            format!("[diagnostic] {global_detail}"),
        ];
        let next = "[next] repair or make readable the 2 unverifiable registration coordinate(s), then rerun `ferric server status` (no process action is authorized)";

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let calls = Rc::clone(&ledger);
        let discovery = complete_lifecycle_health_with(
            discover_inventory_before_health_with(
                inventory_runfiles(&workspace, Some(global.clone())),
                move |capture| {
                    calls
                        .borrow_mut()
                        .push(LifecycleEvent::Acquire(capture.runfile.pid));
                    panic!("a retired registration must block before process acquisition")
                },
            ),
            &mut PanicHealth,
            |_observation| panic!("a retired registration must block before reinspection"),
        );
        assert!(ledger.borrow().is_empty(), "{case}");
        assert!(matches!(
            discovery.managed.state,
            ManagedServerState::Unverifiable { .. }
        ));

        let status = render_status(&status_report(&discovery.managed));
        let held_facts = "recorded-identity=unavailable observed-identity=not-inspected listener=not-inspected health=not-probed";
        assert_eq!(
            status.stdout,
            vec![
                format!("[blocked] {local_detail}; {held_facts}"),
                format!("[blocked] {global_detail}; {held_facts}"),
                "[state] unverifiable".to_string(),
                next.to_string(),
            ],
            "{case}"
        );
        assert_eq!(status.stderr, diagnostics, "{case}");
        assert!(!status.success, "{case}");

        let mut doctor_effects = RecordingDoctorEffects::default();
        let doctor = doctor_report_after_discovery(
            &doctor_fixture_args(),
            &discovery.managed,
            &mut doctor_effects,
        );
        assert!(!doctor.success, "{case}");
        assert_eq!(
            doctor.lines,
            [
                vec![
                    "[BLOCKED] managed registration state is unverifiable before engine/model probes"
                        .to_string()
                ],
                diagnostics.clone(),
                vec![next.to_string()],
            ]
            .concat(),
            "{case}"
        );
        assert!(doctor_effects.events.is_empty(), "{case}");

        let mut effects = ScriptedEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(down_plan_from_lifecycle(discovery), &mut effects);
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_eq!(report.disposition, DownDisposition::Blocked, "{case}");
        assert!(!report.success && !report.signalled, "{case}");
        assert_eq!(
            rendered.stdout,
            vec![
                format!(
                    "[held] local registration {} detail=typed discovery blocked teardown mutation",
                    local.display()
                ),
                format!(
                    "[held] global registration {} detail=typed discovery blocked teardown mutation",
                    global.display()
                ),
                "[state] teardown blocked; registrations kept".to_string(),
                next.to_string(),
            ],
            "{case}"
        );
        assert_eq!(rendered.stderr, diagnostics, "{case}");
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render], "{case}");

        // The native commands refuse the same way and leave both records.
        assert_eq!(
            status_impl(&workspace, Some(global.clone())),
            ExitCode::FAILURE
        );
        assert_eq!(
            down_impl(&workspace, Some(global.clone())),
            ExitCode::FAILURE
        );
        for (path, original) in &originals {
            assert_eq!(&fs::read(path).unwrap(), original, "{case}");
        }
    }
}

#[test]
fn promised_origin_static_matrix_precedes_process_inspection() {
    for changed in [false, true] {
        let local = discovery_fixture_coordinate(RegistrationScope::Local, "local");
        let global = discovery_fixture_coordinate(RegistrationScope::Global, "global");
        let origin = discovery_fixture_coordinate(RegistrationScope::Origin, "origin");
        let expected = discovery_fixture_runfile(4202, "origin");
        let global_capture = CapturedRegistration {
            scope: global.scope,
            path: global.path.clone(),
            raw: serde_json::to_vec(&expected).unwrap(),
            runfile: expected.clone(),
        };
        let origin_slot = if changed {
            let mut replacement = expected.clone();
            replacement.context_size = Some(4096);
            RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: origin.scope,
                path: origin.path.clone(),
                raw: serde_json::to_vec(&replacement).unwrap(),
                runfile: replacement,
            }))
        } else {
            RegistrationSlot::Absent {
                scope: origin.scope,
                path: origin.path.clone(),
            }
        };
        let inventory = RegistrationInventory {
            local: RegistrationSlot::Absent {
                scope: local.scope,
                path: local.path,
            },
            global: Some(RegistrationSlot::Captured(Box::new(global_capture))),
            promised_origins: vec![PromisedOriginRegistration {
                source: global,
                expected_runfile: expected,
                slot: origin_slot,
            }],
        };
        let result = complete_lifecycle_health_with(
            discover_inventory_before_health_with(inventory, |_capture| {
                panic!("origin blocker must precede process acquisition")
            }),
            &mut PanicHealth,
            |_observation| panic!("origin blocker must precede retained reinspection"),
        );
        if changed {
            assert!(matches!(
                result.managed.state,
                ManagedServerState::Conflict { .. }
            ));
        } else {
            assert!(matches!(
                result.managed.state,
                ManagedServerState::Unverifiable { .. }
            ));
        }
        assert_eq!(result.managed.observations.len(), 3);
        assert!(
            result
                .managed
                .observations
                .iter()
                .any(|observation| observation.coordinate.scope == RegistrationScope::Origin)
        );
    }
}

fn doctor_fixture_args() -> ServerUpArgs {
    ServerUpArgs {
        engine: Engine::LlamaServer,
        model: Some("model.gguf".to_string()),
        mmproj: None,
        ctx: 8192,
        port: 8080,
        threads: None,
        gpu_layers: None,
        batch_size: None,
        seed: Some(42),
        parallel: Some(1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorEvent {
    Binary,
    File,
}

struct RecordingDoctorEffects {
    events: Vec<DoctorEvent>,
    binary_present: bool,
    regular_file: bool,
}

impl Default for RecordingDoctorEffects {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            binary_present: true,
            regular_file: true,
        }
    }
}

impl DoctorProbeEffects for RecordingDoctorEffects {
    fn binary_present(&mut self, _engine: Engine) -> bool {
        self.events.push(DoctorEvent::Binary);
        self.binary_present
    }

    fn regular_file(&mut self, _path: &Path) -> bool {
        self.events.push(DoctorEvent::File);
        self.regular_file
    }
}

#[test]
fn status_registration_lines_are_golden_for_every_runtime_observation() {
    let local = discovery_fixture_coordinate(RegistrationScope::Local, "local");
    let path = local.path.display().to_string();
    let identity = process_identity(4101);
    let identity_text = format!(
        "token={} executable={} argv=[\"llama-server\", \"--serve\"]",
        identity.start_token,
        identity.executable.display()
    );
    let line = |runfile: ServerRunfile, runtime| {
        render_registration_status(&discovery_fixture_observation(
            local.clone(),
            runfile,
            runtime,
        ))
    };
    let captured = format!(
        "[captured] local registration {path}: schema=2 engine=LlamaServer pid=4101 base-url=http://127.0.0.1:7101/v1"
    );
    let rows = [
        (
            RuntimeObservation::NotInspected,
            "observed-identity=not-inspected listener=not-inspected health=not-probed".to_string(),
        ),
        (
            RuntimeObservation::Verified {
                identity: identity.clone(),
                listener: ListenerState::OwnedByTarget,
                health: HealthState::Healthy,
            },
            format!("observed-identity={identity_text} listener=owned-loopback health=healthy"),
        ),
        (
            RuntimeObservation::Verified {
                identity: identity.clone(),
                listener: ListenerState::OwnedByTargetWildcard,
                health: HealthState::Unhealthy,
            },
            format!("observed-identity={identity_text} listener=wildcard-public health=unhealthy"),
        ),
        (
            RuntimeObservation::Stale {
                reason: "generation changed".to_string(),
                observed_identity: Some(identity.clone()),
                listener: ListenerState::Absent,
            },
            format!(
                "observed-identity={identity_text} stale-reason=generation changed listener=absent health=not-probed"
            ),
        ),
        (
            RuntimeObservation::Stale {
                reason: "process exited".to_string(),
                observed_identity: None,
                listener: ListenerState::OwnedByOther(vec![9, 4101]),
            },
            "observed-identity=stale-unavailable stale-reason=process exited listener=foreign-or-shared:[9, 4101] health=not-probed".to_string(),
        ),
        (
            RuntimeObservation::Unverifiable {
                reason: "probe failed".to_string(),
                observed_identity: Some(identity.clone()),
                listener: Some(ListenerState::Uninspectable("denied".to_string())),
                health: HealthState::NotProbed,
            },
            format!(
                "observed-identity={identity_text} unverifiable-reason=probe failed listener=uninspectable:denied health=not-probed"
            ),
        ),
        (
            RuntimeObservation::Unverifiable {
                reason: "no access".to_string(),
                observed_identity: None,
                listener: None,
                health: HealthState::Unhealthy,
            },
            "observed-identity=unavailable unverifiable-reason=no access listener=unverifiable health=unhealthy".to_string(),
        ),
    ];
    for (runtime, observed) in rows {
        assert_eq!(
            line(discovery_fixture_runfile(4101, "local"), runtime),
            format!("{captured} recorded-identity={identity_text} {observed}")
        );
    }

    let global = discovery_fixture_coordinate(RegistrationScope::Global, "global");
    let origin = discovery_fixture_coordinate(RegistrationScope::Origin, "origin");
    let promised_absent = ManagedRegistrationObservation {
        coordinate: origin.clone(),
        promised: Some(PromisedOriginProvenance {
            source: global.clone(),
            expected_runfile: discovery_fixture_runfile(4101, "origin"),
        }),
        state: ManagedRegistrationState::Absent,
    };
    assert_eq!(
        render_registration_status(&promised_absent),
        format!(
            "[absent] origin registration {} promised-by=global registration {}: recorded-identity=none observed-identity=none listener=none health=none",
            origin.path.display(),
            global.path.display()
        )
    );
    let blocked = ManagedRegistrationObservation {
        coordinate: local.clone(),
        promised: None,
        state: ManagedRegistrationState::Blocked {
            reason: "unreadable registration".to_string(),
        },
    };
    assert_eq!(
        render_registration_status(&blocked),
        format!(
            "[blocked] local registration {path}: unreadable registration; recorded-identity=unavailable observed-identity=not-inspected listener=not-inspected health=not-probed"
        )
    );
}

fn assert_status_matrix_row(
    discovery: &ManagedServerDiscovery,
    expected_action: StatusNextAction,
    expected_state: &str,
    expected_next: &str,
    expected_stderr: &[String],
    expected_success: bool,
) {
    let before = discovery.clone();
    let report = status_report(discovery);
    assert_eq!(discovery, &before, "status reporting must be pure");
    assert_eq!(report.registrations, discovery.observations);
    assert_eq!(report.state, discovery.state);
    assert_eq!(report.next_action, expected_action);
    assert_eq!(report.success, expected_success);

    let rendered = render_status(&report);
    let mut expected_stdout = discovery
        .observations
        .iter()
        .map(render_registration_status)
        .collect::<Vec<_>>();
    expected_stdout.push(expected_state.to_string());
    expected_stdout.push(expected_next.to_string());
    assert_eq!(rendered.stdout, expected_stdout);
    assert_eq!(rendered.stderr, expected_stderr);
    assert_eq!(rendered.success, expected_success);
}

#[test]
fn status_reports_scope_identity_health_and_next_action() {
    let empty = discovery_fixture_empty();
    let empty_report = status_report(&empty);
    let empty_rendered = render_status(&empty_report);
    assert_eq!(empty_report.state, ManagedServerState::Empty);
    assert_eq!(empty_report.next_action, StatusNextAction::StartServer);
    assert_eq!(empty_rendered.stderr, Vec::<String>::new());
    assert_eq!(
            empty_rendered.stdout,
            vec![
                format!(
                    "[absent] local registration {}: recorded-identity=none observed-identity=none listener=none health=none",
                    discovery_fixture_path("local").display()
                ),
                format!(
                    "[absent] global registration {}: recorded-identity=none observed-identity=none listener=none health=none",
                    discovery_fixture_path("global").display()
                ),
                "[state] empty".to_string(),
                "[next] no registration is active; inspect required launch options with `ferric server up --help`".to_string(),
            ]
        );

    let ready = discovery_fixture_ready();
    let ready_report = status_report(&ready);
    let ready_rendered = render_status(&ready_report);
    assert!(matches!(
        ready_report.next_action,
        StatusNextAction::ContinueManaged { .. }
    ));
    assert!(ready_rendered.success);
    assert!(ready_rendered.stdout[0].contains("recorded-identity=token="));
    assert!(ready_rendered.stdout[0].contains("observed-identity=token="));
    assert!(ready_rendered.stdout[0].contains("listener=owned-loopback"));
    assert!(ready_rendered.stdout[0].contains("health=healthy"));

    let unhealthy =
        discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy);
    assert!(matches!(
        status_report(&unhealthy).next_action,
        StatusNextAction::StopManaged { .. }
    ));
    let absent = discovery_fixture_degraded(ListenerState::Absent, HealthState::NotProbed);
    assert!(matches!(
        status_report(&absent).next_action,
        StatusNextAction::StopManaged { .. }
    ));
    let wildcard =
        discovery_fixture_degraded(ListenerState::OwnedByTargetWildcard, HealthState::Healthy);
    assert!(matches!(
        status_report(&wildcard).next_action,
        StatusNextAction::InspectWildcard { .. }
    ));
    assert!(
        render_status(&status_report(&wildcard))
            .stdout
            .last()
            .unwrap()
            .contains("teardown is not authorized")
    );

    let stale = discovery_fixture_stale_only();
    assert_eq!(
        status_report(&stale).next_action,
        StatusNextAction::CleanStale
    );

    let mut split = discovery_fixture_ready();
    let stale_coordinate = discovery_fixture_coordinate(RegistrationScope::Global, "stale-global");
    let stale_record = discovery_fixture_runfile(4102, "stale-global");
    let stale_raw = serde_json::to_vec(&stale_record).unwrap();
    split.inventory.global = Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
        scope: stale_coordinate.scope,
        path: stale_coordinate.path.clone(),
        raw: stale_raw,
        runfile: stale_record.clone(),
    })));
    split.observations.push(discovery_fixture_observation(
        stale_coordinate.clone(),
        stale_record,
        RuntimeObservation::Stale {
            reason: "generation changed".to_string(),
            observed_identity: Some(process_identity(4102)),
            listener: ListenerState::Absent,
        },
    ));
    if let ManagedServerState::Ready(server) = &mut split.state {
        server.stale.push(stale_coordinate);
    }
    let split_rendered = render_status(&status_report(&split));
    assert_eq!(split_rendered.stdout.len(), 4);
    assert!(split_rendered.stdout[1].contains("observed-identity=token="));
    assert!(split_rendered.stdout[1].contains("stale-reason=generation changed"));

    let conflict = discovery_fixture_blocked(true);
    assert!(matches!(
        status_report(&conflict).next_action,
        StatusNextAction::ResolveConflict { .. }
    ));
    assert_eq!(render_status(&status_report(&conflict)).stderr.len(), 1);

    let unverifiable = discovery_fixture_blocked(false);
    assert!(matches!(
        status_report(&unverifiable).next_action,
        StatusNextAction::RepairUnverifiable { .. }
    ));

    let mut missing_origin = discovery_fixture_empty();
    let origin = discovery_fixture_coordinate(RegistrationScope::Origin, "missing-origin");
    let source = discovery_fixture_coordinate(RegistrationScope::Global, "global");
    let expected = discovery_fixture_runfile(4103, "missing-origin");
    let promised = PromisedOriginProvenance {
        source: source.clone(),
        expected_runfile: expected.clone(),
    };
    missing_origin
        .observations
        .push(ManagedRegistrationObservation {
            coordinate: origin.clone(),
            promised: Some(promised),
            state: ManagedRegistrationState::Absent,
        });
    missing_origin.inventory.promised_origins = vec![PromisedOriginRegistration {
        source,
        expected_runfile: expected,
        slot: RegistrationSlot::Absent {
            scope: origin.scope,
            path: origin.path.clone(),
        },
    }];
    missing_origin.state = ManagedServerState::Unverifiable {
        issues: vec![ResolutionIssue {
            coordinates: vec![origin.clone()],
            kind: ResolutionIssueKind::Unverifiable,
            detail: "promised origin is absent".to_string(),
        }],
    };
    assert_eq!(
        status_report(&missing_origin).next_action,
        StatusNextAction::InspectPromisedOrigin {
            path: origin.path.clone()
        }
    );
    let missing_rendered = render_status(&status_report(&missing_origin));
    assert!(missing_rendered.stdout[2].contains("promised-by=global registration"));

    let local_coordinate = discovery_fixture_coordinate(RegistrationScope::Local, "local");

    assert_status_matrix_row(
        &empty,
        StatusNextAction::StartServer,
        "[state] empty",
        "[next] no registration is active; inspect required launch options with `ferric server up --help`",
        &[],
        false,
    );
    for (discovery, stale_count) in [(&ready, 0), (&split, 1)] {
        assert_status_matrix_row(
            discovery,
            StatusNextAction::ContinueManaged {
                base_url: "http://127.0.0.1:7101/v1".to_string(),
            },
            &format!(
                "[state] ready pid=4101 aliases=0 stale={stale_count} listener=owned-loopback health=healthy"
            ),
            "[next] managed server is ready at http://127.0.0.1:7101/v1; continue with the intended Ferric command and omit `--api-base` to use it",
            &[],
            true,
        );
    }
    assert_status_matrix_row(
        &unhealthy,
        StatusNextAction::StopManaged { pid: 4101 },
        "[state] degraded pid=4101 listener=owned-loopback health=unhealthy",
        "[next] managed PID 4101 is identity-authorized for recovery; run `ferric server down`",
        &["[diagnostic] fixture degraded state".to_string()],
        false,
    );
    assert_status_matrix_row(
        &absent,
        StatusNextAction::StopManaged { pid: 4101 },
        "[state] degraded pid=4101 listener=absent health=not-probed",
        "[next] managed PID 4101 is identity-authorized for recovery; run `ferric server down`",
        &["[diagnostic] fixture degraded state".to_string()],
        false,
    );
    assert_status_matrix_row(
        &wildcard,
        StatusNextAction::InspectWildcard { port: 7101 },
        "[state] degraded pid=4101 listener=wildcard-public health=healthy",
        "[next] port 7101 is wildcard/public; reconfigure it to bind only 127.0.0.1, then rerun `ferric server status` (teardown is not authorized)",
        &["[diagnostic] fixture degraded state".to_string()],
        false,
    );
    assert_status_matrix_row(
        &stale,
        StatusNextAction::CleanStale,
        "[state] stale-only registrations=1",
        "[next] only cleanup-safe stale registrations remain; run `ferric server down`",
        &[],
        false,
    );
    assert_status_matrix_row(
        &conflict,
        StatusNextAction::ResolveConflict {
            coordinates: vec![local_coordinate.clone()],
        },
        "[state] conflict",
        "[next] resolve the 1 conflicting registration coordinate(s) without signalling a process, then rerun `ferric server status`",
        &["[diagnostic] fixture registration conflict".to_string()],
        false,
    );
    assert_status_matrix_row(
        &unverifiable,
        StatusNextAction::RepairUnverifiable {
            coordinates: vec![local_coordinate],
        },
        "[state] unverifiable",
        "[next] repair or make readable the 1 unverifiable registration coordinate(s), then rerun `ferric server status` (no process action is authorized)",
        &["[diagnostic] fixture registration is unverifiable".to_string()],
        false,
    );
    assert_status_matrix_row(
        &missing_origin,
        StatusNextAction::InspectPromisedOrigin {
            path: origin.path.clone(),
        },
        "[state] unverifiable",
        &format!(
            "[next] the promised origin {} is missing or changed; restore or reconcile that exact registration, then rerun `ferric server status`",
            origin.path.display()
        ),
        &["[diagnostic] promised origin is absent".to_string()],
        false,
    );
}

#[test]
fn doctor_blockers_precede_all_probes() {
    let mut effects = RecordingDoctorEffects::default();
    let mut invalid = doctor_fixture_args();
    invalid.port = 0;
    invalid.ctx = 0;
    invalid.model = None;
    invalid.parallel = Some(0);
    let discovery_calls = Rc::new(RefCell::new(0_usize));
    let calls = Rc::clone(&discovery_calls);
    let report = doctor_report_with(
        &invalid,
        move || {
            *calls.borrow_mut() += 1;
            Ok(discovery_fixture_ready())
        },
        &mut effects,
    );
    assert!(!report.success);
    assert_eq!(*discovery_calls.borrow(), 0);
    assert!(effects.events.is_empty());

    let blocked_args = doctor_fixture_args();
    for discovery in [
        discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy),
        discovery_fixture_stale_only(),
        discovery_fixture_blocked(true),
        discovery_fixture_blocked(false),
    ] {
        effects.events.clear();
        let report = doctor_report_after_discovery(&blocked_args, &discovery, &mut effects);
        assert!(!report.success);
        assert!(report.lines[0].starts_with("[BLOCKED]"));
        assert!(effects.events.is_empty());
    }

    effects.events.clear();
    let ready = doctor_report_after_discovery(
        &doctor_fixture_args(),
        &discovery_fixture_ready(),
        &mut effects,
    );
    assert!(ready.success);
    assert_eq!(effects.events, vec![DoctorEvent::Binary, DoctorEvent::File]);
}

fn assert_blocked_down_consumer(discovery: &ManagedServerDiscovery, resolution: Resolution) {
    let expected_held = discovery
        .observations
        .iter()
        .filter(|observation| !matches!(observation.state, ManagedRegistrationState::Absent))
        .count();
    assert!(
        expected_held > 0,
        "blocked fixture must carry recovery state"
    );
    let plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
        managed: discovery.clone(),
        observations: Vec::new(),
        resolution,
    });
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut effects = ScriptedEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(plan, &mut effects);
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_eq!(report.disposition, DownDisposition::Blocked);
    assert_eq!(report.registrations.len(), expected_held);
    assert!(
        report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        ))
    );
    assert!(!report.signalled);
    assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
    assert_eq!(
        *ledger.borrow(),
        vec![LifecycleEvent::Render],
        "blocked down must not revalidate, inspect, signal, wait, inspect a listener, or remove"
    );
}

fn assert_down_consumer_matrix_row(discovery: &ManagedServerDiscovery) {
    match &discovery.state {
        ManagedServerState::Empty => {
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
                managed: discovery.clone(),
                observations: Vec::new(),
                resolution: Resolution::Empty,
            });
            let mut effects = ScriptedEffects::new(
                Vec::<ListenerState>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            ledger_rendered(render_down_report(&report), &ledger);
            assert_eq!(report.disposition, DownDisposition::Empty);
            assert!(report.success);
            assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
        }
        ManagedServerState::Ready(server) | ManagedServerState::Degraded { server, .. } => {
            let capture = match &discovery.inventory.local {
                RegistrationSlot::Captured(capture) => capture.as_ref().clone(),
                slot => panic!("target fixture lost its local capture: {slot:?}"),
            };
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process =
                ScriptedProcess::new(server.runfile.pid, "consumer-down", Rc::clone(&ledger))
                    .with_inspection(Ok(ProcessFacts {
                        identity: server.identity.clone(),
                        listener: server.listener.clone(),
                    }));
            let candidate = Candidate {
                coordinate: server.registration.clone(),
                runfile: Some(server.runfile.clone()),
                state: CandidateState::Verified {
                    identity: server.identity.clone(),
                    listener: server.listener.clone(),
                    health: server.health,
                },
            };
            let resolution = match &discovery.state {
                ManagedServerState::Ready(_) => Resolution::Ready {
                    target: 0,
                    aliases: Vec::new(),
                    stale: Vec::new(),
                },
                ManagedServerState::Degraded { issues, .. } => Resolution::Degraded {
                    target: 0,
                    aliases: Vec::new(),
                    stale: Vec::new(),
                    listener: server.listener.clone(),
                    health: server.health,
                    issues: issues.clone(),
                },
                _ => unreachable!(),
            };
            let plan = down_plan_from_lifecycle(LifecycleDiscovery {
                managed: discovery.clone(),
                observations: vec![LifecycleObservation {
                    candidate,
                    capture: Some(capture.clone()),
                    process: Some(process),
                }],
                resolution,
            });
            let mut effects = ScriptedEffects::new(
                [ListenerState::Absent],
                [Ok(RemovalOutcome::Removed)],
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            ledger_rendered(render_down_report(&report), &ledger);
            assert_eq!(report.disposition, DownDisposition::Stopped);
            assert!(report.success);
            assert_eq!(report.registrations.len(), 1);
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Inspect("consumer-down", server.runfile.port),
                    LifecycleEvent::Terminate("consumer-down"),
                    LifecycleEvent::RetainedWait("consumer-down"),
                    LifecycleEvent::Listener(server.runfile.pid, server.runfile.port),
                    scripted_remove_event(&capture),
                    LifecycleEvent::Render,
                ]
            );
        }
        ManagedServerState::StaleOnly { .. } => {
            let capture = match &discovery.inventory.local {
                RegistrationSlot::Captured(capture) => capture.as_ref().clone(),
                slot => panic!("stale fixture lost its local capture: {slot:?}"),
            };
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
                managed: discovery.clone(),
                observations: vec![LifecycleObservation {
                    candidate: Candidate {
                        coordinate: capture.coordinate(),
                        runfile: Some(capture.runfile.clone()),
                        state: CandidateState::Stale {
                            reason: "PID is absent".to_string(),
                            observed_identity: None,
                            listener: ListenerState::Absent,
                        },
                    },
                    capture: Some(capture.clone()),
                    process: None,
                }],
                resolution: Resolution::StaleOnly { stale: vec![0] },
            });
            let mut effects = ScriptedEffects::new(
                [ListenerState::Absent],
                [Ok(RemovalOutcome::Removed)],
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            ledger_rendered(render_down_report(&report), &ledger);
            assert_eq!(report.disposition, DownDisposition::StaleCleaned);
            assert!(report.success);
            assert_eq!(report.registrations.len(), 1);
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Listener(capture.runfile.pid, capture.runfile.port),
                    scripted_remove_event(&capture),
                    LifecycleEvent::Render,
                ]
            );
        }
        ManagedServerState::Conflict { issues } => assert_blocked_down_consumer(
            discovery,
            Resolution::Conflict {
                issues: issues.clone(),
            },
        ),
        ManagedServerState::Unverifiable { issues } => assert_blocked_down_consumer(
            discovery,
            Resolution::Unverifiable {
                issues: issues.clone(),
            },
        ),
    }
}

#[test]
fn registration_consumers_propagate_typed_ambiguity() {
    let scope = ManagedDiscoveryScope {
        workspace: discovery_fixture_path("workspace")
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf(),
        global: Some(discovery_fixture_path("global")),
    };
    let explicit = crate::backend::select_endpoint_with(Some("http://explicit.example/v1"), || {
        panic!("explicit endpoint selection must not run managed discovery")
    })
    .unwrap();
    assert!(matches!(
        explicit,
        crate::backend::EndpointSelection::Explicit { .. }
    ));
    assert!(
        down_mutation_blocker(
            &discovery_fixture_degraded(
                ListenerState::OwnedByTargetWildcard,
                HealthState::NotProbed,
            )
            .state
        )
        .is_some()
    );
    let fixtures = [
        discovery_fixture_empty(),
        discovery_fixture_ready(),
        discovery_fixture_degraded(ListenerState::OwnedByTarget, HealthState::Unhealthy),
        discovery_fixture_stale_only(),
        discovery_fixture_blocked(true),
        discovery_fixture_blocked(false),
    ];
    for discovery in fixtures {
        assert_down_consumer_matrix_row(&discovery);
        let status = status_report(&discovery);
        assert_eq!(status.state, discovery.state);

        let mut effects = RecordingDoctorEffects::default();
        let doctor =
            doctor_report_after_discovery(&doctor_fixture_args(), &discovery, &mut effects);
        let automatic =
            crate::backend::automatic_endpoint_from_discovery(scope.clone(), discovery.clone());
        let down_blocker = down_mutation_blocker(&discovery.state);
        match &discovery.state {
            ManagedServerState::Empty => {
                assert!(matches!(
                    automatic,
                    Ok(crate::backend::EndpointSelection::Default { .. })
                ));
                assert!(doctor.success);
                assert!(!effects.events.is_empty());
                assert!(down_blocker.is_none());
            }
            ManagedServerState::Ready(_) => {
                assert!(matches!(
                    automatic,
                    Ok(crate::backend::EndpointSelection::Managed { .. })
                ));
                assert!(doctor.success);
                assert!(!effects.events.is_empty());
                assert!(down_blocker.is_none());
            }
            ManagedServerState::Degraded { .. } => {
                assert!(matches!(
                    automatic,
                    Err(crate::backend::EndpointSelectionError::Degraded(_))
                ));
                assert!(!doctor.success);
                assert!(effects.events.is_empty());
                assert!(down_blocker.is_none());
            }
            ManagedServerState::StaleOnly { .. } => {
                assert!(matches!(
                    automatic,
                    Err(crate::backend::EndpointSelectionError::StaleOnly(_))
                ));
                assert!(!doctor.success);
                assert!(effects.events.is_empty());
                assert!(down_blocker.is_none());
            }
            ManagedServerState::Conflict { .. } => {
                assert!(matches!(
                    automatic,
                    Err(crate::backend::EndpointSelectionError::Conflict(_))
                ));
                assert!(!doctor.success);
                assert!(effects.events.is_empty());
                assert!(down_blocker.is_some());
            }
            ManagedServerState::Unverifiable { .. } => {
                assert!(matches!(
                    automatic,
                    Err(crate::backend::EndpointSelectionError::Unverifiable(_))
                ));
                assert!(!doctor.success);
                assert!(effects.events.is_empty());
                assert!(down_blocker.is_some());
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecycleEvent {
    Spawn(u32),
    Acquire(u32),
    PidMapReplace(u32, &'static str),
    ChildTryWait(u32),
    ChildKill(u32),
    ChildWait(u32),
    Inspect(&'static str, u16),
    Terminate(&'static str),
    RetainedWait(&'static str),
    Listener(u32, u16),
    Health(u16),
    ClockNow,
    Sleep,
    Publish,
    Revalidate,
    Remove(PathBuf, String),
    RemoveStage(PathBuf, Option<String>),
    Persistence(PersistencePhase, PathBuf),
    Render,
}

type EventLedger = Rc<RefCell<Vec<LifecycleEvent>>>;

#[derive(Debug, Clone)]
struct ScriptedExit(&'static str);

impl std::fmt::Display for ScriptedExit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

struct ScriptedChild {
    pid: u32,
    try_wait: VecDeque<Result<Option<ScriptedExit>, String>>,
    wait: VecDeque<Result<ScriptedExit, String>>,
    kill: VecDeque<Result<(), String>>,
    ledger: EventLedger,
}

impl ScriptedChild {
    fn new(
        pid: u32,
        try_wait: impl IntoIterator<Item = Result<Option<ScriptedExit>, String>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            pid,
            try_wait: try_wait.into_iter().collect(),
            wait: VecDeque::from([Ok(ScriptedExit("exited"))]),
            kill: VecDeque::from([Ok(())]),
            ledger,
        }
    }
}

impl SpawnedChild for ScriptedChild {
    type ExitStatus = ScriptedExit;

    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Self::ExitStatus>, String> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::ChildTryWait(self.pid));
        self.try_wait
            .pop_front()
            .expect("scripted child try_wait result")
    }

    fn wait(&mut self) -> Result<Self::ExitStatus, String> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::ChildWait(self.pid));
        self.wait.pop_front().expect("scripted child wait result")
    }

    fn kill(&mut self) -> Result<(), String> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::ChildKill(self.pid));
        self.kill.pop_front().expect("scripted child kill result")
    }
}

#[derive(Debug, Clone)]
struct ScriptedProcess {
    pid: u32,
    generation: &'static str,
    inspect: Rc<RefCell<VecDeque<Result<ProcessFacts, ProcessError>>>>,
    terminate: Rc<RefCell<VecDeque<Result<bool, ProcessError>>>>,
    wait: Rc<RefCell<VecDeque<Result<bool, ProcessError>>>>,
    ledger: EventLedger,
}

impl ScriptedProcess {
    fn new(pid: u32, generation: &'static str, ledger: EventLedger) -> Self {
        Self {
            pid,
            generation,
            inspect: Rc::new(RefCell::new(VecDeque::new())),
            terminate: Rc::new(RefCell::new(VecDeque::from([Ok(true)]))),
            wait: Rc::new(RefCell::new(VecDeque::from([Ok(true)]))),
            ledger,
        }
    }

    fn with_inspection(self, result: Result<ProcessFacts, ProcessError>) -> Self {
        self.inspect.borrow_mut().push_back(result);
        self
    }

    fn with_terminate(self, result: Result<bool, ProcessError>) -> Self {
        *self.terminate.borrow_mut() = VecDeque::from([result]);
        self
    }

    fn with_wait(self, result: Result<bool, ProcessError>) -> Self {
        *self.wait.borrow_mut() = VecDeque::from([result]);
        self
    }
}

impl RetainedProcessHandle for ScriptedProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn inspect(&self, port: u16) -> Result<ProcessFacts, ProcessError> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Inspect(self.generation, port));
        self.inspect
            .borrow_mut()
            .pop_front()
            .expect("scripted retained-process inspection")
    }

    fn terminate(&self) -> Result<bool, ProcessError> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Terminate(self.generation));
        self.terminate
            .borrow_mut()
            .pop_front()
            .expect("scripted retained-process terminate result")
    }

    fn wait(&self, _timeout: Duration) -> Result<bool, ProcessError> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::RetainedWait(self.generation));
        self.wait
            .borrow_mut()
            .pop_front()
            .expect("scripted retained-process wait result")
    }
}

struct ScriptedRuntime {
    acquisitions: RefCell<VecDeque<Result<ScriptedProcess, String>>>,
    ledger: EventLedger,
}

impl ScriptedRuntime {
    fn new(result: Result<ScriptedProcess, String>, ledger: EventLedger) -> Self {
        Self {
            acquisitions: RefCell::new(VecDeque::from([result])),
            ledger,
        }
    }

    fn acquire(&self, pid: u32) -> Result<ScriptedProcess, ProcessError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
        self.acquisitions
            .borrow_mut()
            .pop_front()
            .expect("scripted process acquisition")
            .map_err(ProcessError::Operation)
    }
}

impl SpawnedProcessRuntime<ScriptedChild> for ScriptedRuntime {
    type Process = ScriptedProcess;

    fn acquire_child(&self, child: &ScriptedChild) -> Result<Self::Process, String> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Acquire(child.pid));
        self.acquisitions
            .borrow_mut()
            .pop_front()
            .expect("scripted process acquisition")
    }
}

struct ScriptedPidMapRuntime {
    processes: RefCell<HashMap<u32, ScriptedProcess>>,
    ledger: EventLedger,
}

impl ScriptedPidMapRuntime {
    fn new(process: ScriptedProcess, ledger: EventLedger) -> Self {
        Self {
            processes: RefCell::new(HashMap::from([(process.pid(), process)])),
            ledger,
        }
    }

    fn replace(&self, pid: u32, generation: &'static str, process: ScriptedProcess) {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::PidMapReplace(pid, generation));
        self.processes.borrow_mut().insert(pid, process);
    }

    fn acquire(&self, pid: u32) -> Result<ScriptedProcess, ProcessError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
        self.processes
            .borrow()
            .get(&pid)
            .cloned()
            .ok_or(ProcessError::NotFound(pid))
    }
}

/// The one registration-mutation and teardown double. Every mutation is
/// recorded in the ledger; a `Some` script answers it, while `None` runs the
/// native conditional operation against the real filesystem.
struct ScriptedEffects {
    removals: Option<VecDeque<Result<RemovalOutcome, RemovalError>>>,
    stage_removals: Option<VecDeque<Result<RemovalOutcome, RemovalError>>>,
    /// `Some` revalidates against that scope's real inventory instead of
    /// replaying `revalidations`.
    scope: Option<ManagedDiscoveryScope>,
    revalidations: VecDeque<Result<(), String>>,
    listeners: VecDeque<ListenerState>,
    ledger: EventLedger,
}

impl ScriptedEffects {
    /// Native mutations, recorded.
    fn filesystem(ledger: EventLedger) -> Self {
        Self {
            removals: None,
            stage_removals: None,
            scope: None,
            revalidations: VecDeque::from([Ok(())]),
            listeners: VecDeque::new(),
            ledger,
        }
    }

    /// Scripted teardown: listener states and registration removals.
    fn new(
        listeners: impl IntoIterator<Item = ListenerState>,
        removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            removals: Some(removals.into_iter().collect()),
            stage_removals: Some(VecDeque::new()),
            listeners: listeners.into_iter().collect(),
            ..Self::filesystem(ledger)
        }
    }

    /// Scripted publication compensation: final and stage removals.
    fn publication(
        removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        stage_removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            stage_removals: Some(stage_removals.into_iter().collect()),
            ..Self::new([], removals, ledger)
        }
    }

    fn with_scope(mut self, scope: ManagedDiscoveryScope) -> Self {
        self.scope = Some(scope);
        self
    }

    fn with_listeners(mut self, listeners: impl IntoIterator<Item = ListenerState>) -> Self {
        self.listeners = listeners.into_iter().collect();
        self
    }
}

impl RegistrationStore for ScriptedEffects {
    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        self.ledger
            .borrow_mut()
            .push(scripted_remove_event(captured));
        match &mut self.removals {
            None => remove_if_unchanged(captured),
            Some(script) => script
                .pop_front()
                .expect("scripted conditional-removal result"),
        }
    }

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError> {
        self.ledger.borrow_mut().push(LifecycleEvent::RemoveStage(
            stage.path.clone(),
            stage.raw.as_deref().map(ferric_bench::sha256_bytes),
        ));
        match &mut self.stage_removals {
            None => remove_publication_stage_if_unchanged(stage),
            Some(script) => script
                .pop_front()
                .expect("scripted publication-stage removal"),
        }
    }
}

impl DownEffects for ScriptedEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
        match &self.scope {
            Some(scope) => revalidate_scope_registrations(scope, expected),
            None => self
                .revalidations
                .pop_front()
                .expect("scripted registration revalidation"),
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.listeners
            .pop_front()
            .expect("scripted down-listener state")
    }
}

/// Scripted persistence that also records each phase in the lifecycle ledger.
fn ledgered(
    effects: ScriptedPersistenceEffects,
    ledger: &EventLedger,
) -> ScriptedPersistenceEffects {
    let ledger = Rc::clone(ledger);
    effects.on_phase(move |phase, path| {
        ledger
            .borrow_mut()
            .push(LifecycleEvent::Persistence(phase, path.to_path_buf()));
    })
}

/// Record the render step in the ledger once `rendered` exists.
fn ledger_rendered(rendered: Rendered, ledger: &EventLedger) -> Rendered {
    ledger.borrow_mut().push(LifecycleEvent::Render);
    rendered
}

fn assert_down_failure_kept_recovery(report: &DownReport, rendered: &Rendered) {
    assert!(!report.success);
    assert_eq!(report.disposition, DownDisposition::Failed);
    assert!(
        report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        ))
    );
    assert!(
        rendered.stdout.iter().all(|line| !line.contains("stopped")),
        "failure report must never claim stopped: {:?}",
        rendered.stdout
    );
}

#[test]
fn down_signals_only_the_retained_handle() {
    for (case, listener) in [
        ("owned", ListenerState::OwnedByTarget),
        ("absent", ListenerState::Absent),
    ] {
        for recorded_health in [HealthState::Healthy, HealthState::Unhealthy] {
            let pid = if recorded_health == HealthState::Healthy {
                6101
            } else {
                6102
            };
            let capture = discovery_fixture_capture(RegistrationScope::Local, pid, case);
            let port = capture.runfile.port;
            let expected = capture.runfile.process_identity.clone().unwrap();
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let process = ScriptedProcess::new(pid, "retained-target", Rc::clone(&ledger))
                .with_inspection(Ok(ProcessFacts {
                    identity: expected.clone(),
                    listener: listener.clone(),
                }));
            let mut effects = ScriptedEffects::new(
                [ListenerState::Absent],
                [Ok(RemovalOutcome::Removed)],
                Rc::clone(&ledger),
            );

            let mut server = match discovery_fixture_ready().state {
                ManagedServerState::Ready(server) => server,
                _ => unreachable!("ready fixture changed state"),
            };
            server.runfile = capture.runfile.clone();
            server.identity = expected.clone();
            server.listener = listener.clone();
            server.health = recorded_health;
            let recorded_state = if recorded_health == HealthState::Healthy
                && listener == ListenerState::OwnedByTarget
            {
                ManagedServerState::Ready(server)
            } else {
                ManagedServerState::Degraded {
                    server,
                    issues: Vec::new(),
                }
            };

            let report = execute_down_plan(
                retained_target_down_plan(
                    &recorded_state,
                    process,
                    vec![capture.clone()],
                    Vec::new(),
                )
                .unwrap(),
                &mut effects,
            );
            let rendered = ledger_rendered(render_down_report(&report), &ledger);

            assert!(report.success, "{case}, health={recorded_health:?}");
            assert_eq!(report.disposition, DownDisposition::Stopped);
            assert!(report.signalled);
            assert!(report.exit_proven);
            assert!(report.listener_released);
            assert!(rendered.stdout.iter().any(|line| line.contains("stopped")));
            assert_eq!(
                *ledger.borrow(),
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Inspect("retained-target", port),
                    LifecycleEvent::Terminate("retained-target"),
                    LifecycleEvent::RetainedWait("retained-target"),
                    LifecycleEvent::Listener(pid, port),
                    scripted_remove_event(&capture),
                    LifecycleEvent::Render,
                ],
                "HTTP health={recorded_health:?} must not affect retained-handle teardown"
            );
        }
    }
}

#[test]
fn down_exit_and_listener_postconditions_gate_success() {
    let capture = discovery_fixture_capture(RegistrationScope::Local, 6201, "down-gates");
    let pid = capture.runfile.pid;
    let port = capture.runfile.port;
    let expected = capture.runfile.process_identity.clone().unwrap();
    let facts = ProcessFacts {
        identity: expected.clone(),
        listener: ListenerState::OwnedByTarget,
    };

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "signal-error", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_terminate(Err(ProcessError::Operation("access denied".to_string())));
    let mut effects = ScriptedEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected: expected.clone(),
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_down_failure_kept_recovery(&report, &rendered);
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("signal-error", port),
            LifecycleEvent::Terminate("signal-error"),
            LifecycleEvent::Render,
        ]
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "revision-change", Rc::clone(&ledger));
    let mut effects = ScriptedEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    effects.revalidations = VecDeque::from([Err(
        "a conflicting registration appeared before signal".to_string(),
    )]);
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected: expected.clone(),
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_down_failure_kept_recovery(&report, &rendered);
    assert_eq!(
        *ledger.borrow(),
        vec![LifecycleEvent::Revalidate, LifecycleEvent::Render],
        "a changed inventory must block before retained inspection, signal, wait, listener, or cleanup"
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "exit-before-signal", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_terminate(Ok(false))
        .with_wait(Ok(true));
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected: expected.clone(),
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert!(report.success);
    assert_eq!(report.disposition, DownDisposition::AlreadyExited);
    assert!(!report.signalled);
    assert!(report.exit_proven);
    assert!(report.listener_released);
    let expected_state =
        format!("[state] managed server pid {pid} was already exited; no process was signalled");
    assert_eq!(
        rendered.stdout.last().map(String::as_str),
        Some(expected_state.as_str())
    );
    assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("exit-before-signal", port),
            LifecycleEvent::Terminate("exit-before-signal"),
            LifecycleEvent::RetainedWait("exit-before-signal"),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&capture),
            LifecycleEvent::Render,
        ],
        "an inspect-to-terminate exit race must still prove exit and listener release before cleanup"
    );

    for (generation, wait) in [
        ("wait-timeout", Ok(false)),
        (
            "wait-error",
            Err(ProcessError::Operation("wait unavailable".to_string())),
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, generation, Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(wait);
        let mut effects = ScriptedEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_down_failure_kept_recovery(&report, &rendered);
        assert!(report.signalled);
        assert!(!report.exit_proven);
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect(generation, port),
                LifecycleEvent::Terminate(generation),
                LifecycleEvent::RetainedWait(generation),
                LifecycleEvent::Render,
            ]
        );
    }

    for (case, residual) in [
        ("target", ListenerState::OwnedByTarget),
        ("wildcard", ListenerState::OwnedByTargetWildcard),
        ("foreign", ListenerState::OwnedByOther(vec![7777])),
        (
            "uninspectable",
            ListenerState::Uninspectable("access denied".to_string()),
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "listener-held", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()));
        let mut effects = ScriptedEffects::new(
            [residual],
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected: expected.clone(),
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_down_failure_kept_recovery(&report, &rendered);
        assert!(report.signalled, "{case}");
        assert!(report.exit_proven, "{case}");
        assert!(!report.listener_released, "{case}");
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Inspect("listener-held", port),
                LifecycleEvent::Terminate("listener-held"),
                LifecycleEvent::RetainedWait("listener-held"),
                LifecycleEvent::Listener(pid, port),
                LifecycleEvent::Render,
            ],
            "{case}"
        );
    }

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "released", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()));
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected: expected.clone(),
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_eq!(report.disposition, DownDisposition::Stopped);
    assert!(report.success);
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.starts_with("[state] stopped"))
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("released", port),
            LifecycleEvent::Terminate("released"),
            LifecycleEvent::RetainedWait("released"),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&capture),
            LifecycleEvent::Render,
        ]
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "already-exited", Rc::clone(&ledger))
        .with_inspection(Err(ProcessError::NotFound(pid)));
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected,
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert!(report.success);
    assert_eq!(report.disposition, DownDisposition::AlreadyExited);
    assert!(!report.signalled);
    assert!(report.exit_proven);
    assert!(
        rendered
            .stdout
            .iter()
            .all(|line| !line.contains("[state] stopped"))
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("already-exited", port),
            LifecycleEvent::RetainedWait("already-exited"),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&capture),
            LifecycleEvent::Render,
        ]
    );
}

#[test]
fn down_cleanup_outcome_matrix() {
    let stopped_capture = discovery_fixture_capture(RegistrationScope::Local, 6299, "stopped");
    let stopped_expected = stopped_capture.runfile.process_identity.clone().unwrap();
    let stopped_port = stopped_capture.runfile.port;
    let stopped_ledger = Rc::new(RefCell::new(Vec::new()));
    let stopped_process = ScriptedProcess::new(6299, "stopped", Rc::clone(&stopped_ledger))
        .with_inspection(Ok(ProcessFacts {
            identity: stopped_expected.clone(),
            listener: ListenerState::OwnedByTarget,
        }));
    let mut stopped_effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&stopped_ledger),
    );
    let stopped_report = execute_down_plan(
        DownPlan::Target {
            process: stopped_process,
            expected: stopped_expected,
            pid: 6299,
            port: stopped_port,
            captures: vec![stopped_capture.clone()],
            expected_revisions: Vec::new(),
        },
        &mut stopped_effects,
    );
    let stopped_rendered = ledger_rendered(render_down_report(&stopped_report), &stopped_ledger);
    assert_eq!(stopped_report.disposition, DownDisposition::Stopped);
    assert!(stopped_report.success);
    assert!(
        stopped_rendered
            .stdout
            .last()
            .is_some_and(|line| line.starts_with("[state] stopped"))
    );
    assert_eq!(
        *stopped_ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("stopped", stopped_port),
            LifecycleEvent::Terminate("stopped"),
            LifecycleEvent::RetainedWait("stopped"),
            LifecycleEvent::Listener(6299, stopped_port),
            scripted_remove_event(&stopped_capture),
            LifecycleEvent::Render,
        ],
        "the stopped row must preserve the same exact per-path order as every stale cleanup row"
    );

    let holding = discovery_fixture_path("holding");
    let cases = vec![
        ("removed", Ok(RemovalOutcome::Removed), "[removed]", true),
        (
            "absent",
            Ok(RemovalOutcome::Absent),
            "[already-absent]",
            true,
        ),
        (
            "replacement",
            Ok(RemovalOutcome::ReplacementPreserved {
                path: holding.clone(),
                detail: "concurrent replacement retained".to_string(),
            }),
            "[replacement-preserved]",
            false,
        ),
        (
            "restore",
            Err(RemovalError {
                path: discovery_fixture_path("restore"),
                kind: RemovalFailureKind::Restore,
                detail: "restore denied".to_string(),
                preserved_at: Some(holding.clone()),
            }),
            "[restore-failed]",
            false,
        ),
        (
            "remove",
            Err(RemovalError {
                path: discovery_fixture_path("remove"),
                kind: RemovalFailureKind::Remove,
                detail: "remove denied".to_string(),
                preserved_at: Some(holding.clone()),
            }),
            "[removal-failed]",
            false,
        ),
        (
            "other",
            Err(RemovalError {
                path: discovery_fixture_path("other"),
                kind: RemovalFailureKind::Other,
                detail: "holding directory cleanup failed".to_string(),
                preserved_at: Some(holding.clone()),
            }),
            "[cleanup-failed]",
            false,
        ),
    ];

    for (index, (name, outcome, marker, complete)) in cases.into_iter().enumerate() {
        let pid = 6301 + u32::try_from(index).unwrap();
        let capture = discovery_fixture_capture(RegistrationScope::Local, pid, name);
        let port = capture.runfile.port;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut effects =
            ScriptedEffects::new([ListenerState::Absent], [outcome], Rc::clone(&ledger));
        let report = execute_down_plan(
            DownPlan::<ScriptedProcess>::Stale {
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_eq!(report.success, complete, "{name}");
        assert_eq!(
            report.disposition,
            if complete {
                DownDisposition::StaleCleaned
            } else {
                DownDisposition::CleanupPartial
            },
            "{name}"
        );
        let row = rendered
            .stdout
            .iter()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("missing {marker} row for {name}: {:?}", rendered.stdout));
        if !complete {
            assert!(
                row.contains(&holding.display().to_string()),
                "{name}: {row}"
            );
        }
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some(if complete {
                "[state] stale-cleaned"
            } else {
                "[state] exit/quiescence confirmed where applicable; cleanup partial"
            }),
            "{name}"
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Revalidate,
                LifecycleEvent::Listener(pid, port),
                scripted_remove_event(&capture),
                LifecycleEvent::Render,
            ],
            "{name}"
        );
    }

    let local = discovery_fixture_capture(RegistrationScope::Local, 6401, "partial-local");
    let mut global = local.clone();
    global.scope = RegistrationScope::Global;
    global.path = discovery_fixture_path("partial-global");
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent, ListenerState::Absent],
        [
            Ok(RemovalOutcome::Removed),
            Err(RemovalError {
                path: global.path.clone(),
                kind: RemovalFailureKind::Remove,
                detail: "second alias retained".to_string(),
                preserved_at: Some(holding.clone()),
            }),
        ],
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::<ScriptedProcess>::Stale {
            captures: vec![local.clone(), global.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_eq!(report.disposition, DownDisposition::CleanupPartial);
    assert!(!report.success);
    assert_eq!(report.registrations[0].coordinate.path, local.path);
    assert_eq!(report.registrations[1].coordinate.path, global.path);
    assert!(
        rendered.stdout[1].contains(&holding.display().to_string()),
        "every recovery path must survive rendering: {:?}",
        rendered.stdout
    );
    assert_eq!(
        rendered.stdout.last().map(String::as_str),
        Some("[state] exit/quiescence confirmed where applicable; cleanup partial")
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Listener(local.runfile.pid, local.runfile.port),
            scripted_remove_event(&local),
            scripted_remove_event(&global),
            LifecycleEvent::Render,
        ]
    );

    let physical = discovery_fixture_capture(RegistrationScope::Local, 6410, "same-physical-path");
    let mut duplicate_alias = physical.clone();
    duplicate_alias.scope = RegistrationScope::Global;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::<ScriptedProcess>::Stale {
            captures: vec![physical.clone(), duplicate_alias.clone()],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert!(report.success);
    assert_eq!(report.registrations.len(), 2);
    assert!(
        report
            .registrations
            .iter()
            .all(|registration| matches!(registration.outcome, DownRegistrationOutcome::Removed))
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::Listener(physical.runfile.pid, physical.runfile.port),
            scripted_remove_event(&physical),
            LifecycleEvent::Render,
        ],
        "one physical path must receive one conditional removal while retaining both alias reports"
    );
    assert_eq!(
        rendered.stdout.last().map(String::as_str),
        Some("[state] stale-cleaned")
    );

    duplicate_alias.raw.push(b' ');
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut effects = ScriptedEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(
        DownPlan::<ScriptedProcess>::Stale {
            captures: vec![physical, duplicate_alias],
            expected_revisions: Vec::new(),
        },
        &mut effects,
    );
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_down_failure_kept_recovery(&report, &rendered);
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
}

#[test]
fn ambiguous_or_unverifiable_down_is_non_mutating() {
    fn write_runfile_slot(
        scope: RegistrationScope,
        path: &Path,
        runfile: &ServerRunfile,
    ) -> (RegistrationSlot, Vec<u8>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = serde_json::to_vec_pretty(runfile).unwrap();
        std::fs::write(path, &raw).unwrap();
        (capture_registration_path(scope, path), raw)
    }

    let root = tempfile::tempdir().unwrap();
    for blocker in [
        "two live keys",
        "malformed peer",
        "unreadable peer",
        "retired schema-1 registration",
        "wildcard listener",
        "shared listener",
        "foreign listener",
        "uninspectable listener",
        "invalid creation token",
        "durable Tailscale state",
    ] {
        let case_dir = root.path().join(blocker.replace(' ', "-"));
        let local_path = case_dir
            .join("workspace")
            .join(".ferric")
            .join("server.json");
        let global_path = case_dir.join("global.json");
        let ledger = Rc::new(RefCell::new(Vec::new()));

        let mut local_runfile = discovery_fixture_runfile(6450, "blocked-local");
        local_runfile.origin_local_runfile = Some(local_path.clone());
        let (inventory, originals, expected_acquires) = match blocker {
            "malformed peer" => {
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                std::fs::create_dir_all(global_path.parent().unwrap()).unwrap();
                let malformed = b"{not-valid-json".to_vec();
                std::fs::write(&global_path, &malformed).unwrap();
                (
                    RegistrationInventory {
                        local,
                        global: Some(capture_registration_path(
                            RegistrationScope::Global,
                            &global_path,
                        )),
                        promised_origins: Vec::new(),
                    },
                    vec![
                        (local_path.clone(), local_raw),
                        (global_path.clone(), malformed),
                    ],
                    0,
                )
            }
            "unreadable peer" => {
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: Some(RegistrationSlot::Blocked {
                            scope: RegistrationScope::Global,
                            path: global_path,
                            reason: RegistrationBlock::Unreadable(
                                "injected permission denial".to_string(),
                            ),
                        }),
                        promised_origins: Vec::new(),
                    },
                    vec![(local_path.clone(), local_raw)],
                    0,
                )
            }
            "invalid creation token" => {
                local_runfile.process_identity.as_mut().unwrap().start_token =
                    "invalid-token".to_string();
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: None,
                        promised_origins: Vec::new(),
                    },
                    vec![(local_path.clone(), local_raw)],
                    0,
                )
            }
            "durable Tailscale state" => {
                local_runfile.tailscale = true;
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: None,
                        promised_origins: Vec::new(),
                    },
                    vec![(local_path.clone(), local_raw)],
                    0,
                )
            }
            "retired schema-1 registration" => {
                local_runfile.schema_version = 1;
                local_runfile.process_identity = None;
                local_runfile.origin_local_runfile = None;
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                let (global, global_raw) =
                    write_runfile_slot(RegistrationScope::Global, &global_path, &local_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: Some(global),
                        promised_origins: Vec::new(),
                    },
                    vec![
                        (local_path.clone(), local_raw),
                        (global_path.clone(), global_raw),
                    ],
                    0,
                )
            }
            "two live keys" => {
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                let mut global_runfile = local_runfile.clone();
                global_runfile.pid = 6451;
                global_runfile.process_identity = Some(process_identity(6451));
                let (global, global_raw) =
                    write_runfile_slot(RegistrationScope::Global, &global_path, &global_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: Some(global),
                        promised_origins: Vec::new(),
                    },
                    vec![
                        (local_path.clone(), local_raw),
                        (global_path.clone(), global_raw),
                    ],
                    2,
                )
            }
            _ => {
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                (
                    RegistrationInventory {
                        local,
                        global: None,
                        promised_origins: Vec::new(),
                    },
                    vec![(local_path.clone(), local_raw)],
                    1,
                )
            }
        };
        let expected_held = flatten_inventory(&inventory)
            .iter()
            .filter(|observation| !matches!(observation.state, ManagedRegistrationState::Absent))
            .count();
        let observe_ledger = Rc::clone(&ledger);
        let discovery = discover_inventory_before_health_with(inventory, move |capture| {
            observe_ledger
                .borrow_mut()
                .push(LifecycleEvent::Acquire(capture.runfile.pid));
            let listener = match blocker {
                "wildcard listener" => ListenerState::OwnedByTargetWildcard,
                "shared listener" => ListenerState::OwnedByOther(vec![6450, 6451]),
                "foreign listener" => ListenerState::OwnedByOther(vec![6451]),
                "uninspectable listener" => ListenerState::Uninspectable("denied".to_string()),
                _ => ListenerState::OwnedByTarget,
            };
            let state = if blocker == "uninspectable listener" {
                CandidateState::Unverifiable {
                    reason: blocker.to_string(),
                    observed_identity: capture.runfile.process_identity.clone(),
                    listener: Some(listener),
                    health: HealthState::NotProbed,
                }
            } else {
                CandidateState::Verified {
                    identity: capture.runfile.process_identity.clone().unwrap(),
                    listener,
                    health: HealthState::NotProbed,
                }
            };
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: capture.coordinate(),
                    runfile: Some(capture.runfile.clone()),
                    state,
                },
                capture: Some(capture),
                process: None,
            }
        });
        assert!(
            down_mutation_blocker(&discovery.managed.state).is_some(),
            "{blocker} must become a typed mutation blocker through inventory and resolution"
        );
        let plan = down_plan_from_lifecycle(discovery);
        let mut effects = ScriptedEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(plan, &mut effects);
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_eq!(report.disposition, DownDisposition::Blocked, "{blocker}");
        assert!(!report.success, "{blocker}");
        assert!(!report.signalled, "{blocker}");
        assert_eq!(report.registrations.len(), expected_held, "{blocker}");
        assert!(report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        )));
        assert!(
            rendered.stdout.iter().all(|line| !line.contains("stopped")),
            "{blocker}: {:?}",
            rendered.stdout
        );
        assert_eq!(
            ledger
                .borrow()
                .iter()
                .filter(|event| matches!(event, LifecycleEvent::Acquire(_)))
                .count(),
            expected_acquires,
            "{blocker} must stop acquiring as soon as its real trigger becomes authoritative"
        );
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| matches!(event, LifecycleEvent::Acquire(_) | LifecycleEvent::Render)),
            "{blocker} must have empty signal/listener/delete/HTTP ledgers: {:?}",
            ledger.borrow()
        );
        for (path, original) in originals {
            assert_eq!(std::fs::read(path).unwrap(), original, "{blocker}");
        }
    }
}

struct ScriptedListener {
    states: RefCell<VecDeque<ListenerState>>,
    ledger: EventLedger,
}

impl ScriptedListener {
    fn new(state: ListenerState, ledger: EventLedger) -> Self {
        Self {
            states: RefCell::new(VecDeque::from([state])),
            ledger,
        }
    }
}

impl ListenerInspector for ScriptedListener {
    fn listener_state(&self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.states
            .borrow_mut()
            .pop_front()
            .expect("scripted listener state")
    }
}

struct ScriptedHealth {
    results: VecDeque<bool>,
    ledger: EventLedger,
}

impl HealthProbe for ScriptedHealth {
    fn status_ok(&mut self, _host: &str, port: u16, _path: &str) -> bool {
        self.ledger.borrow_mut().push(LifecycleEvent::Health(port));
        self.results.pop_front().expect("scripted health result")
    }
}

struct ScriptedClock {
    now: Instant,
    ledger: EventLedger,
}

impl LifecycleClock for ScriptedClock {
    fn now(&mut self) -> Instant {
        self.ledger.borrow_mut().push(LifecycleEvent::ClockNow);
        self.now
    }

    fn sleep(&mut self, duration: Duration) {
        self.ledger.borrow_mut().push(LifecycleEvent::Sleep);
        self.now += duration;
    }
}

fn scripted_facts(listener: ListenerState) -> ProcessFacts {
    ProcessFacts {
        identity: ProcessIdentity {
            start_token: "scripted-generation".to_string(),
            executable: PathBuf::from("scripted-engine"),
            argv: vec!["scripted-engine".to_string()],
        },
        listener,
    }
}

fn composition_runfile(pid: u32, local_path: &Path) -> ServerRunfile {
    let mut runfile = discovery_fixture_runfile(pid, "composition");
    runfile.origin_local_runfile = Some(local_path.to_path_buf());
    runfile
}

#[test]
fn bound_child_try_wait_error_uses_retained_cleanup_or_preserves_recovery() {
    let pid = 4101;
    let port = 9411;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "generation-a", Rc::clone(&ledger));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(
        pid,
        [Err("post-bind try_wait failed".to_string())],
        Rc::clone(&ledger),
    );

    let error = bind_spawned_child(&mut child, &runtime, port, &listener)
        .expect_err("a post-bind child inspection error must fail launch");
    assert!(error.contains("exact retained child was stopped"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::Terminate("generation-a"),
            LifecycleEvent::RetainedWait("generation-a"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
        ]
    );

    let recovery_ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "generation-b", Rc::clone(&recovery_ledger))
        .with_terminate(Err(ProcessError::Operation("access denied".to_string())))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&recovery_ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&recovery_ledger));
    let mut child = ScriptedChild::new(
        pid,
        [Err("post-bind try_wait failed".to_string())],
        Rc::clone(&recovery_ledger),
    );

    let error = bind_spawned_child(&mut child, &runtime, port, &listener)
        .expect_err("unproved retained cleanup must preserve a recovery clue");
    assert!(error.contains("recovery failure for retained PID 4101"));
    assert!(error.contains("access denied"));
    assert_eq!(
        *recovery_ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::Terminate("generation-b"),
            LifecycleEvent::RetainedWait("generation-b"),
        ]
    );
}

#[test]
fn stop_managed_child_reaps_proven_exit_before_listener_failure() {
    let pid = 4102;
    let port = 9412;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "generation-a", Rc::clone(&ledger));
    let listener =
        ScriptedListener::new(ListenerState::OwnedByOther(vec![9999]), Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));

    let error = stop_managed_child_with(&mut child, &process, port, &listener)
        .expect_err("a residual listener must fail the cleanup postcondition");
    assert!(error.contains("remains owned by PIDs"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Terminate("generation-a"),
            LifecycleEvent::RetainedWait("generation-a"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
        ],
        "known-exited child must be reaped before listener postcondition reporting"
    );
}

#[test]
fn partial_publication_stops_child_and_compensates_exactly() {
    fn mirrored_captures(pid: u32) -> (CapturedRegistration, CapturedRegistration) {
        let local = discovery_fixture_capture(RegistrationScope::Local, pid, "publication-local");
        let global = CapturedRegistration {
            scope: RegistrationScope::Global,
            path: discovery_fixture_path("publication-global"),
            raw: local.raw.clone(),
            runfile: local.runfile.clone(),
        };
        (local, global)
    }

    fn publication_stage(scope: RegistrationScope, final_path: &Path) -> PublicationStage {
        PublicationStage {
            scope,
            final_path: final_path.to_path_buf(),
            path: final_path.with_file_name(".server-registration-stage"),
            raw: Some(b"exact staged registration bytes".to_vec()),
            identity: None,
        }
    }

    fn mirror_failure(
        local: &CapturedRegistration,
        stage: &PublicationStage,
    ) -> Result<PublishedRegistrations, PublishError> {
        Err(PublishError::Mirror {
            path: stage.final_path.clone(),
            detail: "injected global precommit failure".to_string(),
            local: Box::new(local.clone()),
            attempt: Box::new(PublicationAttempt {
                finals: vec![local.clone()],
                stages: vec![stage.clone()],
                terminal_phase: PersistencePhase::FileSync,
                final_committed: false,
            }),
        })
    }

    let pid = 4401;
    let port = 9441;
    let (local, global) = mirrored_captures(pid);
    let global_stage = publication_stage(RegistrationScope::Global, &global.path);

    // A global precommit failure retains the local final and global stage
    // until exact exit/reap/listener release, then removes finals before
    // stages and renders only after every outcome is known.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "global-precommit", Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut effects = ScriptedEffects::publication(
        [Ok(RemovalOutcome::Removed)],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = complete_publication_with(
        &mut child,
        &process,
        port,
        mirror_failure(&local, &global_stage),
        &listener,
        &mut effects,
    );
    let rendered = ledger_rendered(render_publication_report(&report), &ledger);
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!report.success);
    assert!(
        rendered
            .stdout
            .last()
            .unwrap()
            .contains("rollback complete")
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Terminate("global-precommit"),
            LifecycleEvent::RetainedWait("global-precommit"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&local),
            LifecycleEvent::RemoveStage(
                global_stage.path.clone(),
                global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
            ),
            LifecycleEvent::Render,
        ]
    );

    // A signal error is not itself exit proof, but the exact retained
    // handle is still deliberately waited. A successful retained wait,
    // reap, and absent-listener check independently authorize rollback.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "signal-error-exited", Rc::clone(&ledger))
        .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
        .with_wait(Ok(true));
    let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut effects = ScriptedEffects::publication(
        [Ok(RemovalOutcome::Removed)],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    );
    let report = complete_publication_with(
        &mut child,
        &process,
        port,
        mirror_failure(&local, &global_stage),
        &listener,
        &mut effects,
    );
    ledger_rendered(render_publication_report(&report), &ledger);
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("signal denied"))
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Terminate("signal-error-exited"),
            LifecycleEvent::RetainedWait("signal-error-exited"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&local),
            LifecycleEvent::RemoveStage(
                global_stage.path.clone(),
                global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
            ),
            LifecycleEvent::Render,
        ]
    );

    // A local committed-but-durability failure is also a partial
    // publication, even though no stage remains.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "local-durability", Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let published_local = PublishedRegistrations {
        local: local.clone(),
        global: None,
    };
    let failure = Err(PublishError::Durability {
        path: local.path.clone(),
        detail: "injected local parent-sync failure".to_string(),
        published: Box::new(published_local),
        attempt: Box::new(PublicationAttempt {
            finals: vec![local.clone()],
            stages: Vec::new(),
            terminal_phase: PersistencePhase::ParentSync,
            final_committed: true,
        }),
    });
    let mut effects = ScriptedEffects::publication(
        [Ok(RemovalOutcome::Removed)],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report =
        complete_publication_with(&mut child, &process, port, failure, &listener, &mut effects);
    ledger_rendered(render_publication_report(&report), &ledger);
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert_eq!(report.finals.len(), 1);
    assert!(report.stages.is_empty());
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Terminate("local-durability"),
            LifecycleEvent::RetainedWait("local-durability"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&local),
            LifecycleEvent::Render,
        ]
    );

    // A child observed exited after both finals appear still goes through
    // the retained wait/reap/listener proof before either final rolls back.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "exit-during-publication", Rc::clone(&ledger))
        .with_terminate(Ok(false))
        .with_wait(Ok(true));
    let mut child = ScriptedChild::new(
        pid,
        [Ok(Some(ScriptedExit("exited during publication")))],
        Rc::clone(&ledger),
    );
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut effects = ScriptedEffects::publication(
        [Ok(RemovalOutcome::Removed), Ok(RemovalOutcome::Removed)],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = complete_publication_with(
        &mut child,
        &process,
        port,
        Ok(PublishedRegistrations {
            local: local.clone(),
            global: Some(global.clone()),
        }),
        &listener,
        &mut effects,
    );
    ledger_rendered(render_publication_report(&report), &ledger);
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::Terminate("exit-during-publication"),
            LifecycleEvent::RetainedWait("exit-during-publication"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&local),
            scripted_remove_event(&global),
            LifecycleEvent::Render,
        ]
    );

    // Every unproved exit/reap/listener row holds both finals and stages
    // and produces no removal event.
    for case in [
        "terminate-error",
        "wait-timeout",
        "wait-error",
        "reap-error",
        "listener-survived",
    ] {
        let recovery_root = tempfile::tempdir().unwrap();
        let mut held_local = local.clone();
        held_local.path = recovery_root.path().join("local-server.json");
        std::fs::write(&held_local.path, &held_local.raw).unwrap();
        let mut held_stage = global_stage.clone();
        held_stage.final_path = recovery_root.path().join("global-server.json");
        held_stage.path = recovery_root.path().join(".server-registration-held-stage");
        let held_stage_raw = held_stage
            .raw
            .as_ref()
            .expect("scripted held stage has exact bytes")
            .clone();
        std::fs::write(&held_stage.path, &held_stage_raw).unwrap();
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = match case {
            "terminate-error" => ScriptedProcess::new(pid, case, Rc::clone(&ledger))
                .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
                .with_wait(Ok(false)),
            "wait-timeout" => {
                ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_wait(Ok(false))
            }
            "wait-error" => ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_wait(Err(
                ProcessError::Operation("retained wait failed".to_string()),
            )),
            "reap-error" | "listener-survived" => {
                ScriptedProcess::new(pid, case, Rc::clone(&ledger))
            }
            _ => unreachable!(),
        };
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        if case == "reap-error" {
            child.wait = VecDeque::from([Err("child reap failed".to_string())]);
        }
        let listener = ScriptedListener::new(
            if case == "listener-survived" {
                ListenerState::OwnedByTarget
            } else {
                ListenerState::Absent
            },
            Rc::clone(&ledger),
        );
        let mut effects = ScriptedEffects::publication(
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            port,
            mirror_failure(&held_local, &held_stage),
            &listener,
            &mut effects,
        );
        let rendered = ledger_rendered(render_publication_report(&report), &ledger);
        assert_eq!(
            report.disposition,
            PublicationDisposition::RecoveryHeld,
            "{case}"
        );
        assert_eq!(report.finals.len(), 1, "{case}");
        assert_eq!(
            report.finals[0].coordinate,
            held_local.coordinate(),
            "{case}"
        );
        assert_eq!(report.stages.len(), 1, "{case}");
        assert_eq!(report.stages[0].scope, held_stage.scope, "{case}");
        assert_eq!(report.stages[0].final_path, held_stage.final_path, "{case}");
        assert_eq!(report.stages[0].path, held_stage.path, "{case}");
        assert!(
            report
                .finals
                .iter()
                .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
        );
        assert!(
            report
                .stages
                .iter()
                .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
        );
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains(&held_local.path.display().to_string())),
            "{case}: every held final must survive rendering"
        );
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.contains(&held_stage.path.display().to_string())),
            "{case}: every held stage must survive rendering"
        );
        assert_eq!(
            std::fs::read(&held_local.path).unwrap(),
            held_local.raw,
            "{case}: held final bytes changed"
        );
        assert_eq!(
            std::fs::read(&held_stage.path).unwrap(),
            held_stage_raw,
            "{case}: held stage bytes changed"
        );
        assert!(ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
        )));
        assert_eq!(ledger.borrow().last(), Some(&LifecycleEvent::Render));
    }

    // Cleanup continues across a concurrent final replacement, a second
    // final's conditional-removal failure, and a stage-cleanup failure;
    // every preserved path survives in the structured and rendered report.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "partial-cleanup", Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let replacement_path = discovery_fixture_path("concurrent-replacement");
    let final_holding = discovery_fixture_path("final-holding");
    let stage_holding = discovery_fixture_path("stage-holding");
    let published = PublishedRegistrations {
        local: local.clone(),
        global: Some(global.clone()),
    };
    let failure = Err(PublishError::Durability {
        path: global.path.clone(),
        detail: "injected global durability and stage cleanup failure".to_string(),
        published: Box::new(published),
        attempt: Box::new(PublicationAttempt {
            finals: vec![local.clone(), global.clone()],
            stages: vec![global_stage.clone()],
            terminal_phase: PersistencePhase::StageCleanup,
            final_committed: true,
        }),
    });
    let mut effects = ScriptedEffects::publication(
        [
            Ok(RemovalOutcome::ReplacementPreserved {
                path: replacement_path.clone(),
                detail: "concurrent replacement preserved".to_string(),
            }),
            Err(RemovalError {
                path: global.path.clone(),
                kind: RemovalFailureKind::Remove,
                detail: "conditional final cleanup failed".to_string(),
                preserved_at: Some(final_holding.clone()),
            }),
        ],
        [Err(RemovalError {
            path: global_stage.path.clone(),
            kind: RemovalFailureKind::Remove,
            detail: "conditional stage cleanup failed".to_string(),
            preserved_at: Some(stage_holding.clone()),
        })],
        Rc::clone(&ledger),
    );
    let report =
        complete_publication_with(&mut child, &process, port, failure, &listener, &mut effects);
    let rendered = ledger_rendered(render_publication_report(&report), &ledger);
    assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| { line.contains(&replacement_path.display().to_string()) })
    );
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.contains(&final_holding.display().to_string()))
    );
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.contains(&stage_holding.display().to_string()))
    );
    assert_eq!(
        ledger
            .borrow()
            .iter()
            .rev()
            .take(4)
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            LifecycleEvent::Render,
            LifecycleEvent::RemoveStage(
                global_stage.path.clone(),
                global_stage.raw.as_deref().map(ferric_bench::sha256_bytes),
            ),
            scripted_remove_event(&global),
            scripted_remove_event(&local),
        ],
        "all finals must be attempted before stages and rendering"
    );
}

#[test]
fn up_nonexclusive_listener_stops_retained_child_and_publishes_nothing() {
    for coordinate in 0..5 {
        let pid = 4201 + u32::try_from(coordinate).unwrap();
        let port = 9421 + u16::try_from(coordinate).unwrap();
        let state = match coordinate {
            0 => ListenerState::OwnedByTargetWildcard,
            1 => ListenerState::OwnedByOther(vec![5101]),
            2 => ListenerState::OwnedByOther(vec![5102, 5103]),
            // Listener inspection includes the target PID when the
            // target and a peer share ownership of the selected port.
            3 => ListenerState::OwnedByOther(vec![pid, 5104]),
            4 => ListenerState::Uninspectable("listener table denied".to_string()),
            _ => unreachable!(),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "bound-generation", Rc::clone(&ledger))
            .with_inspection(Ok(scripted_facts(state.clone())));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [Ok(None), Ok(None), Ok(None)], Rc::clone(&ledger));
        let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&ledger),
        };
        wait_healthy_with(
            &mut child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            Duration::from_secs(1),
            &mut health,
            &mut clock,
        )
        .unwrap();

        let publication =
            inspect_bound_child_for_publication(&mut child, &process, port, &listener);
        if publication.is_ok() {
            ledger.borrow_mut().push(LifecycleEvent::Publish);
        }
        let error = publication.expect_err("non-exclusive ownership must block publication");
        assert!(error.contains("no registration may be published"));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::ClockNow,
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Health(port),
                LifecycleEvent::ChildTryWait(pid),
                LifecycleEvent::Inspect("bound-generation", port),
                LifecycleEvent::Terminate("bound-generation"),
                LifecycleEvent::RetainedWait("bound-generation"),
                LifecycleEvent::ChildWait(pid),
                LifecycleEvent::Listener(pid, port),
            ],
            "case {state:?} must clean only the retained generation and publish nothing"
        );
    }
}

#[test]
fn spawned_child_binding_window_matrix() {
    let pid = 4301;
    let port = 9431;

    // Exit/reuse before a retained object can be acquired: inspecting the
    // original Child proves exit, so no numeric-PID replacement is killed.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let runtime = ScriptedRuntime::new(
        Err("PID now maps to replacement".to_string()),
        Rc::clone(&ledger),
    );
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [Ok(Some(ScriptedExit("exited")))], Rc::clone(&ledger));
    bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid)
        ]
    );

    // The retained object can be acquired just before the original Child
    // reports exit. That proves this generation ended and must not signal
    // either the retained object or a numeric-PID replacement.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "generation-exited-at-bind", Rc::clone(&ledger));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(
        pid,
        [Ok(Some(ScriptedExit("exited immediately after bind")))],
        Rc::clone(&ledger),
    );
    let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
    assert!(error.contains("exited before retained-process binding could be confirmed"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid)
        ]
    );

    // Binding failure while the original Child is live stops and reaps it
    // through that still-authoritative Child object.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let runtime = ScriptedRuntime::new(Err("pidfd open failed".to_string()), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [Ok(None)], Rc::clone(&ledger));
    let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
    assert!(error.contains("the original child was stopped"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::ChildKill(pid),
            LifecycleEvent::ChildWait(pid),
        ]
    );

    // An inspection error immediately after binding cannot fall back to a
    // PID. Cleanup calls only the retained generation; an unproved wait is
    // returned as a recovery clue.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process =
        ScriptedProcess::new(pid, "generation-at-bind", Rc::clone(&ledger)).with_wait(Err(
            ProcessError::Operation("retained wait unavailable".to_string()),
        ));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(
        pid,
        [Err("child inspection unavailable".to_string())],
        Rc::clone(&ledger),
    );
    let error = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap_err();
    assert!(error.contains("recovery failure for retained PID 4301"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::Terminate("generation-at-bind"),
            LifecycleEvent::RetainedWait("generation-at-bind"),
        ]
    );

    // Exit during readiness is cleaned and reaped through the object that
    // was retained before polling began.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "generation-before-poll", Rc::clone(&ledger));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(
        pid,
        [Ok(None), Ok(Some(ScriptedExit("exit during readiness")))],
        Rc::clone(&ledger),
    );
    let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
    let mut health = ScriptedHealth {
        results: VecDeque::new(),
        ledger: Rc::clone(&ledger),
    };
    let mut clock = ScriptedClock {
        now: Instant::now(),
        ledger: Rc::clone(&ledger),
    };
    wait_healthy_with(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        port,
        Duration::from_secs(1),
        &mut health,
        &mut clock,
    )
    .unwrap_err();
    stop_managed_child_with(&mut child, &process, port, &listener).unwrap();
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::ClockNow,
            LifecycleEvent::ChildTryWait(pid),
            LifecycleEvent::Terminate("generation-before-poll"),
            LifecycleEvent::RetainedWait("generation-before-poll"),
            LifecycleEvent::ChildWait(pid),
            LifecycleEvent::Listener(pid, port),
        ]
    );

    // A healthy child becomes publishable only after bind, both liveness
    // checks, HTTP readiness, and exact listener inspection.
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "healthy-generation", Rc::clone(&ledger))
        .with_inspection(Ok(scripted_facts(ListenerState::OwnedByTarget)));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut child = ScriptedChild::new(pid, [Ok(None), Ok(None), Ok(None)], Rc::clone(&ledger));
    let process = bind_spawned_child(&mut child, &runtime, port, &listener).unwrap();
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: Rc::clone(&ledger),
    };
    let mut clock = ScriptedClock {
        now: Instant::now(),
        ledger: Rc::clone(&ledger),
    };
    wait_healthy_with(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        port,
        Duration::from_secs(1),
        &mut health,
        &mut clock,
    )
    .unwrap();
    inspect_bound_child_for_publication(&mut child, &process, port, &listener).unwrap();
    ledger.borrow_mut().push(LifecycleEvent::Publish);
    assert_eq!(
        ledger.borrow().last(),
        Some(&LifecycleEvent::Publish),
        "publication is the final event after retained-generation validation"
    );
    assert!(!ledger.borrow().iter().any(|event| matches!(
        event,
        LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
    )));
}

#[test]
fn status_and_discovery_two_scope_matrix() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let local_path = runfile_path(&workspace);
    let global_path = root.path().join("global/server.json");
    let pid = 4511;
    let runfile = composition_runfile(pid, &local_path);
    let published = publish_mirrored(&workspace, Some(&global_path), &runfile).unwrap();
    let expected_raw = published.local.raw.clone();
    assert_eq!(published.global.as_ref().unwrap().raw, expected_raw);

    let inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: Rc::clone(&ledger),
    };
    let lifecycle = complete_lifecycle_health_with(
        discover_inventory_before_health_with(inventory, verified_not_probed_observation),
        &mut health,
        |_observation| Ok(()),
    );
    let managed = lifecycle.managed.clone();
    let server = match &managed.state {
        ManagedServerState::Ready(server) => server.clone(),
        state => panic!("two exact mirrors must resolve ready, got {state:?}"),
    };
    assert_eq!(
        server.aliases.len(),
        2,
        "the global mirror retains its promised local-origin observation"
    );
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Health(runfile.port)]);

    let rendered = render_status(&status_report(&managed));
    assert!(rendered.success);
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.contains("aliases=2"))
    );
    assert_eq!(
        rendered
            .stdout
            .iter()
            .filter(|line| line.starts_with("[captured]"))
            .count(),
        3
    );

    let scope = ManagedDiscoveryScope {
        workspace: workspace.clone(),
        global: Some(global_path.clone()),
    };
    assert!(matches!(
        crate::backend::automatic_endpoint_from_discovery(scope.clone(), managed.clone()),
        Ok(crate::backend::EndpointSelection::Managed { .. })
    ));
    let mut doctor_effects = RecordingDoctorEffects::default();
    let doctor =
        doctor_report_after_discovery(&doctor_fixture_args(), &managed, &mut doctor_effects);
    assert!(doctor.success);
    assert_eq!(
        doctor_effects.events,
        vec![DoctorEvent::Binary, DoctorEvent::File]
    );

    let facts = ProcessFacts {
        identity: server.identity.clone(),
        listener: ListenerState::OwnedByTarget,
    };
    let process = ScriptedProcess::new(pid, "two-scope-generation", Rc::clone(&ledger))
        .with_inspection(Ok(facts));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let retained = runtime.acquire(pid).unwrap();
    let captures = vec![published.local, published.global.unwrap()];
    let plan = retained_target_down_plan(
        &managed.state,
        retained,
        captures.clone(),
        discovery_revisions(&managed.observations),
    )
    .unwrap();
    let mut down_effects = ScriptedEffects::filesystem(Rc::clone(&ledger))
        .with_scope(scope)
        .with_listeners(VecDeque::from([ListenerState::Absent]));
    let report = execute_down_plan(plan, &mut down_effects);
    let down = ledger_rendered(render_down_report(&report), &ledger);
    assert!(down.success);
    assert_eq!(report.disposition, DownDisposition::Stopped);
    assert!(
        report
            .registrations
            .iter()
            .all(|registration| { registration.outcome == DownRegistrationOutcome::Removed })
    );
    assert!(!local_path.exists());
    assert!(!global_path.exists());
    assert_eq!(
        ledger.borrow().as_slice(),
        &[
            LifecycleEvent::Health(runfile.port),
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("two-scope-generation", runfile.port),
            LifecycleEvent::Terminate("two-scope-generation"),
            LifecycleEvent::RetainedWait("two-scope-generation"),
            LifecycleEvent::Listener(pid, runfile.port),
            scripted_remove_event(&captures[0]),
            scripted_remove_event(&captures[1]),
            LifecycleEvent::Render,
        ]
    );
}

#[test]
fn down_retained_handle_transition_matrix() {
    let pid = 4521;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("transition-workspace");
    let local_path = runfile_path(&workspace);
    let global_path = root.path().join("transition-global/server.json");
    let runfile = composition_runfile(pid, &local_path);
    let port = runfile.port;
    let identity = runfile.process_identity.clone().unwrap();
    let published = publish_mirrored(&workspace, Some(&global_path), &runfile).unwrap();
    let local = published.local;
    let global = published.global.unwrap();
    let captures = vec![local.clone(), global.clone()];
    let scope = ManagedDiscoveryScope {
        workspace: workspace.clone(),
        global: Some(global_path),
    };
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: Rc::clone(&ledger),
    };
    let lifecycle = complete_lifecycle_health_with(
        discover_inventory_before_health_with(
            inventory_runfiles(&workspace, scope.global.clone()),
            verified_not_probed_observation,
        ),
        &mut health,
        |_observation| Ok(()),
    );
    let managed = lifecycle.managed;
    assert!(matches!(&managed.state, ManagedServerState::Ready(_)));
    let revisions = discovery_revisions(&managed.observations);

    // Resolve the real inventory first, acquire its exact retained
    // generation once, then remap the same numeric PID in the fake process
    // table. Planning and execution must keep using the already-retained
    // object while real inventory revisions gate pre-signal mutation.
    let process = ScriptedProcess::new(pid, "retained-before-remap", Rc::clone(&ledger))
        .with_inspection(Ok(ProcessFacts {
            identity: identity.clone(),
            listener: ListenerState::OwnedByTarget,
        }));
    let runtime = ScriptedPidMapRuntime::new(process, Rc::clone(&ledger));
    let retained = runtime.acquire(pid).unwrap();
    runtime.replace(
        pid,
        "replacement-generation",
        ScriptedProcess::new(pid, "replacement-generation", Rc::clone(&ledger)).with_inspection(
            Ok(ProcessFacts {
                identity: process_identity(u64::from(pid) + 1),
                listener: ListenerState::OwnedByTarget,
            }),
        ),
    );
    let replacement = discovery_fixture_path("transition-replacement");
    let mut effects = ScriptedEffects::new(
        [ListenerState::Absent],
        [
            Ok(RemovalOutcome::Removed),
            Ok(RemovalOutcome::ReplacementPreserved {
                path: replacement.clone(),
                detail: "concurrent alias replacement".to_string(),
            }),
        ],
        Rc::clone(&ledger),
    )
    .with_scope(scope.clone());
    let plan = retained_target_down_plan(
        &managed.state,
        retained,
        captures.clone(),
        revisions.clone(),
    )
    .unwrap();
    let report = execute_down_plan(plan, &mut effects);
    let rendered = ledger_rendered(render_down_report(&report), &ledger);
    assert_eq!(report.disposition, DownDisposition::CleanupPartial);
    assert!(report.exit_proven && report.listener_released);
    assert!(rendered.stdout.iter().any(|line| {
        line.contains("replacement-preserved") && line.contains(&replacement.display().to_string())
    }));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Health(port),
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::PidMapReplace(pid, "replacement-generation"),
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("retained-before-remap", port),
            LifecycleEvent::Terminate("retained-before-remap"),
            LifecycleEvent::RetainedWait("retained-before-remap"),
            LifecycleEvent::Listener(pid, port),
            scripted_remove_event(&local),
            scripted_remove_event(&global),
            LifecycleEvent::Render,
        ],
        "observe/acquire, pre-signal revalidation, post-exit listener proof, and per-alias cleanup must remain ordered"
    );
    assert_eq!(
        ledger
            .borrow()
            .iter()
            .filter(|event| matches!(event, LifecycleEvent::Acquire(_)))
            .count(),
        1,
        "PID remap must not trigger a numeric-PID reacquisition"
    );
    assert!(ledger.borrow().iter().all(|event| !matches!(
        event,
        LifecycleEvent::Inspect("replacement-generation", _)
            | LifecycleEvent::Terminate("replacement-generation")
            | LifecycleEvent::RetainedWait("replacement-generation")
    )));

    for case in [
        "pre-signal-revision-change",
        "pre-signal-listener-transfer",
        "terminate-failure",
        "wait-failure",
        "post-exit-listener-transfer",
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let inspected_listener = if case == "pre-signal-listener-transfer" {
            ListenerState::OwnedByOther(vec![9901])
        } else {
            ListenerState::OwnedByTarget
        };
        let mut process =
            ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_inspection(Ok(ProcessFacts {
                identity: identity.clone(),
                listener: inspected_listener,
            }));
        if case == "terminate-failure" {
            process = process.with_terminate(Err(ProcessError::Operation(
                "retained terminate failed".to_string(),
            )));
        }
        if case == "wait-failure" {
            process = process.with_wait(Err(ProcessError::Operation(
                "retained wait failed".to_string(),
            )));
        }
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let retained = runtime.acquire(pid).unwrap();
        let mut effects = ScriptedEffects::new(
            [if case == "post-exit-listener-transfer" {
                ListenerState::OwnedByOther(vec![9902])
            } else {
                ListenerState::Absent
            }],
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        if case == "pre-signal-revision-change" {
            effects.revalidations =
                VecDeque::from([Err("registration changed before signal".to_string())]);
        }
        let plan = retained_target_down_plan(
            &managed.state,
            retained,
            captures.clone(),
            revisions.clone(),
        )
        .unwrap();
        let report = execute_down_plan(plan, &mut effects);
        let rendered = ledger_rendered(render_down_report(&report), &ledger);
        assert_eq!(report.disposition, DownDisposition::Failed, "{case}");
        assert!(!rendered.success, "{case}");
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Remove(_, _))),
            "{case} must preserve every alias"
        );
        assert_eq!(ledger.borrow().first(), Some(&LifecycleEvent::Acquire(pid)));
        assert_eq!(ledger.borrow().last(), Some(&LifecycleEvent::Render));
        let events = ledger.borrow();
        let inspect = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::Inspect(_, _)));
        let terminate = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::Terminate(_)));
        if case == "pre-signal-revision-change" {
            assert!(inspect.is_none() && terminate.is_none());
        } else if case == "pre-signal-listener-transfer" {
            assert!(inspect.is_some() && terminate.is_none());
        } else {
            assert!(inspect < terminate);
        }
    }
}

#[test]
fn up_spawned_child_binding_precedes_readiness() {
    let pid = 4531;
    let port = 9531;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("orchestrated-up");
    let mut launch_cfg = cfg(Engine::LlamaServer);
    launch_cfg.port = port;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "up-bound-generation", Rc::clone(&ledger))
        .with_inspection(Ok(ProcessFacts {
            identity: process_identity(u64::from(pid)),
            listener: ListenerState::OwnedByTarget,
        }))
        .with_inspection(Ok(ProcessFacts {
            identity: process_identity(u64::from(pid)),
            listener: ListenerState::OwnedByTarget,
        }));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: Rc::clone(&ledger),
    };
    let mut clock = ScriptedClock {
        now: Instant::now(),
        ledger: Rc::clone(&ledger),
    };
    let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
    let mut compensation = ScriptedEffects::publication(
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let spawn_ledger = Rc::clone(&ledger);
    let launched = orchestrate_launch_with(
        &workspace,
        None,
        &launch_cfg,
        move || {
            spawn_ledger.borrow_mut().push(LifecycleEvent::Spawn(pid));
            Ok(ScriptedChild::new(
                pid,
                [Ok(None), Ok(None), Ok(None), Ok(None)],
                Rc::clone(&spawn_ledger),
            ))
        },
        &runtime,
        &listener,
        &mut health,
        &mut clock,
        |workspace, global, runfile| {
            publish_mirrored_with(workspace, global, runfile, &mut persistence)
        },
        &mut compensation,
    )
    .unwrap();
    assert_eq!(launched.pid, pid);
    assert_eq!(persistence.serializations, 1);
    assert!(launched.published.local.path.exists());
    let events = ledger.borrow();
    assert_eq!(events.first(), Some(&LifecycleEvent::Spawn(pid)));
    let bind = events
        .iter()
        .position(|event| *event == LifecycleEvent::Acquire(pid))
        .unwrap();
    let readiness = events
        .iter()
        .position(|event| *event == LifecycleEvent::Health(port))
        .unwrap();
    let publication = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleEvent::Persistence(PersistencePhase::CreateStage, _)
            )
        })
        .unwrap();
    let pre_publication_inspection = events
        .iter()
        .position(|event| *event == LifecycleEvent::Inspect("up-bound-generation", port))
        .unwrap();
    let post_publication_child_check = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| {
            (publication < index && *event == LifecycleEvent::ChildTryWait(pid)).then_some(index)
        })
        .unwrap();
    let post_publication_inspection = events
        .iter()
        .rposition(|event| *event == LifecycleEvent::Inspect("up-bound-generation", port))
        .unwrap();
    assert!(
        bind < readiness
            && readiness < pre_publication_inspection
            && pre_publication_inspection < publication
            && publication < post_publication_child_check
            && post_publication_child_check < post_publication_inspection
    );
    drop(events);

    // A live child may change identity or listener ownership while the
    // registration files are being persisted. Neither transition may
    // cross the final Ready boundary: stop the retained generation and
    // compensate the exact attempt-owned publication instead.
    for (case, changed_identity, post_listener) in [
        ("identity-transition", true, ListenerState::OwnedByTarget),
        (
            "listener-transition",
            false,
            ListenerState::OwnedByTargetWildcard,
        ),
    ] {
        let case_pid = pid + if changed_identity { 10 } else { 20 };
        let case_port = port + if changed_identity { 10 } else { 20 };
        let case_workspace = root.path().join(case);
        let expected_identity = process_identity(u64::from(case_pid));
        let post_identity = if changed_identity {
            process_identity(u64::from(case_pid + 1))
        } else {
            expected_identity.clone()
        };
        let case_ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(
            case_pid,
            "post-publication-transition",
            Rc::clone(&case_ledger),
        )
        .with_inspection(Ok(ProcessFacts {
            identity: expected_identity,
            listener: ListenerState::OwnedByTarget,
        }))
        .with_inspection(Ok(ProcessFacts {
            identity: post_identity,
            listener: post_listener,
        }));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&case_ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&case_ledger));
        let mut health = ScriptedHealth {
            results: VecDeque::from([true]),
            ledger: Rc::clone(&case_ledger),
        };
        let mut clock = ScriptedClock {
            now: Instant::now(),
            ledger: Rc::clone(&case_ledger),
        };
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &case_ledger);
        let mut compensation = ScriptedEffects::filesystem(Rc::clone(&case_ledger));
        let mut case_cfg = cfg(Engine::LlamaServer);
        case_cfg.port = case_port;
        let spawn_ledger = Rc::clone(&case_ledger);
        let result = orchestrate_launch_with(
            &case_workspace,
            None,
            &case_cfg,
            move || {
                spawn_ledger
                    .borrow_mut()
                    .push(LifecycleEvent::Spawn(case_pid));
                Ok(ScriptedChild::new(
                    case_pid,
                    [Ok(None), Ok(None), Ok(None), Ok(None)],
                    Rc::clone(&spawn_ledger),
                ))
            },
            &runtime,
            &listener,
            &mut health,
            &mut clock,
            |workspace, global, runfile| {
                publish_mirrored_with(workspace, global, runfile, &mut persistence)
            },
            &mut compensation,
        );
        let Err(LaunchOrchestrationError::Publication(report)) = result else {
            panic!("{case} must fail through publication compensation");
        };
        assert_eq!(
            report.disposition,
            PublicationDisposition::RolledBack,
            "{case}"
        );
        assert!(!report.success, "{case}");
        assert!(!runfile_path(&case_workspace).exists(), "{case}");
        let events = case_ledger.borrow();
        let inspections = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                (*event == LifecycleEvent::Inspect("post-publication-transition", case_port))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(inspections.len(), 2, "{case}");
        let publication = events
            .iter()
            .rposition(|event| matches!(event, LifecycleEvent::Persistence(_, _)))
            .unwrap();
        let terminate = events
            .iter()
            .position(|event| *event == LifecycleEvent::Terminate("post-publication-transition"))
            .unwrap();
        let remove = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
            .unwrap();
        assert!(inspections[0] < publication && publication < inspections[1]);
        assert!(inspections[1] < terminate && terminate < remove);
    }

    let blocked_ledger = Rc::new(RefCell::new(Vec::new()));
    let runtime = ScriptedRuntime::new(
        Err("retained handle unavailable".to_string()),
        Rc::clone(&blocked_ledger),
    );
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&blocked_ledger));
    let mut health = ScriptedHealth {
        results: VecDeque::new(),
        ledger: Rc::clone(&blocked_ledger),
    };
    let mut clock = ScriptedClock {
        now: Instant::now(),
        ledger: Rc::clone(&blocked_ledger),
    };
    let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &blocked_ledger);
    let mut compensation = ScriptedEffects::publication(
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&blocked_ledger),
    );
    let spawn_ledger = Rc::clone(&blocked_ledger);
    let blocked = orchestrate_launch_with(
        &root.path().join("blocked-up"),
        None,
        &launch_cfg,
        move || {
            spawn_ledger.borrow_mut().push(LifecycleEvent::Spawn(pid));
            Ok(ScriptedChild::new(
                pid,
                [Ok(None)],
                Rc::clone(&spawn_ledger),
            ))
        },
        &runtime,
        &listener,
        &mut health,
        &mut clock,
        |workspace, global, runfile| {
            publish_mirrored_with(workspace, global, runfile, &mut persistence)
        },
        &mut compensation,
    );
    assert!(matches!(
        blocked,
        Err(LaunchOrchestrationError::Bind { .. })
    ));
    assert_eq!(persistence.serializations, 0);
    assert!(blocked_ledger.borrow().iter().all(|event| !matches!(
        event,
        LifecycleEvent::Health(_) | LifecycleEvent::Persistence(_, _)
    )));
}

#[test]
fn registration_publication_failure_matrix() {
    fn attempt(error: &PublishError) -> &PublicationAttempt {
        match error {
            PublishError::Write { attempt, .. }
            | PublishError::Mirror { attempt, .. }
            | PublishError::Durability { attempt, .. } => attempt,
            PublishError::Invalid { .. } | PublishError::Serialize(_) => {
                panic!("publication fault did not retain an attempt: {error}")
            }
        }
    }

    // First prove both successful shapes cross the real publication
    // algorithm and reach the coordinator only after all persistence
    // phases. The coordinator performs no shutdown or cleanup while the
    // retained child is still live.
    for mirrored in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("success-workspace");
        let local = runfile_path(&workspace);
        let global = root.path().join("success-global/server.json");
        let pid = 4550 + u32::from(mirrored);
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
        let publication = publish_mirrored_with(
            &workspace,
            mirrored.then_some(global.as_path()),
            &runfile,
            &mut persistence,
        );
        assert_eq!(persistence.serializations, 1);
        let process = ScriptedProcess::new(pid, "publication-success", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: runfile
                    .process_identity
                    .clone()
                    .expect("successful schema-v2 publication has process identity"),
                listener: ListenerState::OwnedByTarget,
            }));
        let mut child = ScriptedChild::new(pid, [Ok(None)], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::publication(
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::Ready);
        assert!(report.success);
        assert_eq!(
            fs::read(&local).unwrap(),
            serde_json::to_vec_pretty(&runfile).unwrap()
        );
        if mirrored {
            assert!(global.exists());
        } else {
            assert!(!global.exists());
        }
        assert_eq!(
            ledger.borrow().last(),
            Some(&LifecycleEvent::Inspect(
                "publication-success",
                runfile.port
            ))
        );
        assert!(ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Terminate(_)
                | LifecycleEvent::Remove(_, _)
                | LifecycleEvent::RemoveStage(_, _)
        )));
    }

    // Every local and global persistence boundary feeds the exact
    // PublicationAttempt produced by publish_mirrored_with into the
    // shutdown/compensation coordinator. Cleanup is real and conditional.
    for fail_global in [false, true] {
        for phase in [
            PersistencePhase::CreateStage,
            PersistencePhase::WriteAll,
            PersistencePhase::Flush,
            PersistencePhase::FileSync,
            PersistencePhase::PersistNoClobber,
            PersistencePhase::StageCleanup,
            PersistencePhase::ParentSync,
        ] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(format!(
                "{}-{phase:?}",
                if fail_global { "global" } else { "local" }
            ));
            let local = runfile_path(&workspace);
            let global = root.path().join("global/server.json");
            let pid = 4560 + u32::from(fail_global);
            let runfile = composition_runfile(pid, &local);
            let target = if fail_global { &global } else { &local };
            let ledger = Rc::new(RefCell::new(Vec::new()));
            let mut persistence = if phase == PersistencePhase::StageCleanup {
                ledgered(
                    ScriptedPersistenceEffects::retaining_committed_stage(target),
                    &ledger,
                )
            } else {
                ledgered(ScriptedPersistenceEffects::failing(target, phase), &ledger)
            };
            let publication = publish_mirrored_with(
                &workspace,
                fail_global.then_some(global.as_path()),
                &runfile,
                &mut persistence,
            );
            assert_eq!(persistence.serializations, 1, "{fail_global} {phase:?}");
            let error = publication.as_ref().unwrap_err();
            let retained = attempt(error).clone();
            assert_eq!(retained.terminal_phase, phase);
            assert_eq!(
                retained.final_committed,
                matches!(
                    phase,
                    PersistencePhase::StageCleanup | PersistencePhase::ParentSync
                )
            );
            assert_eq!(
                retained.finals.len(),
                usize::from(fail_global)
                    + usize::from(matches!(
                        phase,
                        PersistencePhase::StageCleanup | PersistencePhase::ParentSync
                    )),
                "{fail_global} {phase:?} must expose every committed final"
            );
            assert_eq!(
                retained.stages.len(),
                usize::from(!matches!(
                    phase,
                    PersistencePhase::CreateStage | PersistencePhase::ParentSync
                )),
                "{fail_global} {phase:?} must explain every retained stage"
            );

            let process = ScriptedProcess::new(pid, "publication-boundary", Rc::clone(&ledger));
            let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
            let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
            let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
            let report = complete_publication_with(
                &mut child,
                &process,
                runfile.port,
                publication,
                &listener,
                &mut cleanup,
            );
            assert_eq!(
                report.disposition,
                PublicationDisposition::RolledBack,
                "{fail_global} {phase:?}: {report:?}"
            );
            assert!(!local.exists(), "{fail_global} {phase:?}");
            assert!(!global.exists(), "{fail_global} {phase:?}");
            for stage in &retained.stages {
                assert!(!stage.path.exists(), "{fail_global} {phase:?}");
            }
            let events = ledger.borrow();
            let last_persistence = events
                .iter()
                .rposition(|event| matches!(event, LifecycleEvent::Persistence(_, _)))
                .unwrap();
            let terminate = events
                .iter()
                .position(|event| *event == LifecycleEvent::Terminate("publication-boundary"))
                .unwrap();
            let first_cleanup = events.iter().position(|event| {
                matches!(
                    event,
                    LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
                )
            });
            assert!(last_persistence < terminate);
            if let Some(first_cleanup) = first_cleanup {
                let released = events
                    .iter()
                    .position(|event| *event == LifecycleEvent::Listener(pid, runfile.port))
                    .unwrap();
                assert!(released < first_cleanup);
            }
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                    .count(),
                retained.finals.len()
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, LifecycleEvent::RemoveStage(_, _)))
                    .count(),
                retained.stages.len()
            );
        }
    }

    // Real no-clobber conflicts preserve the winner while the
    // coordinator removes only attempt-owned finals and stages.
    for existing_global in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("occupied-workspace");
        let local = runfile_path(&workspace);
        let global = root.path().join("occupied-global/server.json");
        let occupied = if existing_global { &global } else { &local };
        fs::create_dir_all(occupied.parent().unwrap()).unwrap();
        fs::write(occupied, b"external-publication-winner").unwrap();
        let pid = 4570 + u32::from(existing_global);
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
        let publication = publish_mirrored_with(
            &workspace,
            existing_global.then_some(global.as_path()),
            &runfile,
            &mut persistence,
        );
        assert_eq!(
            attempt(publication.as_ref().unwrap_err()).terminal_phase,
            PersistencePhase::PersistNoClobber
        );
        let process = ScriptedProcess::new(pid, "publication-no-clobber", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert_eq!(fs::read(occupied).unwrap(), b"external-publication-winner");
        if existing_global {
            assert!(!local.exists());
        }
    }

    // Lexical alias rejection occurs before serialization/staging, but
    // still enters the same child-owned failure coordinator.
    {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("alias-workspace");
        let local = runfile_path(&workspace);
        let alias = local.parent().unwrap().join(".").join("server.json");
        let pid = 4581;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
        let publication =
            publish_mirrored_with(&workspace, Some(&alias), &runfile, &mut persistence);
        assert!(matches!(publication, Err(PublishError::Invalid { .. })));
        assert_eq!(persistence.serializations, 0);
        assert!(ledger.borrow().is_empty());
        let process = ScriptedProcess::new(pid, "publication-alias", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert!(report.finals.is_empty() && report.stages.is_empty());
        assert!(!local.exists());
    }

    // A child exit after a successful mirrored publication is another
    // coordinator boundary: both real finals are rolled back only after
    // retained exit, reap, and listener-release proof.
    {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("post-publish-exit");
        let local = runfile_path(&workspace);
        let global = root.path().join("post-publish-global/server.json");
        let pid = 4591;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let process = ScriptedProcess::new(pid, "publication-child-exit", Rc::clone(&ledger))
            .with_terminate(Ok(false));
        let mut child = ScriptedChild::new(
            pid,
            [Ok(Some(ScriptedExit("exited after publication")))],
            Rc::clone(&ledger),
        );
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert!(!local.exists() && !global.exists());
        let events = ledger.borrow();
        let child_exit = events
            .iter()
            .position(|event| *event == LifecycleEvent::ChildTryWait(pid))
            .unwrap();
        let first_remove = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
            .unwrap();
        assert!(child_exit < first_remove);
    }

    // Successful real publication followed by an inconclusive Child
    // status check must enter the same retained-object shutdown path. A
    // later retained wait can independently prove exit and authorize
    // rollback; without that proof both finals remain held.
    for retained_exit_proven in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join(if retained_exit_proven {
            "try-wait-error-exited"
        } else {
            "try-wait-error-unproved"
        });
        let local = runfile_path(&workspace);
        let global = root.path().join("try-wait-error-global/server.json");
        let pid = 4595 + u32::from(retained_exit_proven);
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(ScriptedPersistenceEffects::default(), &ledger);
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        assert!(publication.is_ok());
        let process = ScriptedProcess::new(
            pid,
            if retained_exit_proven {
                "try-wait-error-exited"
            } else {
                "try-wait-error-unproved"
            },
            Rc::clone(&ledger),
        )
        .with_wait(Ok(retained_exit_proven));
        let mut child = ScriptedChild::new(
            pid,
            [Err("post-publication child status unavailable".to_string())],
            Rc::clone(&ledger),
        );
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert!(
            report.diagnostics[0].contains("could not confirm the engine child after publication")
        );
        if retained_exit_proven {
            assert_eq!(report.disposition, PublicationDisposition::RolledBack);
            assert!(!local.exists() && !global.exists());
            assert_eq!(
                ledger
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
                    .count(),
                2
            );
        } else {
            assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
            assert!(local.exists() && global.exists());
            assert_eq!(report.finals.len(), 2);
            assert!(
                report
                    .finals
                    .iter()
                    .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
            );
            assert!(ledger.borrow().iter().all(|event| !matches!(
                event,
                LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
            )));
        }
        let events = ledger.borrow();
        let child_status = events
            .iter()
            .position(|event| *event == LifecycleEvent::ChildTryWait(pid))
            .unwrap();
        let retained_wait = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::RetainedWait(_)))
            .unwrap();
        assert!(child_status < retained_wait);
    }

    // Runtime proof failures all begin with a real global FileSync fault.
    // No unproved-exit row reaches either conditional store adapter.
    for case in [
        "terminate-timeout",
        "wait-timeout",
        "wait-error",
        "reap-error",
        "listener-survival",
    ] {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join(format!("runtime-{case}"));
        let local = runfile_path(&workspace);
        let global = root.path().join("runtime-global/server.json");
        let pid = 4601;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(
            ScriptedPersistenceEffects::failing(&global, PersistencePhase::FileSync),
            &ledger,
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let mut process = ScriptedProcess::new(pid, case, Rc::clone(&ledger));
        if case == "terminate-timeout" {
            process = process
                .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
                .with_wait(Ok(false));
        } else if case == "wait-timeout" {
            process = process.with_wait(Ok(false));
        } else if case == "wait-error" {
            process = process.with_wait(Err(ProcessError::Operation(
                "retained wait unavailable".to_string(),
            )));
        }
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        if case == "reap-error" {
            child.wait = VecDeque::from([Err("reap failed".to_string())]);
        }
        let listener = ScriptedListener::new(
            if case == "listener-survival" {
                ListenerState::OwnedByTarget
            } else {
                ListenerState::Absent
            },
            Rc::clone(&ledger),
        );
        let mut cleanup = ScriptedEffects::publication(
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(
            report.disposition,
            PublicationDisposition::RecoveryHeld,
            "{case}"
        );
        assert!(
            report
                .finals
                .iter()
                .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
        );
        assert!(
            report
                .stages
                .iter()
                .all(|entry| matches!(entry.outcome, DownRegistrationOutcome::Held { .. }))
        );
        assert!(ledger.borrow().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Remove(_, _) | LifecycleEvent::RemoveStage(_, _)
        )));
    }

    // A terminate error does not itself prove exit, but a later successful
    // wait on that same retained object, reap, and listener release do.
    {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("terminate-error-exited");
        let local = runfile_path(&workspace);
        let global = root.path().join("terminate-error-global/server.json");
        let pid = 4611;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(
            ScriptedPersistenceEffects::failing(&global, PersistencePhase::FileSync),
            &ledger,
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let process = ScriptedProcess::new(pid, "terminate-error-exited", Rc::clone(&ledger))
            .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
            .with_wait(Ok(true));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::RolledBack);
        assert!(!local.exists() && !global.exists());
    }

    // A concurrent final replacement is preserved while the unchanged
    // stage is still removed, producing an explicit partial recovery.
    {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("replacement-race");
        let local = runfile_path(&workspace);
        let global = root.path().join("replacement-global/server.json");
        let pid = 4621;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(
            ScriptedPersistenceEffects::failing(&global, PersistencePhase::FileSync),
            &ledger,
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        fs::write(&local, b"concurrent final replacement").unwrap();
        let process = ScriptedProcess::new(pid, "publication-replacement", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::filesystem(Rc::clone(&ledger));
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
        assert!(matches!(
            report.finals[0].outcome,
            DownRegistrationOutcome::ReplacementPreserved { .. }
        ));
        assert!(matches!(
            report.stages[0].outcome,
            DownRegistrationOutcome::Removed
        ));
        assert_eq!(fs::read(&local).unwrap(), b"concurrent final replacement");
    }

    // Store-adapter failures after proven exit attempt every final before
    // every stage and retain each explicit recovery location.
    {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("cleanup-failures");
        let local = runfile_path(&workspace);
        let global = root.path().join("cleanup-global/server.json");
        let final_holding = root.path().join("final-holding");
        let stage_holding = root.path().join("stage-holding");
        let pid = 4631;
        let runfile = composition_runfile(pid, &local);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut persistence = ledgered(
            ScriptedPersistenceEffects::failing(&global, PersistencePhase::FileSync),
            &ledger,
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let process = ScriptedProcess::new(pid, "publication-cleanup-failure", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedEffects::publication(
            [Err(RemovalError {
                path: local.clone(),
                kind: RemovalFailureKind::Remove,
                detail: "conditional final cleanup failed".to_string(),
                preserved_at: Some(final_holding.clone()),
            })],
            [Err(RemovalError {
                path: global.clone(),
                kind: RemovalFailureKind::Remove,
                detail: "conditional stage cleanup failed".to_string(),
                preserved_at: Some(stage_holding.clone()),
            })],
            Rc::clone(&ledger),
        );
        let report = complete_publication_with(
            &mut child,
            &process,
            runfile.port,
            publication,
            &listener,
            &mut cleanup,
        );
        let rendered = ledger_rendered(render_publication_report(&report), &ledger);
        assert_eq!(report.disposition, PublicationDisposition::RecoveryPartial);
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| { line.contains(&final_holding.display().to_string()) })
        );
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| { line.contains(&stage_holding.display().to_string()) })
        );
        let events = ledger.borrow();
        let final_cleanup = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
            .unwrap();
        let stage_cleanup = events
            .iter()
            .position(|event| matches!(event, LifecycleEvent::RemoveStage(_, _)))
            .unwrap();
        assert!(final_cleanup < stage_cleanup);
        assert_eq!(events.last(), Some(&LifecycleEvent::Render));
    }
}

fn unused_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn write_test_runfile(path: &Path, runfile: &ServerRunfile) {
    std::fs::create_dir_all(path.parent().expect("runfile parent")).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(runfile).unwrap()).unwrap();
}

#[test]
fn blocked_local_inventory_prevents_global_autodiscovery() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let local = runfile_path(&workspace);
    let global = root.path().join("config").join("server.json");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, b"{not-json").unwrap();
    write_test_runfile(&global, &discovery_fixture_runfile(4301, "global-origin"));

    let discovery = discover_managed_server_in(&ManagedDiscoveryScope {
        workspace,
        global: Some(global),
    });
    let (ManagedServerState::Conflict { issues } | ManagedServerState::Unverifiable { issues }) =
        discovery.state
    else {
        panic!("blocked local registration must block resolution: {discovery:?}");
    };
    let local = local.display().to_string();
    assert!(
        issues.iter().any(|issue| issue.detail.contains(&local)),
        "{issues:?}"
    );
}

#[test]
fn durable_tailscale_registration_is_retained_as_a_blocker() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let local = runfile_path(&workspace);
    write_test_runfile(
        &local,
        &ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: u32::MAX,
            port: 8080,
            base_url: "https://example-host.tailnet-example.ts.net/v1".to_string(),
            tailscale: true,
            tailscale_serve: None,
            model: None,
            context_size: None,
            sampling_seed: None,
            parallel_slots: None,
            process_identity: None,
            origin_local_runfile: None,
        },
    );

    assert_eq!(down_impl(&workspace, None), ExitCode::FAILURE);
    assert!(
        local.exists(),
        "the registration must retain the clue to durable proxy state"
    );
}

fn llama_args(model: &Path) -> ServerUpArgs {
    ServerUpArgs {
        engine: Engine::LlamaServer,
        model: Some(model.display().to_string()),
        mmproj: None,
        ctx: 8192,
        port: unused_port(),
        threads: None,
        gpu_layers: Some(0),
        batch_size: None,
        seed: None,
        parallel: None,
    }
}

fn serve_one_status(path: &'static str, status: &'static str) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0_u8; 512];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(
            request.starts_with(&format!("GET {path} HTTP/1.1\r\n")),
            "unexpected request: {request}"
        );
        let body = "{}";
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (port, handle)
}

#[test]
fn llama_server_argv() {
    let c = command(&cfg(Engine::LlamaServer));
    assert_eq!(c.program, "llama-server");
    assert_eq!(
        c.args,
        vec![
            "-m",
            "model.gguf",
            "-c",
            "4096",
            "--host",
            "127.0.0.1",
            "--port",
            "8080"
        ]
    );
    assert!(c.env.is_empty());
}

#[test]
fn llama_server_mmproj() {
    let mut config = cfg(Engine::LlamaServer);
    config.mmproj = Some(PathBuf::from("proj.gguf"));
    let c = command(&config);
    assert!(c.args.windows(2).any(|w| w == ["--mmproj", "proj.gguf"]));
}

#[test]
fn ollama_argv_and_env() {
    let c = command(&cfg(Engine::Ollama));
    assert_eq!(c.program, "ollama");
    assert_eq!(c.args, vec!["serve"]);
    assert_eq!(
        c.env,
        vec![("OLLAMA_HOST".to_string(), "127.0.0.1:8080".to_string())]
    );
}

#[test]
fn llama_server_edge_tuning_flags() {
    // WHEN threads/gpu_layers/batch_size are set with llama-server THEN
    // argv SHALL include the matching flags (sprint 35).
    let mut config = cfg(Engine::LlamaServer);
    config.threads = Some(4);
    config.gpu_layers = Some(20);
    config.batch_size = Some(512);
    config.seed = Some(42);
    config.parallel = Some(1);
    let c = command(&config);
    assert!(c.args.windows(2).any(|w| w == ["-t", "4"]));
    assert!(c.args.windows(2).any(|w| w == ["-ngl", "20"]));
    assert!(c.args.windows(2).any(|w| w == ["-b", "512"]));
    assert!(c.args.windows(2).any(|w| w == ["--seed", "42"]));
    assert!(c.args.windows(2).any(|w| w == ["--parallel", "1"]));
}

#[test]
fn ollama_ignores_edge_tuning_flags() {
    // Ollama doesn't take these as CLI flags — set-but-unused, argv unchanged.
    let mut config = cfg(Engine::Ollama);
    config.threads = Some(4);
    config.gpu_layers = Some(20);
    config.batch_size = Some(512);
    let c = command(&config);
    assert_eq!(
        c.args,
        vec!["serve"],
        "edge-tuning flags must not leak into Ollama argv"
    );
}

#[test]
fn ollama_preflight_rejects_claiming_llama_sampling_controls() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    let mut args = llama_args(&model);
    args.engine = Engine::Ollama;
    args.model = Some("example-model".to_string());
    args.seed = Some(42);

    let error = validate_launch_preconditions(dir.path(), &args, None).unwrap_err();
    assert!(error.contains("supported only by llama-server"), "{error}");

    args.seed = None;
    args.parallel = Some(1);
    let error = validate_launch_preconditions(dir.path(), &args, None).unwrap_err();
    assert!(error.contains("supported only by llama-server"), "{error}");
}

#[test]
fn host_is_loopback() {
    // ADR-005: the launcher binds loopback only.
    for engine in [Engine::LlamaServer, Engine::Ollama] {
        let c = command(&cfg(engine));
        let joined = format!("{} {:?}", c.args.join(" "), c.env);
        assert!(joined.contains("127.0.0.1"));
        assert!(!joined.contains("0.0.0.0"));
    }
}

#[test]
fn health_url_per_engine() {
    assert_eq!(
        health_url(Engine::LlamaServer, "http://127.0.0.1:8080/v1"),
        "http://127.0.0.1:8080/health"
    );
    assert_eq!(
        health_url(Engine::Ollama, "http://127.0.0.1:11434/v1"),
        "http://127.0.0.1:11434/v1/models"
    );
}

#[test]
fn launch_preflight_rejects_existing_local_registration() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    let local = runfile_path(dir.path());
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, b"stale or live registration").unwrap();

    let error = validate_launch_preconditions(dir.path(), &llama_args(&model), None)
        .expect_err("an existing local registration must block launch");
    assert!(error.contains("local server registration already exists"));
}

#[test]
fn launch_preflight_rejects_existing_global_registration() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    let global = dir.path().join("global").join("server.json");
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, b"stale or live registration").unwrap();

    let error = validate_launch_preconditions(dir.path(), &llama_args(&model), Some(&global))
        .expect_err("an existing global registration must block launch");
    assert!(error.contains("global server registration already exists"));
}

#[test]
fn launch_preflight_rejects_occupied_target_port() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut args = llama_args(&model);
    args.port = listener.local_addr().unwrap().port();

    let error = validate_launch_preconditions(dir.path(), &args, None)
        .expect_err("an occupied port must block launch");
    assert!(error.contains("is already listening"));
}

#[test]
fn llama_launch_requires_regular_model_and_mmproj_files() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.gguf");
    let mut args = llama_args(&missing);
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("model must be a regular file")
    );

    args.model = None;
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("--model is required")
    );

    args.model = Some(dir.path().display().to_string());
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("model must be a regular file")
    );

    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    args.model = Some(model.display().to_string());
    args.mmproj = Some(dir.path().to_path_buf());
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("projector must be a regular file")
    );

    let mmproj = dir.path().join("mmproj.gguf");
    std::fs::write(&mmproj, b"projector").unwrap();
    args.mmproj = Some(mmproj);
    validate_launch_preconditions(dir.path(), &args, None).unwrap();
}

#[test]
fn llama_launch_rejects_zero_context_or_port() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.gguf");
    std::fs::write(&model, b"model").unwrap();
    let mut args = llama_args(&model);
    args.ctx = 0;
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("--ctx must be greater than zero")
    );

    args.ctx = 8192;
    args.port = 0;
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("--port must be greater than zero")
    );

    args.port = unused_port();
    args.parallel = Some(0);
    assert!(
        validate_launch_preconditions(dir.path(), &args, None)
            .unwrap_err()
            .contains("--parallel must be greater than zero")
    );
}

#[test]
fn http_probe_requires_engine_path_and_status_200() {
    let (ok_port, ok_server) = serve_one_status("/health", "200 OK");
    assert!(http_status_ok("127.0.0.1", ok_port, "/health"));
    ok_server.join().unwrap();

    let (failed_port, failed_server) = serve_one_status("/v1/models", "503 Loading");
    assert!(!http_status_ok("127.0.0.1", failed_port, "/v1/models"));
    failed_server.join().unwrap();
}

#[test]
fn readiness_fails_when_child_exits_before_http_health() {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd");
        command
            .args(["/C", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sh");
        command
            .args(["-c", "exit 7"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    let mut child = spawn_contained_test_child(&mut command, "exited readiness probe");

    let error = wait_healthy_with(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        unused_port(),
        Duration::from_secs(2),
        &mut NativeHealthProbe,
        &mut SystemLifecycleClock,
    )
    .expect_err("an exited child cannot become ready");
    assert!(error.contains("exited before readiness"));
    child.terminate_and_reap().unwrap();
}

#[test]
fn readiness_succeeds_only_while_child_is_alive_and_http_is_200() {
    let (port, server) = serve_one_status("/health", "200 OK");
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sleep");
        command
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    let mut child = spawn_contained_test_child(&mut command, "live readiness probe");

    wait_healthy_with(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        port,
        Duration::from_secs(2),
        &mut NativeHealthProbe,
        &mut SystemLifecycleClock,
    )
    .expect("a live child plus HTTP 200 is ready");
    assert!(matches!(child.try_wait_leader(), Ok(None)));
    child.terminate_and_reap().unwrap();
    server.join().unwrap();
}

#[test]
fn promised_origin_expansion_keeps_independent_capture_and_source_aware_blocker() {
    use crate::server_registration::{
        PromisedOriginRegistration, RegistrationBlock, RegistrationCoordinate,
    };

    let root = tempfile::tempdir().unwrap();
    let local_path = root
        .path()
        .join("workspace")
        .join(".ferric")
        .join("server.json");
    let global_path = root.path().join("config").join("server.json");
    let blocked_path = root
        .path()
        .join("blocked-workspace")
        .join(".ferric")
        .join("server.json");
    let executable = root.path().join(if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    });
    let runfile = |pid, token_coordinate| ServerRunfile {
        schema_version: RUNFILE_SCHEMA_V2,
        engine: Engine::LlamaServer,
        pid,
        port: 8080,
        base_url: "http://127.0.0.1:8080/v1".to_string(),
        tailscale: false,
        tailscale_serve: None,
        model: None,
        context_size: None,
        sampling_seed: None,
        parallel_slots: None,
        process_identity: Some(ProcessIdentity {
            start_token: canonical_test_start_token(token_coordinate),
            executable: executable.clone(),
            argv: vec![
                "llama-server".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ],
        }),
        origin_local_runfile: Some(local_path.clone()),
    };
    let direct_runfile = runfile(1, 1);
    let changed_origin_runfile = runfile(2, 2);
    let source = RegistrationCoordinate {
        scope: RegistrationScope::Global,
        path: global_path,
    };
    let inventory = RegistrationInventory {
        local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
            scope: RegistrationScope::Local,
            path: local_path.clone(),
            raw: b"direct-local-snapshot".to_vec(),
            runfile: direct_runfile.clone(),
        })),
        global: None,
        promised_origins: vec![
            PromisedOriginRegistration {
                source: source.clone(),
                expected_runfile: direct_runfile.clone(),
                slot: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                    scope: RegistrationScope::Origin,
                    path: local_path,
                    raw: b"changed-origin-snapshot".to_vec(),
                    runfile: changed_origin_runfile.clone(),
                })),
            },
            PromisedOriginRegistration {
                source: source.clone(),
                expected_runfile: direct_runfile,
                slot: RegistrationSlot::Blocked {
                    scope: RegistrationScope::Origin,
                    path: blocked_path.clone(),
                    reason: RegistrationBlock::NonRegular,
                },
            },
        ],
    };

    let (captures, observations) = expand_registration_captures(inventory);
    assert_eq!(captures.len(), 2);
    assert!(captures.iter().any(|capture| {
        capture.scope == RegistrationScope::Local && capture.raw == b"direct-local-snapshot"
    }));
    assert!(captures.iter().any(|capture| {
        capture.scope == RegistrationScope::Origin
            && capture.raw == b"changed-origin-snapshot"
            && capture.runfile == changed_origin_runfile
    }));
    assert_eq!(observations.len(), 1);
    assert_eq!(
        observations[0].candidate.coordinate,
        RegistrationCoordinate {
            scope: RegistrationScope::Origin,
            path: blocked_path.clone(),
        }
    );
    assert!(matches!(
        &observations[0].candidate.state,
        CandidateState::Unverifiable { reason, .. }
            if reason.contains("promised by global registration")
                && reason.contains(&source.path.display().to_string())
    ));
}

#[test]
fn read_runfile_absent_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = discover_managed_server_in(&ManagedDiscoveryScope {
        workspace: dir.path().to_path_buf(),
        global: None,
    });
    assert_eq!(discovery.state, ManagedServerState::Empty);
}

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod native {
    use super::*;

    const LIFECYCLE_HELPER_ENV: &str = "FERRIC_TEST_LIFECYCLE_HELPER";

    const LIFECYCLE_HELPER_WILDCARD_ENV: &str = "FERRIC_TEST_LIFECYCLE_WILDCARD";

    static LIFECYCLE_PARENT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const LIFECYCLE_HELPER_READY_TIMEOUT: Duration = Duration::from_secs(30);

    const LIFECYCLE_HELPER_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

    struct LifecycleHelperGuard {
        child: ferric_process::ProcessTree,
    }

    impl LifecycleHelperGuard {
        fn new(child: ferric_process::ProcessTree) -> Self {
            Self { child }
        }

        fn child(&self) -> &Child {
            self.child.child()
        }

        fn child_mut(&mut self) -> &mut ferric_process::ProcessTree {
            &mut self.child
        }

        fn id(&self) -> u32 {
            self.child().id()
        }

        fn wait_for_exit_and_disarm(&mut self, context: &str) -> Result<(), String> {
            self.child
                .wait_for_exit(LIFECYCLE_HELPER_EXIT_TIMEOUT)
                .map(|_| ())
                .map_err(|error| format!("{context}: {error}"))
        }

        fn reap_and_disarm(&mut self, context: &str) {
            self.wait_for_exit_and_disarm(context)
                .unwrap_or_else(|error| panic!("{error}"));
        }

        fn terminate_reap_and_disarm(&mut self, context: &str) {
            self.child
                .terminate_and_reap()
                .unwrap_or_else(|error| panic!("{context}: {error}"));
        }
    }

    /// Child-scoped test process used by the cross-workspace lifecycle
    /// regression below. A normal test-harness invocation returns immediately;
    /// only the explicitly spawned child enters the serving loop.
    #[test]
    fn lifecycle_helper_process() {
        if std::env::var(LIFECYCLE_HELPER_ENV).ok().as_deref() != Some("1") {
            return;
        }
        ferric_process::testing::contain_current_process()
            .expect("arm lifecycle-helper parent-death containment");
        let port = std::env::var("FERRIC_TEST_LIFECYCLE_PORT")
            .expect("helper port")
            .parse::<u16>()
            .expect("numeric helper port");
        let bind_host = if std::env::var(LIFECYCLE_HELPER_WILDCARD_ENV).ok().as_deref() == Some("1")
        {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        };
        let listener = TcpListener::bind((bind_host, port)).expect("bind helper listener");
        loop {
            let (mut stream, _) = listener.accept().expect("accept helper request");
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .expect("write helper response");
        }
    }

    fn spawn_lifecycle_helper(port: u16) -> LifecycleHelperGuard {
        spawn_lifecycle_helper_with_binding(port, false)
    }

    fn spawn_lifecycle_helper_with_binding(port: u16, wildcard: bool) -> LifecycleHelperGuard {
        let mut command = ferric_process::testing::harness_command(
            "server::tests::native::lifecycle_helper_process",
        );
        command
            .env(LIFECYCLE_HELPER_ENV, "1")
            .env(
                LIFECYCLE_HELPER_WILDCARD_ENV,
                if wildcard { "1" } else { "0" },
            )
            .env("FERRIC_TEST_LIFECYCLE_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child =
            ferric_process::testing::spawn(&mut command).expect("spawn contained lifecycle helper");
        LifecycleHelperGuard::new(child)
    }

    fn lifecycle_parent_test_guard() -> std::sync::MutexGuard<'static, ()> {
        ferric_process::testing::contain_current_process()
            .expect("install lifecycle-test process containment");
        LIFECYCLE_PARENT_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait_for_lifecycle_helper(child: &mut ferric_process::ProcessTree, port: u16) {
        if let Err(error) = wait_healthy_with(
            child,
            Engine::LlamaServer,
            "127.0.0.1",
            port,
            LIFECYCLE_HELPER_READY_TIMEOUT,
            &mut NativeHealthProbe,
            &mut SystemLifecycleClock,
        ) {
            let cleanup = stop_child(child);
            panic!(
                "lifecycle helper did not become HTTP-ready: {error}; cleanup result: {cleanup:?}"
            );
        }
    }

    fn native_listener_matches_or_has_documented_visibility_limit(
        actual: &ListenerState,
        expected: ListenerState,
    ) -> bool {
        if actual == &expected {
            return true;
        }
        #[cfg(target_os = "linux")]
        if let ListenerState::Uninspectable(error) = actual {
            assert!(
                is_documented_linux_listener_visibility_error(error),
                "unexpected Linux listener-inspection failure: {error}"
            );
            return false;
        }
        panic!("expected native listener state {expected:?}, got {actual:?}");
    }

    #[cfg(target_os = "linux")]
    fn is_documented_linux_listener_visibility_error(error: &str) -> bool {
        error.contains("listener owner enumeration is incomplete")
            || error.contains("whose owners are not inspectable")
    }

    #[test]
    fn cross_workspace_stale_local_selects_and_stops_only_verified_global() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace_a = root.path().join("workspace-a");
        let workspace_b = root.path().join("workspace-b");
        std::fs::create_dir_all(&workspace_a).unwrap();
        std::fs::create_dir_all(&workspace_b).unwrap();
        let local_a = std::path::absolute(runfile_path(&workspace_a)).unwrap();
        let local_b = std::path::absolute(runfile_path(&workspace_b)).unwrap();
        let global = root.path().join("config").join("server.json");
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(helper.child_mut(), port);

        let helper_pid = helper.id();
        let helper_facts = LiveProcess::acquire_child(helper.child())
            .unwrap()
            .inspect(port)
            .unwrap();
        if !native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTarget,
        ) {
            helper.terminate_reap_and_disarm("clean up visibility-limited lifecycle helper");
            return;
        }
        let live = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper_pid,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            tailscale_serve: None,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local_b.clone()),
        };
        write_test_runfile(&local_b, &live);
        write_test_runfile(&global, &live);

        // The stale local record names the test runner itself with a deliberately
        // wrong creation token. Old local-first/PID-only teardown would signal
        // this PID. Identity resolution must classify it stale and target only
        // the exact helper retained by the global/origin mirrors.
        let mut stale_identity = LiveProcess::acquire(std::process::id())
            .unwrap()
            .inspect(port)
            .unwrap()
            .identity;
        let alternative = canonical_test_start_token(1);
        stale_identity.start_token = if stale_identity.start_token == alternative {
            canonical_test_start_token(2)
        } else {
            alternative
        };
        let mut stale = live.clone();
        stale.pid = std::process::id();
        stale.process_identity = Some(stale_identity);
        stale.origin_local_runfile = Some(local_a.clone());
        write_test_runfile(&local_a, &stale);

        let discovery = discover_managed_server_in(&ManagedDiscoveryScope {
            workspace: workspace_a.clone(),
            global: Some(global.clone()),
        });
        let ManagedServerState::Ready(discovered) = discovery.state else {
            panic!("read-only consumers resolve the verified global server: {discovery:?}");
        };
        assert_eq!(discovered.runfile.pid, helper_pid);
        assert_eq!(
            status_impl(&workspace_a, Some(global.clone())),
            ExitCode::SUCCESS
        );
        assert_eq!(
            down_impl(&workspace_a, Some(global.clone())),
            ExitCode::SUCCESS
        );
        helper.reap_and_disarm("reap terminated helper");
        assert!(!local_a.exists(), "stale current-workspace alias cleaned");
        assert!(!local_b.exists(), "selected global origin alias cleaned");
        assert!(!global.exists(), "selected global alias cleaned");
    }

    #[test]
    fn wildcard_listener_blocks_teardown_and_preserves_registration() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper_with_binding(port, true);
        wait_for_lifecycle_helper(helper.child_mut(), port);

        let helper_pid = helper.id();
        let helper_facts = LiveProcess::acquire_child(helper.child())
            .unwrap()
            .inspect(port)
            .unwrap();
        native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTargetWildcard,
        );
        let record = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper_pid,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            tailscale_serve: None,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &record);
        let original_registration = std::fs::read(&local).unwrap();

        assert_eq!(status_impl(&workspace, None), ExitCode::FAILURE);
        let result = down_impl(&workspace, None);
        let registration_after_down = std::fs::read(&local).unwrap();
        let helper_remained_live = helper.child_mut().try_wait().unwrap().is_none();
        helper.terminate_reap_and_disarm("clean up wildcard lifecycle helper");

        assert_eq!(result, ExitCode::FAILURE);
        assert_eq!(
            registration_after_down, original_registration,
            "wildcard teardown must have an empty registration-delete ledger"
        );
        assert!(
            helper_remained_live,
            "wildcard teardown must have an empty process-signal ledger"
        );
    }

    #[test]
    fn stale_registration_keeps_live_foreign_listener_and_recovery_record() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(helper.child_mut(), port);

        let helper_facts = LiveProcess::acquire_child(helper.child())
            .unwrap()
            .inspect(port)
            .unwrap();
        native_listener_matches_or_has_documented_visibility_limit(
            &helper_facts.listener,
            ListenerState::OwnedByTarget,
        );
        let stale = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: u32::MAX,
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            tailscale_serve: None,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(helper_facts.identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &stale);

        let result = down_impl(&workspace, None);
        let registration_remained = local.exists();
        let helper_remained_live = helper.child_mut().try_wait().unwrap().is_none();
        helper.terminate_reap_and_disarm("clean up foreign-listener lifecycle helper");

        assert_eq!(result, ExitCode::FAILURE);
        assert!(
            registration_remained,
            "an active endpoint must keep its recovery registration"
        );
        assert!(
            helper_remained_live,
            "a listener owned by a foreign PID must never be signalled"
        );
    }

    #[test]
    fn live_retired_registrations_are_never_signalled_or_deleted() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(helper.child_mut(), port);
        let identity = LiveProcess::acquire_child(helper.child())
            .and_then(|process| process.inspect(port))
            .map(|facts| facts.identity);

        // The Tailscale record carries the helper's exact identity, so only
        // the retirement refusal stands between `down` and a signal.
        let mut outcomes = Vec::new();
        for case in ["schema-1", "tailscale-owned"] {
            let workspace = root.path().join(case);
            let local = std::path::absolute(runfile_path(&workspace)).unwrap();
            let mut record = ServerRunfile {
                schema_version: 1,
                engine: Engine::LlamaServer,
                pid: helper.id(),
                port,
                base_url: format!("http://127.0.0.1:{port}/v1"),
                tailscale: false,
                tailscale_serve: None,
                model: Some("example.gguf".to_string()),
                context_size: Some(8192),
                sampling_seed: Some(42),
                parallel_slots: Some(1),
                process_identity: None,
                origin_local_runfile: None,
            };
            if case == "tailscale-owned" {
                record.schema_version = RUNFILE_SCHEMA_V2;
                record.tailscale = true;
                record.tailscale_serve = Some(retired_tailscale_serve());
                record.process_identity = identity.as_ref().ok().cloned();
                record.origin_local_runfile = Some(local.clone());
            }
            write_test_runfile(&local, &record);
            let original = std::fs::read(&local).unwrap();
            let status = status_impl(&workspace, None);
            let down = down_impl(&workspace, None);
            outcomes.push((
                case,
                status,
                down,
                std::fs::read(&local).ok() == Some(original),
                helper.child_mut().try_wait().unwrap().is_none(),
            ));
        }
        helper.terminate_reap_and_disarm("clean up retired-registration lifecycle helper");

        assert!(identity.is_ok(), "inspect the live helper: {identity:?}");
        for (case, status, down, retained, live) in outcomes {
            assert_eq!(status, ExitCode::FAILURE, "{case}");
            assert_eq!(down, ExitCode::FAILURE, "{case}");
            assert!(
                retained,
                "{case}: a retired record must be kept byte-identical"
            );
            assert!(
                live,
                "{case}: a retired record must never authorize a signal"
            );
        }
    }

    #[test]
    fn malformed_v2_token_blocks_down_without_signal_or_delete() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(helper.child_mut(), port);

        let mut identity = LiveProcess::acquire_child(helper.child())
            .unwrap()
            .inspect(port)
            .unwrap()
            .identity;
        identity.start_token = "opaque".to_string();
        let record = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper.id(),
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            tailscale_serve: None,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &record);
        let original_registration = std::fs::read(&local).unwrap();

        let result = down_impl(&workspace, None);
        let registration_after_down = std::fs::read(&local).unwrap();
        let helper_remained_live = helper.child_mut().try_wait().unwrap().is_none();
        helper.terminate_reap_and_disarm("clean up malformed-token lifecycle helper");

        assert_eq!(result, ExitCode::FAILURE);
        assert_eq!(registration_after_down, original_registration);
        assert!(
            helper_remained_live,
            "a malformed creation token must block before process acquisition or signalling"
        );
    }

    #[test]
    fn same_creation_with_different_process_metadata_blocks_cleanup_and_signal() {
        let _lifecycle_serial = lifecycle_parent_test_guard();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let local = std::path::absolute(runfile_path(&workspace)).unwrap();
        let port = unused_port();
        let mut helper = spawn_lifecycle_helper(port);
        wait_for_lifecycle_helper(helper.child_mut(), port);

        let mut identity = LiveProcess::acquire_child(helper.child())
            .unwrap()
            .inspect(port)
            .unwrap()
            .identity;
        identity.argv.push("--not-the-observed-command".to_string());
        let record = ServerRunfile {
            schema_version: RUNFILE_SCHEMA_V2,
            engine: Engine::LlamaServer,
            pid: helper.id(),
            port,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            tailscale: false,
            tailscale_serve: None,
            model: Some("example.gguf".to_string()),
            context_size: Some(8192),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: Some(identity),
            origin_local_runfile: Some(local.clone()),
        };
        write_test_runfile(&local, &record);

        let result = down_impl(&workspace, None);
        let registration_remained = local.exists();
        let helper_remained_live = helper.child_mut().try_wait().unwrap().is_none();
        helper.terminate_reap_and_disarm("clean up metadata-mismatch lifecycle helper");

        assert_eq!(result, ExitCode::FAILURE);
        assert!(
            registration_remained,
            "a live same-creation metadata mismatch must retain its recovery coordinate"
        );
        assert!(
            helper_remained_live,
            "a live same-creation metadata mismatch must never be signalled"
        );
    }
}
