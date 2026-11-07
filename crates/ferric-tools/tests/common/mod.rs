//! Fixtures shared by the ferric-tools integration tests. Each test binary
//! uses a different subset.
#![allow(dead_code)]

use ferric_guard::{Provenance, SinkPolicy, Workspace};
use ferric_tools::{
    ControlledOutcome, DEFAULT_TRUNCATION_LIMIT, ExecuteOutcome, PrepareError, PrepareOutcome,
    PreparedCall, PreparedIntent, Registry, register_builtin_tools,
};
use serde_json::Value;

/// A temporary workspace and a registry holding the builtin tools.
pub fn setup() -> (tempfile::TempDir, Workspace, Registry) {
    setup_with_limit(DEFAULT_TRUNCATION_LIMIT)
}

/// [`setup`] with a model-facing truncation limit.
pub fn setup_with_limit(limit: usize) -> (tempfile::TempDir, Workspace, Registry) {
    let directory = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(directory.path()).unwrap();
    let mut registry = Registry::with_truncation_limit(limit);
    register_builtin_tools(&mut registry);
    (directory, workspace, registry)
}

/// Legacy execution of a clean run under a deny sink policy.
pub trait RegistryTestExt {
    fn execute_test(&self, workspace: &Workspace, name: &str, args: &Value) -> ExecuteOutcome;
}

impl RegistryTestExt for Registry {
    fn execute_test(&self, workspace: &Workspace, name: &str, args: &Value) -> ExecuteOutcome {
        self.execute(
            workspace,
            name,
            args,
            Provenance::Clean,
            &SinkPolicy::deny(),
            None,
        )
    }
}

/// A controlled preparation that must succeed.
pub fn prepare<'a>(
    registry: &'a Registry,
    workspace: &'a Workspace,
    tool: &str,
    args: &Value,
) -> PreparedCall<'a> {
    match registry.prepare_controlled(workspace, tool, args) {
        PrepareOutcome::Prepared(prepared) => prepared,
        other => panic!("expected {tool} preparation, got {other:?}"),
    }
}

/// A controlled preparation that must be rejected.
pub fn rejected(
    registry: &Registry,
    workspace: &Workspace,
    tool: &str,
    args: &Value,
) -> PrepareError {
    match registry.prepare_controlled(workspace, tool, args) {
        PrepareOutcome::Rejected { error, .. } => error,
        other => panic!("expected {tool} rejection, got {other:?}"),
    }
}

/// Commit an admitted preparation for a clean run.
pub fn commit(registry: &Registry, prepared: PreparedCall<'_>) -> ControlledOutcome {
    registry.commit_admitted(prepared, Provenance::Clean, &SinkPolicy::deny(), None)
}

/// Prepare and commit one call, returning its intent and outcome.
pub fn controlled_call(
    registry: &Registry,
    workspace: &Workspace,
    tool: &str,
    args: &Value,
) -> (PreparedIntent, ControlledOutcome) {
    let prepared = prepare(registry, workspace, tool, args);
    let intent = prepared.intent().clone();
    (intent, commit(registry, prepared))
}
