//! Integration tests for the ICM workspace model (sprint 73, ADR-064):
//! scaffold -> discover -> compose -> plan on real temp-dir workspaces, plus the
//! workspace-boundary guarantee that a contract cannot pull outside context.

use std::fs;

use ferric_icm::{IcmWorkspace, Layer, compose_stage, plan, scaffold_workspace};
use tempfile::TempDir;

/// Scaffold into a fresh subdir of a tempdir and return (tempdir, root).
fn scaffolded() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    scaffold_workspace(&root).unwrap();
    (tmp, root)
}

#[test]
fn scaffold_then_discover_finds_three_ordered_stages() {
    let (_tmp, root) = scaffolded();
    let ws = IcmWorkspace::discover(&root).unwrap();

    assert_eq!(ws.stages.len(), 3);
    assert_eq!(ws.stages[0].name, "01_research");
    assert_eq!(ws.stages[1].name, "02_script");
    assert_eq!(ws.stages[2].name, "03_production");
    // Numeric order is execution order.
    assert_eq!(ws.stages[0].index, 1);
    assert_eq!(ws.stages[2].index, 3);
    // Layer 0/1 were scaffolded and picked up.
    assert_eq!(ws.identity_file.as_deref(), Some("Animus.md"));
    assert!(ws.routing.is_some());
}

#[test]
fn discovery_orders_stages_numerically_not_lexically() {
    // 2 must sort before 10 (lexical order would invert them).
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    for name in ["stages/02_b", "stages/10_c", "stages/01_a"] {
        fs::create_dir_all(root.join(name)).unwrap();
        fs::write(root.join(name).join("CONTEXT.md"), "## Process\ndo\n").unwrap();
    }
    let ws = IcmWorkspace::discover(root).unwrap();
    let order: Vec<u32> = ws.stages.iter().map(|s| s.index).collect();
    assert_eq!(order, vec![1, 2, 10]);
}

#[test]
fn compose_scopes_layers_and_wires_prior_output_as_layer4() {
    let (_tmp, root) = scaffolded();
    // Populate stage 1's output and the shared voice file so stage 2 (script)
    // has real Layer 4 + Layer 3 inputs to pull.
    fs::write(
        root.join("stages/01_research/output/research.md"),
        "KEY FINDING: ferrets are agile.",
    )
    .unwrap();
    fs::write(root.join("_config/voice.md"), "Voice: terse and factual.").unwrap();

    let ws = IcmWorkspace::discover(&root).unwrap();
    let script = compose_stage(&ws, 1).unwrap(); // 02_script

    // The composed prompt carries identity (L0), routing (L1), the contract
    // (L2), the prior stage output (L4), and the voice reference (L3).
    assert!(script.prompt.contains("Layer 0"));
    assert!(script.prompt.contains("KEY FINDING: ferrets are agile."));
    assert!(script.prompt.contains("Voice: terse and factual."));

    // Provenance names every layer that contributed, and the L4 source is the
    // upstream stage's output file.
    let layers: Vec<u8> = script.provenance.iter().map(|p| p.layer).collect();
    assert!(layers.contains(&0) && layers.contains(&1) && layers.contains(&2));
    assert!(
        script.provenance.iter().any(|p| p.layer == 4
            && p.source.contains("01_research/output/research.md")
            && p.present)
    );
    assert!(
        script
            .provenance
            .iter()
            .any(|p| p.layer == 3 && p.source.contains("_config/voice.md") && p.present)
    );
}

#[test]
fn composed_prompt_carries_the_output_directive() {
    // A live stage agent must be told where to write; the composed prompt
    // includes the contract's Outputs (sprint 74).
    let (_tmp, root) = scaffolded();
    let ws = IcmWorkspace::discover(&root).unwrap();
    let research = compose_stage(&ws, 0).unwrap(); // 01_research -> research.md
    assert!(
        research.prompt.contains("Outputs") && research.prompt.contains("research.md"),
        "the stage's output contract must reach the agent; got:\n{}",
        research.prompt
    );
}

#[test]
fn missing_input_is_recorded_not_fatal() {
    // A freshly scaffolded workspace has an empty 01_research/output/, so
    // stage 2's declared Layer 4 input is absent — plan must still succeed and
    // record the gap.
    let (_tmp, root) = scaffolded();
    let ws = IcmWorkspace::discover(&root).unwrap();
    let script = compose_stage(&ws, 1).unwrap();
    assert!(
        script.provenance.iter().any(|p| p.layer == 4 && !p.present),
        "the empty upstream output must be recorded as a missing Layer 4 input"
    );
}

#[test]
fn plan_composes_every_stage_in_order() {
    let (_tmp, root) = scaffolded();
    let ws = IcmWorkspace::discover(&root).unwrap();
    let p = plan(&ws).unwrap();
    assert_eq!(p.stages.len(), 3);
    assert_eq!(p.stages[0].name, "01_research");
    assert_eq!(p.stages[2].name, "03_production");
}

#[test]
fn a_contract_cannot_pull_context_from_outside_the_workspace() {
    let (_tmp, root) = scaffolded();
    // Rewrite stage 1's contract to point a Layer 3 input outside the root.
    fs::write(
        root.join("stages/01_research/CONTEXT.md"),
        "## Inputs\n- Layer 3 (reference): ../../../../../../etc/passwd\n\n## Process\nleak\n",
    )
    .unwrap();
    let ws = IcmWorkspace::discover(&root).unwrap();
    let err = compose_stage(&ws, 0).unwrap_err();
    assert!(
        matches!(err, ferric_icm::IcmError::Boundary(_)),
        "an escaping input path must be refused at the workspace boundary, got {err:?}"
    );
}

#[test]
fn scaffold_refuses_to_clobber_a_nonempty_dir() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("keep.txt"), "important").unwrap();

    let err = scaffold_workspace(&root).unwrap_err();
    assert!(matches!(err, ferric_icm::IcmError::TargetNotEmpty(_)));
    // The pre-existing file is untouched.
    assert_eq!(
        fs::read_to_string(root.join("keep.txt")).unwrap(),
        "important"
    );
}

#[test]
fn discover_rejects_a_non_workspace() {
    let tmp = TempDir::new().unwrap();
    let err = IcmWorkspace::discover(tmp.path()).unwrap_err();
    assert!(matches!(err, ferric_icm::IcmError::NotAWorkspace(_)));
}

#[test]
fn reference_and_working_layers_are_classified() {
    let (_tmp, root) = scaffolded();
    let ws = IcmWorkspace::discover(&root).unwrap();
    // 02_script declares one working (L4) and one reference (L3) input.
    let script = &ws.stages[1];
    let layers: Vec<Layer> = script.contract.inputs.iter().map(|i| i.layer).collect();
    assert!(layers.contains(&Layer::Working));
    assert!(layers.contains(&Layer::Reference));
}
