use super::*;
use crate::server_process::canonical_test_start_token;
use crate::server_registration::{
    PersistenceEffects, PersistencePhase, PromisedOriginRegistration, RegistrationBlock,
    StagePersistError, capture_registration_path, publish_mirrored_with,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::net::TcpListener;
use std::rc::Rc;
use std::thread;
use tempfile::NamedTempFile;

fn spawn_contained_test_child(
    command: &mut Command,
    label: &str,
) -> crate::test_process_containment::ContainedChild {
    crate::test_process_containment::ContainedChild::spawn(command)
        .unwrap_or_else(|error| panic!("spawn {label}: {error}"))
}

fn cfg(engine: Engine) -> ServerConfig {
    ServerConfig {
        engine,
        model: Some("model.gguf".to_string()),
        mmproj: None,
        ctx: 4096,
        host: "127.0.0.1".to_string(),
        port: 8080,
        threads: None,
        gpu_layers: None,
        batch_size: None,
        seed: None,
        parallel: None,
        tailscale: false,
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

fn discovery_fixture_executable() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\fixture\llama-server.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/fixture/llama-server")
    }
}

fn discovery_fixture_identity(seed: u64) -> ProcessIdentity {
    ProcessIdentity {
        start_token: canonical_test_start_token(seed),
        executable: discovery_fixture_executable(),
        argv: vec!["llama-server".to_string(), "--serve".to_string()],
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
        process_identity: Some(discovery_fixture_identity(u64::from(pid))),
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

fn discovery_fixture_tailscale_capture(
    scope: RegistrationScope,
    pid: u32,
    name: &str,
) -> (CapturedRegistration, TailscaleServeOwnership) {
    let mut capture = discovery_fixture_capture(scope, pid, name);
    let mut ownership = tailscale_ownership(capture.runfile.port);
    ownership.apply_confirmed = true;
    capture.runfile.tailscale = true;
    capture.runfile.tailscale_serve = Some(ownership.clone());
    capture.raw = serde_json::to_vec_pretty(&capture.runfile).unwrap();
    (capture, ownership)
}

fn legacy_adoption_fixture(pid: u32) -> (Vec<CapturedRegistration>, ProcessFacts) {
    let port = u16::try_from(7600 + pid % 100).unwrap();
    let runfile = ServerRunfile {
        schema_version: 1,
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
        process_identity: None,
        origin_local_runfile: None,
    };
    let identity = ProcessIdentity {
        start_token: canonical_test_start_token(u64::from(pid)),
        executable: discovery_fixture_executable(),
        argv: vec![
            "llama-server".to_string(),
            "-m".to_string(),
            "model.gguf".to_string(),
            "-c".to_string(),
            "8192".to_string(),
            "--seed".to_string(),
            "42".to_string(),
            "--parallel".to_string(),
            "1".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            port.to_string(),
        ],
    };
    let raw = serde_json::to_vec_pretty(&runfile).unwrap();
    let captures = [
        (RegistrationScope::Local, "legacy-local"),
        (RegistrationScope::Global, "legacy-global"),
    ]
    .into_iter()
    .map(|(scope, name)| CapturedRegistration {
        scope,
        path: discovery_fixture_path(name),
        raw: raw.clone(),
        runfile: runfile.clone(),
    })
    .collect();
    (
        captures,
        ProcessFacts {
            identity,
            listener: ListenerState::OwnedByTarget,
        },
    )
}

fn adoption_fixture_replacement_raw(
    captures: &[CapturedRegistration],
    facts: &ProcessFacts,
) -> Vec<u8> {
    let mut runfile = captures[0].runfile.clone();
    runfile.schema_version = RUNFILE_SCHEMA_V2;
    runfile.process_identity = Some(facts.identity.clone());
    runfile.origin_local_runfile = Some(captures[0].path.clone());
    serde_json::to_vec_pretty(&runfile).unwrap()
}

fn scripted_remove_event(capture: &CapturedRegistration) -> LifecycleEvent {
    LifecycleEvent::Remove(
        capture.path.clone(),
        ferric_bench::sha256_bytes(&capture.raw),
    )
}

fn scripted_replace_event(path: &Path, captured: &[u8], replacement: &[u8]) -> LifecycleEvent {
    LifecycleEvent::Replace(
        path.to_path_buf(),
        ferric_bench::sha256_bytes(captured),
        ferric_bench::sha256_bytes(replacement),
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
        id: ObservationId(0),
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
                id: ObservationId(0),
                coordinate: local,
                promised: None,
                state: ManagedRegistrationState::Absent,
            },
            ManagedRegistrationObservation {
                id: ObservationId(1),
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
        id: ObservationId(0),
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

fn discovery_fixture_ready_tailscale() -> (ManagedServerDiscovery, TailscaleServeOwnership) {
    let mut discovery = discovery_fixture_ready();
    let port = match &discovery.state {
        ManagedServerState::Ready(server) => server.runfile.port,
        _ => unreachable!("ready fixture changed state"),
    };
    let mut ownership = tailscale_ownership(port);
    ownership.apply_confirmed = true;

    let RegistrationSlot::Captured(capture) = &mut discovery.inventory.local else {
        unreachable!("ready fixture lost its local capture")
    };
    capture.runfile.tailscale = true;
    capture.runfile.tailscale_serve = Some(ownership.clone());
    capture.raw = serde_json::to_vec(&capture.runfile).unwrap();
    let raw_sha256 = ferric_bench::sha256_bytes(&capture.raw);

    let ManagedRegistrationState::Captured {
        runfile,
        raw_sha256: observation_sha256,
        ..
    } = &mut discovery.observations[0].state
    else {
        unreachable!("ready fixture lost its managed observation")
    };
    **runfile = capture.runfile.clone();
    *observation_sha256 = raw_sha256.clone();

    let ManagedServerState::Ready(server) = &mut discovery.state else {
        unreachable!("ready fixture changed state")
    };
    server.runfile = capture.runfile.clone();
    server.fingerprint.runfile = capture.runfile.clone();
    server.fingerprint.revisions[0].state = RegistrationRevisionState::Captured(raw_sha256);

    (discovery, ownership)
}

fn discovery_fixture_stale_tailscale() -> (ManagedServerDiscovery, TailscaleServeOwnership) {
    let (mut discovery, ownership) = discovery_fixture_ready_tailscale();
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
    (discovery, ownership)
}

fn discovery_fixture_wildcard_tailscale() -> (ManagedServerDiscovery, TailscaleServeOwnership) {
    let (mut discovery, ownership) = discovery_fixture_ready_tailscale();
    let ManagedServerState::Ready(mut server) = discovery.state.clone() else {
        unreachable!("typed Tailscale fixture must start ready")
    };
    server.listener = ListenerState::OwnedByTargetWildcard;
    if let ManagedRegistrationState::Captured { runtime, .. } = &mut discovery.observations[0].state
    {
        *runtime = RuntimeObservation::Verified {
            identity: server.identity.clone(),
            listener: ListenerState::OwnedByTargetWildcard,
            health: HealthState::Healthy,
        };
    }
    let coordinate = server.registration.clone();
    discovery.state = ManagedServerState::Degraded {
        server,
        issues: vec![ResolutionIssue {
            coordinates: vec![coordinate],
            kind: ResolutionIssueKind::Degraded,
            detail: "fixture wildcard listener".to_string(),
        }],
    };
    (discovery, ownership)
}

fn add_distinct_stale_tailscale_peer(
    discovery: &mut ManagedServerDiscovery,
    pid: u32,
    name: &str,
) -> TailscaleServeOwnership {
    let mut capture = discovery_fixture_capture(RegistrationScope::Global, pid, name);
    let identity = test_tailscale_identity();
    let coordinate = coordinate_from_token(
        capture.runfile.port,
        &identity,
        "ffeeddccbbaa99887766554433221100".to_string(),
    )
    .unwrap();
    let mut observation = crate::tailscale_serve::project_localapi_status(
        b"{}",
        &coordinate.fqdn,
        &coordinate.mount_path,
    )
    .unwrap();
    observation.identity = Some(identity);
    let ownership = coordinate.into_ownership(&observation).unwrap();
    capture.runfile.tailscale = true;
    capture.runfile.tailscale_serve = Some(ownership.clone());
    capture.raw = serde_json::to_vec_pretty(&capture.runfile).unwrap();
    let coordinate = RegistrationCoordinate {
        scope: capture.scope,
        path: capture.path.clone(),
    };
    discovery.inventory.global = Some(RegistrationSlot::Captured(Box::new(capture.clone())));
    discovery.observations.push(ManagedRegistrationObservation {
        id: ObservationId(discovery.observations.len()),
        coordinate: coordinate.clone(),
        promised: None,
        state: ManagedRegistrationState::Captured {
            runfile: Box::new(capture.runfile),
            raw_sha256: ferric_bench::sha256_bytes(&capture.raw),
            runtime: RuntimeObservation::Stale {
                reason: "PID is absent".to_string(),
                observed_identity: None,
                listener: ListenerState::Absent,
            },
        },
    });
    match &mut discovery.state {
        ManagedServerState::Ready(server) | ManagedServerState::Degraded { server, .. } => {
            server.stale.push(coordinate)
        }
        ManagedServerState::StaleOnly { stale } => stale.push(coordinate),
        ManagedServerState::Empty
        | ManagedServerState::Conflict { .. }
        | ManagedServerState::Unverifiable { .. } => {
            unreachable!("fixture cannot add a stale peer to blocked discovery")
        }
    }
    ownership
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

struct PanicHealth;

impl HealthProbe for PanicHealth {
    fn status_ok(&mut self, _host: &str, _port: u16, _path: &str) -> bool {
        panic!("static registration blockers must precede HTTP health")
    }
}

#[test]
pub(crate) fn legacy_tailscale_registration_remains_unowned() {
    let root = tempfile::tempdir().unwrap();
    for scope in [
        RegistrationScope::Local,
        RegistrationScope::Global,
        RegistrationScope::Origin,
    ] {
        let path = root
            .path()
            .join(format!("scope-{scope}"))
            .join("server.json");
        let mut runfile = discovery_fixture_runfile(4199, "legacy-scope");
        runfile.tailscale = true;
        runfile.tailscale_serve = None;
        let raw = serde_json::to_vec_pretty(&runfile).unwrap();
        let observation = observe_registration(CapturedRegistration {
            scope,
            path: path.clone(),
            raw: raw.clone(),
            runfile,
        });
        assert!(observation.process.is_none(), "{scope}");
        assert!(matches!(
            observation.candidate.state,
            CandidateState::Unverifiable { .. }
        ));
        assert_eq!(observation.capture.as_ref().unwrap().raw, raw, "{scope}");
        assert_eq!(observation.capture.as_ref().unwrap().path, path, "{scope}");
    }
    for (process_case, pid, simulated_present) in [("present", 4201, true), ("absent", 4202, false)]
    {
        let registration_path = root
            .path()
            .join(process_case)
            .join("workspace/.ferric/server.json");
        std::fs::create_dir_all(registration_path.parent().unwrap()).unwrap();
        let mut runfile = discovery_fixture_runfile(pid, process_case);
        runfile.tailscale = true;
        runfile.origin_local_runfile = Some(registration_path.clone());
        let mut raw = if simulated_present {
            serde_json::to_vec_pretty(&runfile).unwrap()
        } else {
            serde_json::to_vec(&runfile).unwrap()
        };
        raw.push(b'\n');
        std::fs::write(&registration_path, &raw).unwrap();
        let inventory = RegistrationInventory {
            local: capture_registration_path(RegistrationScope::Local, &registration_path),
            global: None,
            promised_origins: Vec::new(),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let calls = Rc::clone(&ledger);
        let discovery = discover_inventory_with(
            inventory,
            move |capture| {
                calls
                    .borrow_mut()
                    .push(LifecycleEvent::Acquire(capture.runfile.pid));
                if simulated_present {
                    blocked_observation_with_facts(
                        capture,
                        "simulated present process".to_string(),
                        Some(discovery_fixture_identity(u64::from(pid))),
                        Some(ListenerState::OwnedByTarget),
                        HealthState::NotProbed,
                    )
                } else {
                    stale_observation(
                        capture,
                        "simulated absent process".to_string(),
                        None,
                        ListenerState::Absent,
                    )
                }
            },
            &mut PanicHealth,
            |_observation| panic!("Tailscale blocker must precede retained reinspection"),
        );
        assert!(ledger.borrow().is_empty(), "{process_case}");
        assert!(matches!(
            discovery.managed.state,
            ManagedServerState::Unverifiable { .. }
        ));
        assert!(matches!(
            discovery.managed.observations[0].state,
            ManagedRegistrationState::Captured {
                runtime: RuntimeObservation::NotInspected,
                ..
            }
        ));
        let status = render_status(&status_report(&discovery.managed));
        assert!(!status.success, "{process_case}");
        assert!(
            status
                .stdout
                .iter()
                .any(|line| line.contains("tailscale=legacy-unowned")),
            "{process_case}: {:?}",
            status.stdout
        );
        assert!(status.stdout.iter().any(|line| {
            line.starts_with("[next]")
                && line.contains("remove only that exact Serve endpoint")
                && line.contains("will not inspect or signal")
                && line.contains("blind node-wide reset")
        }));
        let plan = down_plan_from_lifecycle(discovery);
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(plan, &mut effects);
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::Blocked);
        assert!(!report.signalled);
        assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
        assert_eq!(std::fs::read(&registration_path).unwrap(), raw);
    }
}

#[test]
pub(crate) fn promised_origin_static_matrix_precedes_process_inspection() {
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
        let result = discover_inventory_with(
            inventory,
            |_capture| panic!("origin blocker must precede process acquisition"),
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
        tailscale: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorEvent {
    Binary,
    File,
    TailscaleIdentity,
    TailscaleStatus,
}

struct RecordingDoctorEffects {
    events: Vec<DoctorEvent>,
    binary_present: bool,
    regular_file: bool,
    tailscale_identity: Result<String, String>,
    tailscale_status: Result<(), String>,
}

impl Default for RecordingDoctorEffects {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            binary_present: true,
            regular_file: true,
            tailscale_identity: Ok("example-host.tailnet-example.ts.net".to_string()),
            tailscale_status: Ok(()),
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

    fn tailscale_identity(&mut self) -> Result<String, String> {
        self.events.push(DoctorEvent::TailscaleIdentity);
        self.tailscale_identity.clone()
    }

    fn tailscale_status(&mut self, _fqdn: &str) -> Result<(), String> {
        self.events.push(DoctorEvent::TailscaleStatus);
        self.tailscale_status.clone()
    }
}

fn expected_status_listener(listener: &ListenerState) -> String {
    match listener {
        ListenerState::OwnedByTarget => "owned-loopback".to_string(),
        ListenerState::OwnedByTargetWildcard => "wildcard-public".to_string(),
        ListenerState::Absent => "absent".to_string(),
        ListenerState::OwnedByOther(owners) => format!("foreign-or-shared:{owners:?}"),
        ListenerState::Uninspectable(detail) => format!("uninspectable:{detail}"),
    }
}

fn expected_status_health(health: HealthState) -> &'static str {
    match health {
        HealthState::NotProbed => "not-probed",
        HealthState::Healthy => "healthy",
        HealthState::Unhealthy => "unhealthy",
    }
}

fn expected_registration_status(observation: &ManagedRegistrationObservation) -> String {
    let promised = observation
        .promised
        .as_ref()
        .map_or_else(String::new, |promised| {
            format!(
                " promised-by={} registration {}",
                promised.source.scope,
                promised.source.path.display()
            )
        });
    let prefix = format!(
        "{} registration {}{promised}",
        observation.coordinate.scope,
        observation.coordinate.path.display()
    );
    match &observation.state {
        ManagedRegistrationState::Absent => format!(
            "[absent] {prefix}: recorded-identity=none observed-identity=none listener=none health=none"
        ),
        ManagedRegistrationState::Blocked { reason } => format!(
            "[blocked] {prefix}: {reason}; recorded-identity=unavailable observed-identity=not-inspected listener=not-inspected health=not-probed"
        ),
        ManagedRegistrationState::Captured {
            runfile, runtime, ..
        } => {
            let tailscale = runfile.tailscale_serve.as_ref().map_or_else(
                || {
                    if runfile.tailscale {
                        " tailscale=legacy-unowned".to_string()
                    } else {
                        String::new()
                    }
                },
                |ownership| {
                    format!(
                        " tailscale=owned mount={} remote-base={}",
                        ownership.mount_path, ownership.remote_base_url
                    )
                },
            );
            let recorded = runfile.process_identity.as_ref().map_or_else(
                || "legacy-none".to_string(),
                |identity| {
                    format!(
                        "token={} executable={} argv={:?}",
                        identity.start_token,
                        identity.executable.display(),
                        identity.argv
                    )
                },
            );
            let observed = match runtime {
                RuntimeObservation::NotInspected => {
                    "observed-identity=not-inspected listener=not-inspected health=not-probed"
                        .to_string()
                }
                RuntimeObservation::Verified {
                    identity,
                    listener,
                    health,
                } => format!(
                    "observed-identity=token={} executable={} argv={:?} listener={} health={}",
                    identity.start_token,
                    identity.executable.display(),
                    identity.argv,
                    expected_status_listener(listener),
                    expected_status_health(*health)
                ),
                RuntimeObservation::Stale {
                    reason,
                    observed_identity,
                    listener,
                } => {
                    let identity = observed_identity.as_ref().map_or_else(
                        || "stale-unavailable".to_string(),
                        |identity| {
                            format!(
                                "token={} executable={} argv={:?}",
                                identity.start_token,
                                identity.executable.display(),
                                identity.argv
                            )
                        },
                    );
                    format!(
                        "observed-identity={identity} stale-reason={reason} listener={} health=not-probed",
                        expected_status_listener(listener)
                    )
                }
                RuntimeObservation::LegacyLive { pid } => format!(
                    "observed-identity=legacy-live-pid:{pid} listener=unverified health=not-probed"
                ),
                RuntimeObservation::Unverifiable {
                    reason,
                    observed_identity,
                    listener,
                    health,
                } => {
                    let identity = observed_identity.as_ref().map_or_else(
                        || "unavailable".to_string(),
                        |identity| {
                            format!(
                                "token={} executable={} argv={:?}",
                                identity.start_token,
                                identity.executable.display(),
                                identity.argv
                            )
                        },
                    );
                    let listener = listener
                        .as_ref()
                        .map_or_else(|| "unverifiable".to_string(), expected_status_listener);
                    format!(
                        "observed-identity={identity} unverifiable-reason={reason} listener={listener} health={}",
                        expected_status_health(*health)
                    )
                }
            };
            format!(
                "[captured] {prefix}: schema={} engine={:?} pid={} base-url={} recorded-identity={recorded}{tailscale} {observed}",
                runfile.schema_version, runfile.engine, runfile.pid, runfile.base_url,
            )
        }
    }
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
        .map(expected_registration_status)
        .collect::<Vec<_>>();
    expected_stdout.push(expected_state.to_string());
    if let Some(reason) = &report.tailscale_issue {
        expected_stdout.push(format!("[tailscale] ownership-blocked reason={reason}"));
    }
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
            observed_identity: Some(discovery_fixture_identity(4102)),
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
            id: ObservationId(2),
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

    let mut legacy = discovery_fixture_ready();
    let legacy_pid = 4101;
    if let ManagedRegistrationState::Captured {
        runfile, runtime, ..
    } = &mut legacy.observations[0].state
    {
        runfile.schema_version = 1;
        runfile.process_identity = None;
        *runtime = RuntimeObservation::LegacyLive { pid: legacy_pid };
    }
    let legacy_runfile = match &legacy.observations[0].state {
        ManagedRegistrationState::Captured { runfile, .. } => runfile.as_ref().clone(),
        _ => unreachable!(),
    };
    let legacy_raw = serde_json::to_vec(&legacy_runfile).unwrap();
    if let RegistrationSlot::Captured(local) = &mut legacy.inventory.local {
        local.runfile = legacy_runfile.clone();
        local.raw = legacy_raw.clone();
    }
    let legacy_global = discovery_fixture_coordinate(RegistrationScope::Global, "legacy-global");
    legacy.inventory.global = Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
        scope: legacy_global.scope,
        path: legacy_global.path.clone(),
        raw: legacy_raw,
        runfile: legacy_runfile.clone(),
    })));
    let mut legacy_alias = discovery_fixture_observation(
        legacy_global.clone(),
        legacy_runfile,
        RuntimeObservation::LegacyLive { pid: legacy_pid },
    );
    legacy_alias.id = ObservationId(1);
    legacy.observations.push(legacy_alias);
    legacy.state = ManagedServerState::Unverifiable {
        issues: vec![ResolutionIssue {
            coordinates: vec![
                legacy.observations[0].coordinate.clone(),
                legacy_global.clone(),
            ],
            kind: ResolutionIssueKind::Unverifiable,
            detail: "live legacy registration".to_string(),
        }],
    };
    assert_eq!(
        status_report(&legacy).next_action,
        StatusNextAction::AdoptLegacy { pid: legacy_pid }
    );
    assert!(
        render_status(&status_report(&legacy))
            .stdout
            .last()
            .unwrap()
            .contains("`ferric server adopt --pid 4101`")
    );
    let mut incompatible_legacy = legacy.clone();
    if let ManagedRegistrationState::Captured { runfile, .. } =
        &mut incompatible_legacy.observations[1].state
    {
        runfile.context_size = Some(4096);
    }
    assert!(matches!(
        status_report(&incompatible_legacy).next_action,
        StatusNextAction::RepairUnverifiable { .. }
    ));

    let mut tailscale = discovery_fixture_ready();
    if let ManagedRegistrationState::Captured {
        runfile, runtime, ..
    } = &mut tailscale.observations[0].state
    {
        runfile.tailscale = true;
        *runtime = RuntimeObservation::NotInspected;
    }
    tailscale.state = ManagedServerState::Unverifiable {
        issues: vec![ResolutionIssue {
            coordinates: vec![tailscale.observations[0].coordinate.clone()],
            kind: ResolutionIssueKind::Unverifiable,
            detail: "durable Tailscale Serve state".to_string(),
        }],
    };
    assert!(matches!(
        status_report(&tailscale).next_action,
        StatusNextAction::InspectTailscale { .. }
    ));
    assert_eq!(
        status_report(&tailscale).tailscale_issue.as_deref(),
        Some("legacy Tailscale registration has no endpoint-scoped ownership metadata")
    );

    let local_coordinate = discovery_fixture_coordinate(RegistrationScope::Local, "local");
    let mut legacy_coordinates = vec![local_coordinate.clone(), legacy_global];
    legacy_coordinates.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.scope.to_string().cmp(&right.scope.to_string()))
    });

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
    assert_status_matrix_row(
        &incompatible_legacy,
        StatusNextAction::RepairUnverifiable {
            coordinates: legacy_coordinates,
        },
        "[state] unverifiable",
        "[next] repair or make readable the 2 unverifiable registration coordinate(s), then rerun `ferric server status` (no process action is authorized)",
        &["[diagnostic] live legacy registration".to_string()],
        false,
    );
    assert_status_matrix_row(
        &legacy,
        StatusNextAction::AdoptLegacy { pid: legacy_pid },
        "[state] unverifiable",
        "[next] verify and record the live legacy process without signalling it: `ferric server adopt --pid 4101`",
        &["[diagnostic] live legacy registration".to_string()],
        false,
    );
    assert_status_matrix_row(
        &tailscale,
        StatusNextAction::InspectTailscale { port: 7101 },
        "[state] unverifiable",
        "[next] registration port 7101 claims durable Tailscale Serve state; scoped proxy cleanup is unavailable, so Ferric will not inspect or signal its PID, delete its registration, invoke Tailscale, or run a blind node-wide reset; inspect and remove only that exact Serve endpoint with Tailscale tooling",
        &["[diagnostic] durable Tailscale Serve state".to_string()],
        false,
    );
}

#[test]
fn status_reports_each_proxy_state() {
    let cases = [
        (
            "active",
            ServePathState::Proxy {
                target: "http://127.0.0.1:7101".to_string(),
            },
        ),
        ("pending", ServePathState::Absent),
        (
            "replaced",
            ServePathState::Proxy {
                target: "http://127.0.0.1:7999".to_string(),
            },
        ),
    ];

    for (case, path_state) in cases {
        let (discovery, ownership) = discovery_fixture_ready_tailscale();
        let local_base_url = match &discovery.state {
            ManagedServerState::Ready(server) => server.runfile.base_url.clone(),
            state => panic!("Tailscale status fixture is not ready: {state:?}"),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new(
            [Ok(tailscale_observation(&ownership, path_state, 'b'))],
            Rc::clone(&ledger),
        );
        let report = status_report_with_tailscale(&discovery, &serve);
        let rendered = render_status(&report);
        assert!(matches!(report.state, ManagedServerState::Ready(_)));
        assert_eq!(
            report
                .tailscale
                .as_ref()
                .map(|tailscale| &tailscale.ownership),
            Some(&ownership)
        );
        assert!(rendered.stdout.iter().any(|line| {
            line.contains(&ownership.remote_base_url) && line.contains(&ownership.mount_path)
        }));
        match case {
            "active" => {
                assert!(report.success);
                assert_eq!(
                    report.next_action,
                    StatusNextAction::ContinueManaged {
                        base_url: local_base_url.clone(),
                    }
                );
                assert!(rendered.stdout.contains(&format!(
                        "[next] managed server is ready at {local_base_url}; continue with the intended Ferric command and omit `--api-base` to use it"
                    )));
                assert!(matches!(
                    report.tailscale.as_ref().map(|tailscale| &tailscale.status),
                    Some(TailscaleProxyStatus::Active)
                ));
            }
            "pending" => {
                assert!(!report.success);
                assert!(matches!(
                    report.tailscale.as_ref().map(|tailscale| &tailscale.status),
                    Some(TailscaleProxyStatus::Pending)
                ));
                assert!(matches!(
                    &report.next_action,
                    StatusNextAction::RecoverOwnedTailscale {
                        remote_base_url,
                        mount_path,
                        reason,
                        subject: TailscaleRecoverySubject::ManagedProcess,
                        apply_confirmed: true,
                    } if remote_base_url == &ownership.remote_base_url
                        && mount_path == &ownership.mount_path
                        && reason.contains("absent")
                ));
            }
            "replaced" => {
                assert!(!report.success);
                assert!(matches!(
                    report.tailscale.as_ref().map(|tailscale| &tailscale.status),
                    Some(TailscaleProxyStatus::Replaced { observed_target })
                        if observed_target == "http://127.0.0.1:7999"
                ));
                assert!(matches!(
                    &report.next_action,
                    StatusNextAction::RecoverOwnedTailscale {
                        remote_base_url,
                        mount_path,
                        reason,
                        subject: TailscaleRecoverySubject::ManagedProcess,
                        apply_confirmed: true,
                    } if remote_base_url == &ownership.remote_base_url
                        && mount_path == &ownership.mount_path
                        && reason.contains("7999")
                ));
            }
            _ => unreachable!(),
        }
        assert_eq!(rendered.success, case == "active");
        assert!(
            rendered
                .stdout
                .iter()
                .any(|line| line.starts_with(&format!("[tailscale] {case}")))
        );
        assert!(rendered.stdout.iter().any(|line| {
            line.starts_with(&format!("[tailscale] {case}"))
                && line.contains("apply-confirmed=true")
        }));
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::TailscaleObserve],
            "status must only observe the recorded coordinate"
        );
    }

    let (discovery, ownership) = discovery_fixture_ready_tailscale();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Err(
            crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                "injected read failure".to_string(),
            ),
        )],
        Rc::clone(&ledger),
    );
    let report = status_report_with_tailscale(&discovery, &serve);
    let rendered = render_status(&report);
    assert!(!report.success);
    assert!(matches!(
        report.tailscale.as_ref().map(|tailscale| &tailscale.status),
        Some(TailscaleProxyStatus::Uninspectable { .. })
    ));
    assert!(matches!(
        report.next_action,
        StatusNextAction::RecoverOwnedTailscale {
            remote_base_url,
            mount_path,
            ..
        } if remote_base_url == ownership.remote_base_url && mount_path == ownership.mount_path
    ));
    assert!(rendered.stdout.iter().any(|line| {
        line.starts_with("[tailscale] uninspectable")
            && line.contains(&ownership.remote_base_url)
            && line.contains(&ownership.mount_path)
    }));
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);

    let (mut discovery, ownership) = discovery_fixture_ready_tailscale();
    let ManagedServerState::Ready(mut server) = discovery.state.clone() else {
        unreachable!("typed status fixture must start ready")
    };
    server.health = HealthState::Unhealthy;
    discovery.state = ManagedServerState::Degraded {
        issues: vec![ResolutionIssue {
            coordinates: vec![server.registration.clone()],
            kind: ResolutionIssueKind::Degraded,
            detail: "injected native degradation".to_string(),
        }],
        server,
    };
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Proxy {
                target: ownership.proxy_target.clone(),
            },
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let report = status_report_with_tailscale(&discovery, &serve);
    let rendered = render_status(&report);
    assert!(
        !report.success,
        "active proxy cannot hide degraded native state"
    );
    assert!(matches!(
        report.tailscale.as_ref().map(|tailscale| &tailscale.status),
        Some(TailscaleProxyStatus::Active)
    ));
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.starts_with("[state] degraded"))
    );
    assert!(!rendered.success);
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);
}

#[test]
fn status_reports_absent_ancestor_route_as_uninspectable() {
    for ancestor in ["/", "/_ferric"] {
        let (discovery, ownership) = discovery_fixture_ready_tailscale();
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let mut observation = tailscale_observation(&ownership, ServePathState::Absent, 'b');
        observation.route_shadow = Some(ancestor.to_string());
        let serve = ScriptedTailscaleServe::new([Ok(observation)], Rc::clone(&ledger));

        let report = status_report_with_tailscale(&discovery, &serve);
        let rendered = render_status(&report);
        assert!(!report.success, "{ancestor}");
        assert!(matches!(
            report.tailscale.as_ref().map(|tailscale| &tailscale.status),
            Some(TailscaleProxyStatus::Uninspectable { reason })
                if reason.contains(&format!("Web handler {ancestor} overrides owned path"))
        ));
        assert!(matches!(
            &report.next_action,
            StatusNextAction::RecoverOwnedTailscale {
                remote_base_url,
                mount_path,
                reason,
                subject: TailscaleRecoverySubject::ManagedProcess,
                apply_confirmed: true,
            } if remote_base_url == &ownership.remote_base_url
                && mount_path == &ownership.mount_path
                && reason.contains(&format!("Web handler {ancestor} overrides owned path"))
        ));
        assert!(rendered.stdout.iter().any(|line| {
            line.starts_with("[tailscale] uninspectable")
                && line.contains(ancestor)
                && line.contains(&ownership.mount_path)
        }));
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);
    }
}

#[test]
fn status_future_version_exact_proxy_is_uninspectable() {
    let (discovery, ownership) = discovery_fixture_ready_tailscale();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut observation = tailscale_observation(
        &ownership,
        ServePathState::Proxy {
            target: ownership.proxy_target.clone(),
        },
        'b',
    );
    observation.cleanup_semantics_pinned = false;
    let serve = ScriptedTailscaleServe::new([Ok(observation)], Rc::clone(&ledger));

    let report = status_report_with_tailscale(&discovery, &serve);
    let rendered = render_status(&report);
    assert!(!report.success);
    assert!(matches!(
        report.tailscale.as_ref().map(|tailscale| &tailscale.status),
        Some(TailscaleProxyStatus::Uninspectable { reason })
            if reason.contains("newer routing semantics")
                && reason.contains("cannot prove the owned endpoint is active")
    ));
    assert!(matches!(
        &report.next_action,
        StatusNextAction::RecoverOwnedTailscale {
            remote_base_url,
            mount_path,
            reason,
            subject: TailscaleRecoverySubject::ManagedProcess,
            apply_confirmed: true,
        } if remote_base_url == &ownership.remote_base_url
            && mount_path == &ownership.mount_path
            && reason.contains("newer routing semantics")
    ));
    assert!(rendered.stdout.iter().any(|line| {
        line.starts_with("[tailscale] uninspectable") && line.contains("newer routing semantics")
    }));
    assert!(
        !rendered
            .stdout
            .iter()
            .any(|line| line.starts_with("[tailscale] active"))
    );
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);
}

#[test]
fn status_recovery_guidance_respects_native_authority() {
    for case in ["pending", "replaced", "uninspectable"] {
        let (discovery, ownership) = discovery_fixture_stale_tailscale();
        let observation = match case {
            "pending" => Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            "replaced" => Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: "http://127.0.0.1:7999".to_string(),
                },
                'b',
            )),
            "uninspectable" => Err(
                crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                    "injected read failure".to_string(),
                ),
            ),
            _ => unreachable!(),
        };
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new([observation], Rc::clone(&ledger));
        let report = status_report_with_tailscale(&discovery, &serve);
        let rendered = render_status(&report);
        assert!(!report.success, "{case}");
        assert!(matches!(
            report.next_action,
            StatusNextAction::RecoverOwnedTailscale {
                subject: TailscaleRecoverySubject::StaleRegistration,
                ..
            }
        ));
        let next = rendered.stdout.last().expect("stale next action");
        assert!(
            next.contains("no managed process is present"),
            "{case}: {next}"
        );
        assert!(
            next.contains("no process will be signalled"),
            "{case}: {next}"
        );
        assert!(!next.contains("stop the independently owned process"));
        assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);
    }

    let (mut discovery, mut ownership) = discovery_fixture_stale_tailscale();
    ownership.apply_confirmed = false;
    if let RegistrationSlot::Captured(capture) = &mut discovery.inventory.local {
        capture
            .runfile
            .tailscale_serve
            .as_mut()
            .unwrap()
            .apply_confirmed = false;
        capture.raw = serde_json::to_vec_pretty(&capture.runfile).unwrap();
    }
    if let ManagedRegistrationState::Captured {
        runfile,
        raw_sha256,
        ..
    } = &mut discovery.observations[0].state
    {
        runfile.tailscale_serve.as_mut().unwrap().apply_confirmed = false;
        *raw_sha256 =
            ferric_bench::sha256_bytes(&serde_json::to_vec_pretty(runfile.as_ref()).unwrap());
    }
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Absent,
            'c',
        ))],
        Rc::clone(&ledger),
    );
    let report = status_report_with_tailscale(&discovery, &serve);
    assert!(matches!(
        report.next_action,
        StatusNextAction::RecoverOwnedTailscale {
            apply_confirmed: false,
            subject: TailscaleRecoverySubject::StaleRegistration,
            ..
        }
    ));
    let rendered = render_status(&report);
    assert!(rendered.stdout.iter().any(|line| {
        line.starts_with("[tailscale] pending") && line.contains("apply-confirmed=false")
    }));
    let next = rendered.stdout.last().unwrap();
    assert!(next.contains("absent-only check cannot authorize deletion"));
    assert!(next.contains("daemon-generation/manual proof"));

    let (discovery, ownership) = discovery_fixture_wildcard_tailscale();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Absent,
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let report = status_report_with_tailscale(&discovery, &serve);
    let rendered = render_status(&report);
    assert!(!report.success);
    assert!(matches!(
        report.next_action,
        StatusNextAction::InspectWildcard { .. }
    ));
    let next = rendered.stdout.last().expect("wildcard next action");
    assert!(next.contains("teardown is not authorized"));
    assert!(!next.contains("run `ferric server down`"));
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleObserve]);
}

#[test]
fn status_never_hides_tailscale_ownership_conflicts() {
    for stale_only in [false, true] {
        let (mut discovery, _ownership) = if stale_only {
            discovery_fixture_stale_tailscale()
        } else {
            discovery_fixture_ready_tailscale()
        };
        add_distinct_stale_tailscale_peer(&mut discovery, 4102, "stale-tailscale-peer");
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new(
            Vec::<
                Result<
                    crate::tailscale_serve::ServeStatusObservation,
                    crate::tailscale_serve::TailscaleServeError,
                >,
            >::new(),
            Rc::clone(&ledger),
        );
        let report = status_report_with_tailscale(&discovery, &serve);
        let rendered = render_status(&report);

        assert!(!report.success, "stale_only={stale_only}");
        assert!(report.tailscale.is_none());
        assert!(
            report.tailscale_issue.as_deref().is_some_and(|issue| {
                issue.contains("disagree about Tailscale Serve ownership")
            })
        );
        assert!(matches!(
            report.next_action,
            StatusNextAction::ResolveConflict { ref coordinates }
                if coordinates.len() == 2
        ));
        assert!(rendered.stdout.iter().any(|line| {
            line.starts_with("[tailscale] ownership-blocked")
                && line.contains("disagree about Tailscale Serve ownership")
        }));
        assert!(
            rendered
                .stdout
                .last()
                .is_some_and(|line| line.contains("2 conflicting registration coordinate(s)"))
        );
        assert!(
            ledger.borrow().is_empty(),
            "ownership conflict must not probe"
        );
    }
}

#[test]
fn mirrored_tailscale_provenance_conflicts_block_before_effects() {
    for (case_index, case) in [
        "before-status-digest",
        "tcp-map",
        "tcp-https",
        "web-map",
        "web-host",
    ]
    .into_iter()
    .enumerate()
    {
        let root = tempfile::tempdir().unwrap();
        let local_path = root.path().join(case).join("local/server.json");
        let global_path = root.path().join(case).join("global/server.json");
        fs::create_dir_all(local_path.parent().unwrap()).unwrap();
        fs::create_dir_all(global_path.parent().unwrap()).unwrap();

        let pid = 4700 + u32::try_from(case_index).unwrap();
        let mut local = discovery_fixture_capture(RegistrationScope::Local, pid, case);
        local.path = local_path.clone();
        local.runfile.origin_local_runfile = Some(local_path.clone());
        let port = local.runfile.port;

        let mut local_ownership = tailscale_ownership(port);
        local_ownership.apply_confirmed = true;
        let mut global_ownership = local_ownership.clone();
        match case {
            "before-status-digest" => {
                global_ownership.before_status_sha256 = "b".repeat(64);
            }
            "tcp-map" => {
                global_ownership.tcp_map_preexisting = true;
            }
            "tcp-https" => {
                local_ownership.tcp_map_preexisting = true;
                global_ownership = local_ownership.clone();
                global_ownership.tcp_https_preexisting = true;
            }
            "web-map" => {
                global_ownership.web_map_preexisting = true;
            }
            "web-host" => {
                local_ownership.tcp_map_preexisting = true;
                local_ownership.tcp_https_preexisting = true;
                local_ownership.web_map_preexisting = true;
                global_ownership = local_ownership.clone();
                global_ownership.web_host_preexisting = true;
            }
            _ => unreachable!(),
        }
        local_ownership.validate_for_port(port).unwrap();
        global_ownership.validate_for_port(port).unwrap();
        assert!(
            !local_ownership.same_coordinate(&global_ownership),
            "fixture must differ in lifecycle authority: {case}"
        );

        local.runfile.tailscale = true;
        local.runfile.tailscale_serve = Some(local_ownership);
        local.raw = serde_json::to_vec_pretty(&local.runfile).unwrap();
        let local_raw = local.raw.clone();
        fs::write(&local_path, &local_raw).unwrap();

        let mut global = local.clone();
        global.scope = RegistrationScope::Global;
        global.path = global_path.clone();
        global.runfile.tailscale_serve = Some(global_ownership);
        global.raw = serde_json::to_vec_pretty(&global.runfile).unwrap();
        let global_raw = global.raw.clone();
        fs::write(&global_path, &global_raw).unwrap();

        let inventory = RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(local)),
            global: Some(RegistrationSlot::Captured(Box::new(global))),
            promised_origins: Vec::new(),
        };
        let discovery = discover_inventory_with(
            inventory,
            |_capture| panic!("{case}: provenance conflict reached process inspection"),
            &mut PanicHealth,
            |_observation| panic!("{case}: provenance conflict reached revalidation"),
        );
        let issues = match &discovery.managed.state {
            ManagedServerState::Conflict { issues } => issues,
            state => panic!("{case}: provenance disagreement was not a conflict: {state:?}"),
        };
        assert!(issues.iter().any(|issue| {
            issue
                .detail
                .contains("same persisted process key has conflicting registration metadata")
        }));
        assert!(discovery.managed.observations.iter().all(|observation| {
            matches!(
                observation.state,
                ManagedRegistrationState::Captured {
                    runtime: RuntimeObservation::Unverifiable {
                        observed_identity: None,
                        listener: None,
                        health: HealthState::NotProbed,
                        ..
                    },
                    ..
                }
            )
        }));

        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new(
            Vec::<
                Result<
                    crate::tailscale_serve::ServeStatusObservation,
                    crate::tailscale_serve::TailscaleServeError,
                >,
            >::new(),
            Rc::clone(&ledger),
        );
        let status = status_report_with_tailscale(&discovery.managed, &serve);
        assert!(!status.success, "{case}");
        assert!(status.tailscale.is_none(), "{case}");
        assert!(
            status.tailscale_issue.as_deref().is_some_and(|issue| {
                issue.contains("disagree about Tailscale Serve ownership")
            })
        );
        assert!(
            ledger.borrow().is_empty(),
            "{case}: blocked status performed a Tailscale effect"
        );

        let plan = down_plan_from_lifecycle(discovery);
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(plan, &mut effects);
        let rendered = render_down_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, DownDisposition::Blocked, "{case}");
        assert!(!report.signalled, "{case}");
        assert_eq!(report.registrations.len(), 2, "{case}");
        assert!(report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        )));
        assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::Render],
            "{case}: blocked down probed or mutated external state"
        );
        assert_eq!(fs::read(&local_path).unwrap(), local_raw, "{case}");
        assert_eq!(fs::read(&global_path).unwrap(), global_raw, "{case}");
    }
}

#[test]
fn status_exact_proxy_is_not_ready_after_tailscale_identity_drift() {
    let mut renamed = test_tailscale_identity();
    renamed.fqdn = "renamed.tailnet-example.ts.net".to_string();
    let mut switched = test_tailscale_identity();
    switched.stable_node_id = "other-stable-node".to_string();

    for (case, identity, expected_detail) in [
        (
            "same-node-rename",
            renamed,
            "renamed.tailnet-example.ts.net",
        ),
        ("stable-node-switch", switched, "other-stable-node"),
    ] {
        let (discovery, ownership) = discovery_fixture_ready_tailscale();
        let mut observation = tailscale_observation(
            &ownership,
            ServePathState::Proxy {
                target: ownership.proxy_target.clone(),
            },
            'b',
        );
        observation.identity = Some(identity);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new([Ok(observation)], Rc::clone(&ledger));

        let report = status_report_with_tailscale(&discovery, &serve);
        let rendered = render_status(&report);
        assert!(
            matches!(report.state, ManagedServerState::Ready(_)),
            "{case}"
        );
        assert!(!report.success, "{case}");
        let reason = match report.tailscale.as_ref().map(|tailscale| &tailscale.status) {
            Some(TailscaleProxyStatus::Uninspectable { reason }) => reason,
            status => panic!("{case}: identity drift did not fail closed: {status:?}"),
        };
        assert!(
            reason.contains("differs from journaled Serve identity"),
            "{case}: {reason}"
        );
        assert!(reason.contains(expected_detail), "{case}: {reason}");
        assert!(matches!(
            report.next_action,
            StatusNextAction::RecoverOwnedTailscale {
                subject: TailscaleRecoverySubject::ManagedProcess,
                apply_confirmed: true,
                ..
            }
        ));
        assert!(rendered.stdout.iter().any(|line| {
            line.starts_with("[tailscale] uninspectable") && line.contains(&ownership.mount_path)
        }));
        assert_eq!(
            *ledger.borrow(),
            vec![LifecycleEvent::TailscaleObserve],
            "{case}: status must remain read-only"
        );
    }
}

#[test]
fn doctor_tailscale_is_bounded_and_read_only() {
    let mut tailscale_args = doctor_fixture_args();
    tailscale_args.tailscale = true;
    let mut effects = RecordingDoctorEffects::default();
    let report = doctor_report_with(
        &tailscale_args,
        || Ok(discovery_fixture_empty()),
        &mut effects,
    );
    assert!(report.success);
    assert_eq!(
        effects.events,
        vec![
            DoctorEvent::Binary,
            DoctorEvent::File,
            DoctorEvent::TailscaleIdentity,
            DoctorEvent::TailscaleStatus,
        ]
    );
    assert!(report.lines.iter().any(|line| {
        line == "[ok] Tailscale canonical self identity `example-host.tailnet-example.ts.net`"
    }));
    assert!(report.lines.iter().any(|line| {
        line == "[ok] Tailscale Serve status is readable through a bounded read-only probe"
    }));
    assert!(report.lines.iter().all(|line| {
        !line.contains("reset") && !line.contains("set-config") && !line.contains("--bg")
    }));

    for (case, identity, status, expected_events, expected_detail) in [
            (
                "missing-localapi",
                Err("could not connect to Tailscale LocalAPI for identity probe".to_string()),
                Ok(()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                ],
                "could not connect to Tailscale LocalAPI",
            ),
            (
                "incompatible-tailscale",
                Err(
                    "unsupported Tailscale daemon; normal operation requires capability 142 and version core 1.102.2"
                        .to_string(),
                ),
                Ok(()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                ],
                "requires capability 142 and version core 1.102.2",
            ),
            (
                "identity-failure",
                Err("Tailscale identity probe failed".to_string()),
                Ok(()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                ],
                "identity probe failed",
            ),
            (
                "daemon-status-failure",
                Ok("example-host.tailnet-example.ts.net".to_string()),
                Err("Tailscale daemon is unavailable".to_string()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                    DoctorEvent::TailscaleStatus,
                ],
                "daemon is unavailable",
            ),
            (
                "malformed-status",
                Ok("example-host.tailnet-example.ts.net".to_string()),
                Err("invalid Tailscale Serve status: Web must be an object".to_string()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                    DoctorEvent::TailscaleStatus,
                ],
                "Web must be an object",
            ),
            (
                "bounded-timeout",
                Ok("example-host.tailnet-example.ts.net".to_string()),
                Err("Tailscale LocalAPI request exceeded its bounded deadline".to_string()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                    DoctorEvent::TailscaleStatus,
                ],
                "bounded deadline",
            ),
            (
                "bounded-output",
                Ok("example-host.tailnet-example.ts.net".to_string()),
                Err("Tailscale LocalAPI response exceeded its bounded body allowance".to_string()),
                vec![
                    DoctorEvent::Binary,
                    DoctorEvent::File,
                    DoctorEvent::TailscaleIdentity,
                    DoctorEvent::TailscaleStatus,
                ],
                "bounded body allowance",
            ),
        ] {
            let mut effects = RecordingDoctorEffects {
                tailscale_identity: identity,
                tailscale_status: status,
                ..RecordingDoctorEffects::default()
            };
            let report = doctor_report_with(
                &tailscale_args,
                || Ok(discovery_fixture_empty()),
                &mut effects,
            );
            assert!(!report.success, "{case}");
            assert_eq!(effects.events, expected_events, "{case}");
            assert!(
                report
                    .lines
                    .iter()
                    .any(|line| line.contains(expected_detail)),
                "{case}: {:?}",
                report.lines
            );
        }
}

#[test]
fn doctor_blockers_precede_all_probes() {
    let mut effects = RecordingDoctorEffects::default();
    let mut invalid = doctor_fixture_args();
    invalid.tailscale = true;
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

    let mut blocked_args = doctor_fixture_args();
    blocked_args.tailscale = true;
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

#[test]
fn tailscale_operator_rendering_is_copy_paste_complete() {
    let pid = 6381;
    let (local, ownership) =
        discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, "operator-local");
    let mut global = local.clone();
    global.scope = RegistrationScope::Global;
    global.path = discovery_fixture_path("operator-global");
    let launched = LaunchOrchestrationSuccess {
        pid,
        base_url: local.runfile.base_url.clone(),
        remote_base_url: Some(ownership.remote_base_url.clone()),
        published: PublishedRegistrations {
            local: local.clone(),
            global: Some(global),
        },
    };
    let launch_lines = render_launch_success(&launched);

    let (discovery, status_ownership) = discovery_fixture_ready_tailscale();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &status_ownership,
            ServePathState::Absent,
            'b',
        ))],
        ledger,
    );
    let status = render_status(&status_report_with_tailscale(&discovery, &serve));

    let down = render_down_report(&held_down_report(
        Some(pid),
        std::slice::from_ref(&local),
        true,
        true,
        true,
        vec![retain_owned_proxy_diagnostic(&ownership)],
    ));

    let all = launch_lines
        .iter()
        .chain(status.stdout.iter())
        .chain(status.stderr.iter())
        .chain(down.stdout.iter())
        .chain(down.stderr.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(all.contains(&local.runfile.base_url));
    assert!(all.contains(&ownership.remote_base_url));
    assert!(all.contains(&ownership.fqdn));
    assert!(all.contains(&ownership.mount_path));
    assert!(all.contains(&status_ownership.proxy_target));
    assert!(all.contains(&local.path.display().to_string()));
    assert!(all.contains("`ferric server down`"));
    assert!(all.contains("exact-coordinate"));
    assert!(!all.contains("serve reset"));
    assert!(!all.contains("set-config"));
    assert!(!all.contains("C:\\Users\\<you>"));
    assert!(!all.contains("tailnet.ts.net"));
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
    let mut effects = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(plan, &mut effects);
    let rendered = render_down_with_ledger(&report, &ledger);
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
            let mut effects = ScriptedDownEffects::new(
                Vec::<ListenerState>::new(),
                Vec::<Result<RemovalOutcome, RemovalError>>::new(),
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            render_down_with_ledger(&report, &ledger);
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
                    label: registration_label(capture.scope, &capture.path),
                    candidate,
                    capture: Some(capture.clone()),
                    process: Some(process),
                }],
                resolution,
            });
            let mut effects = ScriptedDownEffects::new(
                [ListenerState::Absent],
                [Ok(RemovalOutcome::Removed)],
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            render_down_with_ledger(&report, &ledger);
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
                        coordinate: RegistrationCoordinate {
                            scope: capture.scope,
                            path: capture.path.clone(),
                        },
                        runfile: Some(capture.runfile.clone()),
                        state: CandidateState::Stale {
                            reason: "PID is absent".to_string(),
                            observed_identity: None,
                            listener: ListenerState::Absent,
                        },
                    },
                    label: registration_label(capture.scope, &capture.path),
                    capture: Some(capture.clone()),
                    process: None,
                }],
                resolution: Resolution::StaleOnly { stale: vec![0] },
            });
            let mut effects = ScriptedDownEffects::new(
                [ListenerState::Absent],
                [Ok(RemovalOutcome::Removed)],
                Rc::clone(&ledger),
            );
            let report = execute_down_plan(plan, &mut effects);
            render_down_with_ledger(&report, &ledger);
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
        let strict =
            crate::backend::require_managed_endpoint(scope.clone(), discovery.clone(), None);
        let down_blocker = down_mutation_blocker(&discovery.state);
        match &discovery.state {
            ManagedServerState::Empty => {
                assert!(matches!(
                    automatic,
                    Ok(crate::backend::EndpointSelection::Default { .. })
                ));
                assert!(strict.is_err());
                assert!(doctor.success);
                assert!(!effects.events.is_empty());
                assert!(down_blocker.is_none());
            }
            ManagedServerState::Ready(server) => {
                assert!(matches!(
                    automatic,
                    Ok(crate::backend::EndpointSelection::Managed { .. })
                ));
                assert!(strict.is_ok());
                let explicit_match = format!("{}/", server.runfile.base_url);
                let matching = crate::backend::require_managed_endpoint(
                    scope.clone(),
                    discovery.clone(),
                    Some(&explicit_match),
                )
                .unwrap();
                assert!(matches!(
                    matching,
                    crate::backend::EndpointSelection::Managed {
                        explicit_base_url: Some(_),
                        ..
                    }
                ));
                assert!(matches!(
                    crate::backend::require_managed_endpoint(
                        scope.clone(),
                        discovery.clone(),
                        Some("http://127.0.0.1:65535/v1"),
                    ),
                    Err(crate::backend::EndpointSelectionError::ExplicitManagedMismatch { .. })
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
                assert!(matches!(
                    strict,
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
                assert!(matches!(
                    strict,
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
                assert!(matches!(
                    strict,
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
                assert!(matches!(
                    strict,
                    Err(crate::backend::EndpointSelectionError::Unverifiable(_))
                ));
                assert!(!doctor.success);
                assert!(effects.events.is_empty());
                assert!(down_blocker.is_some());
            }
        }
    }

    // The frozen E03-C command owns the consumer-effect proof. Keep the
    // focused neighbors as independently runnable regressions, but invoke
    // their real autonomy/HTTP revalidation, doctor probe-ordering, and
    // down mutation ledgers here so a name-filtered acceptance run cannot
    // pass on policy-return values alone.
    strict_autonomy_requires_fresh_managed_discovery_before_http();
    registered_consumer_effect_revalidates_retained_generation_on_every_outcome();
    doctor_blockers_precede_all_probes();
    ambiguous_or_unverifiable_down_is_non_mutating();
}

#[test]
fn strict_autonomy_requires_fresh_managed_discovery_before_http() {
    fn mirrored_inventory() -> RegistrationInventory {
        let local = discovery_fixture_coordinate(RegistrationScope::Local, "strict-local");
        let global = discovery_fixture_coordinate(RegistrationScope::Global, "strict-global");
        let runfile = discovery_fixture_runfile(4101, "strict-local");
        let raw = serde_json::to_vec(&runfile).unwrap();
        RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: local.scope,
                path: local.path,
                raw: raw.clone(),
                runfile: runfile.clone(),
            })),
            global: Some(RegistrationSlot::Captured(Box::new(CapturedRegistration {
                scope: global.scope,
                path: global.path,
                raw,
                runfile,
            }))),
            promised_origins: Vec::new(),
        }
    }

    fn observe_exact(capture: CapturedRegistration) -> LifecycleObservation {
        let identity = capture.runfile.process_identity.clone().unwrap();
        let label = registration_label(capture.scope, &capture.path);
        LifecycleObservation {
            candidate: Candidate {
                coordinate: RegistrationCoordinate {
                    scope: capture.scope,
                    path: capture.path.clone(),
                },
                runfile: Some(capture.runfile.clone()),
                state: CandidateState::Verified {
                    identity,
                    listener: ListenerState::OwnedByTarget,
                    health: HealthState::NotProbed,
                },
            },
            label,
            capture: Some(capture),
            process: None,
        }
    }

    let inventory = mirrored_inventory();
    let initial = discover_inventory_before_health_with(inventory.clone(), observe_exact);
    let expected = match &initial.managed.state {
        ManagedServerState::Degraded { server, .. } => server.fingerprint.clone(),
        state => panic!("pre-health exact owner must be typed Degraded, got {state:?}"),
    };
    crate::autonomy_cmd::require_matching_pre_health_discovery(&initial.managed, &expected)
        .unwrap();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: ledger.clone(),
    };
    let ready = complete_lifecycle_health_with(initial, &mut health, |_| Ok(()));
    assert!(matches!(ready.managed.state, ManagedServerState::Ready(_)));
    assert_eq!(ledger.borrow().as_slice(), &[LifecycleEvent::Health(7101)]);
    let ready_server = match &ready.managed.state {
        ManagedServerState::Ready(server) => server,
        _ => unreachable!(),
    };
    let facts = ProcessFacts {
        identity: ready_server.identity.clone(),
        listener: ListenerState::OwnedByTarget,
    };
    ledger.borrow_mut().clear();
    let process = ScriptedProcess::new(
        ready_server.runfile.pid,
        "strict-generation",
        ledger.clone(),
    )
    .with_inspection(Ok(facts.clone()))
    .with_inspection(Ok(facts));
    let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
    bracket_registered_effect_with(&runtime, &ready_server.runfile, || {
        ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        ledger.borrow().as_slice(),
        &[
            LifecycleEvent::Acquire(4101),
            LifecycleEvent::Inspect("strict-generation", 7101),
            LifecycleEvent::ConsumerHttp,
            LifecycleEvent::Inspect("strict-generation", 7101),
        ]
    );

    let mut changed_revision = inventory.clone();
    let Some(RegistrationSlot::Captured(global)) = &mut changed_revision.global else {
        unreachable!()
    };
    global.raw.push(b' ');

    let mut missing_alias = inventory.clone();
    let global_path = match missing_alias.global.take().unwrap() {
        RegistrationSlot::Captured(global) => global.path,
        _ => unreachable!(),
    };
    missing_alias.global = Some(RegistrationSlot::Absent {
        scope: RegistrationScope::Global,
        path: global_path,
    });

    let mut conflicting_peer = inventory;
    let Some(RegistrationSlot::Captured(global)) = &mut conflicting_peer.global else {
        unreachable!()
    };
    global.runfile.pid = 4102;
    global.runfile.process_identity = Some(discovery_fixture_identity(4102));
    global.raw = serde_json::to_vec(&global.runfile).unwrap();

    for changed in [changed_revision, missing_alias, conflicting_peer] {
        ledger.borrow_mut().clear();
        let before_health = discover_inventory_before_health_with(changed, observe_exact);
        assert!(
            crate::autonomy_cmd::require_matching_pre_health_discovery(
                &before_health.managed,
                &expected,
            )
            .is_err()
        );
        assert!(
            ledger.borrow().is_empty(),
            "fingerprint or conflict rejection must precede HTTP health"
        );
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
    ConsumerHttp,
    ClockNow,
    Sleep,
    Publish,
    Revalidate,
    Remove(PathBuf, String),
    RemoveStage(PathBuf, Option<String>),
    Replace(PathBuf, String, String),
    Persistence(PersistencePhase, PathBuf),
    TailscaleIdentity,
    TailscaleObserve,
    TailscaleApply,
    TailscaleOff,
    Render,
}

type EventLedger = Rc<RefCell<Vec<LifecycleEvent>>>;
type ScriptedObserveWrite = Option<(PathBuf, Vec<u8>)>;

struct ScriptedTailscaleServe {
    identities: RefCell<
        VecDeque<
            Result<
                crate::tailscale_serve::TailscaleIdentity,
                crate::tailscale_serve::TailscaleServeError,
            >,
        >,
    >,
    observations: RefCell<
        VecDeque<
            Result<
                crate::tailscale_serve::ServeStatusObservation,
                crate::tailscale_serve::TailscaleServeError,
            >,
        >,
    >,
    applies: RefCell<VecDeque<Result<(), crate::tailscale_serve::TailscaleServeError>>>,
    offs: RefCell<VecDeque<Result<(), crate::tailscale_serve::TailscaleServeError>>>,
    observe_writes: RefCell<VecDeque<ScriptedObserveWrite>>,
    ledger: EventLedger,
}

impl ScriptedTailscaleServe {
    fn new(
        observations: impl IntoIterator<
            Item = Result<
                crate::tailscale_serve::ServeStatusObservation,
                crate::tailscale_serve::TailscaleServeError,
            >,
        >,
        ledger: EventLedger,
    ) -> Self {
        Self {
            identities: RefCell::new((0..16).map(|_| Ok(test_tailscale_identity())).collect()),
            observations: RefCell::new(observations.into_iter().collect()),
            applies: RefCell::new(VecDeque::from([Ok(())])),
            offs: RefCell::new(VecDeque::from([Ok(())])),
            observe_writes: RefCell::new(VecDeque::new()),
            ledger,
        }
    }

    fn with_apply(self, result: Result<(), crate::tailscale_serve::TailscaleServeError>) -> Self {
        *self.applies.borrow_mut() = VecDeque::from([result]);
        self
    }

    fn with_off(self, result: Result<(), crate::tailscale_serve::TailscaleServeError>) -> Self {
        *self.offs.borrow_mut() = VecDeque::from([result]);
        self
    }

    fn with_observe_writes(self, writes: impl IntoIterator<Item = ScriptedObserveWrite>) -> Self {
        *self.observe_writes.borrow_mut() = writes.into_iter().collect();
        self
    }
}

impl TailscaleServeEffects for ScriptedTailscaleServe {
    fn self_identity(
        &self,
    ) -> Result<
        crate::tailscale_serve::TailscaleIdentity,
        crate::tailscale_serve::TailscaleServeError,
    > {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::TailscaleIdentity);
        self.identities
            .borrow_mut()
            .pop_front()
            .expect("scripted Tailscale identity")
    }

    fn probe_status(
        &self,
        _fqdn: &str,
    ) -> Result<String, crate::tailscale_serve::TailscaleServeError> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::TailscaleObserve);
        self.observations
            .borrow_mut()
            .pop_front()
            .expect("scripted Tailscale status probe")
            .map(|observation| observation.status_sha256)
    }

    fn observe_coordinate(
        &self,
        _fqdn: &str,
        _mount_path: &str,
    ) -> Result<
        crate::tailscale_serve::ServeStatusObservation,
        crate::tailscale_serve::TailscaleServeError,
    > {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::TailscaleObserve);
        if let Some(Some((path, raw))) = self.observe_writes.borrow_mut().pop_front() {
            fs::write(path, raw).expect("scripted concurrent registration replacement");
        }
        self.observations
            .borrow_mut()
            .pop_front()
            .expect("scripted Tailscale observation")
    }

    fn apply(
        &self,
        _ownership: &TailscaleServeOwnership,
    ) -> Result<(), crate::tailscale_serve::TailscaleServeError> {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::TailscaleApply);
        self.applies
            .borrow_mut()
            .pop_front()
            .expect("scripted Tailscale apply")
    }

    fn off(
        &self,
        _ownership: &TailscaleServeOwnership,
    ) -> Result<(), crate::tailscale_serve::TailscaleServeError> {
        self.ledger.borrow_mut().push(LifecycleEvent::TailscaleOff);
        self.offs
            .borrow_mut()
            .pop_front()
            .expect("scripted Tailscale off")
    }
}

fn test_tailscale_identity() -> crate::tailscale_serve::TailscaleIdentity {
    crate::tailscale_serve::TailscaleIdentity {
        stable_node_id: "node-fixture".to_string(),
        fqdn: "example-host.tailnet-example.ts.net".to_string(),
        backend_running: true,
        https_capable: true,
        certificate_domain: true,
    }
}

fn tailscale_ownership(port: u16) -> TailscaleServeOwnership {
    let token = "00112233445566778899aabbccddeeff".to_string();
    let coordinate = coordinate_from_token(port, &test_tailscale_identity(), token).unwrap();
    let mut observation = crate::tailscale_serve::project_localapi_status(
        b"{}",
        &coordinate.fqdn,
        &coordinate.mount_path,
    )
    .unwrap();
    observation.identity = Some(test_tailscale_identity());
    coordinate.into_ownership(&observation).unwrap()
}

fn tailscale_observation(
    ownership: &TailscaleServeOwnership,
    path_state: ServePathState,
    digest: char,
) -> crate::tailscale_serve::ServeStatusObservation {
    let active = matches!(path_state, ServePathState::Proxy { .. });
    crate::tailscale_serve::ServeStatusObservation {
        fqdn: ownership.fqdn.clone(),
        https_port: ownership.https_port,
        mount_path: ownership.mount_path.clone(),
        status_sha256: digest.to_string().repeat(64),
        path_state,
        identity: Some(test_tailscale_identity()),
        scaffold: crate::tailscale_serve::ServeScaffoldState {
            tcp_map_present: active,
            tcp_https_present: active,
            web_map_present: active,
            web_host_present: active,
        },
        https_mode_compatible: active,
        funnel_enabled: false,
        foreground_shadows: false,
        route_shadow: None,
        cleanup_semantics_pinned: true,
    }
}

fn representative_revision(capture: &CapturedRegistration) -> RegistrationRevision {
    RegistrationRevision {
        coordinate: RegistrationCoordinate {
            scope: capture.scope,
            path: capture.path.clone(),
        },
        promised: None,
        state: RegistrationRevisionState::Captured(ferric_bench::sha256_bytes(&capture.raw)),
    }
}

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

impl ProcessRuntime for ScriptedRuntime {
    type Process = ScriptedProcess;

    fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
        self.acquisitions
            .borrow_mut()
            .pop_front()
            .expect("scripted process acquisition")
            .map_err(ProcessError::Operation)
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
}

impl ProcessRuntime for ScriptedPidMapRuntime {
    type Process = ScriptedProcess;

    fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Acquire(pid));
        self.processes
            .borrow()
            .get(&pid)
            .cloned()
            .ok_or(ProcessError::NotFound(pid))
    }
}

struct ScriptedDownEffects {
    revalidations: VecDeque<Result<(), String>>,
    listeners: VecDeque<ListenerState>,
    removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
    replacements: VecDeque<Result<ReplacementOutcome, ReplacementError>>,
    tailscale: Option<ScriptedTailscaleServe>,
    ledger: EventLedger,
}

impl ScriptedDownEffects {
    fn new(
        listeners: impl IntoIterator<Item = ListenerState>,
        removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            revalidations: VecDeque::from([Ok(())]),
            listeners: listeners.into_iter().collect(),
            removals: removals.into_iter().collect(),
            replacements: VecDeque::new(),
            tailscale: None,
            ledger,
        }
    }

    fn with_tailscale(mut self, tailscale: ScriptedTailscaleServe) -> Self {
        self.revalidations = VecDeque::from([Ok(()), Ok(()), Ok(())]);
        self.tailscale = Some(tailscale);
        self
    }

    fn with_tailscale_replacements(
        mut self,
        replacements: impl IntoIterator<Item = Result<ReplacementOutcome, ReplacementError>>,
    ) -> Self {
        self.replacements = replacements.into_iter().collect();
        self
    }
}

impl DownEffects for ScriptedDownEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        if self.tailscale.is_some() {
            assert!(
                !expected.is_empty(),
                "typed Tailscale teardown must carry captured registration revisions"
            );
        }
        self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
        self.revalidations
            .pop_front()
            .expect("scripted registration revalidation")
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.listeners
            .pop_front()
            .expect("scripted down-listener state")
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Remove(
            captured.path.clone(),
            ferric_bench::sha256_bytes(&captured.raw),
        ));
        self.removals
            .pop_front()
            .expect("scripted conditional-removal result")
    }

    fn reconcile_tailscale(
        &mut self,
        ownership: &TailscaleServeOwnership,
        captures: &mut [CapturedRegistration],
    ) -> ProxyCleanupReport {
        let replacements = &mut self.replacements;
        let ledger = Rc::clone(&self.ledger);
        reconcile_owned_proxy(
            ownership,
            self.tailscale
                .as_ref()
                .expect("scripted Tailscale down effects"),
            if ownership.apply_confirmed {
                ProxyReconcileContext::EstablishedOwnership
            } else {
                ProxyReconcileContext::AmbiguousApply
            },
            || {
                confirm_tailscale_captures_with(captures, ownership, |captured, raw| {
                    ledger.borrow_mut().push(LifecycleEvent::Replace(
                        captured.path.clone(),
                        ferric_bench::sha256_bytes(&captured.raw),
                        ferric_bench::sha256_bytes(raw),
                    ));
                    replacements
                        .pop_front()
                        .unwrap_or(Ok(ReplacementOutcome::Replaced))
                })
            },
        )
    }
}

struct FilesystemDownEffects {
    scope: ManagedDiscoveryScope,
    listeners: VecDeque<ListenerState>,
    ledger: EventLedger,
}

impl DownEffects for FilesystemDownEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
        let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
        let current = discovery_revisions(&flatten_inventory(&inventory));
        if current == expected {
            Ok(())
        } else {
            Err("composition inventory changed before teardown".to_string())
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.listeners
            .pop_front()
            .expect("scripted composition listener state")
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        self.ledger
            .borrow_mut()
            .push(scripted_remove_event(captured));
        remove_if_unchanged(captured)
    }
}

struct FilesystemTailscaleDownEffects {
    scope: ManagedDiscoveryScope,
    listeners: VecDeque<ListenerState>,
    serve: ScriptedTailscaleServe,
    ledger: EventLedger,
}

impl DownEffects for FilesystemTailscaleDownEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
        let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
        let current = discovery_revisions(&flatten_inventory(&inventory));
        if current == expected {
            Ok(())
        } else {
            Err("Tailscale recovery inventory changed before teardown".to_string())
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.listeners
            .pop_front()
            .expect("scripted Tailscale recovery listener state")
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        self.ledger
            .borrow_mut()
            .push(scripted_remove_event(captured));
        remove_if_unchanged(captured)
    }

    fn reconcile_tailscale(
        &mut self,
        ownership: &TailscaleServeOwnership,
        captures: &mut [CapturedRegistration],
    ) -> ProxyCleanupReport {
        let ledger = Rc::clone(&self.ledger);
        reconcile_owned_proxy(
            ownership,
            &self.serve,
            if ownership.apply_confirmed {
                ProxyReconcileContext::EstablishedOwnership
            } else {
                ProxyReconcileContext::AmbiguousApply
            },
            || {
                confirm_tailscale_captures_with(captures, ownership, |captured, raw| {
                    ledger.borrow_mut().push(LifecycleEvent::Replace(
                        captured.path.clone(),
                        ferric_bench::sha256_bytes(&captured.raw),
                        ferric_bench::sha256_bytes(raw),
                    ));
                    replace_if_unchanged(captured, raw)
                })
            },
        )
    }
}

struct RevisionCheckingDownEffects {
    scope: ManagedDiscoveryScope,
    listeners: VecDeque<ListenerState>,
    removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
    ledger: EventLedger,
}

impl DownEffects for RevisionCheckingDownEffects {
    fn revalidate_registrations(
        &mut self,
        expected: &[RegistrationRevision],
    ) -> Result<(), String> {
        self.ledger.borrow_mut().push(LifecycleEvent::Revalidate);
        let inventory = inventory_runfiles(&self.scope.workspace, self.scope.global.clone());
        let current = discovery_revisions(&flatten_inventory(&inventory));
        if current == expected {
            Ok(())
        } else {
            Err("composition inventory changed before teardown".to_string())
        }
    }

    fn listener_state(&mut self, pid: u32, port: u16) -> ListenerState {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Listener(pid, port));
        self.listeners
            .pop_front()
            .expect("scripted transition listener state")
    }

    fn remove(&mut self, captured: &CapturedRegistration) -> Result<RemovalOutcome, RemovalError> {
        self.ledger
            .borrow_mut()
            .push(scripted_remove_event(captured));
        self.removals
            .pop_front()
            .expect("scripted transition removal")
    }
}

struct ScriptedPublicationEffects {
    final_removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
    stage_removals: VecDeque<Result<RemovalOutcome, RemovalError>>,
    ledger: EventLedger,
}

impl ScriptedPublicationEffects {
    fn new(
        final_removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        stage_removals: impl IntoIterator<Item = Result<RemovalOutcome, RemovalError>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            final_removals: final_removals.into_iter().collect(),
            stage_removals: stage_removals.into_iter().collect(),
            ledger,
        }
    }
}

impl PublicationCompensationEffects for ScriptedPublicationEffects {
    fn replace_final(
        &mut self,
        _captured: &CapturedRegistration,
        _replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        panic!("non-Tailscale scripted publication must not confirm Serve ownership")
    }

    fn remove_final(
        &mut self,
        captured: &CapturedRegistration,
    ) -> Result<RemovalOutcome, RemovalError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Remove(
            captured.path.clone(),
            ferric_bench::sha256_bytes(&captured.raw),
        ));
        self.final_removals
            .pop_front()
            .expect("scripted publication-final removal")
    }

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError> {
        self.ledger.borrow_mut().push(LifecycleEvent::RemoveStage(
            stage.path.clone(),
            stage.raw.as_deref().map(ferric_bench::sha256_bytes),
        ));
        self.stage_removals
            .pop_front()
            .expect("scripted publication-stage removal")
    }
}

struct CompositionPersistenceEffects {
    failure: Option<(PathBuf, PersistencePhase)>,
    retain_stage_after_persist: Option<PathBuf>,
    serializations: usize,
    ledger: EventLedger,
}

impl CompositionPersistenceEffects {
    fn default_with(ledger: EventLedger) -> Self {
        Self {
            failure: None,
            retain_stage_after_persist: None,
            serializations: 0,
            ledger,
        }
    }

    fn failing(final_path: &Path, phase: PersistencePhase, ledger: EventLedger) -> Self {
        Self {
            failure: Some((final_path.to_path_buf(), phase)),
            ..Self::default_with(ledger)
        }
    }

    fn retaining_committed_stage(final_path: &Path, ledger: EventLedger) -> Self {
        Self {
            retain_stage_after_persist: Some(final_path.to_path_buf()),
            ..Self::default_with(ledger)
        }
    }

    fn fails(&self, final_path: &Path, phase: PersistencePhase) -> bool {
        self.failure
            .as_ref()
            .is_some_and(|(path, target)| path == final_path && *target == phase)
    }

    fn record(&self, phase: PersistencePhase, final_path: &Path) {
        self.ledger
            .borrow_mut()
            .push(LifecycleEvent::Persistence(phase, final_path.to_path_buf()));
    }

    fn injected(phase: PersistencePhase) -> io::Error {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("injected {phase:?} composition failure"),
        )
    }
}

impl PersistenceEffects for CompositionPersistenceEffects {
    fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
        self.serializations += 1;
        serde_json::to_vec_pretty(runfile)
    }

    fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
        self.record(PersistencePhase::CreateStage, final_path);
        if self.fails(final_path, PersistencePhase::CreateStage) {
            return Err(Self::injected(PersistencePhase::CreateStage));
        }
        tempfile::Builder::new()
            .prefix(".server-registration-composition-")
            .tempfile_in(parent)
    }

    fn write_all(
        &mut self,
        final_path: &Path,
        stage: &mut NamedTempFile,
        raw: &[u8],
    ) -> io::Result<()> {
        self.record(PersistencePhase::WriteAll, final_path);
        if self.fails(final_path, PersistencePhase::WriteAll) {
            stage.write_all(&raw[..raw.len().min(7)])?;
            return Err(Self::injected(PersistencePhase::WriteAll));
        }
        stage.write_all(raw)
    }

    fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
        self.record(PersistencePhase::Flush, final_path);
        if self.fails(final_path, PersistencePhase::Flush) {
            return Err(Self::injected(PersistencePhase::Flush));
        }
        stage.as_file_mut().flush()
    }

    fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
        self.record(PersistencePhase::FileSync, final_path);
        if self.fails(final_path, PersistencePhase::FileSync) {
            return Err(Self::injected(PersistencePhase::FileSync));
        }
        stage.as_file().sync_all()
    }

    fn persist_noclobber(
        &mut self,
        final_path: &Path,
        mut stage: NamedTempFile,
    ) -> Result<(), StagePersistError> {
        self.record(PersistencePhase::PersistNoClobber, final_path);
        if self.fails(final_path, PersistencePhase::PersistNoClobber) {
            return Err(StagePersistError {
                error: Self::injected(PersistencePhase::PersistNoClobber),
                stage,
            });
        }
        if self
            .retain_stage_after_persist
            .as_ref()
            .is_some_and(|target| target == final_path)
        {
            if let Err(error) = fs::hard_link(stage.path(), final_path) {
                return Err(StagePersistError { error, stage });
            }
            stage.disable_cleanup(true);
            drop(stage);
            return Ok(());
        }
        stage
            .persist_noclobber(final_path)
            .map(drop)
            .map_err(|error| StagePersistError {
                error: error.error,
                stage: error.file,
            })
    }

    fn sync_parent(&mut self, final_path: &Path, _parent: &Path) -> io::Result<()> {
        self.record(PersistencePhase::ParentSync, final_path);
        if self.fails(final_path, PersistencePhase::ParentSync) {
            return Err(Self::injected(PersistencePhase::ParentSync));
        }
        Ok(())
    }
}

struct FilesystemPublicationEffects {
    ledger: EventLedger,
}

impl PublicationCompensationEffects for FilesystemPublicationEffects {
    fn replace_final(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Replace(
            captured.path.clone(),
            ferric_bench::sha256_bytes(&captured.raw),
            ferric_bench::sha256_bytes(replacement),
        ));
        replace_if_unchanged(captured, replacement)
    }

    fn remove_final(
        &mut self,
        captured: &CapturedRegistration,
    ) -> Result<RemovalOutcome, RemovalError> {
        self.ledger
            .borrow_mut()
            .push(scripted_remove_event(captured));
        remove_if_unchanged(captured)
    }

    fn remove_stage(&mut self, stage: &PublicationStage) -> Result<RemovalOutcome, RemovalError> {
        self.ledger.borrow_mut().push(LifecycleEvent::RemoveStage(
            stage.path.clone(),
            stage.raw.as_deref().map(ferric_bench::sha256_bytes),
        ));
        remove_publication_stage_if_unchanged(stage)
    }
}

struct ScriptedAdoptionEffects {
    replacements: VecDeque<Result<ReplacementOutcome, ReplacementError>>,
    ledger: EventLedger,
}

impl ScriptedAdoptionEffects {
    fn new(
        replacements: impl IntoIterator<Item = Result<ReplacementOutcome, ReplacementError>>,
        ledger: EventLedger,
    ) -> Self {
        Self {
            replacements: replacements.into_iter().collect(),
            ledger,
        }
    }
}

impl AdoptionEffects for ScriptedAdoptionEffects {
    fn replace(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        self.ledger.borrow_mut().push(LifecycleEvent::Replace(
            captured.path.clone(),
            ferric_bench::sha256_bytes(&captured.raw),
            ferric_bench::sha256_bytes(replacement),
        ));
        self.replacements
            .pop_front()
            .expect("scripted conditional-replacement result")
    }
}

struct FilesystemAdoptionEffects {
    ledger: EventLedger,
}

impl AdoptionEffects for FilesystemAdoptionEffects {
    fn replace(
        &mut self,
        captured: &CapturedRegistration,
        replacement: &[u8],
    ) -> Result<ReplacementOutcome, ReplacementError> {
        self.ledger.borrow_mut().push(scripted_replace_event(
            &captured.path,
            &captured.raw,
            replacement,
        ));
        replace_if_unchanged(captured, replacement)
    }
}

fn render_down_with_ledger(report: &DownReport, ledger: &EventLedger) -> RenderedDownReport {
    let rendered = render_down_report(report);
    ledger.borrow_mut().push(LifecycleEvent::Render);
    rendered
}

fn render_publication_with_ledger(
    report: &PublicationCompletionReport,
    ledger: &EventLedger,
) -> RenderedPublicationReport {
    let rendered = render_publication_report(report);
    ledger.borrow_mut().push(LifecycleEvent::Render);
    rendered
}

fn render_adoption_with_ledger(
    report: &AdoptionReport,
    ledger: &EventLedger,
) -> RenderedAdoptionReport {
    let rendered = render_adoption_report(report);
    ledger.borrow_mut().push(LifecycleEvent::Render);
    rendered
}

fn assert_down_failure_kept_recovery(report: &DownReport, rendered: &RenderedDownReport) {
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
            let mut effects = ScriptedDownEffects::new(
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
            let rendered = render_down_with_ledger(&report, &ledger);

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
fn down_cleans_proxy_before_process() {
    for active in [true, false] {
        let pid = if active { 6351 } else { 6352 };
        let (capture, ownership) = discovery_fixture_tailscale_capture(
            RegistrationScope::Local,
            pid,
            if active {
                "proxy-active"
            } else {
                "proxy-absent"
            },
        );
        let port = capture.runfile.port;
        let expected = capture.runfile.process_identity.clone().unwrap();
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let observations = if active {
            vec![
                Ok(tailscale_observation(
                    &ownership,
                    ServePathState::Proxy {
                        target: ownership.proxy_target.clone(),
                    },
                    'b',
                )),
                Ok(tailscale_observation(
                    &ownership,
                    ServePathState::Absent,
                    'c',
                )),
            ]
        } else {
            vec![Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            ))]
        };
        let serve = ScriptedTailscaleServe::new(observations, Rc::clone(&ledger));
        let process = ScriptedProcess::new(pid, "owned-down", Rc::clone(&ledger)).with_inspection(
            Ok(ProcessFacts {
                identity: expected.clone(),
                listener: ListenerState::OwnedByTarget,
            }),
        );
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            [Ok(RemovalOutcome::Removed)],
            Rc::clone(&ledger),
        )
        .with_tailscale(serve);

        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected,
                pid,
                port,
                captures: vec![capture.clone()],
                expected_revisions: vec![representative_revision(&capture)],
            },
            &mut effects,
        );

        assert!(report.success, "active={active}: {report:?}");
        assert_eq!(report.disposition, DownDisposition::Stopped);
        assert!(report.signalled && report.exit_proven && report.listener_released);
        let events = ledger.borrow();
        let mut expected_events =
            vec![LifecycleEvent::Revalidate, LifecycleEvent::TailscaleObserve];
        if active {
            expected_events.extend([
                LifecycleEvent::TailscaleOff,
                LifecycleEvent::TailscaleObserve,
            ]);
        }
        expected_events.extend([
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("owned-down", port),
            LifecycleEvent::Terminate("owned-down"),
            LifecycleEvent::RetainedWait("owned-down"),
            LifecycleEvent::Listener(pid, port),
            LifecycleEvent::Revalidate,
            scripted_remove_event(&capture),
        ]);
        assert_eq!(*events, expected_events, "active={active}");
    }
}

#[test]
fn down_proxy_failure_matrix_preserves_journal() {
    for case in [
        "replaced",
        "duplicate",
        "malformed",
        "unreadable",
        "off-failed",
        "post-off-unreadable",
        "route-shadow-before",
        "route-shadow-after",
        "version-drift-before",
        "version-drift-after",
    ] {
        let pid = 6360;
        let (capture, ownership) =
            discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, case);
        let port = capture.runfile.port;
        let expected = capture.runfile.process_identity.clone().unwrap();
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let exact = || {
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'b',
            ))
        };
        let (observations, off_failure) = match case {
            "replaced" => (
                vec![Ok(tailscale_observation(
                    &ownership,
                    ServePathState::Proxy {
                        target: "http://127.0.0.1:7999".to_string(),
                    },
                    'b',
                ))],
                None,
            ),
            "duplicate" => (
                vec![Err(
                    crate::tailscale_serve::TailscaleServeError::InvalidStatus(
                        "duplicate exact token path".to_string(),
                    ),
                )],
                None,
            ),
            "malformed" => (
                vec![Err(
                    crate::tailscale_serve::TailscaleServeError::InvalidStatus(
                        "malformed handler projection".to_string(),
                    ),
                )],
                None,
            ),
            "unreadable" => (
                vec![Err(
                    crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                        "injected read failure".to_string(),
                    ),
                )],
                None,
            ),
            "off-failed" => (
                vec![
                    exact(),
                    Ok(tailscale_observation(
                        &ownership,
                        ServePathState::Absent,
                        'c',
                    )),
                ],
                Some(
                    crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
                        "injected cleanup failure".to_string(),
                    ),
                ),
            ),
            "post-off-unreadable" => (
                vec![
                    exact(),
                    Err(
                        crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                            "injected read failure".to_string(),
                        ),
                    ),
                ],
                None,
            ),
            "route-shadow-before" => {
                let mut observation =
                    tailscale_observation(&ownership, ServePathState::Absent, 'c');
                observation.route_shadow = Some("/".to_string());
                (vec![Ok(observation)], None)
            }
            "route-shadow-after" => {
                let mut observation =
                    tailscale_observation(&ownership, ServePathState::Absent, 'c');
                observation.route_shadow = Some("/_ferric".to_string());
                (vec![exact(), Ok(observation)], None)
            }
            "version-drift-before" => {
                let mut observation =
                    tailscale_observation(&ownership, ServePathState::Absent, 'c');
                observation.cleanup_semantics_pinned = false;
                (vec![Ok(observation)], None)
            }
            "version-drift-after" => {
                let mut observation =
                    tailscale_observation(&ownership, ServePathState::Absent, 'c');
                observation.cleanup_semantics_pinned = false;
                (
                    vec![exact(), Ok(observation)],
                    Some(
                        crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
                            "exact handler removed under unknown routing semantics".to_string(),
                        ),
                    ),
                )
            }
            _ => unreachable!(),
        };
        let mut serve = ScriptedTailscaleServe::new(observations, Rc::clone(&ledger));
        if let Some(error) = off_failure {
            serve = serve.with_off(Err(error));
        }
        let process = ScriptedProcess::new(pid, "proxy-failure", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: expected.clone(),
                listener: ListenerState::OwnedByTarget,
            }));
        let revision = representative_revision(&capture);
        let mut effects = ScriptedDownEffects::new(
            [ListenerState::Absent],
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        )
        .with_tailscale(serve);

        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected,
                pid,
                port,
                captures: vec![capture],
                expected_revisions: vec![revision],
            },
            &mut effects,
        );

        assert!(!report.success, "{case}");
        assert_eq!(report.disposition, DownDisposition::Failed, "{case}");
        assert!(report.signalled, "{case}");
        assert!(report.exit_proven && report.listener_released, "{case}");
        assert!(
            report.registrations.iter().all(|registration| matches!(
                registration.outcome,
                DownRegistrationOutcome::Held { .. }
            )),
            "{case}"
        );
        let diagnostics = report.diagnostics.join(" ");
        assert!(
            diagnostics.contains(&ownership.fqdn),
            "{case}: {diagnostics}"
        );
        assert!(
            diagnostics.contains(&ownership.mount_path),
            "{case}: {diagnostics}"
        );
        assert!(
            diagnostics.contains(&ownership.remote_base_url),
            "{case}: {diagnostics}"
        );
        if case == "route-shadow-before" {
            assert!(
                diagnostics.contains("effective route / still shadows"),
                "{case}: {diagnostics}"
            );
        }
        if case == "route-shadow-after" {
            assert!(
                diagnostics.contains("effective route /_ferric still shadows"),
                "{case}: {diagnostics}"
            );
        }
        assert!(diagnostics.contains("retry `ferric server down`"));
        let events = ledger.borrow();
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Remove(_, _)))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, LifecycleEvent::Inspect("proxy-failure", _)))
        );
        assert!(events.contains(&LifecycleEvent::Terminate("proxy-failure")));
        let mut expected_events =
            vec![LifecycleEvent::Revalidate, LifecycleEvent::TailscaleObserve];
        if matches!(
            case,
            "off-failed" | "post-off-unreadable" | "route-shadow-after" | "version-drift-after"
        ) {
            expected_events.extend([
                LifecycleEvent::TailscaleOff,
                LifecycleEvent::TailscaleObserve,
            ]);
        }
        expected_events.extend([
            LifecycleEvent::Revalidate,
            LifecycleEvent::Inspect("proxy-failure", port),
            LifecycleEvent::Terminate("proxy-failure"),
            LifecycleEvent::RetainedWait("proxy-failure"),
            LifecycleEvent::Listener(pid, port),
        ]);
        assert_eq!(*events, expected_events, "{case}");
        let expected_diagnostic = match case {
            "replaced" => "now targets",
            "duplicate" => "duplicate exact token path",
            "malformed" => "malformed handler projection",
            "unreadable" => "injected read failure",
            "off-failed" => "reported failure",
            "post-off-unreadable" => "could not prove Tailscale Serve absence",
            "route-shadow-before" | "route-shadow-after" => "still shadows",
            "version-drift-before" | "version-drift-after" => "unknown routing semantics",
            _ => unreachable!(),
        };
        assert!(
            diagnostics.contains(expected_diagnostic),
            "{case}: {diagnostics}"
        );
    }

    for (case, revalidations, expect_proxy_cleanup) in [
        (
            "pre-proxy-revision-change",
            VecDeque::from([Err("registration changed before proxy cleanup".to_string())]),
            false,
        ),
        (
            "post-resource-revision-change",
            VecDeque::from([
                Ok(()),
                Ok(()),
                Err(REGISTRATION_REVISION_CHANGED.to_string()),
            ]),
            true,
        ),
    ] {
        let pid = 6361;
        let (capture, ownership) =
            discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, case);
        let port = capture.runfile.port;
        let expected = capture.runfile.process_identity.clone().unwrap();
        let revision = representative_revision(&capture);
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new(
            if expect_proxy_cleanup {
                vec![
                    Ok(tailscale_observation(
                        &ownership,
                        ServePathState::Proxy {
                            target: ownership.proxy_target.clone(),
                        },
                        'b',
                    )),
                    Ok(tailscale_observation(
                        &ownership,
                        ServePathState::Absent,
                        'c',
                    )),
                ]
            } else {
                Vec::new()
            },
            Rc::clone(&ledger),
        );
        let process = ScriptedProcess::new(pid, "revision-failure", Rc::clone(&ledger))
            .with_inspection(Ok(ProcessFacts {
                identity: expected.clone(),
                listener: ListenerState::OwnedByTarget,
            }));
        let mut effects = ScriptedDownEffects::new(
            if expect_proxy_cleanup {
                vec![ListenerState::Absent]
            } else {
                Vec::new()
            },
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        )
        .with_tailscale(serve);
        effects.revalidations = revalidations;

        let report = execute_down_plan(
            DownPlan::Target {
                process,
                expected,
                pid,
                port,
                captures: vec![capture],
                expected_revisions: vec![revision],
            },
            &mut effects,
        );

        assert_eq!(report.disposition, DownDisposition::Failed, "{case}");
        assert!(report.registrations.iter().all(|registration| matches!(
            registration.outcome,
            DownRegistrationOutcome::Held { .. }
        )));
        let diagnostics = report.diagnostics.join(" ");
        if expect_proxy_cleanup {
            assert!(report.signalled && report.exit_proven && report.listener_released);
            assert!(diagnostics.contains(REGISTRATION_REVISION_CHANGED));
            assert!(
                diagnostics.contains("exact process exit and listener release were already proven")
            );
            assert!(!diagnostics.contains("no process was signalled"));
        } else {
            assert!(diagnostics.contains("registration changed"));
        }
        let events = ledger.borrow();
        if expect_proxy_cleanup {
            assert_eq!(
                *events,
                vec![
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::TailscaleObserve,
                    LifecycleEvent::TailscaleOff,
                    LifecycleEvent::TailscaleObserve,
                    LifecycleEvent::Revalidate,
                    LifecycleEvent::Inspect("revision-failure", port),
                    LifecycleEvent::Terminate("revision-failure"),
                    LifecycleEvent::RetainedWait("revision-failure"),
                    LifecycleEvent::Listener(pid, port),
                    LifecycleEvent::Revalidate,
                ],
                "{case}"
            );
        } else {
            assert_eq!(*events, vec![LifecycleEvent::Revalidate], "{case}");
            assert!(!report.signalled && !report.exit_proven && !report.listener_released);
        }
    }
}

#[test]
fn down_retries_absent_proxy_and_stale_process() {
    let pid = 6371;
    let (capture, ownership) =
        discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, "pending-crash");
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Absent,
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let mut effects = ScriptedDownEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    )
    .with_tailscale(serve);
    let revision = representative_revision(&capture);
    let stale = execute_down_plan(
        DownPlan::<ScriptedProcess>::Stale {
            captures: vec![capture],
            expected_revisions: vec![revision],
        },
        &mut effects,
    );
    assert!(stale.success);
    assert_eq!(stale.disposition, DownDisposition::StaleCleaned);
    assert!(ledger.borrow().iter().all(|event| {
        !matches!(
            event,
            LifecycleEvent::TailscaleOff
                | LifecycleEvent::Terminate(_)
                | LifecycleEvent::Acquire(_)
        )
    }));

    let pid = 6373;
    let (capture, ownership) =
        discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, "version-drift-repeat");
    let port = capture.runfile.port;
    let mut drift_absent = tailscale_observation(&ownership, ServePathState::Absent, 'd');
    drift_absent.cleanup_semantics_pinned = false;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new([Ok(drift_absent)], Rc::clone(&ledger));
    let mut effects = ScriptedDownEffects::new(
        [ListenerState::Absent],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    )
    .with_tailscale(serve);
    let revision = representative_revision(&capture);
    let drift_retry = execute_down_plan(
        DownPlan::<ScriptedProcess>::Stale {
            captures: vec![capture],
            expected_revisions: vec![revision],
        },
        &mut effects,
    );
    assert!(!drift_retry.success);
    assert_eq!(drift_retry.disposition, DownDisposition::Failed);
    assert!(
        drift_retry
            .registrations
            .iter()
            .all(|registration| matches!(
                registration.outcome,
                DownRegistrationOutcome::Held { .. }
            ))
    );
    assert!(
        drift_retry
            .diagnostics
            .join(" ")
            .contains("unknown routing semantics")
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::TailscaleObserve,
            LifecycleEvent::Listener(pid, port),
        ]
    );

    let pid = 6372;
    let (capture, ownership) =
        discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, "already-exited");
    let port = capture.runfile.port;
    let expected = capture.runfile.process_identity.clone().unwrap();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Absent,
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let process = ScriptedProcess::new(pid, "already-exited", Rc::clone(&ledger))
        .with_inspection(Err(ProcessError::NotFound(pid)));
    let mut effects = ScriptedDownEffects::new(
        [ListenerState::Absent],
        [Ok(RemovalOutcome::Removed)],
        Rc::clone(&ledger),
    )
    .with_tailscale(serve);
    let revision = representative_revision(&capture);
    let exited = execute_down_plan(
        DownPlan::Target {
            process,
            expected,
            pid,
            port,
            captures: vec![capture],
            expected_revisions: vec![revision],
        },
        &mut effects,
    );
    assert!(exited.success);
    assert_eq!(exited.disposition, DownDisposition::AlreadyExited);
    assert!(!exited.signalled);
    assert!(ledger.borrow().iter().all(|event| {
        !matches!(
            event,
            LifecycleEvent::TailscaleOff | LifecycleEvent::Terminate(_)
        )
    }));

    let repeated_plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
        managed: discovery_fixture_empty(),
        observations: Vec::new(),
        resolution: Resolution::Empty,
    });
    let repeated_ledger = Rc::new(RefCell::new(Vec::new()));
    let mut repeated = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&repeated_ledger),
    );
    let empty = execute_down_plan(repeated_plan, &mut repeated);
    assert!(empty.success);
    assert_eq!(empty.disposition, DownDisposition::Empty);
    assert!(repeated_ledger.borrow().is_empty());
}

#[test]
fn tailscale_fault_seam_clause_matrix() {
    tailscale_pre_mutation_failures_never_apply();
    tailscale_launch_failure_matrix_holds_or_compensates_exactly();
    status_reports_each_proxy_state();
    down_cleans_proxy_before_process();
    down_proxy_failure_matrix_preserves_journal();
    down_retries_absent_proxy_and_stale_process();
    legacy_tailscale_registration_remains_unowned();
    status_recovery_guidance_respects_native_authority();
    status_never_hides_tailscale_ownership_conflicts();
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
        let mut effects = ScriptedDownEffects::new(
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
        let rendered = render_down_with_ledger(&report, &ledger);
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
        let mut effects = ScriptedDownEffects::new(
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
        let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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

    // Retained wait is the down-process postcondition; the companion
    // spawned-child seam proves that a known-exited owned child is also
    // reaped before a residual listener can turn the result into failure.
    // Invoke it from the frozen E04-B name as required by the locked row.
    stop_managed_child_reaps_proven_exit_before_listener_failure();
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
    let mut stopped_effects = ScriptedDownEffects::new(
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
    let stopped_rendered = render_down_with_ledger(&stopped_report, &stopped_ledger);
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
            ScriptedDownEffects::new([ListenerState::Absent], [outcome], Rc::clone(&ledger));
        let report = execute_down_plan(
            DownPlan::<ScriptedProcess>::Stale {
                captures: vec![capture.clone()],
                expected_revisions: Vec::new(),
            },
            &mut effects,
        );
        let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedDownEffects::new(
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
    let rendered = render_down_with_ledger(&report, &ledger);
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
        "live schema-1 registration",
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
            "live schema-1 registration" => {
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
                    2,
                )
            }
            "two live keys" => {
                let (local, local_raw) =
                    write_runfile_slot(RegistrationScope::Local, &local_path, &local_runfile);
                let mut global_runfile = local_runfile.clone();
                global_runfile.pid = 6451;
                global_runfile.process_identity = Some(discovery_fixture_identity(6451));
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
            let state =
                if blocker == "live schema-1 registration" || blocker == "uninspectable listener" {
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
            let label = registration_label(capture.scope, &capture.path);
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate {
                        scope: capture.scope,
                        path: capture.path.clone(),
                    },
                    runfile: Some(capture.runfile.clone()),
                    state,
                },
                label,
                capture: Some(capture),
                process: None,
            }
        });
        assert!(
            down_mutation_blocker(&discovery.managed.state).is_some(),
            "{blocker} must become a typed mutation blocker through inventory and resolution"
        );
        let plan = down_plan_from_lifecycle(discovery);
        let mut effects = ScriptedDownEffects::new(
            Vec::<ListenerState>::new(),
            Vec::<Result<RemovalOutcome, RemovalError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_down_plan(plan, &mut effects);
        let rendered = render_down_with_ledger(&report, &ledger);
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

#[test]
fn tailscale_blocked_commands_preserve_records_and_never_reset() {
    let root = tempfile::tempdir().unwrap();
    let registration_path = root.path().join("workspace/.ferric/server.json");
    std::fs::create_dir_all(registration_path.parent().unwrap()).unwrap();

    let mut runfile = discovery_fixture_runfile(6490, "tailscale-preserved");
    runfile.tailscale = true;
    runfile.origin_local_runfile = Some(registration_path.clone());
    let mut original = serde_json::to_vec_pretty(&runfile).unwrap();
    original.push(b'\n');
    std::fs::write(&registration_path, &original).unwrap();

    let inventory = RegistrationInventory {
        local: capture_registration_path(RegistrationScope::Local, &registration_path),
        global: None,
        promised_origins: Vec::new(),
    };
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let acquire_ledger = Rc::clone(&ledger);
    let discovery = discover_inventory_before_health_with(inventory, move |_capture| {
        acquire_ledger
            .borrow_mut()
            .push(LifecycleEvent::Acquire(6490));
        panic!("Tailscale registration must block before process acquisition")
    });
    assert!(ledger.borrow().is_empty());

    let status = render_status(&status_report(&discovery.managed));
    let expected_guidance = format!(
        "[next] registration port {} claims durable Tailscale Serve state; scoped proxy cleanup is unavailable, so Ferric will not inspect or signal its PID, delete its registration, invoke Tailscale, or run a blind node-wide reset; inspect and remove only that exact Serve endpoint with Tailscale tooling",
        runfile.port
    );
    assert_eq!(status.stdout.last(), Some(&expected_guidance));
    assert!(!status.success);
    assert_eq!(std::fs::read(&registration_path).unwrap(), original);

    let plan = down_plan_from_lifecycle(discovery);
    let mut effects = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_down_plan(plan, &mut effects);
    let rendered = render_down_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, DownDisposition::Blocked);
    assert!(!report.success);
    assert!(!report.signalled);
    assert!(!report.exit_proven);
    assert_eq!(rendered.stdout.last(), Some(&expected_guidance));
    assert!(rendered.stdout.iter().all(|line| !line.contains("stopped")));
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::Render]);
    assert_eq!(std::fs::read(&registration_path).unwrap(), original);
}

#[test]
fn live_v1_guidance_and_explicit_adoption() {
    let pid = 6501;
    let (captures, facts) = legacy_adoption_fixture(pid);
    let adopted_raw = adoption_fixture_replacement_raw(&captures, &facts);
    let coordinates = captures
        .iter()
        .map(|capture| RegistrationCoordinate {
            scope: capture.scope,
            path: capture.path.clone(),
        })
        .collect::<Vec<_>>();
    let issues = vec![ResolutionIssue {
        coordinates: coordinates.clone(),
        kind: ResolutionIssueKind::Unverifiable,
        detail: format!(
            "live schema-1 PID {pid} has no creation identity and cannot authorize teardown"
        ),
    }];
    let inventory = RegistrationInventory {
        local: RegistrationSlot::Captured(Box::new(captures[0].clone())),
        global: Some(RegistrationSlot::Captured(Box::new(captures[1].clone()))),
        promised_origins: Vec::new(),
    };
    let managed_observations = captures
        .iter()
        .enumerate()
        .map(|(index, capture)| ManagedRegistrationObservation {
            id: ObservationId(index),
            coordinate: coordinates[index].clone(),
            promised: None,
            state: ManagedRegistrationState::Captured {
                runfile: Box::new(capture.runfile.clone()),
                raw_sha256: format!("legacy-{index}"),
                runtime: RuntimeObservation::LegacyLive { pid },
            },
        })
        .collect::<Vec<_>>();
    let managed = ManagedServerDiscovery {
        inventory,
        observations: managed_observations,
        state: ManagedServerState::Unverifiable {
            issues: issues.clone(),
        },
    };
    let rendered_status = render_status(&status_report(&managed));
    let expected_command = format!("ferric server adopt --pid {pid}");
    let expected_status_guidance = format!(
        "[next] verify and record the live legacy process without signalling it: `{expected_command}`"
    );
    assert_eq!(rendered_status.stdout.len(), managed.observations.len() + 2);
    assert_eq!(
        &rendered_status.stdout[..managed.observations.len()],
        managed
            .observations
            .iter()
            .map(expected_registration_status)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        rendered_status.stdout.last(),
        Some(&expected_status_guidance)
    );

    // The E04-E trigger is one live legacy registration, not only a
    // mirrored pair. Exercise a real local capture so both status and down
    // must preserve its exact bytes while rendering the complete command.
    let local_only_root = tempfile::tempdir().unwrap();
    let mut local_only_capture = captures[0].clone();
    local_only_capture.path = local_only_root.path().join("workspace/.ferric/server.json");
    std::fs::create_dir_all(local_only_capture.path.parent().unwrap()).unwrap();
    std::fs::write(&local_only_capture.path, &local_only_capture.raw).unwrap();
    let local_only_coordinate = RegistrationCoordinate {
        scope: RegistrationScope::Local,
        path: local_only_capture.path.clone(),
    };
    let local_only_detail =
        format!("live schema-1 PID {pid} has no creation identity and cannot authorize teardown");
    let local_only_issues = vec![ResolutionIssue {
        coordinates: vec![local_only_coordinate.clone()],
        kind: ResolutionIssueKind::Unverifiable,
        detail: local_only_detail.clone(),
    }];
    let local_only_managed = ManagedServerDiscovery {
        inventory: RegistrationInventory {
            local: RegistrationSlot::Captured(Box::new(local_only_capture.clone())),
            global: None,
            promised_origins: Vec::new(),
        },
        observations: vec![ManagedRegistrationObservation {
            id: ObservationId(0),
            coordinate: local_only_coordinate.clone(),
            promised: None,
            state: ManagedRegistrationState::Captured {
                runfile: Box::new(local_only_capture.runfile.clone()),
                raw_sha256: ferric_bench::sha256_bytes(&local_only_capture.raw),
                runtime: RuntimeObservation::LegacyLive { pid },
            },
        }],
        state: ManagedServerState::Unverifiable {
            issues: local_only_issues.clone(),
        },
    };
    assert_status_matrix_row(
        &local_only_managed,
        StatusNextAction::AdoptLegacy { pid },
        "[state] unverifiable",
        &expected_status_guidance,
        &[format!("[diagnostic] {local_only_detail}")],
        false,
    );
    assert_eq!(
        std::fs::read(&local_only_capture.path).unwrap(),
        local_only_capture.raw
    );

    let local_only_plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
        managed: local_only_managed,
        observations: Vec::new(),
        resolution: Resolution::Unverifiable {
            issues: local_only_issues,
        },
    });
    let local_only_ledger = Rc::new(RefCell::new(Vec::new()));
    let mut local_only_effects = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&local_only_ledger),
    );
    let local_only_report = execute_down_plan(local_only_plan, &mut local_only_effects);
    let local_only_rendered = render_down_with_ledger(&local_only_report, &local_only_ledger);
    assert_eq!(local_only_report.disposition, DownDisposition::Blocked);
    assert!(!local_only_report.signalled);
    assert_eq!(local_only_report.registrations.len(), 1);
    assert_eq!(
        local_only_report.registrations[0].coordinate,
        local_only_coordinate
    );
    assert!(matches!(
        local_only_report.registrations[0].outcome,
        DownRegistrationOutcome::Held { .. }
    ));
    assert_eq!(
        local_only_rendered.stdout,
        vec![
            format!(
                "[held] local registration {} detail=typed discovery blocked teardown mutation",
                local_only_capture.path.display()
            ),
            "[state] teardown blocked; registrations kept".to_string(),
            format!("[next] {expected_command}"),
        ]
    );
    assert_eq!(
        local_only_rendered.stderr,
        vec![format!("[diagnostic] {local_only_detail}")]
    );
    assert_eq!(*local_only_ledger.borrow(), vec![LifecycleEvent::Render]);
    assert_eq!(
        std::fs::read(&local_only_capture.path).unwrap(),
        local_only_capture.raw
    );

    let mut global_only = managed.clone();
    global_only.inventory.local = RegistrationSlot::Absent {
        scope: coordinates[0].scope,
        path: coordinates[0].path.clone(),
    };
    global_only
        .observations
        .retain(|observation| observation.coordinate.scope == RegistrationScope::Global);
    assert!(matches!(
        status_report(&global_only).next_action,
        StatusNextAction::RepairUnverifiable { .. }
    ));

    let plan = down_plan_from_lifecycle(LifecycleDiscovery::<ScriptedProcess> {
        managed,
        observations: Vec::new(),
        resolution: Resolution::Unverifiable { issues },
    });
    let down_ledger = Rc::new(RefCell::new(Vec::new()));
    let mut down_effects = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&down_ledger),
    );
    let down_report = execute_down_plan(plan, &mut down_effects);
    let rendered_down = render_down_with_ledger(&down_report, &down_ledger);
    assert_eq!(down_report.disposition, DownDisposition::Blocked);
    assert_eq!(down_report.registrations.len(), 2);
    assert!(
        down_report
            .registrations
            .iter()
            .all(|registration| matches!(
                registration.outcome,
                DownRegistrationOutcome::Held { .. }
            ))
    );
    assert!(
        rendered_down.stdout[..2]
            .iter()
            .zip(&coordinates)
            .all(|(line, coordinate)| line.contains(&coordinate.path.display().to_string()))
    );
    assert_eq!(
        rendered_down.stdout.last(),
        Some(&format!("[next] {expected_command}"))
    );
    assert_eq!(*down_ledger.borrow(), vec![LifecycleEvent::Render]);

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "legacy-generation", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_inspection(Ok(facts.clone()))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        [
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Replaced),
        ],
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    let rendered = render_adoption_with_ledger(&report, &ledger);

    assert!(report.success);
    assert_eq!(report.disposition, AdoptionDisposition::Adopted);
    assert!(report.identity_validated);
    assert!(report.listener_validated);
    assert!(report.final_generation_revalidated);
    assert!(report.registrations.iter().all(|registration| {
        matches!(registration.transition, AdoptionAliasTransition::Adopted)
            && registration.rollback.is_none()
    }));
    assert!(
        rendered
            .stdout
            .iter()
            .any(|line| line.contains("without signalling"))
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::Inspect("legacy-generation", captures[0].runfile.port),
            LifecycleEvent::RetainedWait("legacy-generation"),
            scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
            scripted_replace_event(&captures[1].path, &captures[1].raw, &adopted_raw),
            LifecycleEvent::Inspect("legacy-generation", captures[0].runfile.port),
            LifecycleEvent::Render,
        ]
    );

    // The exact E04-E acceptance name must execute the negative
    // executable/argv/listener, conditional replacement, final-generation
    // and rollback rows; differently named focused tests remain useful but
    // cannot carry this frozen row on their own.
    legacy_adoption_coordinates_require_closed_engine_and_every_recorded_value();
    legacy_adoption_transition_and_rollback_matrix();
}

#[test]
fn legacy_adoption_transition_and_rollback_matrix() {
    let pid = 6601;
    let (captures, facts) = legacy_adoption_fixture(pid);
    let adopted_raw = adoption_fixture_replacement_raw(&captures, &facts);
    let port = captures[0].runfile.port;

    let blocked_rows = [
        (
            "executable",
            {
                let mut changed = facts.clone();
                changed.identity.executable = if cfg!(windows) {
                    PathBuf::from(r"C:\fixture\python.exe")
                } else {
                    PathBuf::from("/fixture/python")
                };
                changed
            },
            "closed",
        ),
        (
            "argv",
            {
                let mut changed = facts.clone();
                changed
                    .identity
                    .argv
                    .extend(["--port".to_string(), (port + 1).to_string()]);
                changed
            },
            "conflicting registered port",
        ),
        (
            "listener",
            {
                let mut changed = facts.clone();
                changed.listener = ListenerState::OwnedByTargetWildcard;
                changed
            },
            "not exclusively owned",
        ),
        (
            "listener-absent",
            {
                let mut changed = facts.clone();
                changed.listener = ListenerState::Absent;
                changed
            },
            "not exclusively owned",
        ),
        (
            "listener-foreign",
            {
                let mut changed = facts.clone();
                changed.listener = ListenerState::OwnedByOther(vec![9999]);
                changed
            },
            "not exclusively owned",
        ),
        (
            "listener-uninspectable",
            {
                let mut changed = facts.clone();
                changed.listener =
                    ListenerState::Uninspectable("listener table denied".to_string());
                changed
            },
            "not exclusively owned",
        ),
    ];
    for (case, blocked_facts, expected_fragment) in blocked_rows {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process =
            ScriptedProcess::new(pid, case, Rc::clone(&ledger)).with_inspection(Ok(blocked_facts));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Blocked, "{case}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains(expected_fragment)),
            "{case}: {:?}",
            report.diagnostics
        );
        assert!(
            rendered
                .stdout
                .iter()
                .all(|line| !line.contains("adopted live"))
        );
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect(case, port),
                LifecycleEvent::Render,
            ],
            "{case} must not wait, replace, rollback, or signal"
        );
    }

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let runtime = ScriptedRuntime::new(
        Err("retained handle unavailable".to_string()),
        Rc::clone(&ledger),
    );
    let mut effects = ScriptedAdoptionEffects::new(
        Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::Blocked);
    assert_eq!(
        *ledger.borrow(),
        vec![LifecycleEvent::Acquire(pid), LifecycleEvent::Render]
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "inspect-error", Rc::clone(&ledger)).with_inspection(
        Err(ProcessError::Operation("inspection denied".to_string())),
    );
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::Blocked);
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::Inspect("inspect-error", port),
            LifecycleEvent::Render,
        ]
    );

    for (case, wait) in [
        ("exited-during-validation", Ok(true)),
        (
            "wait-error",
            Err(ProcessError::Operation("wait denied".to_string())),
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(wait);
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Blocked, "{case}");
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect(case, port),
                LifecycleEvent::RetainedWait(case),
                LifecycleEvent::Render,
            ],
            "{case}"
        );
    }

    for failed_index in 0..captures.len() {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, "alias-failure", Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut outcomes = (0..failed_index)
            .map(|_| Ok(ReplacementOutcome::Replaced))
            .collect::<Vec<_>>();
        outcomes.push(Ok(ReplacementOutcome::Absent));
        outcomes.extend((0..failed_index).map(|_| Ok(ReplacementOutcome::Replaced)));
        let mut effects = ScriptedAdoptionEffects::new(outcomes, Rc::clone(&ledger));
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);
        assert!(!report.success);
        let expected_disposition = if failed_index == 0 {
            AdoptionDisposition::Failed
        } else {
            AdoptionDisposition::RecoveryPartial
        };
        assert_eq!(
            report.disposition, expected_disposition,
            "alias {failed_index} is not proven restored at its original path"
        );
        let expected_state = if failed_index == 0 {
            "[state] adoption failed before any committed replacement"
        } else {
            "[state] adoption failed; recovery partial"
        };
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some(expected_state),
            "alias {failed_index}"
        );
        assert!(matches!(
            report.registrations[failed_index].transition,
            AdoptionAliasTransition::Absent
        ));
        for earlier in &report.registrations[..failed_index] {
            assert_eq!(
                earlier.rollback,
                Some(AdoptionRollbackOutcome::LegacyRestored)
            );
        }
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
            "alias {failed_index}"
        );
    }

    let holding = discovery_fixture_path("adoption-holding");
    for (case, outcome, marker) in [
        (
            "forward-replacement",
            Ok(ReplacementOutcome::ReplacementPreserved {
                path: holding.clone(),
                detail: "concurrent replacement retained".to_string(),
            }),
            "replacement-preserved",
        ),
        (
            "forward-error",
            Err(ReplacementError {
                path: captures[0].path.clone(),
                detail: "replacement publish failed".to_string(),
                preserved_at: Some(holding.clone()),
                replacement_committed: false,
            }),
            "replace-failed",
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new([outcome], Rc::clone(&ledger));
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);
        assert_eq!(report.disposition, AdoptionDisposition::Failed, "{case}");
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some("[state] adoption failed before any committed replacement"),
            "{case}"
        );
        let failed_row = rendered
            .stdout
            .iter()
            .find(|line| line.contains(marker))
            .unwrap();
        assert!(failed_row.contains(&holding.display().to_string()));
        assert_eq!(
            *ledger.borrow(),
            vec![
                LifecycleEvent::Acquire(pid),
                LifecycleEvent::Inspect(case, port),
                LifecycleEvent::RetainedWait(case),
                scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
                LifecycleEvent::Render,
            ],
            "{case}"
        );
    }

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut changed_facts = facts.clone();
    changed_facts.identity.start_token = canonical_test_start_token(9999);
    let process = ScriptedProcess::new(pid, "identity-transition", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_inspection(Ok(changed_facts))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        [
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Replaced),
        ],
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::RolledBack);
    assert!(report.registrations.iter().all(|registration| {
        registration.rollback == Some(AdoptionRollbackOutcome::LegacyRestored)
    }));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::Inspect("identity-transition", port),
            LifecycleEvent::RetainedWait("identity-transition"),
            scripted_replace_event(&captures[0].path, &captures[0].raw, &adopted_raw),
            scripted_replace_event(&captures[1].path, &captures[1].raw, &adopted_raw),
            LifecycleEvent::Inspect("identity-transition", port),
            scripted_replace_event(&captures[1].path, &adopted_raw, &captures[1].raw),
            scripted_replace_event(&captures[0].path, &adopted_raw, &captures[0].raw),
            LifecycleEvent::Render,
        ],
        "final generation failure must rollback in reverse order"
    );

    for (case, final_inspection) in [
        (
            "final-inspect-error",
            Err(ProcessError::Operation(
                "final inspection denied".to_string(),
            )),
        ),
        (
            "final-listener-change",
            Ok(ProcessFacts {
                identity: facts.identity.clone(),
                listener: ListenerState::OwnedByOther(vec![9999]),
            }),
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_inspection(final_inspection)
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Replaced),
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        render_adoption_with_ledger(&report, &ledger);
        assert_eq!(
            report.disposition,
            AdoptionDisposition::RolledBack,
            "{case}"
        );
        assert!(report.registrations.iter().all(|registration| {
            registration.rollback == Some(AdoptionRollbackOutcome::LegacyRestored)
        }));
        assert_eq!(
            ledger
                .borrow()
                .iter()
                .filter(|event| matches!(event, LifecycleEvent::Replace(_, _, _)))
                .count(),
            4,
            "{case}"
        );
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
            "{case}"
        );
    }

    for (case, rollback, expected_rollback) in [
        (
            "concurrent",
            Ok(ReplacementOutcome::ReplacementPreserved {
                path: holding.clone(),
                detail: "concurrent winner kept".to_string(),
            }),
            "replacement-preserved",
        ),
        (
            "rollback-error",
            Err(ReplacementError {
                path: captures[0].path.clone(),
                detail: "rollback durability failed".to_string(),
                preserved_at: Some(holding.clone()),
                replacement_committed: false,
            }),
            "failed",
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(pid, case, Rc::clone(&ledger))
            .with_inspection(Ok(facts.clone()))
            .with_wait(Ok(false));
        let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
        let mut effects = ScriptedAdoptionEffects::new(
            [
                Ok(ReplacementOutcome::Replaced),
                Ok(ReplacementOutcome::Absent),
                rollback,
            ],
            Rc::clone(&ledger),
        );
        let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
        let rendered = render_adoption_with_ledger(&report, &ledger);
        assert_eq!(
            report.disposition,
            AdoptionDisposition::RecoveryPartial,
            "{case}"
        );
        assert_eq!(
            rendered.stdout.last().map(String::as_str),
            Some("[state] adoption failed; recovery partial"),
            "{case}"
        );
        let local_row = rendered
            .stdout
            .iter()
            .find(|line| line.contains(&captures[0].path.display().to_string()))
            .unwrap();
        assert!(local_row.contains(expected_rollback), "{case}: {local_row}");
        assert!(
            local_row.contains(&holding.display().to_string()),
            "{case}: {local_row}"
        );
        assert!(
            ledger
                .borrow()
                .iter()
                .all(|event| !matches!(event, LifecycleEvent::Terminate(_))),
            "{case}"
        );
    }

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "rollback-absent", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        [
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Absent),
            Ok(ReplacementOutcome::Absent),
        ],
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    let rendered = render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::RecoveryPartial);
    assert_eq!(
        report.registrations[0].rollback,
        Some(AdoptionRollbackOutcome::Absent)
    );
    let restored_row = rendered
        .stdout
        .iter()
        .find(|line| line.contains(&captures[0].path.display().to_string()))
        .unwrap();
    assert!(restored_row.contains("rollback=absent"), "{restored_row}");
    assert_eq!(
        rendered.stdout.last().map(String::as_str),
        Some("[state] adoption failed; recovery partial")
    );
    assert!(
        ledger
            .borrow()
            .iter()
            .all(|event| !matches!(event, LifecycleEvent::Terminate(_)))
    );

    let mut same_path_alias = captures[0].clone();
    same_path_alias.scope = RegistrationScope::Global;
    let same_path_captures = vec![captures[0].clone(), same_path_alias.clone()];
    assert_eq!(adoption_mutation_groups(&same_path_captures).len(), 1);
    let alias_root = tempfile::tempdir().unwrap();
    std::fs::create_dir(alias_root.path().join("nested")).unwrap();
    let alias_file = alias_root.path().join("registration.json");
    std::fs::write(&alias_file, &captures[0].raw).unwrap();
    let mut direct_alias = captures[0].clone();
    direct_alias.path = alias_file.clone();
    let mut lexical_alias = same_path_alias.clone();
    lexical_alias.path = alias_root
        .path()
        .join("nested")
        .join("..")
        .join(alias_file.file_name().unwrap());
    assert!(
        validate_mutation_path_aliases(&[direct_alias, lexical_alias]).is_err(),
        "distinct path spellings of one entry must block rather than collapse mutation reports"
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "same-path", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_inspection(Ok(facts.clone()))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects =
        ScriptedAdoptionEffects::new([Ok(ReplacementOutcome::Replaced)], Rc::clone(&ledger));
    let report = execute_legacy_adoption(same_path_captures.clone(), pid, &runtime, &mut effects);
    render_adoption_with_ledger(&report, &ledger);
    assert!(report.success);
    assert_eq!(report.registrations.len(), 2);
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Acquire(pid),
            LifecycleEvent::Inspect("same-path", port),
            LifecycleEvent::RetainedWait("same-path"),
            scripted_replace_event(
                &same_path_captures[0].path,
                &same_path_captures[0].raw,
                &adopted_raw,
            ),
            LifecycleEvent::Inspect("same-path", port),
            LifecycleEvent::Render,
        ]
    );

    same_path_alias.raw.push(b' ');
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "conflicting-path-token", Rc::clone(&ledger));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        Vec::<Result<ReplacementOutcome, ReplacementError>>::new(),
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(
        vec![captures[0].clone(), same_path_alias],
        pid,
        &runtime,
        &mut effects,
    );
    render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::Blocked);
    assert_eq!(
        *ledger.borrow(),
        vec![LifecycleEvent::Render],
        "conflicting tokens for one physical path must block before acquisition"
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "committed-error", Rc::clone(&ledger))
        .with_inspection(Ok(facts))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut effects = ScriptedAdoptionEffects::new(
        [
            Ok(ReplacementOutcome::Replaced),
            Err(ReplacementError {
                path: captures[1].path.clone(),
                detail: "directory sync failed after commit".to_string(),
                preserved_at: Some(holding),
                replacement_committed: true,
            }),
            Ok(ReplacementOutcome::Replaced),
            Ok(ReplacementOutcome::Replaced),
        ],
        Rc::clone(&ledger),
    );
    let report = execute_legacy_adoption(captures.clone(), pid, &runtime, &mut effects);
    render_adoption_with_ledger(&report, &ledger);
    assert_eq!(report.disposition, AdoptionDisposition::RolledBack);
    assert!(matches!(
        report.registrations[1].transition,
        AdoptionAliasTransition::ReplaceFailed {
            replacement_committed: true,
            ..
        }
    ));
    assert_eq!(
        report.registrations[1].rollback,
        Some(AdoptionRollbackOutcome::LegacyRestored)
    );
}

#[test]
fn registered_consumer_effect_revalidates_retained_generation_on_every_outcome() {
    let runfile = discovery_fixture_runfile(4101, "consumer-effect");
    let before = ProcessFacts {
        identity: runfile.process_identity.clone().unwrap(),
        listener: ListenerState::OwnedByTarget,
    };

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(4101, "effect-error", ledger.clone())
        .with_inspection(Ok(before.clone()))
        .with_inspection(Ok(before.clone()));
    let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
    let error = bracket_registered_effect_with(&runtime, &runfile, || {
        ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
        Err::<(), _>("scripted HTTP failure".to_string())
    })
    .unwrap_err();
    assert_eq!(error, "scripted HTTP failure");
    assert_eq!(
        ledger.borrow().as_slice(),
        &[
            LifecycleEvent::Acquire(4101),
            LifecycleEvent::Inspect("effect-error", 7101),
            LifecycleEvent::ConsumerHttp,
            LifecycleEvent::Inspect("effect-error", 7101),
        ]
    );

    for (label, after, expected_error) in [
        (
            "identity-change",
            ProcessFacts {
                identity: discovery_fixture_identity(4102),
                listener: ListenerState::OwnedByTarget,
            },
            "changed process identity",
        ),
        (
            "listener-change",
            ProcessFacts {
                identity: before.identity.clone(),
                listener: ListenerState::OwnedByTargetWildcard,
            },
            "wildcard/public listener",
        ),
    ] {
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let process = ScriptedProcess::new(4101, label, ledger.clone())
            .with_inspection(Ok(before.clone()))
            .with_inspection(Ok(after));
        let runtime = ScriptedRuntime::new(Ok(process), ledger.clone());
        let error = bracket_registered_effect_with(&runtime, &runfile, || {
            ledger.borrow_mut().push(LifecycleEvent::ConsumerHttp);
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains(expected_error), "{label}: {error}");
        assert_eq!(ledger.borrow()[2], LifecycleEvent::ConsumerHttp);
        assert!(matches!(
            ledger.borrow()[3],
            LifecycleEvent::Inspect(_, 7101)
        ));
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
    let mut effects = ScriptedPublicationEffects::new(
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
    let rendered = render_publication_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedPublicationEffects::new(
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
    render_publication_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedPublicationEffects::new(
        [Ok(RemovalOutcome::Removed)],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let report =
        complete_publication_with(&mut child, &process, port, failure, &listener, &mut effects);
    render_publication_with_ledger(&report, &ledger);
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
    let mut effects = ScriptedPublicationEffects::new(
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
    render_publication_with_ledger(&report, &ledger);
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
        let mut effects = ScriptedPublicationEffects::new(
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
        let rendered = render_publication_with_ledger(&report, &ledger);
        assert_eq!(
            report.disposition,
            PublicationDisposition::RecoveryHeld,
            "{case}"
        );
        assert_eq!(report.finals.len(), 1, "{case}");
        assert_eq!(
            report.finals[0].coordinate,
            RegistrationCoordinate {
                scope: held_local.scope,
                path: held_local.path.clone(),
            },
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
    let mut effects = ScriptedPublicationEffects::new(
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
    let rendered = render_publication_with_ledger(&report, &ledger);
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
    let lifecycle = discover_inventory_with(
        inventory,
        |capture| {
            let identity = capture.runfile.process_identity.clone().unwrap();
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate {
                        scope: capture.scope,
                        path: capture.path.clone(),
                    },
                    runfile: Some(capture.runfile.clone()),
                    state: CandidateState::Verified {
                        identity,
                        listener: ListenerState::OwnedByTarget,
                        health: HealthState::NotProbed,
                    },
                },
                label: registration_label(capture.scope, &capture.path),
                capture: Some(capture),
                process: None,
            }
        },
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
    assert!(crate::backend::require_managed_endpoint(scope.clone(), managed.clone(), None).is_ok());
    assert_eq!(
        crate::autonomy_cmd::require_matching_pre_health_discovery(&managed, &server.fingerprint,)
            .unwrap()
            .fingerprint,
        server.fingerprint,
        "strict autonomy must consume the same typed two-scope discovery"
    );
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
    let mut down_effects = FilesystemDownEffects {
        scope,
        listeners: VecDeque::from([ListenerState::Absent]),
        ledger: Rc::clone(&ledger),
    };
    let report = execute_down_plan(plan, &mut down_effects);
    let down = render_down_with_ledger(&report, &ledger);
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
    let lifecycle = discover_inventory_with(
        inventory_runfiles(&workspace, scope.global.clone()),
        |capture| {
            let observed_identity = capture.runfile.process_identity.clone().unwrap();
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate {
                        scope: capture.scope,
                        path: capture.path.clone(),
                    },
                    runfile: Some(capture.runfile.clone()),
                    state: CandidateState::Verified {
                        identity: observed_identity,
                        listener: ListenerState::OwnedByTarget,
                        health: HealthState::NotProbed,
                    },
                },
                label: registration_label(capture.scope, &capture.path),
                capture: Some(capture),
                process: None,
            }
        },
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
                identity: discovery_fixture_identity(u64::from(pid) + 1),
                listener: ListenerState::OwnedByTarget,
            }),
        ),
    );
    let replacement = discovery_fixture_path("transition-replacement");
    let mut effects = RevisionCheckingDownEffects {
        scope: scope.clone(),
        listeners: VecDeque::from([ListenerState::Absent]),
        removals: VecDeque::from([
            Ok(RemovalOutcome::Removed),
            Ok(RemovalOutcome::ReplacementPreserved {
                path: replacement.clone(),
                detail: "concurrent alias replacement".to_string(),
            }),
        ]),
        ledger: Rc::clone(&ledger),
    };
    let plan = retained_target_down_plan(
        &managed.state,
        retained,
        captures.clone(),
        revisions.clone(),
    )
    .unwrap();
    let report = execute_down_plan(plan, &mut effects);
    let rendered = render_down_with_ledger(&report, &ledger);
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
        let mut effects = ScriptedDownEffects::new(
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
        let rendered = render_down_with_ledger(&report, &ledger);
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

fn scripted_tailscale_launch_case(
    root: &Path,
    label: &str,
    pid: u32,
    port: u16,
    serve: &ScriptedTailscaleServe,
    inspections: impl IntoIterator<Item = Result<ProcessFacts, ProcessError>>,
) -> (
    Result<LaunchOrchestrationSuccess, LaunchOrchestrationError>,
    EventLedger,
    PathBuf,
) {
    scripted_tailscale_launch_case_with(
        root,
        label,
        pid,
        port,
        serve,
        inspections,
        std::iter::repeat_with(|| Ok(None)).take(8),
        [true],
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn scripted_tailscale_launch_case_with(
    root: &Path,
    label: &str,
    pid: u32,
    port: u16,
    serve: &ScriptedTailscaleServe,
    inspections: impl IntoIterator<Item = Result<ProcessFacts, ProcessError>>,
    child_try_wait: impl IntoIterator<Item = Result<Option<ScriptedExit>, String>>,
    health_results: impl IntoIterator<Item = bool>,
    publication_error: Option<PublishError>,
) -> (
    Result<LaunchOrchestrationSuccess, LaunchOrchestrationError>,
    EventLedger,
    PathBuf,
) {
    let workspace = root.join(label);
    let ledger = Rc::clone(&serve.ledger);
    let mut process = ScriptedProcess::new(pid, "tailscale-generation", Rc::clone(&ledger));
    for inspection in inspections {
        process = process.with_inspection(inspection);
    }
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
    let mut health = ScriptedHealth {
        results: health_results.into_iter().collect(),
        ledger: Rc::clone(&ledger),
    };
    let mut clock = ScriptedClock {
        now: Instant::now(),
        ledger: Rc::clone(&ledger),
    };
    let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
    let mut compensation = FilesystemPublicationEffects {
        ledger: Rc::clone(&ledger),
    };
    let mut launch_cfg = cfg(Engine::LlamaServer);
    launch_cfg.port = port;
    launch_cfg.tailscale = true;
    let ownership = tailscale_ownership(port);
    let spawn_ledger = Rc::clone(&ledger);
    let result = orchestrate_launch_with(
        &workspace,
        None,
        &launch_cfg,
        Some(ownership),
        serve,
        move || {
            spawn_ledger.borrow_mut().push(LifecycleEvent::Spawn(pid));
            Ok(ScriptedChild::new(
                pid,
                child_try_wait,
                Rc::clone(&spawn_ledger),
            ))
        },
        &runtime,
        &listener,
        &mut health,
        &mut clock,
        |workspace, global, runfile| {
            if let Some(error) = publication_error {
                Err(error)
            } else {
                publish_mirrored_with(workspace, global, runfile, &mut persistence)
            }
        },
        &mut compensation,
    );
    (result, ledger, runfile_path(&workspace))
}

fn exact_launch_facts(pid: u32) -> ProcessFacts {
    ProcessFacts {
        identity: discovery_fixture_identity(u64::from(pid)),
        listener: ListenerState::OwnedByTarget,
    }
}

#[test]
fn tailscale_launch_orders_journal_before_apply() {
    let root = tempfile::tempdir().unwrap();
    let pid = 4521;
    let port = 9521;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "ordered",
        pid,
        port,
        &serve,
        [
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts),
        ],
    );
    let launched = result.unwrap();
    assert_eq!(
        launched.remote_base_url.as_deref(),
        Some(ownership.remote_base_url.as_str())
    );
    let mut confirmed_ownership = ownership.clone();
    confirmed_ownership.before_status_sha256 = "b".repeat(64);
    confirmed_ownership.apply_confirmed = true;
    assert_eq!(
        launched.published.local.runfile.tailscale_serve,
        Some(confirmed_ownership)
    );
    assert!(journal.exists());
    let events = ledger.borrow();
    let health = events
        .iter()
        .position(|event| *event == LifecycleEvent::Health(port))
        .unwrap();
    let journal = events
        .iter()
        .position(|event| {
            matches!(
                event,
                LifecycleEvent::Persistence(PersistencePhase::PersistNoClobber, _)
            )
        })
        .unwrap();
    let observes = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (*event == LifecycleEvent::TailscaleObserve).then_some(index))
        .collect::<Vec<_>>();
    let apply = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleApply)
        .unwrap();
    let final_inspect = events
        .iter()
        .rposition(|event| *event == LifecycleEvent::Inspect("tailscale-generation", port))
        .unwrap();
    let confirmation = events
        .iter()
        .rposition(|event| matches!(event, LifecycleEvent::Replace(_, _, _)))
        .unwrap();
    assert_eq!(observes.len(), 2);
    assert!(health < journal && journal < observes[0]);
    assert!(
        observes[0] < apply
            && apply < observes[1]
            && observes[1] < confirmation
            && confirmation < final_inspect
    );
}

#[test]
fn tailscale_launch_tolerates_unrelated_prestate_drift() {
    let root = tempfile::tempdir().unwrap();
    let pid = 4522;
    let port = 9522;
    let ownership = tailscale_ownership(port);
    assert_ne!(ownership.before_status_sha256, "b".repeat(64));
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, _, journal) = scripted_tailscale_launch_case(
        root.path(),
        "unrelated-drift",
        pid,
        port,
        &serve,
        [
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts.clone()),
            Ok(facts),
        ],
    );
    assert!(
        result.is_ok(),
        "whole-status digest drift is provenance only"
    );
    assert!(journal.exists());
}

#[test]
fn tailscale_pre_mutation_failures_never_apply() {
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new([], Rc::clone(&ledger));
    let engine_spawns = Rc::new(RefCell::new(0_u8));
    let spawn_marker = Rc::clone(&engine_spawns);
    let entropy = with_prepared_tailscale_launch(
        true,
        9523,
        &serve,
        || Err("injected entropy failure".to_string()),
        move |_ownership| {
            *spawn_marker.borrow_mut() += 1;
        },
    );
    assert!(entropy.unwrap_err().contains("injected entropy failure"));
    assert!(ledger.borrow().is_empty());
    assert_eq!(*engine_spawns.borrow(), 0);

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new([], Rc::clone(&ledger));
    *serve.identities.borrow_mut() = VecDeque::from([Err(
        crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
            "injected identity capture".to_string(),
        ),
    )]);
    let engine_spawns = Rc::new(RefCell::new(0_u8));
    let spawn_marker = Rc::clone(&engine_spawns);
    let identity = with_prepared_tailscale_launch(
        true,
        9523,
        &serve,
        || Ok("00112233445566778899aabbccddeeff".to_string()),
        move |_ownership| {
            *spawn_marker.borrow_mut() += 1;
        },
    );
    assert!(identity.unwrap_err().contains("injected identity capture"));
    assert_eq!(*ledger.borrow(), vec![LifecycleEvent::TailscaleIdentity]);
    assert_eq!(*engine_spawns.borrow(), 0);

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Err(
            crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                "injected read failure".to_string(),
            ),
        )],
        Rc::clone(&ledger),
    );
    let capture = prepare_tailscale_ownership_with(9523, &serve, || {
        Ok("00112233445566778899aabbccddeeff".to_string())
    });
    assert!(
        capture
            .unwrap_err()
            .contains("proposed Tailscale Serve path")
    );
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::TailscaleIdentity,
            LifecycleEvent::TailscaleObserve,
        ]
    );

    let port = 9523;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Proxy {
                target: "http://127.0.0.1:9999".to_string(),
            },
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let collision = prepare_tailscale_ownership_with(port, &serve, || Ok(ownership.token.clone()));
    assert!(collision.unwrap_err().contains("already claimed"));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::TailscaleIdentity,
            LifecycleEvent::TailscaleObserve,
        ]
    );

    let root = tempfile::tempdir().unwrap();
    let pid = 4540;
    let port = 9540;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new([], Rc::clone(&ledger));
    let facts = exact_launch_facts(pid);
    let (readiness, ledger, journal) = scripted_tailscale_launch_case_with(
        root.path(),
        "readiness-failure",
        pid,
        port,
        &serve,
        [Ok(facts)],
        [Ok(None), Ok(Some(ScriptedExit("injected readiness exit")))],
        [],
        None,
    );
    assert!(matches!(
        readiness,
        Err(LaunchOrchestrationError::Readiness { .. })
    ));
    assert!(!journal.exists());
    assert!(ledger.borrow().iter().all(|event| !matches!(
        event,
        LifecycleEvent::TailscaleApply
            | LifecycleEvent::TailscaleOff
            | LifecycleEvent::Persistence(_, _)
    )));

    for (label, inspection) in [
        (
            "identity-inspection-failure",
            Err(ProcessError::Operation(
                "injected identity inspection failure".to_string(),
            )),
        ),
        (
            "listener-inspection-failure",
            Ok(ProcessFacts {
                identity: discovery_fixture_identity(u64::from(pid + 1)),
                listener: ListenerState::OwnedByOther(vec![9900]),
            }),
        ),
    ] {
        let case_pid = pid + 1;
        let case_port = port + 1;
        let ledger = Rc::new(RefCell::new(Vec::new()));
        let serve = ScriptedTailscaleServe::new([], Rc::clone(&ledger));
        let (result, ledger, journal) = scripted_tailscale_launch_case(
            root.path(),
            label,
            case_pid,
            case_port,
            &serve,
            [inspection],
        );
        assert!(matches!(result, Err(LaunchOrchestrationError::Inspect(_))));
        assert!(!journal.exists());
        let events = ledger.borrow();
        assert!(events.contains(&LifecycleEvent::Terminate("tailscale-generation")));
        assert!(events.iter().all(|event| !matches!(
            event,
            LifecycleEvent::TailscaleObserve
                | LifecycleEvent::TailscaleApply
                | LifecycleEvent::TailscaleOff
                | LifecycleEvent::Persistence(_, _)
        )));
    }

    let pid = 4542;
    let port = 9542;
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new([], Rc::clone(&ledger));
    let publication_error = PublishError::Invalid {
        scope: RegistrationScope::Local,
        path: root.path().join("publication-failure/.ferric/server.json"),
        detail: "injected journal publication failure".to_string(),
    };
    let facts = exact_launch_facts(pid);
    let (publication, ledger, journal) = scripted_tailscale_launch_case_with(
        root.path(),
        "publication-failure",
        pid,
        port,
        &serve,
        [Ok(facts)],
        std::iter::repeat_with(|| Ok(None)).take(6),
        [true],
        Some(publication_error),
    );
    assert!(matches!(
        publication,
        Err(LaunchOrchestrationError::Publication(_))
    ));
    assert!(!journal.exists());
    let events = ledger.borrow();
    let terminate = events
        .iter()
        .position(|event| *event == LifecycleEvent::Terminate("tailscale-generation"))
        .unwrap();
    let listener = events
        .iter()
        .position(|event| matches!(event, LifecycleEvent::Listener(_, _)))
        .unwrap();
    assert!(terminate < listener);
    assert!(events.iter().all(|event| !matches!(
        event,
        LifecycleEvent::TailscaleObserve
            | LifecycleEvent::TailscaleApply
            | LifecycleEvent::TailscaleOff
    )));

    let root = tempfile::tempdir().unwrap();
    let pid = 4523;
    let port = 9523;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Proxy {
                target: "http://127.0.0.1:9999".to_string(),
            },
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "collision",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(6),
    );
    assert!(matches!(
        result,
        Err(LaunchOrchestrationError::Publication(_))
    ));
    assert!(!journal.exists());
    assert!(
        ledger
            .borrow()
            .iter()
            .all(|event| *event != LifecycleEvent::TailscaleApply)
    );
}

#[test]
fn tailscale_identity_races_never_publish_or_cross_profile_cleanup() {
    let root = tempfile::tempdir().unwrap();

    // A same-node rename between write-ahead publication and the final
    // pre-apply read is a zero-POST compensation path.
    let pid = 4550;
    let port = 9550;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut renamed_absent = tailscale_observation(&ownership, ServePathState::Absent, 'b');
    let mut renamed_identity = test_tailscale_identity();
    renamed_identity.fqdn = "renamed.tailnet-example.ts.net".to_string();
    renamed_absent.identity = Some(renamed_identity.clone());
    let serve = ScriptedTailscaleServe::new([Ok(renamed_absent)], Rc::clone(&ledger));
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "rename-before-apply",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    assert!(matches!(
        result,
        Err(LaunchOrchestrationError::Publication(_))
    ));
    assert!(!journal.exists());
    assert!(ledger.borrow().iter().all(|event| {
        !matches!(
            event,
            LifecycleEvent::TailscaleApply | LifecycleEvent::TailscaleOff
        )
    }));

    // A rename after the CAS can never reach Ready, but cleanup remains
    // authorized for the same stable node and still targets the journaled
    // old host/path.
    let pid = 4551;
    let port = 9551;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let absent = tailscale_observation(&ownership, ServePathState::Absent, 'b');
    let mut renamed_exact = tailscale_observation(
        &ownership,
        ServePathState::Proxy {
            target: ownership.proxy_target.clone(),
        },
        'c',
    );
    renamed_exact.identity = Some(renamed_identity.clone());
    let mut cleanup_exact = renamed_exact.clone();
    cleanup_exact.status_sha256 = "d".repeat(64);
    let mut cleanup_absent = tailscale_observation(&ownership, ServePathState::Absent, 'e');
    cleanup_absent.identity = Some(renamed_identity.clone());
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(absent),
            Ok(renamed_exact),
            Ok(cleanup_exact),
            Ok(cleanup_absent),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "rename-after-apply",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply rename must fail publication");
    };
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!journal.exists());
    assert!(ledger.borrow().contains(&LifecycleEvent::TailscaleOff));

    // A stable-node switch after the CAS is not cleanup authority. Ferric
    // stops its child but holds both ownership mirrors without mutating the
    // other profile's Serve config.
    let pid = 4552;
    let port = 9552;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let absent = tailscale_observation(&ownership, ServePathState::Absent, 'b');
    let mut switched_exact = tailscale_observation(
        &ownership,
        ServePathState::Proxy {
            target: ownership.proxy_target.clone(),
        },
        'c',
    );
    let mut switched_identity = test_tailscale_identity();
    switched_identity.stable_node_id = "other-stable-node".to_string();
    switched_exact.identity = Some(switched_identity.clone());
    let mut cleanup_switched = switched_exact.clone();
    cleanup_switched.status_sha256 = "d".repeat(64);
    let serve = ScriptedTailscaleServe::new(
        [Ok(absent), Ok(switched_exact), Ok(cleanup_switched)],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "stable-node-switch-after-apply",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply stable-node switch must fail publication");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(journal.exists());
    assert!(!ledger.borrow().contains(&LifecycleEvent::TailscaleOff));
}

#[test]
fn tailscale_cleanup_allows_same_node_rename_without_https_authority() {
    let ownership = tailscale_ownership(9553);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut renamed = test_tailscale_identity();
    renamed.fqdn = "renamed.tailnet-example.ts.net".to_string();
    renamed.backend_running = false;
    renamed.https_capable = false;
    renamed.certificate_domain = false;
    let mut exact = tailscale_observation(
        &ownership,
        ServePathState::Proxy {
            target: ownership.proxy_target.clone(),
        },
        'b',
    );
    exact.identity = Some(renamed.clone());
    let mut absent = tailscale_observation(&ownership, ServePathState::Absent, 'c');
    absent.identity = Some(renamed);
    let serve = ScriptedTailscaleServe::new([Ok(exact), Ok(absent)], Rc::clone(&ledger));
    let report = reconcile_owned_proxy(
        &ownership,
        &serve,
        ProxyReconcileContext::EstablishedOwnership,
        || Ok(()),
    );
    assert!(report.resolved, "{report:?}");
    assert!(!report.off_failed);
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::TailscaleObserve,
            LifecycleEvent::TailscaleOff,
            LifecycleEvent::TailscaleObserve,
        ]
    );
}

#[test]
fn tailscale_launch_failure_matrix_holds_or_compensates_exactly() {
    let root = tempfile::tempdir().unwrap();

    let pid = 4524;
    let port = 9524;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'd',
            )),
        ],
        Rc::clone(&ledger),
    )
    .with_apply(Err(
        crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
            "injected apply failure".to_string(),
        ),
    ));
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "apply-failed-but-landed",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("ambiguous apply must compensate through publication reporting");
    };
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!journal.exists());
    let events = ledger.borrow();
    let apply = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleApply)
        .unwrap();
    let off = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleOff)
        .unwrap();
    let terminate = events
        .iter()
        .position(|event| *event == LifecycleEvent::Terminate("tailscale-generation"))
        .unwrap();
    let remove = events
        .iter()
        .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
        .unwrap();
    assert!(apply < off && off < terminate && terminate < remove);
    drop(events);

    let pid = 4525;
    let port = 9525;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Err(crate::tailscale_serve::TailscaleServeError::InvalidStatus(
                "injected malformed verification".to_string(),
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: "http://127.0.0.1:9998".to_string(),
                },
                'c',
            )),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "replacement-held",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("replacement must retain recovery evidence");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(journal.exists());
    assert!(
        ledger
            .borrow()
            .iter()
            .all(|event| *event != LifecycleEvent::TailscaleOff)
    );
    assert!(
        ledger
            .borrow()
            .iter()
            .any(|event| { *event == LifecycleEvent::Terminate("tailscale-generation") })
    );
    assert!(
        ledger
            .borrow()
            .iter()
            .all(|event| !matches!(event, LifecycleEvent::Remove(_, _)))
    );

    let pid = 4526;
    let port = 9526;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'd',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'e',
            )),
        ],
        Rc::clone(&ledger),
    )
    .with_off(Err(
        crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
            "injected cleanup failure".to_string(),
        ),
    ));
    let exact = exact_launch_facts(pid);
    let changed = ProcessFacts {
        identity: discovery_fixture_identity(u64::from(pid + 1)),
        listener: ListenerState::OwnedByTarget,
    };
    let (result, _, journal) = scripted_tailscale_launch_case(
        root.path(),
        "off-error-but-absent",
        pid,
        port,
        &serve,
        [
            Ok(exact.clone()),
            Ok(exact.clone()),
            Ok(exact.clone()),
            Ok(changed.clone()),
            Ok(changed.clone()),
            Ok(changed),
        ],
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply authority loss must compensate");
    };
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("injected cleanup failure")),
        "{:?}",
        report.diagnostics
    );

    // If scoped cleanup removes Ferric's exact handler but a pre-existing
    // ancestor then wins the advertised URL, launch compensation must hold
    // both journals. A later `down` sees the same absent+shadow state and
    // must remain a retryable/manual-recovery failure rather than deleting
    // the evidence.
    let pid = 4528;
    let port = 9528;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let mut shadowed_absent = tailscale_observation(&ownership, ServePathState::Absent, 'e');
    shadowed_absent.route_shadow = Some("/".to_string());
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'd',
            )),
            Ok(shadowed_absent),
        ],
        Rc::clone(&ledger),
    )
    .with_off(Err(
        crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
            "injected cleanup residual".to_string(),
        ),
    ));
    let exact = exact_launch_facts(pid);
    let changed = ProcessFacts {
        identity: discovery_fixture_identity(u64::from(pid + 1)),
        listener: ListenerState::OwnedByTarget,
    };
    let (result, _, journal) = scripted_tailscale_launch_case(
        root.path(),
        "off-shadow-held",
        pid,
        port,
        &serve,
        [Ok(exact.clone()), Ok(exact.clone()), Ok(exact)]
            .into_iter()
            .chain(std::iter::repeat_with(|| Ok(changed.clone())).take(16)),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-cleanup route shadow must retain recovery evidence");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("shadows"))
    );

    // An apply error followed by an immediate absent observation requires
    // no `off`, but the observation is not a completion barrier for an
    // already accepted LocalAPI request. Stop the child and retain the
    // write-ahead journal in its unconfirmed phase.
    let pid = 4527;
    let port = 9527;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'c',
            )),
        ],
        Rc::clone(&ledger),
    )
    .with_apply(Err(
        crate::tailscale_serve::TailscaleServeError::LocalApiIndeterminate(
            "injected apply response loss".to_string(),
        ),
    ));
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "apply-failed-but-absent",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("apply-failed-but-absent must compensate");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(report.shutdown.as_ref().unwrap().cleanup_authorized());
    assert!(journal.exists());
    assert!(
        report.diagnostics.iter().any(|line| {
            line.contains("immediate absent observation is not a completion barrier")
        })
    );
    let events = ledger.borrow();
    assert!(
        events
            .iter()
            .all(|event| *event != LifecycleEvent::TailscaleOff)
    );
    let apply = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleApply)
        .unwrap();
    let terminate = events
        .iter()
        .position(|event| *event == LifecycleEvent::Terminate("tailscale-generation"))
        .unwrap();
    assert!(apply < terminate);
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, LifecycleEvent::Remove(_, _)))
    );
    drop(events);

    // A later absent-only down remains fail-closed: it cannot erase the
    // durable coordinate until the delayed apply is observed exact and
    // removed, or a separately proven daemon-generation recovery occurs.
    let raw = fs::read(&journal).unwrap();
    let runfile: ServerRunfile = serde_json::from_slice(&raw).unwrap();
    assert!(!runfile.tailscale_serve.as_ref().unwrap().apply_confirmed);
    let capture = CapturedRegistration {
        scope: RegistrationScope::Local,
        path: journal.clone(),
        raw,
        runfile,
    };
    let down_ledger = Rc::new(RefCell::new(Vec::new()));
    let absent = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            capture.runfile.tailscale_serve.as_ref().unwrap(),
            ServePathState::Absent,
            'd',
        ))],
        Rc::clone(&down_ledger),
    );
    let mut effects = ScriptedDownEffects::new(
        [ListenerState::Absent],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&down_ledger),
    )
    .with_tailscale(absent);
    let down = execute_down_plan::<ScriptedProcess, _>(
        DownPlan::Stale {
            captures: vec![capture.clone()],
            expected_revisions: vec![representative_revision(&capture)],
        },
        &mut effects,
    );
    assert_eq!(down.disposition, DownDisposition::Failed);
    assert!(!down.success);
    assert!(journal.exists());
    assert!(
        down.diagnostics.iter().any(|line| {
            line.contains("immediate absent observation is not a completion barrier")
        })
    );
    assert!(
        down_ledger
            .borrow()
            .iter()
            .all(|event| !matches!(event, LifecycleEvent::Remove(_, _)))
    );

    // A wrong post-apply target is never switched off by Ferric. The
    // independently owned child is stopped while its journal is held.
    let pid = 4528;
    let port = 9528;
    let ownership = tailscale_ownership(port);
    let replacement = "http://127.0.0.1:9997".to_string();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: replacement.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: replacement.clone(),
                },
                'd',
            )),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "post-apply-replacement",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply replacement must hold recovery evidence");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(report.shutdown.as_ref().unwrap().cleanup_authorized());
    assert!(journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains(&replacement))
    );
    assert!(ledger.borrow().iter().all(|event| {
        *event != LifecycleEvent::TailscaleOff && !matches!(event, LifecycleEvent::Remove(_, _))
    }));

    // Once exact cleanup starts, a failed post-off observation must hold
    // the journal even though the exact child can still be stopped.
    let pid = 4529;
    let port = 9529;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Err(crate::tailscale_serve::TailscaleServeError::InvalidStatus(
                "injected verification failure".to_string(),
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Err(
                crate::tailscale_serve::TailscaleServeError::LocalApiNoMutation(
                    "injected read failure".to_string(),
                ),
            ),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case(
        root.path(),
        "post-off-uninspectable",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("unproved post-off absence must hold recovery evidence");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert!(journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("could not prove Tailscale Serve absence"))
    );
    let events = ledger.borrow();
    let off = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleOff)
        .unwrap();
    let terminate = events
        .iter()
        .position(|event| *event == LifecycleEvent::Terminate("tailscale-generation"))
        .unwrap();
    assert!(off < terminate);
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, LifecycleEvent::Remove(_, _)))
    );
    drop(events);

    // Final child exit after exact proxy verification still compensates
    // proxy-first and removes the journal only after exact quiescence.
    let pid = 4530;
    let port = 9530;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'd',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'e',
            )),
        ],
        Rc::clone(&ledger),
    );
    let facts = exact_launch_facts(pid);
    let (result, ledger, journal) = scripted_tailscale_launch_case_with(
        root.path(),
        "child-exit-after-apply",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
        [
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(None),
            Ok(Some(ScriptedExit("injected post-apply exit"))),
        ],
        [true],
        None,
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply child exit must compensate");
    };
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("exited during Tailscale Serve publication"))
    );
    let events = ledger.borrow();
    let off = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleOff)
        .unwrap();
    let terminate = events
        .iter()
        .position(|event| *event == LifecycleEvent::Terminate("tailscale-generation"))
        .unwrap();
    assert!(off < terminate);
    drop(events);

    // A final listener transfer is a distinct post-apply authority row.
    let pid = 4531;
    let port = 9531;
    let ownership = tailscale_ownership(port);
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'd',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'e',
            )),
        ],
        Rc::clone(&ledger),
    );
    let exact = exact_launch_facts(pid);
    let drifted = ProcessFacts {
        identity: exact.identity.clone(),
        listener: ListenerState::OwnedByTargetWildcard,
    };
    let (result, _, journal) = scripted_tailscale_launch_case(
        root.path(),
        "listener-drift-after-apply",
        pid,
        port,
        &serve,
        [Ok(exact.clone()), Ok(exact), Ok(drifted)],
    );
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("post-apply listener drift must compensate");
    };
    assert_eq!(report.disposition, PublicationDisposition::RolledBack);
    assert!(!journal.exists());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("no longer exclusively owns"))
    );

    // A concurrent final journal replacement is never deleted. Durable
    // confirmation fails before scoped proxy cleanup, so Ferric holds the
    // replacement and never mutates the endpoint from stale authority.
    let pid = 4532;
    let port = 9532;
    let ownership = tailscale_ownership(port);
    let journal = runfile_path(&root.path().join("registration-replacement"));
    let mut replacement_runfile = discovery_fixture_runfile(pid + 1, "concurrent-journal");
    replacement_runfile.origin_local_runfile = Some(journal.clone());
    let replacement_raw = serde_json::to_vec(&replacement_runfile).unwrap();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'c',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'd',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'e',
            )),
        ],
        Rc::clone(&ledger),
    )
    .with_observe_writes([None, Some((journal.clone(), replacement_raw.clone()))]);
    let facts = exact_launch_facts(pid);
    let (result, ledger, returned_journal) = scripted_tailscale_launch_case(
        root.path(),
        "registration-replacement",
        pid,
        port,
        &serve,
        std::iter::repeat_with(|| Ok(facts.clone())).take(8),
    );
    assert_eq!(returned_journal, journal);
    let Err(LaunchOrchestrationError::Publication(report)) = result else {
        panic!("registration replacement must prevent compare-remove");
    };
    assert_eq!(report.disposition, PublicationDisposition::RecoveryHeld);
    assert_eq!(fs::read(&journal).unwrap(), replacement_raw);
    assert!(matches!(
        report.finals[0].outcome,
        DownRegistrationOutcome::Held { .. }
    ));
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("changed while confirming Tailscale apply"))
    );
    let events = ledger.borrow();
    assert!(events.iter().all(|event| {
        *event != LifecycleEvent::TailscaleOff && !matches!(event, LifecycleEvent::Remove(_, _))
    }));
}

#[test]
fn phase_torn_tailscale_mirrors_promote_once_and_clean_fresh_bytes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("phase-torn");
    let local_path = runfile_path(&workspace);
    let global_path = root.path().join("global/server.json");
    let pid = 4533;
    let mut runfile = composition_runfile(pid, &local_path);
    let mut ownership = tailscale_ownership(runfile.port);
    ownership.apply_confirmed = true;
    runfile.tailscale = true;
    runfile.tailscale_serve = Some(ownership.clone());
    let published = publish_mirrored(&workspace, Some(&global_path), &runfile).unwrap();

    // Tear the durable phase exactly between the physical local journal
    // and its global mirror. Inventory also projects the local origin a
    // second time, so confirmation must perform one CAS for two captures.
    let mut local_unconfirmed = published.local.runfile.clone();
    local_unconfirmed
        .tailscale_serve
        .as_mut()
        .unwrap()
        .apply_confirmed = false;
    fs::write(
        &local_path,
        serde_json::to_vec_pretty(&local_unconfirmed).unwrap(),
    )
    .unwrap();

    let scope = ManagedDiscoveryScope {
        workspace: workspace.clone(),
        global: Some(global_path.clone()),
    };
    let lifecycle = discover_inventory_before_health_with(
        inventory_runfiles(&workspace, scope.global.clone()),
        |capture| LifecycleObservation {
            candidate: Candidate {
                coordinate: RegistrationCoordinate {
                    scope: capture.scope,
                    path: capture.path.clone(),
                },
                runfile: Some(capture.runfile.clone()),
                state: CandidateState::Stale {
                    reason: "PID is absent".to_string(),
                    observed_identity: None,
                    listener: ListenerState::Absent,
                },
            },
            label: registration_label(capture.scope, &capture.path),
            capture: Some(capture),
            process: None,
        },
    );
    assert!(
        matches!(
            lifecycle.managed.state,
            ManagedServerState::StaleOnly { .. }
        ),
        "a phase-only mirror tear must remain recoverable, got {:?}",
        lifecycle.managed.state
    );

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'c',
            )),
        ],
        Rc::clone(&ledger),
    );
    let mut effects = FilesystemTailscaleDownEffects {
        scope,
        listeners: VecDeque::from([ListenerState::Absent]),
        serve,
        ledger: Rc::clone(&ledger),
    };
    let report = execute_down_plan(down_plan_from_lifecycle(lifecycle), &mut effects);
    assert_eq!(report.disposition, DownDisposition::StaleCleaned);
    assert!(report.success, "{:?}", report.diagnostics);
    assert!(!local_path.exists());
    assert!(!global_path.exists());
    let events = ledger.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LifecycleEvent::Replace(_, _, _)))
            .count(),
        1,
        "duplicate physical-origin captures require one promotion CAS"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
            .count(),
        2,
        "fresh confirmed bytes must remove both physical journals"
    );
    let replace = events
        .iter()
        .position(|event| matches!(event, LifecycleEvent::Replace(_, _, _)))
        .unwrap();
    let off = events
        .iter()
        .position(|event| *event == LifecycleEvent::TailscaleOff)
        .unwrap();
    let first_remove = events
        .iter()
        .position(|event| matches!(event, LifecycleEvent::Remove(_, _)))
        .unwrap();
    assert!(replace < off && off < first_remove);
}

#[test]
fn partial_tailscale_confirmation_holds_every_journal_before_off() {
    let pid = 4534;
    let (mut local, ownership) =
        discovery_fixture_tailscale_capture(RegistrationScope::Local, pid, "confirmation-local");
    let (mut global, _) =
        discovery_fixture_tailscale_capture(RegistrationScope::Global, pid, "confirmation-global");
    for capture in [&mut local, &mut global] {
        capture
            .runfile
            .tailscale_serve
            .as_mut()
            .unwrap()
            .apply_confirmed = false;
        capture.raw = serde_json::to_vec_pretty(&capture.runfile).unwrap();
    }
    let expected_revisions = vec![
        representative_revision(&local),
        representative_revision(&global),
    ];
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let serve = ScriptedTailscaleServe::new(
        [Ok(tailscale_observation(
            &ownership,
            ServePathState::Proxy {
                target: ownership.proxy_target.clone(),
            },
            'b',
        ))],
        Rc::clone(&ledger),
    );
    let mut effects = ScriptedDownEffects::new(
        [ListenerState::Absent],
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    )
    .with_tailscale(serve)
    .with_tailscale_replacements([
        Ok(ReplacementOutcome::Replaced),
        Ok(ReplacementOutcome::ReplacementPreserved {
            path: global.path.clone(),
            detail: "injected phase CAS race".to_string(),
        }),
    ]);
    let report = execute_down_plan::<ScriptedProcess, _>(
        DownPlan::Stale {
            captures: vec![local, global],
            expected_revisions,
        },
        &mut effects,
    );
    assert_eq!(report.disposition, DownDisposition::Failed);
    assert!(!report.success);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|line| line.contains("injected phase CAS race"))
    );
    assert!(ledger.borrow().iter().all(|event| {
        *event != LifecycleEvent::TailscaleOff && !matches!(event, LifecycleEvent::Remove(_, _))
    }));
}

#[test]
fn tailscale_reconciliation_revision_race_never_signals_process() {
    let pid = 4535;
    let (capture, ownership) = discovery_fixture_tailscale_capture(
        RegistrationScope::Local,
        pid,
        "reconciliation-revision-race",
    );
    let port = capture.runfile.port;
    let expected = capture.runfile.process_identity.clone().unwrap();
    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "must-not-be-signalled", Rc::clone(&ledger));
    let serve = ScriptedTailscaleServe::new(
        [
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Proxy {
                    target: ownership.proxy_target.clone(),
                },
                'b',
            )),
            Ok(tailscale_observation(
                &ownership,
                ServePathState::Absent,
                'c',
            )),
        ],
        Rc::clone(&ledger),
    );
    let mut effects = ScriptedDownEffects::new(
        Vec::<ListenerState>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    )
    .with_tailscale(serve);
    effects.revalidations = VecDeque::from([
        Ok(()),
        Err("injected post-proxy registration replacement".to_string()),
    ]);
    let report = execute_down_plan(
        DownPlan::Target {
            process,
            expected,
            pid,
            port,
            captures: vec![capture.clone()],
            expected_revisions: vec![representative_revision(&capture)],
        },
        &mut effects,
    );
    assert_eq!(report.disposition, DownDisposition::Failed);
    assert!(!report.signalled);
    assert!(report.diagnostics.iter().any(|line| {
        line.contains("injected post-proxy registration replacement")
            && line.contains("was not signalled")
    }));
    assert_eq!(
        *ledger.borrow(),
        vec![
            LifecycleEvent::Revalidate,
            LifecycleEvent::TailscaleObserve,
            LifecycleEvent::TailscaleOff,
            LifecycleEvent::TailscaleObserve,
            LifecycleEvent::Revalidate,
        ]
    );
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
            identity: discovery_fixture_identity(u64::from(pid)),
            listener: ListenerState::OwnedByTarget,
        }))
        .with_inspection(Ok(ProcessFacts {
            identity: discovery_fixture_identity(u64::from(pid)),
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
    let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
    let mut compensation = ScriptedPublicationEffects::new(
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&ledger),
    );
    let spawn_ledger = Rc::clone(&ledger);
    let launched = orchestrate_launch_with(
        &workspace,
        None,
        &launch_cfg,
        None,
        &TailscaleServeAdapter::native(),
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
        let expected_identity = discovery_fixture_identity(u64::from(case_pid));
        let post_identity = if changed_identity {
            discovery_fixture_identity(u64::from(case_pid + 1))
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&case_ledger));
        let mut compensation = FilesystemPublicationEffects {
            ledger: Rc::clone(&case_ledger),
        };
        let mut case_cfg = cfg(Engine::LlamaServer);
        case_cfg.port = case_port;
        let spawn_ledger = Rc::clone(&case_ledger);
        let result = orchestrate_launch_with(
            &case_workspace,
            None,
            &case_cfg,
            None,
            &TailscaleServeAdapter::native(),
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
    let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&blocked_ledger));
    let mut compensation = ScriptedPublicationEffects::new(
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Vec::<Result<RemovalOutcome, RemovalError>>::new(),
        Rc::clone(&blocked_ledger),
    );
    let spawn_ledger = Rc::clone(&blocked_ledger);
    let blocked = orchestrate_launch_with(
        &root.path().join("blocked-up"),
        None,
        &launch_cfg,
        None,
        &TailscaleServeAdapter::native(),
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
fn legacy_adoption_then_down() {
    let pid = 4541;
    let (fixture, facts) = legacy_adoption_fixture(pid);
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("legacy-workspace");
    let local_path = runfile_path(&workspace);
    let global_path = root.path().join("legacy-global/server.json");
    fs::create_dir_all(local_path.parent().unwrap()).unwrap();
    fs::create_dir_all(global_path.parent().unwrap()).unwrap();
    fs::write(&local_path, &fixture[0].raw).unwrap();
    fs::write(&global_path, &fixture[0].raw).unwrap();
    let scope = ManagedDiscoveryScope {
        workspace: workspace.clone(),
        global: Some(global_path.clone()),
    };
    let inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
    let (legacy, blocked) = expand_registration_captures(inventory);
    assert!(blocked.is_empty());
    assert_eq!(legacy.len(), 2);
    assert_eq!(legacy[0].path, local_path);
    assert_eq!(legacy[1].path, global_path);

    let ledger = Rc::new(RefCell::new(Vec::new()));
    let process = ScriptedProcess::new(pid, "adopted-generation", Rc::clone(&ledger))
        .with_inspection(Ok(facts.clone()))
        .with_inspection(Ok(facts.clone()))
        .with_wait(Ok(false));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let mut adoption_effects = FilesystemAdoptionEffects {
        ledger: Rc::clone(&ledger),
    };
    let adoption = execute_legacy_adoption(legacy, pid, &runtime, &mut adoption_effects);
    let adoption_rendered = render_adoption_with_ledger(&adoption, &ledger);
    assert!(adoption_rendered.success);
    assert_eq!(adoption.disposition, AdoptionDisposition::Adopted);
    assert!(ledger.borrow().iter().all(|event| !matches!(
        event,
        LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
    )));

    let adopted_raw = fs::read(&local_path).unwrap();
    assert_eq!(fs::read(&global_path).unwrap(), adopted_raw);
    let adopted_runfile: ServerRunfile = serde_json::from_slice(&adopted_raw).unwrap();
    assert_eq!(adopted_runfile.schema_version, RUNFILE_SCHEMA_V2);
    let parsed_identity = adopted_runfile.process_identity.clone().unwrap();
    assert_eq!(parsed_identity, facts.identity);
    assert_eq!(
        adopted_runfile.origin_local_runfile.as_deref(),
        Some(local_path.as_path())
    );

    // Re-read and parse the bytes written by the real conditional
    // replacement adapter, then resolve that inventory before deriving
    // teardown authority from its persisted identity.
    let adopted_inventory = inventory_runfiles(&workspace, Some(global_path.clone()));
    let (adopted_captures, blocked) = expand_registration_captures(adopted_inventory.clone());
    assert!(blocked.is_empty());
    assert_eq!(adopted_captures.len(), 3);
    let mut health = ScriptedHealth {
        results: VecDeque::from([true]),
        ledger: Rc::clone(&ledger),
    };
    let lifecycle = discover_inventory_with(
        adopted_inventory,
        |capture| {
            let identity = capture.runfile.process_identity.clone().unwrap();
            LifecycleObservation {
                candidate: Candidate {
                    coordinate: RegistrationCoordinate {
                        scope: capture.scope,
                        path: capture.path.clone(),
                    },
                    runfile: Some(capture.runfile.clone()),
                    state: CandidateState::Verified {
                        identity,
                        listener: ListenerState::OwnedByTarget,
                        health: HealthState::NotProbed,
                    },
                },
                label: registration_label(capture.scope, &capture.path),
                capture: Some(capture),
                process: None,
            }
        },
        &mut health,
        |_observation| Ok(()),
    );
    let managed = lifecycle.managed;
    assert!(matches!(&managed.state, ManagedServerState::Ready(_)));

    let process = ScriptedProcess::new(pid, "adopted-generation", Rc::clone(&ledger))
        .with_inspection(Ok(ProcessFacts {
            identity: parsed_identity.clone(),
            listener: ListenerState::OwnedByTarget,
        }));
    let runtime = ScriptedRuntime::new(Ok(process), Rc::clone(&ledger));
    let retained = runtime.acquire(pid).unwrap();
    let plan = retained_target_down_plan(
        &managed.state,
        retained,
        adopted_captures,
        discovery_revisions(&managed.observations),
    )
    .unwrap();
    let mut down_effects = FilesystemDownEffects {
        scope,
        listeners: VecDeque::from([ListenerState::Absent]),
        ledger: Rc::clone(&ledger),
    };
    let down = execute_down_plan(plan, &mut down_effects);
    let down_rendered = render_down_with_ledger(&down, &ledger);
    assert!(down_rendered.success);
    assert_eq!(down.disposition, DownDisposition::Stopped);
    assert!(
        down.registrations
            .iter()
            .all(|registration| { registration.outcome == DownRegistrationOutcome::Removed })
    );
    assert!(!local_path.exists());
    assert!(!global_path.exists());

    let events = ledger.borrow();
    let adoption_finish = events
        .iter()
        .position(|event| *event == LifecycleEvent::Render)
        .unwrap();
    let down_acquire = events
        .iter()
        .enumerate()
        .find(|(index, event)| *index > adoption_finish && **event == LifecycleEvent::Acquire(pid))
        .map(|(index, _)| index)
        .unwrap();
    assert!(events[..adoption_finish].iter().all(|event| !matches!(
        event,
        LifecycleEvent::Terminate(_) | LifecycleEvent::ChildKill(_)
    )));
    assert_eq!(
        events[down_acquire..]
            .iter()
            .filter(|event| matches!(event, LifecycleEvent::Inspect("adopted-generation", _)))
            .count(),
        1
    );
    assert!(events[down_acquire..].contains(&LifecycleEvent::Terminate("adopted-generation")));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, LifecycleEvent::Remove(_, _)))
            .count(),
        2
    );
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
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
        let mut cleanup = ScriptedPublicationEffects::new(
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
                CompositionPersistenceEffects::retaining_committed_stage(target, Rc::clone(&ledger))
            } else {
                CompositionPersistenceEffects::failing(target, phase, Rc::clone(&ledger))
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
            let mut cleanup = FilesystemPublicationEffects {
                ledger: Rc::clone(&ledger),
            };
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
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
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
        let publication =
            publish_mirrored_with(&workspace, Some(&alias), &runfile, &mut persistence);
        assert!(matches!(publication, Err(PublishError::Invalid { .. })));
        assert_eq!(persistence.serializations, 0);
        assert!(ledger.borrow().is_empty());
        let process = ScriptedProcess::new(pid, "publication-alias", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
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
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::default_with(Rc::clone(&ledger));
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
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::failing(
            &global,
            PersistencePhase::FileSync,
            Rc::clone(&ledger),
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
        let mut cleanup = ScriptedPublicationEffects::new(
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
        let mut persistence = CompositionPersistenceEffects::failing(
            &global,
            PersistencePhase::FileSync,
            Rc::clone(&ledger),
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let process = ScriptedProcess::new(pid, "terminate-error-exited", Rc::clone(&ledger))
            .with_terminate(Err(ProcessError::Operation("signal denied".to_string())))
            .with_wait(Ok(true));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::failing(
            &global,
            PersistencePhase::FileSync,
            Rc::clone(&ledger),
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        fs::write(&local, b"concurrent final replacement").unwrap();
        let process = ScriptedProcess::new(pid, "publication-replacement", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = FilesystemPublicationEffects {
            ledger: Rc::clone(&ledger),
        };
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
        let mut persistence = CompositionPersistenceEffects::failing(
            &global,
            PersistencePhase::FileSync,
            Rc::clone(&ledger),
        );
        let publication =
            publish_mirrored_with(&workspace, Some(&global), &runfile, &mut persistence);
        let process = ScriptedProcess::new(pid, "publication-cleanup-failure", Rc::clone(&ledger));
        let mut child = ScriptedChild::new(pid, [], Rc::clone(&ledger));
        let listener = ScriptedListener::new(ListenerState::Absent, Rc::clone(&ledger));
        let mut cleanup = ScriptedPublicationEffects::new(
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
        let rendered = render_publication_with_ledger(&report, &ledger);
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
const LIFECYCLE_HELPER_ENV: &str = "FERRIC_TEST_LIFECYCLE_HELPER";

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
const LIFECYCLE_HELPER_WILDCARD_ENV: &str = "FERRIC_TEST_LIFECYCLE_WILDCARD";

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
static LIFECYCLE_PARENT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
const LIFECYCLE_HELPER_READY_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
const LIFECYCLE_HELPER_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
struct LifecycleHelperGuard {
    child: crate::test_process_containment::ContainedChild,
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
impl LifecycleHelperGuard {
    fn new(child: crate::test_process_containment::ContainedChild) -> Self {
        Self { child }
    }

    fn child(&self) -> &Child {
        self.child.child()
    }

    fn child_mut(&mut self) -> &mut crate::test_process_containment::ContainedChild {
        &mut self.child
    }

    fn id(&self) -> u32 {
        self.child().id()
    }

    fn wait_for_exit_and_disarm(&mut self, context: &str) -> Result<(), String> {
        self.child
            .wait_for_exit_and_disarm(LIFECYCLE_HELPER_EXIT_TIMEOUT)
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
#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn lifecycle_helper_process() {
    if std::env::var(LIFECYCLE_HELPER_ENV).ok().as_deref() != Some("1") {
        return;
    }
    crate::test_process_containment::arm_current_process_parent_death_signal()
        .expect("arm lifecycle-helper parent-death containment");
    let port = std::env::var("FERRIC_TEST_LIFECYCLE_PORT")
        .expect("helper port")
        .parse::<u16>()
        .expect("numeric helper port");
    let bind_host = if std::env::var(LIFECYCLE_HELPER_WILDCARD_ENV).ok().as_deref() == Some("1") {
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
fn spawn_lifecycle_helper(port: u16) -> LifecycleHelperGuard {
    spawn_lifecycle_helper_with_binding(port, false)
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
fn spawn_lifecycle_helper_with_binding(port: u16, wildcard: bool) -> LifecycleHelperGuard {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .args([
            "--exact",
            "server::tests::lifecycle_helper_process",
            "--nocapture",
        ])
        .env(LIFECYCLE_HELPER_ENV, "1")
        .env(
            LIFECYCLE_HELPER_WILDCARD_ENV,
            if wildcard { "1" } else { "0" },
        )
        .env("FERRIC_TEST_LIFECYCLE_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = crate::test_process_containment::ContainedChild::spawn(&mut command)
        .expect("spawn contained lifecycle helper");
    LifecycleHelperGuard::new(child)
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
fn lifecycle_parent_test_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::test_process_containment::ensure_current_process_tree_is_contained()
        .expect("install lifecycle-test process containment");
    LIFECYCLE_PARENT_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
fn wait_for_lifecycle_helper(
    child: &mut crate::test_process_containment::ContainedChild,
    port: u16,
) {
    if let Err(error) = wait_healthy(
        child,
        Engine::LlamaServer,
        "127.0.0.1",
        port,
        LIFECYCLE_HELPER_READY_TIMEOUT,
    ) {
        let cleanup = stop_child(child);
        panic!("lifecycle helper did not become HTTP-ready: {error}; cleanup result: {cleanup:?}");
    }
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
            error.contains("listener owner enumeration is incomplete")
                || error.contains("whose owners are not inspectable"),
            "unexpected Linux listener-inspection failure: {error}"
        );
        return false;
    }
    panic!("expected native listener state {expected:?}, got {actual:?}");
}

#[cfg(all(
    target_os = "linux",
    target_endian = "little",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn is_documented_linux_listener_visibility_error(error: &str) -> bool {
    error.contains("listener owner enumeration is incomplete")
        || error.contains("whose owners are not inspectable")
}

fn write_test_runfile(path: &Path, runfile: &ServerRunfile) {
    std::fs::create_dir_all(path.parent().expect("runfile parent")).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(runfile).unwrap()).unwrap();
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

    let discovered = read_runfile_result_impl(&workspace_a, Some(global.clone()))
        .unwrap()
        .expect("read-only consumers resolve the verified global server");
    assert_eq!(discovered.pid, helper_pid);
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn live_legacy_registration_cannot_authorize_teardown() {
    let _lifecycle_serial = lifecycle_parent_test_guard();
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let local = std::path::absolute(runfile_path(&workspace)).unwrap();
    let port = unused_port();
    let mut helper = spawn_lifecycle_helper(port);
    wait_for_lifecycle_helper(helper.child_mut(), port);

    let legacy = ServerRunfile {
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
    write_test_runfile(&local, &legacy);

    let result = down_impl(&workspace, None);
    let registration_remained = local.exists();
    let helper_remained_live = helper.child_mut().try_wait().unwrap().is_none();
    helper.terminate_reap_and_disarm("clean up legacy lifecycle helper");

    assert_eq!(result, ExitCode::FAILURE);
    assert!(
        registration_remained,
        "blocked legacy record must be retained"
    );
    assert!(
        helper_remained_live,
        "a live schema-1 PID must never be signalled without creation identity"
    );
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
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

#[test]
fn legacy_adoption_coordinates_require_closed_engine_and_every_recorded_value() {
    let executable = if cfg!(windows) {
        PathBuf::from(r"C:\tools\llama-server.exe")
    } else {
        PathBuf::from("/tools/llama-server")
    };
    let runfile = ServerRunfile {
        schema_version: 1,
        engine: Engine::LlamaServer,
        pid: 42,
        port: 8080,
        base_url: "http://127.0.0.1:8080/v1".to_string(),
        tailscale: false,
        tailscale_serve: None,
        model: Some("model.gguf".to_string()),
        context_size: Some(8192),
        sampling_seed: Some(42),
        parallel_slots: Some(1),
        process_identity: None,
        origin_local_runfile: None,
    };
    let identity = ProcessIdentity {
        start_token: canonical_test_start_token(1),
        executable,
        argv: vec![
            "llama-server".to_string(),
            "-m".to_string(),
            "model.gguf".to_string(),
            "-c".to_string(),
            "8192".to_string(),
            "--seed".to_string(),
            "42".to_string(),
            "--parallel".to_string(),
            "1".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            "8080".to_string(),
        ],
    };
    validate_legacy_process_coordinates(&runfile, &identity).unwrap();

    for (flag, coordinate) in [
        ("-m", "recorded model"),
        ("-c", "recorded context size"),
        ("--seed", "recorded sampling seed"),
        ("--parallel", "recorded parallel slot count"),
        ("--host", "loopback host"),
        ("--port", "registered port"),
    ] {
        let mut missing = identity.clone();
        let index = missing
            .argv
            .iter()
            .position(|argument| argument == flag)
            .unwrap();
        missing.argv.drain(index..=index + 1);
        let error = validate_legacy_process_coordinates(&runfile, &missing).unwrap_err();
        assert!(error.contains(coordinate), "{flag}: {error}");

        let mut conflicting = identity.clone();
        conflicting
            .argv
            .extend([flag.to_string(), "conflicting-value".to_string()]);
        let error = validate_legacy_process_coordinates(&runfile, &conflicting).unwrap_err();
        assert!(
            error.contains(&format!("conflicting {coordinate}")),
            "{flag}: {error}"
        );
    }

    let mut inline = identity.clone();
    for (flag, replacement) in [
        ("-m", "--model=model.gguf"),
        ("-c", "--ctx-size=8192"),
        ("--host", "--host=127.0.0.1"),
        ("--port", "--port=8080"),
        ("--seed", "--seed=42"),
        ("--parallel", "--parallel=1"),
    ] {
        let index = inline
            .argv
            .iter()
            .position(|argument| argument == flag)
            .unwrap();
        inline
            .argv
            .splice(index..=index + 1, [replacement.to_string()]);
    }
    validate_legacy_process_coordinates(&runfile, &inline).unwrap();
    let mut conflicting_inline = identity.clone();
    conflicting_inline
        .argv
        .push("--model=other.gguf".to_string());
    let error = validate_legacy_process_coordinates(&runfile, &conflicting_inline).unwrap_err();
    assert!(error.contains("conflicting recorded model"));

    let mut missing_port = identity.clone();
    missing_port.argv.truncate(missing_port.argv.len() - 2);
    let error = validate_legacy_process_coordinates(&runfile, &missing_port).unwrap_err();
    assert!(error.contains("registered port"));

    let mut conflicting_port = identity.clone();
    conflicting_port.argv.extend([
        "--port".to_string(),
        "8081".to_string(),
        "--model".to_string(),
        "other.gguf".to_string(),
    ]);
    let error = validate_legacy_process_coordinates(&runfile, &conflicting_port).unwrap_err();
    assert!(error.contains("conflicting registered port"));

    let mut wrong_engine = identity;
    wrong_engine.executable = if cfg!(windows) {
        PathBuf::from(r"C:\tools\python.exe")
    } else {
        PathBuf::from("/tools/python")
    };
    let error = validate_legacy_process_coordinates(&runfile, &wrong_engine).unwrap_err();
    assert!(error.contains("closed"));

    let ollama = ServerRunfile {
        engine: Engine::Ollama,
        model: None,
        context_size: None,
        sampling_seed: None,
        parallel_slots: None,
        ..runfile
    };
    let mut ollama_identity = ProcessIdentity {
        start_token: canonical_test_start_token(2),
        executable: if cfg!(windows) {
            PathBuf::from(r"C:\tools\ollama.exe")
        } else {
            PathBuf::from("/tools/ollama")
        },
        argv: vec!["ollama".to_string(), "serve".to_string()],
    };
    validate_legacy_process_coordinates(&ollama, &ollama_identity).unwrap();
    ollama_identity.argv = vec![
        "ollama".to_string(),
        "status".to_string(),
        "serve".to_string(),
    ];
    let error = validate_legacy_process_coordinates(&ollama, &ollama_identity).unwrap_err();
    assert!(error.contains("closed `ollama serve`"));
}

#[test]
fn blocked_local_inventory_prevents_global_autodiscovery() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let local = runfile_path(&workspace);
    let global = root.path().join("config").join("server.json");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, b"{not-json").unwrap();
    write_test_runfile(
        &global,
        &ServerRunfile {
            schema_version: 1,
            engine: Engine::LlamaServer,
            pid: 1,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            tailscale_serve: None,
            model: None,
            context_size: None,
            sampling_seed: None,
            parallel_slots: None,
            process_identity: None,
            origin_local_runfile: None,
        },
    );

    let error = read_runfile_result_impl(&workspace, Some(global)).unwrap_err();
    assert!(error.contains("blocked"), "unexpected error: {error}");
    assert!(error.contains(&local.display().to_string()));
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
        tailscale: false,
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

#[cfg(any(
    windows,
    all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
#[test]
fn live_registration_inspection_binds_pid_listener_and_health() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, released) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 512];
        let read = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..read]).starts_with("GET /health "));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
        released.recv_timeout(Duration::from_secs(15)).unwrap();
    });
    let runfile = ServerRunfile {
        schema_version: 1,
        engine: Engine::LlamaServer,
        pid: std::process::id(),
        port,
        base_url: format!("http://127.0.0.1:{port}/v1"),
        tailscale: false,
        tailscale_serve: None,
        model: Some("model.gguf".to_string()),
        context_size: Some(8192),
        sampling_seed: Some(42),
        parallel_slots: Some(1),
        process_identity: None,
        origin_local_runfile: None,
    };
    let inspected = inspect_registered_server(&runfile);
    if inspected.is_err() {
        // A restricted Linux /proc can reject before the HTTP request.
        // Wake the server thread with a valid probe so this native smoke
        // still exits cleanly while reporting the visibility limitation.
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.write_all(
                format!(
                    "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            );
        }
    }
    release.send(()).unwrap();
    server.join().unwrap();
    match inspected {
        Ok(inspected) => {
            assert_eq!(inspected.pid, std::process::id());
            assert_eq!(inspected.listener_owner_pid, std::process::id());
            assert!(!inspected.argv.is_empty());
            assert!(inspected.executable.is_file());
        }
        #[cfg(target_os = "linux")]
        Err(error) => assert!(
            is_documented_linux_listener_visibility_error(&error),
            "unexpected Linux registered-server inspection failure: {error}"
        ),
        #[cfg(windows)]
        Err(error) => panic!("registered-server inspection failed: {error}"),
    }
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
fn tailscale_launch_static_blockers_precede_external_effects() {
    let dir = tempfile::tempdir().unwrap();
    let missing_model = dir.path().join("missing.gguf");
    let mut args = llama_args(&missing_model);
    args.tailscale = true;
    args.port = 0;
    args.ctx = 0;
    args.parallel = Some(0);

    let error = validate_launch_preconditions(dir.path(), &args, None)
        .expect_err("invalid static coordinates must precede Tailscale inspection");
    assert_eq!(error, "--port must be greater than zero");
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

    let error = wait_healthy(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        unused_port(),
        Duration::from_secs(2),
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

    wait_healthy(
        &mut child,
        Engine::LlamaServer,
        "127.0.0.1",
        port,
        Duration::from_secs(2),
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
        observations[0].label,
        registration_label(RegistrationScope::Origin, &blocked_path)
    );
    assert!(matches!(
        &observations[0].candidate.state,
        CandidateState::Unverifiable { reason, .. }
            if reason.contains("promised by global registration")
                && reason.contains(&source.path.display().to_string())
    ));
}

#[test]
fn runfile_serde_roundtrip() {
    let rf = ServerRunfile {
        schema_version: 1,
        engine: Engine::LlamaServer,
        pid: 4321,
        port: 8080,
        base_url: "http://127.0.0.1:8080/v1".to_string(),
        tailscale: false,
        tailscale_serve: None,
        model: Some("model.gguf".to_string()),
        context_size: Some(4096),
        sampling_seed: Some(42),
        parallel_slots: Some(1),
        process_identity: None,
        origin_local_runfile: None,
    };
    let s = serde_json::to_string(&rf).unwrap();
    let back: ServerRunfile = serde_json::from_str(&s).unwrap();
    assert_eq!(back.pid, 4321);
    assert_eq!(back.engine, Engine::LlamaServer);
    assert_eq!(back.base_url, rf.base_url);
    assert_eq!(back.sampling_seed, Some(42));
    assert_eq!(back.parallel_slots, Some(1));
    assert_eq!(back.context_size, Some(4096));
    assert_eq!(back.model.as_deref(), Some("model.gguf"));
}

#[test]
fn old_runfile_sampling_metadata_defaults_to_unknown() {
    let old = r#"{"engine":"llama-server","pid":4321,"port":8080,"base_url":"http://127.0.0.1:8080/v1","tailscale":false}"#;
    let runfile: ServerRunfile = serde_json::from_str(old).unwrap();
    assert!(runfile.model.is_none());
    assert!(runfile.context_size.is_none());
    assert!(runfile.sampling_seed.is_none());
    assert!(runfile.parallel_slots.is_none());
}

#[test]
fn read_runfile_absent_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        read_runfile_result_impl(dir.path(), None)
            .unwrap()
            .is_none()
    );
}
