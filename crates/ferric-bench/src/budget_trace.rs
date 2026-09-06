//! Parent-authored, no-clobber attribution beside unchanged child trace bytes.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use ferric_trace::{Event, ParsedEvent, TraceReader};
use serde::{Deserialize, Serialize};

use crate::budget::{
    AttemptBudgetEvidence, AttemptIdentity, ObservedMainActionBudget, RetainedBudgetEvidence,
    TraceBudgetObservation, TraceEvidenceState,
};
use crate::provenance::sha256_file;

pub const BUDGET_SIDECAR_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetSidecarV1 {
    pub schema_version: u32,
    pub identity: AttemptIdentity,
    pub trace_path: String,
    pub trace_sha256: String,
    pub evidence: AttemptBudgetEvidence,
}

pub(crate) fn observe_trace(path: &Path) -> io::Result<TraceBudgetObservation> {
    // Opening failures are infrastructure, distinct from readable malformed
    // bytes. Missing is supplied only when no trace path was discovered.
    let reader = TraceReader::open(path).map_err(io::Error::other)?;
    let mut budgets = Vec::new();
    let mut terminal = None;
    for record in reader {
        let record = match record {
            Ok(record) => record,
            Err(error) => return Ok(malformed(error.to_string())),
        };
        match record.event {
            ParsedEvent::Known(Event::MainActionBudget { turn, budget }) => {
                budgets.push(ObservedMainActionBudget { turn, budget });
            }
            ParsedEvent::Known(Event::SessionEnd { reason }) => terminal = Some(reason),
            ParsedEvent::Unknown(raw)
                if matches!(
                    raw.get("type").and_then(serde_json::Value::as_str),
                    Some("main_action_budget" | "session_end")
                ) =>
            {
                // The generic reader intentionally accepts future vocabulary;
                // malformed vocabulary this observer knows is not 'unknown'.
                return Ok(malformed("malformed known budget or terminal event".into()));
            }
            _ => {}
        }
    }
    Ok(TraceBudgetObservation {
        state: TraceEvidenceState::Readable,
        main_action_budgets: (!budgets.is_empty()).then_some(budgets),
        child_terminal: terminal,
    })
}

fn malformed(error: String) -> TraceBudgetObservation {
    TraceBudgetObservation {
        state: TraceEvidenceState::Malformed { error },
        main_action_budgets: None,
        child_terminal: None,
    }
}

/// Retain an immutable byte copy and its parent-authored sidecar. Success
/// means both newly owned files are complete and the binding was read back.
/// On failure, old artifacts are untouched and any partial newly created file
/// is deliberately left as failed evidence, never returned as a valid pair.
pub fn retain_budget_trace(
    source: &Path,
    results_dir: &Path,
    identity: AttemptIdentity,
    evidence: &AttemptBudgetEvidence,
) -> io::Result<RetainedBudgetEvidence> {
    retain_with_hooks(
        source,
        results_dir,
        identity,
        evidence,
        || Ok(()),
        |_| Ok(()),
    )
}

fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Readers may verify while these handles exist, but another writer
        // cannot replace/modify the newly owned files before verification.
        options.share_mode(1); // FILE_SHARE_READ
    }
    options.open(path)
}

fn retain_with_hooks(
    source: &Path,
    results_dir: &Path,
    identity: AttemptIdentity,
    evidence: &AttemptBudgetEvidence,
    between_creates: impl FnOnce() -> io::Result<()>,
    before_sidecar_write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<RetainedBudgetEvidence> {
    identity.validate()?;
    let (trace_path, sidecar_path) = identity.paths();
    let destination = results_dir.join(&trace_path);
    let sidecar_destination = results_dir.join(&sidecar_path);
    let mut input = File::open(source)?;
    std::fs::create_dir_all(destination.parent().expect("identity paths have a parent"))?;

    // Reserve BOTH names before writing either. In particular a sidecar-only
    // collision must not overwrite its paired trace or publish a new success.
    let mut trace = create_new(&destination)?;
    between_creates()?;
    let mut sidecar = create_new(&sidecar_destination)?;
    io::copy(&mut input, &mut trace)?;
    trace.flush()?;
    trace.sync_all()?;
    let reference = RetainedBudgetEvidence {
        identity: identity.clone(),
        trace_path: trace_path.clone(),
        sidecar_path,
        trace_sha256: sha256_file(&destination)?,
    };
    let mut retained_evidence = evidence.clone();
    retained_evidence.trace = observe_trace(&destination)?;
    retained_evidence.retained = Some(reference.clone());
    let document = BudgetSidecarV1 {
        schema_version: BUDGET_SIDECAR_VERSION,
        identity,
        trace_path,
        trace_sha256: reference.trace_sha256.clone(),
        evidence: retained_evidence,
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
    before_sidecar_write(&mut sidecar)?;
    sidecar.write_all(&bytes)?;
    sidecar.write_all(b"\n")?;
    sidecar.flush()?;
    sidecar.sync_all()?;
    let verified = verify_budget_trace(results_dir, &reference)?;
    if verified != document {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "budget sidecar read-back differs from publication",
        ));
    }
    Ok(reference)
}

/// Read back a parent sidecar and prove its identity, path, digest, and child
/// observations agree with the referenced unchanged retained bytes.
pub fn verify_budget_trace(
    results_dir: &Path,
    reference: &RetainedBudgetEvidence,
) -> io::Result<BudgetSidecarV1> {
    reference.identity.validate()?;
    let (trace_path, sidecar_path) = reference.identity.paths();
    if reference.trace_path != trace_path || reference.sidecar_path != sidecar_path {
        return Err(invalid(
            "budget evidence paths do not match attempt identity",
        ));
    }
    let document: BudgetSidecarV1 =
        serde_json::from_slice(&std::fs::read(results_dir.join(sidecar_path))?)
            .map_err(io::Error::other)?;
    if document.schema_version != BUDGET_SIDECAR_VERSION
        || document.identity != reference.identity
        || document.trace_path != reference.trace_path
        || document.trace_sha256 != reference.trace_sha256
        || document.evidence.retained.as_ref() != Some(reference)
    {
        return Err(invalid(
            "budget sidecar version, identity or reference mismatch",
        ));
    }
    let trace = results_dir.join(trace_path);
    if sha256_file(&trace)? != reference.trace_sha256 {
        return Err(invalid("retained trace SHA-256 mismatch"));
    }
    if observe_trace(&trace)? != document.evidence.trace {
        return Err(invalid("retained trace observations differ from sidecar"));
    }
    let resolved = document
        .evidence
        .controls
        .resolve_agent(document.evidence.base_timeout_s)
        .map_err(invalid)?;
    if crate::budget::ExactDuration::from(resolved.duration())
        != document.evidence.enforced_duration
    {
        return Err(invalid(
            "retained enforced duration differs from checked controls",
        ));
    }
    Ok(document)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetControls, ParentTermination};

    fn evidence() -> AttemptBudgetEvidence {
        BudgetControls::new(0.125, Some(1024), 7.0, 4096)
            .unwrap()
            .resolve_agent(60)
            .unwrap()
            .evidence(Some(0), false)
    }

    fn identity() -> AttemptIdentity {
        AttemptIdentity::new("run-test", "trial-001", 0).unwrap()
    }

    fn trace_bytes(terminal: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (seq, event) in [
            serde_json::json!({"type":"main_action_budget","turn":0,"budget":{"requested":1024,"effective":1024,"declared_ctx":4096,"source":"explicit"}}),
            serde_json::json!({"type":"session_end","reason":terminal}),
        ].into_iter().enumerate() {
            serde_json::to_writer(&mut bytes, &serde_json::json!({"v":1,"ts_ms":1,"session":"child","seq":seq,"event":event})).unwrap();
            bytes.push(b'\n');
        }
        bytes
    }

    #[test]
    fn bench_budget_trace_sidecar_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jsonl");
        let bytes = trace_bytes("task_complete");
        std::fs::write(&source, &bytes).unwrap();
        let results = dir.path().join("results");
        let reference = retain_budget_trace(&source, &results, identity(), &evidence()).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        assert_eq!(
            std::fs::read(results.join(&reference.trace_path)).unwrap(),
            bytes
        );
        let sidecar = verify_budget_trace(&results, &reference).unwrap();
        assert_eq!(
            sidecar.evidence.trace.child_terminal.as_deref(),
            Some("task_complete")
        );
        assert_eq!(
            sidecar.evidence.trace.main_action_budgets.as_ref().unwrap()[0]
                .budget
                .effective,
            1024
        );
        assert_eq!(sidecar.evidence.enforced_duration.secs, 7);
        assert_eq!(sidecar.evidence.enforced_duration.nanos, 500_000_000);
        assert_eq!(sidecar.evidence.retained.as_ref(), Some(&reference));

        let mut bad_reference = reference.clone();
        bad_reference.identity.level = 1;
        assert!(verify_budget_trace(&results, &bad_reference).is_err());
        bad_reference = reference.clone();
        bad_reference.trace_sha256 = "0".repeat(64);
        assert!(verify_budget_trace(&results, &bad_reference).is_err());
        std::fs::write(results.join(&reference.trace_path), b"tampered").unwrap();
        assert!(verify_budget_trace(&results, &reference).is_err());
    }

    #[test]
    fn bench_budget_pair_collision_preserves_evidence() {
        // Pre-existing trace-only, sidecar-only and pair, plus a sidecar
        // contender winning between the two create-new calls.
        for mode in ["trace", "sidecar", "pair", "race"] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source.jsonl");
            std::fs::write(&source, trace_bytes("task_complete")).unwrap();
            let (trace_rel, sidecar_rel) = identity().paths();
            let trace = dir.path().join(trace_rel);
            let sidecar = dir.path().join(sidecar_rel);
            std::fs::create_dir_all(trace.parent().unwrap()).unwrap();
            if matches!(mode, "trace" | "pair") {
                std::fs::write(&trace, b"OLD TRACE").unwrap();
            }
            if matches!(mode, "sidecar" | "pair") {
                std::fs::write(&sidecar, b"OLD SIDECAR").unwrap();
            }
            let result = retain_with_hooks(
                &source,
                dir.path(),
                identity(),
                &evidence(),
                || {
                    if mode == "race" {
                        create_new(&sidecar)?.write_all(b"RACING SIDECAR")?;
                    }
                    Ok(())
                },
                |_| Ok(()),
            );
            assert!(result.is_err(), "{mode}");
            if matches!(mode, "trace" | "pair") {
                assert_eq!(std::fs::read(&trace).unwrap(), b"OLD TRACE");
            }
            if matches!(mode, "sidecar" | "pair") {
                assert_eq!(std::fs::read(&sidecar).unwrap(), b"OLD SIDECAR");
            }
            if mode == "race" {
                assert_eq!(std::fs::read(&sidecar).unwrap(), b"RACING SIDECAR");
            }
            if matches!(mode, "sidecar" | "race") {
                assert!(
                    std::fs::read(&trace).unwrap().is_empty(),
                    "partial new trace must not masquerade as a publication"
                );
            }
        }
    }

    #[test]
    fn bench_budget_recording_failure_is_infrastructure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jsonl");
        let bytes = trace_bytes("task_complete");
        std::fs::write(&source, &bytes).unwrap();
        let error = retain_with_hooks(
            &source,
            dir.path(),
            identity(),
            &evidence(),
            || Ok(()),
            |file| {
                file.write_all(b"partial sidecar")?;
                Err(io::Error::other("injected sidecar write failure"))
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("injected"));
        let (trace, sidecar) = identity().paths();
        assert_eq!(std::fs::read(dir.path().join(trace)).unwrap(), bytes);
        assert_eq!(
            std::fs::read(dir.path().join(sidecar)).unwrap(),
            b"partial sidecar"
        );
        // Failure returns no usable reference; retry may not clobber either
        // failed artifact to turn the observation into a success.
        assert!(retain_budget_trace(&source, dir.path(), identity(), &evidence()).is_err());
    }

    #[test]
    fn budget_sidecar_rejects_tampered_metadata_and_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jsonl");
        std::fs::write(&source, trace_bytes("task_complete")).unwrap();
        let reference = retain_budget_trace(&source, dir.path(), identity(), &evidence()).unwrap();
        let sidecar_path = dir.path().join(&reference.sidecar_path);
        let original: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&sidecar_path).unwrap()).unwrap();
        for (pointer, replacement) in [
            ("/schema_version", serde_json::json!(999)),
            ("/identity/run_id", serde_json::json!("run-relabeled")),
            ("/identity/trial_id", serde_json::json!("trial-relabeled")),
            ("/identity/level", serde_json::json!(1)),
            ("/trace_path", serde_json::json!("../outside.jsonl")),
            ("/evidence/enforced_duration/secs", serde_json::json!(8)),
            (
                "/evidence/enforced_duration/nanos",
                serde_json::json!(1_000_000_000u32),
            ),
            ("/evidence/base_timeout_s", serde_json::json!(0)),
            (
                "/evidence/trace/child_terminal",
                serde_json::json!("provider_error"),
            ),
            (
                "/evidence/trace/main_action_budgets/0/budget/effective",
                serde_json::json!(1),
            ),
            ("/evidence/controls/timeout_scale", serde_json::json!(0)),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            std::fs::write(&sidecar_path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                verify_budget_trace(dir.path(), &reference).is_err(),
                "accepted {pointer}"
            );
        }
        std::fs::write(&sidecar_path, b"{malformed").unwrap();
        assert!(verify_budget_trace(dir.path(), &reference).is_err());
        std::fs::write(&sidecar_path, serde_json::to_vec(&original).unwrap()).unwrap();
        verify_budget_trace(dir.path(), &reference).unwrap();
    }

    #[test]
    fn benchmark_termination_causes_remain_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        for terminal in ["provider_error", "truncated_action", "task_complete"] {
            std::fs::write(&path, trace_bytes(terminal)).unwrap();
            let mut evidence = evidence();
            evidence.observe_trace(Some(&path)).unwrap();
            assert_eq!(evidence.trace.child_terminal.as_deref(), Some(terminal));
            assert_eq!(
                evidence.parent_termination,
                ParentTermination::Exited { exit_code: Some(0) }
            );
        }
        let controls = BudgetControls::new(0.5, None, 7.0, 4096).unwrap();
        let parent = controls.resolve_agent(60).unwrap().evidence(None, true);
        assert_eq!(
            parent.parent_termination,
            ParentTermination::ExecutionTimeout
        );
        assert_eq!(parent.trace, TraceBudgetObservation::missing());
    }

    #[test]
    fn budget_observation_missing_malformed_and_future_vocabulary() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jsonl");
        let mut evidence = evidence();
        evidence.observe_trace(None).unwrap();
        assert_eq!(evidence.trace, TraceBudgetObservation::missing());
        for bytes in [b"{partial".as_slice(), b"{\"v\":1,\"ts_ms\":1,\"session\":\"s\",\"seq\":0,\"event\":{\"type\":\"main_action_budget\",\"turn\":0,\"budget\":{\"effective\":\"bad\"}}}\n", b"{\"v\":1,\"ts_ms\":1,\"session\":\"s\",\"seq\":0,\"event\":{\"type\":\"session_end\",\"reason\":123}}\n"] {
            std::fs::write(&source, bytes).unwrap();
            evidence.observe_trace(Some(&source)).unwrap();
            assert!(matches!(evidence.trace.state, TraceEvidenceState::Malformed { .. }));
            assert!(evidence.trace.main_action_budgets.is_none());
            assert!(evidence.trace.child_terminal.is_none());
        }
        std::fs::write(&source, b"{\"v\":1,\"ts_ms\":1,\"session\":\"s\",\"seq\":0,\"event\":{\"type\":\"future_event\"}}\n").unwrap();
        evidence.observe_trace(Some(&source)).unwrap();
        assert_eq!(evidence.trace.state, TraceEvidenceState::Readable);
        assert!(evidence.trace.main_action_budgets.is_none());
    }
}
