use std::fmt;
use std::path::Path;

use ferric_guard::{PermissionLevel, Workspace};
use sha2::{Digest, Sha256};

/// Side-effect-free context supplied while a model-authored call is prepared.
///
/// Preparation may inspect workspace state and construct exact candidate data,
/// but it must not mutate the workspace. `truncation_limit` is included so an
/// observation can describe the view the model will actually receive while the
/// trace still retains the complete tool output.
pub struct PrepareCtx<'a> {
    pub workspace: &'a Workspace,
    pub truncation_limit: usize,
}

/// Inclusive, one-indexed line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u64,
    pub end: u64,
}

/// The range arguments supplied to `read_file`; `None` means the argument was
/// omitted, rather than a value inferred by the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestedLineRange {
    pub start: Option<u64>,
    pub end: Option<u64>,
}

/// Typed evidence produced by a successful controlled `read_file` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileObservation {
    /// Canonical workspace-relative path using `/` separators.
    pub path: String,
    /// SHA-256 of the complete raw file bytes, independent of pagination.
    pub sha256: String,
    pub bytes: u64,
    pub total_lines: u64,
    pub requested: RequestedLineRange,
    pub returned: Option<LineRange>,
    /// True when the registry's model-facing truncation will shorten the full
    /// output retained by the trace.
    pub model_truncated: bool,
    /// True only when the model received a complete, untruncated file view.
    pub complete: bool,
    /// Untruncated ranges eligible for a later evidence ledger. A truncated
    /// result deliberately establishes no coverage, even if its visible prefix
    /// happened to contain one or more complete lines.
    pub coverage: Vec<LineRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKind {
    FindFiles,
    SearchFiles,
}

/// Typed result of a literal repository-navigation operation. Navigation can
/// locate candidate paths but never establishes file-content coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationObservation {
    pub kind: NavigationKind,
    pub root: String,
    pub literal: String,
    /// SHA-256 of the canonical result-set bytes: result lines joined with
    /// `\n`, or the empty byte string for zero results. This never hashes the
    /// human envelope or zero-result prose around the result.
    pub result_sha256: String,
    pub matches: u64,
    pub limit: u64,
    /// The returned set filled the requested cap.
    pub cap_reached: bool,
    /// At least one additional match existed beyond the returned set.
    pub has_more: bool,
    pub model_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolObservation {
    File(FileObservation),
    Navigation(NavigationObservation),
}

/// Exact prior content an eventual evidence controller must have observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRequirement {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    CreateFile,
    ModifyFile,
    DeleteFile,
    MovePath,
    CreateDirectory,
    DeleteDirectory,
}

/// Public, byte-redacted description of a prepared mutation. Exact candidate
/// bytes remain private to the consumed preparation object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationIntent {
    pub kind: MutationKind,
    pub requirements: Vec<ObservationRequirement>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationIntent {
    pub name: String,
}

/// Safety-relevant meaning of a prepared call. Controllers consume this value;
/// they never infer meaning from a tool name or human-readable output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedIntent {
    ReadOnly,
    FileObservation(FileObservation),
    Navigation(NavigationObservation),
    Mutation(MutationIntent),
    Verification(VerificationIntent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareErrorKind {
    InvalidArguments,
    Io,
    OutputLimitTooSmall,
    NoEffect,
    SyntaxRejected,
    OpaqueMutation,
    UnsupportedOperation,
}

/// Typed preparation failure with a separate model-facing diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareError {
    pub kind: PrepareErrorKind,
    pub message: String,
}

impl PrepareError {
    pub fn new(kind: PrepareErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn opaque(permission: PermissionLevel) -> Self {
        Self::new(
            PrepareErrorKind::OpaqueMutation,
            format!(
                "controlled execution rejected an opaque {permission:?} tool; implement typed preparation before offering it to an evidence-policy model"
            ),
        )
    }
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PrepareError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathState {
    Absent,
    File {
        sha256: String,
        bytes: u64,
        /// Logical text-line count: empty is zero; otherwise newline count
        /// plus one when the final byte is not `\n`.
        lines: u64,
    },
    Directory,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEffect {
    pub path: String,
    pub before: PathState,
    pub after: PathState,
}

/// `Measured([])` is positive proof of no byte/path effect. It is intentionally
/// distinct from a legacy call for which no measurement was attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEffectReport {
    Measured(Vec<WorkspaceEffect>),
    /// A compatibility read adapter ran under its declared read-only contract,
    /// but the registry did not independently compare workspace state.
    UnmeasuredReadOnly,
    UnmeasuredLegacy,
}

impl WorkspaceEffectReport {
    pub fn measured_none() -> Self {
        Self::Measured(Vec::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFailureKind {
    ToolError,
    StalePrecondition,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFailure {
    pub kind: ControlFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationAttempt {
    pub name: String,
    pub passed: bool,
}

/// Metadata independent from the textual success/error channel.
///
/// In particular, an errored future mutation may still carry non-empty measured
/// effects when an I/O operation changed bytes before failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlMetadata {
    pub observation: Option<ToolObservation>,
    pub verification: Option<VerificationAttempt>,
    pub effects: WorkspaceEffectReport,
    pub failure: Option<ControlFailure>,
}

/// Prepared execution owned by `ToolPreparation`. The mutation variants are
/// intentionally private to this crate; public controller-facing state is the
/// redacted `PreparedIntent` above.
pub(crate) enum PreparedExecution {
    Deferred {
        effects: WorkspaceEffectReport,
    },
    Immediate {
        full: String,
        is_error: bool,
        effects: WorkspaceEffectReport,
        failure: Option<ControlFailure>,
    },
}

/// Side-effect-free tool preparation returned through the controlled registry
/// path. Fields stay private so an intent cannot be separated from the exact
/// operation that produced it.
pub struct ToolPreparation {
    pub(crate) intent: PreparedIntent,
    pub(crate) execution: PreparedExecution,
}

impl ToolPreparation {
    pub fn deferred_read_only() -> Self {
        Self {
            intent: PreparedIntent::ReadOnly,
            execution: PreparedExecution::Deferred {
                effects: WorkspaceEffectReport::UnmeasuredReadOnly,
            },
        }
    }

    pub fn file_observation(full: String, observation: FileObservation) -> Self {
        Self {
            intent: PreparedIntent::FileObservation(observation),
            execution: PreparedExecution::Immediate {
                full,
                is_error: false,
                effects: WorkspaceEffectReport::measured_none(),
                failure: None,
            },
        }
    }

    pub fn navigation(full: String, observation: NavigationObservation) -> Self {
        Self {
            intent: PreparedIntent::Navigation(observation),
            execution: PreparedExecution::Immediate {
                full,
                is_error: false,
                effects: WorkspaceEffectReport::measured_none(),
                failure: None,
            },
        }
    }
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn logical_line_count(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        return 0;
    }

    let newlines = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
    newlines + u64::from(bytes.last() != Some(&b'\n'))
}

pub(crate) fn normalized_relative_path(
    workspace: &Workspace,
    resolved: &Path,
) -> Result<String, PrepareError> {
    let relative = resolved.strip_prefix(workspace.root()).map_err(|_| {
        PrepareError::new(
            PrepareErrorKind::Io,
            format!(
                "resolved path {} is not relative to workspace {}",
                resolved.display(),
                workspace.root().display()
            ),
        )
    })?;
    let normalized = relative.to_string_lossy().replace('\\', "/");
    Ok(if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    })
}

#[cfg(test)]
mod tests {
    use super::logical_line_count;

    #[test]
    fn logical_lines_preserve_empty_crlf_and_trailing_newline_semantics() {
        assert_eq!(logical_line_count(b""), 0);
        assert_eq!(logical_line_count(b"one"), 1);
        assert_eq!(logical_line_count(b"one\n"), 1);
        assert_eq!(logical_line_count(b"one\r\ntwo\r\n"), 2);
        assert_eq!(logical_line_count(b"one\r\ntwo"), 2);
    }
}
