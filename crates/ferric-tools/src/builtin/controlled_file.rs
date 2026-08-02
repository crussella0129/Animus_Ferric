use std::ffi::OsString;
use std::fs::{File, Metadata, Permissions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt as _;
use cap_fs_ext::{
    DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use ferric_guard::Workspace;

use crate::control::{
    CandidatePathState, ControlFailure, ControlFailureKind, ControlFailureWitness,
    FileMutationCandidate, MutationIntent, MutationKind, NoEffectKind, ObservationRequirement,
    PathState, PrepareCtx, PrepareError, PrepareErrorKind, PrepareFailureWitness,
    StaleObservationWitness, ToolPreparation, UnsupportedMutationKind, WorkspaceEffect,
    WorkspaceEffectKind, WorkspaceEffectReport, logical_line_count, sha256_bytes,
};

use super::check_syntax::candidate_syntax_transition;

pub(crate) struct InspectedFileTarget {
    path: String,
    state: PathState,
    bytes: Option<Vec<u8>>,
}

struct ObservedTarget {
    state: PathState,
    bytes: Option<Vec<u8>>,
    unsupported_reason: Option<String>,
    identity: Option<FileIdentity>,
    permissions: Option<Permissions>,
}

struct ShapeFailure {
    message: String,
    observed: PathState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

/// Capability handles for every directory component from the workspace root
/// through the target's parent. On Windows cap-std opens these without delete
/// sharing, so names cannot be replaced while the chain is alive. On Unix the
/// dirfds keep every lookup relative and non-escaping; identity revalidation
/// brackets publication because POSIX permits renaming an open directory.
struct PinnedParent {
    chain: Vec<Dir>,
    identities: Vec<FileIdentity>,
    leaf: OsString,
    #[cfg(windows)]
    absolute: PathBuf,
}

impl PinnedParent {
    fn dir(&self) -> &Dir {
        self.chain.last().expect("pinned chain always has its root")
    }
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) struct FileCommitResult {
    pub full: String,
    pub is_error: bool,
    pub effects: WorkspaceEffectReport,
    pub failure: Option<ControlFailure>,
}

#[derive(Clone, Copy)]
enum WriteFault {
    None,
    #[cfg(test)]
    AfterPrefix(usize),
}

/// Inspect a controlled mutation target without following its final component.
/// Regular-file bytes and metadata are obtained from the same opened handle;
/// pathname metadata is used only to classify absent/non-file outcomes.
pub(crate) fn inspect_for_prepare(
    ctx: &PrepareCtx<'_>,
    requested_path: &str,
    allow_absent: bool,
) -> Result<InspectedFileTarget, PrepareError> {
    let (path, absolute) = logical_target(ctx.workspace, requested_path)?;
    let parent = pin_parent(ctx.workspace, &absolute)
        .map_err(|failure| unsupported_path_error(&path, failure.message))?;
    let observed = observe_target(&parent)
        .map_err(|message| PrepareError::new(PrepareErrorKind::Io, message))?;

    if let Some(reason) = observed.unsupported_reason {
        return Err(unsupported_path_error(&path, reason));
    }
    match observed.state {
        PathState::Directory => {
            return Err(unsupported_path_error(
                &path,
                "target is a directory".to_string(),
            ));
        }
        PathState::Other => {
            return Err(unsupported_path_error(
                &path,
                "target is not a regular file".to_string(),
            ));
        }
        PathState::Absent if !allow_absent => {
            let state = CandidatePathState {
                path: path.clone(),
                before: PathState::Absent,
                candidate: PathState::Absent,
            };
            return Err(no_effect_error(
                NoEffectKind::MatchNotFound,
                state,
                format!("file not found: {path}"),
            ));
        }
        PathState::Absent | PathState::File { .. } => {}
    }

    Ok(InspectedFileTarget {
        path,
        state: observed.state,
        bytes: observed.bytes,
    })
}

pub(crate) fn utf8_preimage<'a>(
    target: &'a InspectedFileTarget,
    operation: &str,
) -> Result<&'a str, PrepareError> {
    let bytes = target.bytes.as_deref().ok_or_else(|| {
        PrepareError::new(
            PrepareErrorKind::NoEffect,
            format!("{operation}: file not found: {}", target.path),
        )
    })?;
    std::str::from_utf8(bytes).map_err(|error| {
        PrepareError::new(
            PrepareErrorKind::UnsupportedOperation,
            format!(
                "{operation}: {} is not valid UTF-8 and cannot be text-edited: {error}",
                target.path
            ),
        )
        .with_witness(PrepareFailureWitness::UnsupportedMutation(
            UnsupportedMutationKind::UnsupportedOperation,
        ))
    })
}

pub(crate) fn reject_unchanged(
    target: &InspectedFileTarget,
    kind: NoEffectKind,
    message: impl Into<String>,
) -> PrepareError {
    no_effect_error(
        kind,
        CandidatePathState {
            path: target.path.clone(),
            before: target.state.clone(),
            candidate: target.state.clone(),
        },
        message.into(),
    )
}

pub(crate) fn compile_candidate(
    target: InspectedFileTarget,
    candidate: Vec<u8>,
    no_effect_kind: NoEffectKind,
    success: String,
) -> Result<ToolPreparation, PrepareError> {
    let candidate_state = state_for_bytes(&candidate);
    let path_state = CandidatePathState {
        path: target.path.clone(),
        before: target.state.clone(),
        candidate: candidate_state,
    };
    if target.bytes.as_deref() == Some(candidate.as_slice()) {
        return Err(no_effect_error(
            no_effect_kind,
            path_state,
            format!("candidate produces no byte change for {}", target.path),
        ));
    }

    let syntax =
        candidate_syntax_transition(Path::new(&target.path), target.bytes.as_deref(), &candidate);
    if syntax.blocks_mutation() {
        return Err(PrepareError::new(
            PrepareErrorKind::SyntaxRejected,
            format!(
                "candidate syntax regression for {} (before {:?}, candidate invalid)",
                target.path, syntax.before
            ),
        )
        .with_witness(PrepareFailureWitness::SyntaxRegression(syntax)));
    }

    let mut success = success;
    if let Some(warning) = &syntax.warning {
        success.push_str("\n⚠ ");
        success.push_str(warning);
    }
    let kind = if matches!(target.state, PathState::Absent) {
        MutationKind::CreateFile
    } else {
        MutationKind::ModifyFile
    };
    let requirements = match &target.state {
        PathState::File { sha256, .. } => vec![ObservationRequirement {
            path: target.path.clone(),
            sha256: sha256.clone(),
        }],
        PathState::Absent => Vec::new(),
        PathState::Directory | PathState::Other => unreachable!("unsupported target rejected"),
    };
    let intent = MutationIntent {
        kind,
        requirements,
        paths: vec![target.path.clone()],
        states: vec![path_state],
        syntax: Some(syntax),
    };
    let operation = FileMutationCandidate {
        path: target.path,
        expected: target.state,
        candidate,
        success,
    };
    Ok(ToolPreparation::file_mutation(intent, operation))
}

/// Publish exact candidate bytes through a fresh inode under a capability-pinned
/// parent. Existing inodes are never modified, so hard-link aliases cannot turn
/// a workspace edit into an out-of-workspace write. Directory and leaf identity
/// are revalidated immediately before atomic publication and verified again at
/// the intended path afterward.
pub(crate) fn commit_candidate(
    workspace: &Workspace,
    operation: FileMutationCandidate,
) -> FileCommitResult {
    commit_candidate_with(workspace, operation, WriteFault::None, |_| {})
}

fn commit_candidate_with<F>(
    workspace: &Workspace,
    operation: FileMutationCandidate,
    fault: WriteFault,
    before_open: F,
) -> FileCommitResult
where
    F: FnOnce(&Path),
{
    let absolute = workspace.root().join(Path::new(&operation.path));
    let parent = match pin_parent(workspace, &absolute) {
        Ok(parent) => parent,
        Err(failure) => {
            return stale_result(
                operation.path,
                operation.expected,
                failure.observed,
                failure.message,
            );
        }
    };
    before_open(&absolute);

    if let Err(failure) = validate_parent_binding(workspace, &absolute, &parent) {
        return stale_result(
            operation.path,
            operation.expected,
            failure.observed,
            failure.message,
        );
    }
    if matches!(operation.expected, PathState::Directory | PathState::Other) {
        return failure_result(
            ControlFailureKind::Io,
            "invalid prepared mutation state".to_string(),
            None,
            Vec::new(),
        );
    }

    let first = match observe_target(&parent) {
        Ok(observed) => observed,
        Err(message) => {
            return failure_result(ControlFailureKind::Io, message, None, Vec::new());
        }
    };
    if first.state != operation.expected || first.unsupported_reason.is_some() {
        return stale_result(
            operation.path,
            operation.expected,
            first.state,
            first
                .unsupported_reason
                .unwrap_or_else(|| "target state changed after preparation".to_string()),
        );
    }
    #[cfg(windows)]
    if first
        .permissions
        .as_ref()
        .is_some_and(std::fs::Permissions::readonly)
    {
        return failure_result(
            ControlFailureKind::Io,
            format!("controlled target is read-only: {}", operation.path),
            None,
            Vec::new(),
        );
    }

    let (mut candidate_file, mut temporary) = match create_temporary(&parent, &operation.path) {
        Ok(value) => value,
        Err(message) => {
            return failure_result(ControlFailureKind::Io, message, None, Vec::new());
        }
    };
    if let Some(permissions) = first.permissions.clone()
        && let Err(error) = candidate_file.set_permissions(permissions)
    {
        return cleanup_failure(
            &mut temporary,
            format!("set candidate permissions for {}: {error}", operation.path),
        );
    }
    if let Err(error) = write_candidate(&mut candidate_file, &operation.candidate, fault) {
        drop(candidate_file);
        return cleanup_failure(
            &mut temporary,
            format!("write candidate for {}: {error}", operation.path),
        );
    }
    if let Err(error) = candidate_file.sync_data() {
        drop(candidate_file);
        return cleanup_failure(
            &mut temporary,
            format!("sync candidate for {}: {error}", operation.path),
        );
    }
    drop(candidate_file);

    if let Err(failure) = validate_parent_binding(workspace, &absolute, &parent) {
        return cleanup_stale(
            &mut temporary,
            &operation,
            failure.observed,
            failure.message,
        );
    }
    let second = match observe_target(&parent) {
        Ok(observed) => observed,
        Err(message) => return cleanup_failure(&mut temporary, message),
    };
    if second.state != operation.expected
        || second.unsupported_reason.is_some()
        || second.identity != first.identity
    {
        return cleanup_stale(
            &mut temporary,
            &operation,
            second.state,
            second
                .unsupported_reason
                .unwrap_or_else(|| "target identity changed before atomic publication".to_string()),
        );
    }

    let candidate_state = state_for_bytes(&operation.candidate);
    let temporary_observation = match observe_leaf(parent.dir(), &temporary.name) {
        Ok(observed) => observed,
        Err(message) => return cleanup_failure(&mut temporary, message),
    };
    if temporary_observation.state != candidate_state
        || temporary_observation.identity != Some(temporary.identity)
        || temporary_observation.unsupported_reason.is_some()
    {
        return cleanup_failure(
            &mut temporary,
            format!(
                "controlled candidate identity changed before publishing {}",
                operation.path
            ),
        );
    }

    match publish_temporary(&parent, &temporary.name, &operation.expected) {
        Ok(()) => temporary.disarm(),
        Err(error) if error.target_published => {
            if let Err(cleanup) = temporary.cleanup() {
                let observed = observe_target(&parent)
                    .map(|value| value.state)
                    .unwrap_or(PathState::Other);
                let mut effects =
                    measured_effects(&operation.path, operation.expected.clone(), observed);
                effects.extend(temporary.measured_effects());
                return failure_result(
                    ControlFailureKind::Io,
                    format!(
                        "publish {} succeeded but candidate cleanup failed: {}; cleanup: {cleanup}",
                        operation.path, error.error
                    ),
                    None,
                    effects,
                );
            }
            // The no-clobber publication succeeded and a retry removed the
            // private link, so normal postimage verification decides success.
        }
        Err(error) => {
            let observed = observe_target(&parent).ok();
            let cleanup_error = temporary.cleanup().err().map(|error| error.to_string());
            if let Some(observed) = observed
                && observed.state != operation.expected
            {
                if let Some(cleanup) = cleanup_error {
                    return failure_result(
                        ControlFailureKind::Io,
                        format!(
                            "target changed while publishing {}; candidate cleanup failed: {cleanup}",
                            operation.path
                        ),
                        None,
                        temporary.measured_effects(),
                    );
                }
                return stale_result(
                    operation.path,
                    operation.expected,
                    observed.state,
                    observed.unsupported_reason.unwrap_or_else(|| {
                        format!("target changed while publishing: {}", error.error)
                    }),
                );
            }
            let detail = cleanup_error.map_or_else(
                || error.error.to_string(),
                |cleanup| format!("{}; cleanup: {cleanup}", error.error),
            );
            return failure_result(
                ControlFailureKind::Io,
                format!("publish {}: {detail}", operation.path),
                None,
                temporary.measured_effects(),
            );
        }
    }

    let verified_parent = match pin_parent(workspace, &absolute) {
        Ok(value) if value.identities == parent.identities => value,
        Ok(_) => {
            return failure_result(
                ControlFailureKind::Io,
                format!(
                    "parent identity changed after publishing {}",
                    operation.path
                ),
                None,
                vec![opaque_effect(
                    &operation.path,
                    operation.expected,
                    PathState::Other,
                )],
            );
        }
        Err(failure) => {
            return failure_result(
                ControlFailureKind::Io,
                format!(
                    "cannot verify intended path after publishing {}: {}",
                    operation.path, failure.message
                ),
                None,
                vec![opaque_effect(
                    &operation.path,
                    operation.expected,
                    failure.observed,
                )],
            );
        }
    };
    let after = match observe_target(&verified_parent) {
        Ok(value) => value,
        Err(message) => {
            return failure_result(
                ControlFailureKind::Io,
                message,
                None,
                vec![opaque_effect(
                    &operation.path,
                    operation.expected,
                    PathState::Other,
                )],
            );
        }
    };
    let effects = measured_effects(
        &operation.path,
        operation.expected.clone(),
        after.state.clone(),
    );
    if after.state != candidate_state || after.unsupported_reason.is_some() {
        let witness = StaleObservationWitness {
            path: operation.path.clone(),
            expected: candidate_state,
            observed: after.state,
        };
        return failure_result(
            ControlFailureKind::Io,
            format!("post-publication identity mismatch for {}", operation.path),
            Some(ControlFailureWitness::StaleObservation(witness)),
            effects,
        );
    }

    FileCommitResult {
        full: operation.success,
        is_error: false,
        effects: WorkspaceEffectReport::Measured(effects),
        failure: None,
    }
}

fn write_candidate(file: &mut File, candidate: &[u8], fault: WriteFault) -> Result<(), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    match fault {
        WriteFault::None => {
            file.write_all(candidate)
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())
        }
        #[cfg(test)]
        WriteFault::AfterPrefix(length) => {
            let length = length.min(candidate.len());
            file.write_all(&candidate[..length])
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())?;
            Err("injected failure after partial controlled write".to_string())
        }
    }
}

struct TemporaryPath<'a> {
    parent: &'a PinnedParent,
    name: OsString,
    path: String,
    identity: FileIdentity,
    active: bool,
}

impl TemporaryPath<'_> {
    fn cleanup(&mut self) -> std::io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.parent.dir().remove_file(&self.name)?;
        self.active = false;
        Ok(())
    }

    fn disarm(&mut self) {
        self.active = false;
    }

    fn measured_effects(&self) -> Vec<WorkspaceEffect> {
        if !self.active {
            return Vec::new();
        }
        let after = observe_leaf(self.parent.dir(), &self.name)
            .map(|observed| observed.state)
            .unwrap_or(PathState::Other);
        measured_effects(&self.path, PathState::Absent, after)
    }
}

impl Drop for TemporaryPath<'_> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn create_temporary<'a>(
    parent: &'a PinnedParent,
    target_path: &str,
) -> Result<(File, TemporaryPath<'a>), String> {
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = OsString::from(format!(
            ".ferric-candidate-{}-{sequence}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            options.mode(0o666);
        }
        match parent.dir().open_with(&name, &options) {
            Ok(file) => {
                let metadata = match file.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        drop(file);
                        let _ = parent.dir().remove_file(&name);
                        return Err(format!("inspect controlled candidate: {error}"));
                    }
                };
                let identity = cap_identity(&metadata);
                let links = MetadataExt::nlink(&metadata);
                if !metadata.is_file() || metadata.file_type().is_symlink() || links != 1 {
                    let _ = parent.dir().remove_file(&name);
                    return Err(
                        "exclusive controlled candidate was not a single-link regular file"
                            .to_string(),
                    );
                }
                let path = temporary_display_path(target_path, &name);
                return Ok((
                    file.into_std(),
                    TemporaryPath {
                        parent,
                        name,
                        path,
                        identity,
                        active: true,
                    },
                ));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create controlled candidate: {error}")),
        }
    }
    Err("could not allocate a unique controlled candidate name".to_string())
}

fn temporary_display_path(target_path: &str, name: &OsString) -> String {
    let parent = Path::new(target_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    parent.join(name).to_string_lossy().replace('\\', "/")
}

fn cleanup_failure(temporary: &mut TemporaryPath<'_>, message: String) -> FileCommitResult {
    let cleanup_error = temporary.cleanup().err();
    let effects = temporary.measured_effects();
    let message = cleanup_error.map_or(message.clone(), |error| {
        format!("{message}; candidate cleanup failed: {error}")
    });
    failure_result(ControlFailureKind::Io, message, None, effects)
}

fn cleanup_stale(
    temporary: &mut TemporaryPath<'_>,
    operation: &FileMutationCandidate,
    observed: PathState,
    detail: String,
) -> FileCommitResult {
    if let Err(error) = temporary.cleanup() {
        return failure_result(
            ControlFailureKind::Io,
            format!(
                "stale precondition for {}; cleanup failed: {error}",
                operation.path
            ),
            None,
            temporary.measured_effects(),
        );
    }
    stale_result(
        operation.path.clone(),
        operation.expected.clone(),
        observed,
        detail,
    )
}

fn opaque_effect(path: &str, before: PathState, after: PathState) -> WorkspaceEffect {
    WorkspaceEffect {
        path: path.to_string(),
        kind: WorkspaceEffectKind::Opaque,
        before,
        after,
    }
}

struct PublishFailure {
    error: std::io::Error,
    target_published: bool,
}

impl PublishFailure {
    fn unchanged(error: std::io::Error) -> Self {
        Self {
            error,
            target_published: false,
        }
    }

    #[cfg(unix)]
    fn published(error: std::io::Error) -> Self {
        Self {
            error,
            target_published: true,
        }
    }
}

#[cfg(unix)]
fn publish_temporary(
    parent: &PinnedParent,
    temporary: &OsString,
    expected: &PathState,
) -> Result<(), PublishFailure> {
    match expected {
        PathState::Absent => {
            // `linkat`-style publication is no-clobber on every Unix target.
            // Removing our private name leaves the published inode with one link.
            parent
                .dir()
                .hard_link(temporary, parent.dir(), &parent.leaf)
                .map_err(PublishFailure::unchanged)?;
            parent
                .dir()
                .remove_file(temporary)
                .map_err(PublishFailure::published)
        }
        PathState::File { .. } => parent
            .dir()
            .rename(temporary, parent.dir(), &parent.leaf)
            .map_err(PublishFailure::unchanged),
        PathState::Directory | PathState::Other => Err(PublishFailure::unchanged(
            std::io::Error::new(ErrorKind::InvalidInput, "unsupported prepared target shape"),
        )),
    }
}

#[cfg(windows)]
fn publish_temporary(
    parent: &PinnedParent,
    temporary: &OsString,
    expected: &PathState,
) -> Result<(), PublishFailure> {
    if matches!(expected, PathState::Absent) {
        // std::fs::rename is an exclusive move on Windows: it fails rather than
        // replacing a destination which appeared after the final CAS read.
        return parent
            .dir()
            .rename(temporary, parent.dir(), &parent.leaf)
            .map_err(PublishFailure::unchanged);
    }
    if !matches!(expected, PathState::File { .. }) {
        return Err(PublishFailure::unchanged(std::io::Error::new(
            ErrorKind::InvalidInput,
            "unsupported prepared target shape",
        )));
    }

    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let replaced: Vec<u16> = parent
        .absolute
        .join(&parent.leaf)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let replacement: Vec<u16> = parent
        .absolute
        .join(temporary)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both buffers are live, NUL-terminated Windows paths; the optional
    // backup/exclusion/reserved pointers are explicitly null as required.
    let replaced_ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced_ok == 0 {
        Err(PublishFailure::unchanged(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn stale_result(
    path: String,
    expected: PathState,
    observed: PathState,
    detail: String,
) -> FileCommitResult {
    let witness = StaleObservationWitness {
        path: path.clone(),
        expected,
        observed,
    };
    failure_result(
        ControlFailureKind::StalePrecondition,
        format!("stale precondition for {path}: {detail}"),
        Some(ControlFailureWitness::StaleObservation(witness)),
        Vec::new(),
    )
}

fn failure_result(
    kind: ControlFailureKind,
    message: String,
    witness: Option<ControlFailureWitness>,
    effects: Vec<WorkspaceEffect>,
) -> FileCommitResult {
    FileCommitResult {
        full: message.clone(),
        is_error: true,
        effects: WorkspaceEffectReport::Measured(effects),
        failure: Some(ControlFailure {
            kind,
            message,
            witness,
        }),
    }
}

fn measured_effects(path: &str, before: PathState, after: PathState) -> Vec<WorkspaceEffect> {
    if before == after {
        return Vec::new();
    }
    let kind = match (&before, &after) {
        (PathState::Absent, PathState::File { .. }) => WorkspaceEffectKind::Created,
        (PathState::File { .. }, PathState::File { .. }) => WorkspaceEffectKind::Modified,
        (PathState::File { .. }, PathState::Absent) => WorkspaceEffectKind::Deleted,
        _ => WorkspaceEffectKind::Opaque,
    };
    vec![WorkspaceEffect {
        path: path.to_string(),
        kind,
        before,
        after,
    }]
}

fn state_for_bytes(bytes: &[u8]) -> PathState {
    PathState::File {
        sha256: sha256_bytes(bytes),
        bytes: bytes.len() as u64,
        lines: logical_line_count(bytes),
    }
}

fn no_effect_error(kind: NoEffectKind, state: CandidatePathState, message: String) -> PrepareError {
    PrepareError::new(PrepareErrorKind::NoEffect, message).with_witness(
        PrepareFailureWitness::NoEffect {
            kind,
            states: vec![state],
        },
    )
}

fn unsupported_path_error(path: &str, reason: String) -> PrepareError {
    PrepareError::new(
        PrepareErrorKind::UnsupportedOperation,
        format!("unsupported controlled mutation target {path}: {reason}"),
    )
    .with_witness(PrepareFailureWitness::UnsupportedMutation(
        UnsupportedMutationKind::UnsupportedOperation,
    ))
}

fn logical_target(
    workspace: &Workspace,
    requested_path: &str,
) -> Result<(String, PathBuf), PrepareError> {
    let requested = Path::new(requested_path);
    let relative = if requested.is_absolute() {
        requested.strip_prefix(workspace.root()).map_err(|_| {
            PrepareError::new(
                PrepareErrorKind::UnsupportedOperation,
                "controlled mutation absolute path is not lexically rooted in the workspace",
            )
        })?
    } else {
        requested
    };
    let mut components: Vec<OsString> = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err(PrepareError::new(
                        PrepareErrorKind::UnsupportedOperation,
                        "controlled mutation path escapes the workspace lexically",
                    ));
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(PrepareError::new(
                    PrepareErrorKind::UnsupportedOperation,
                    "controlled mutation path contains an unsupported root or prefix",
                ));
            }
        }
    }
    let relative: PathBuf = components.iter().collect();
    let normalized = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().replace('\\', "/")
    };
    Ok((normalized, workspace.root().join(relative)))
}

fn pin_parent(workspace: &Workspace, absolute: &Path) -> Result<PinnedParent, ShapeFailure> {
    let relative = absolute
        .strip_prefix(workspace.root())
        .map_err(|_| ShapeFailure {
            message: "target is not relative to the workspace capability".to_string(),
            observed: PathState::Other,
        })?;
    let leaf = relative.file_name().ok_or_else(|| ShapeFailure {
        message: "controlled mutation target must name a file".to_string(),
        observed: PathState::Other,
    })?;
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let root = Dir::open_ambient_dir(workspace.root(), ambient_authority()).map_err(|error| {
        ShapeFailure {
            message: format!("open workspace capability root: {error}"),
            observed: PathState::Other,
        }
    })?;
    let root_metadata = root.dir_metadata().map_err(|error| ShapeFailure {
        message: format!("inspect workspace capability root: {error}"),
        observed: PathState::Other,
    })?;
    let mut chain = vec![root];
    let mut identities = vec![cap_identity(&root_metadata)];
    let mut absolute_parent = workspace.root().to_path_buf();

    for component in parent_relative.components() {
        let Component::Normal(name) = component else {
            return Err(ShapeFailure {
                message: "controlled parent contains an unsupported path component".to_string(),
                observed: PathState::Other,
            });
        };
        let current = chain.last().expect("capability chain has a root");
        let before = current
            .symlink_metadata(name)
            .map_err(|error| ShapeFailure {
                message: if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) {
                    format!(
                        "parent directory {} does not exist",
                        absolute_parent.join(name).display()
                    )
                } else {
                    format!(
                        "inspect parent {}: {error}",
                        absolute_parent.join(name).display()
                    )
                },
                observed: PathState::Other,
            })?;
        if before.file_type().is_symlink() || !before.is_dir() {
            return Err(ShapeFailure {
                message: format!(
                    "ancestor {} is not a plain directory",
                    absolute_parent.join(name).display()
                ),
                observed: PathState::Other,
            });
        }
        let before_identity = cap_identity(&before);
        let child = current
            .open_dir_nofollow(name)
            .map_err(|error| ShapeFailure {
                message: format!(
                    "open parent {} without escaping: {error}",
                    absolute_parent.join(name).display()
                ),
                observed: PathState::Other,
            })?;
        let opened = child.dir_metadata().map_err(|error| ShapeFailure {
            message: format!(
                "inspect opened parent {}: {error}",
                absolute_parent.join(name).display()
            ),
            observed: PathState::Other,
        })?;
        let after = current
            .symlink_metadata(name)
            .map_err(|error| ShapeFailure {
                message: format!(
                    "revalidate parent {}: {error}",
                    absolute_parent.join(name).display()
                ),
                observed: PathState::Other,
            })?;
        if after.file_type().is_symlink()
            || !after.is_dir()
            || cap_identity(&opened) != before_identity
            || cap_identity(&after) != before_identity
        {
            return Err(ShapeFailure {
                message: format!(
                    "ancestor {} changed while its capability was opened",
                    absolute_parent.join(name).display()
                ),
                observed: PathState::Other,
            });
        }
        absolute_parent.push(name);
        identities.push(before_identity);
        chain.push(child);
    }

    Ok(PinnedParent {
        chain,
        identities,
        leaf: leaf.to_os_string(),
        #[cfg(windows)]
        absolute: absolute_parent,
    })
}

fn validate_parent_binding(
    workspace: &Workspace,
    absolute: &Path,
    expected: &PinnedParent,
) -> Result<(), ShapeFailure> {
    let current = pin_parent(workspace, absolute)?;
    if current.identities != expected.identities || current.leaf != expected.leaf {
        return Err(ShapeFailure {
            message: "target parent identity changed during controlled commit".to_string(),
            observed: PathState::Other,
        });
    }
    Ok(())
}

fn cap_identity(metadata: &cap_std::fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: MetadataExt::dev(metadata),
        inode: MetadataExt::ino(metadata),
    }
}

#[cfg(unix)]
fn opened_identity_and_links(
    _file: &File,
    metadata: &Metadata,
) -> std::io::Result<(FileIdentity, u64)> {
    Ok((
        FileIdentity {
            device: std::os::unix::fs::MetadataExt::dev(metadata),
            inode: std::os::unix::fs::MetadataExt::ino(metadata),
        },
        std::os::unix::fs::MetadataExt::nlink(metadata),
    ))
}

#[cfg(windows)]
fn opened_identity_and_links(
    file: &File,
    _metadata: &Metadata,
) -> std::io::Result<(FileIdentity, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    // SAFETY: the output structure is plain data and the handle remains live
    // for the duration of the call.
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        FileIdentity {
            device: information.dwVolumeSerialNumber as u64,
            inode: u64::from(information.nFileIndexHigh) << 32
                | u64::from(information.nFileIndexLow),
        },
        u64::from(information.nNumberOfLinks),
    ))
}

fn observe_target(parent: &PinnedParent) -> Result<ObservedTarget, String> {
    observe_leaf(parent.dir(), &parent.leaf)
}

fn observe_leaf(parent: &Dir, leaf: &OsString) -> Result<ObservedTarget, String> {
    let before = match parent.symlink_metadata(leaf) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(ObservedTarget {
                state: PathState::Absent,
                bytes: None,
                unsupported_reason: None,
                identity: None,
                permissions: None,
            });
        }
        Err(error) => return Err(format!("inspect controlled target: {error}")),
    };
    if before.file_type().is_symlink() {
        return Ok(ObservedTarget {
            state: PathState::Other,
            bytes: None,
            unsupported_reason: Some("target is a symlink or reparse point".to_string()),
            identity: None,
            permissions: None,
        });
    }
    if before.is_dir() {
        return Ok(ObservedTarget {
            state: PathState::Directory,
            bytes: None,
            unsupported_reason: None,
            identity: Some(cap_identity(&before)),
            permissions: None,
        });
    }
    if !before.is_file() {
        return Ok(ObservedTarget {
            state: PathState::Other,
            bytes: None,
            unsupported_reason: Some("target is not a regular file".to_string()),
            identity: Some(cap_identity(&before)),
            permissions: None,
        });
    }
    let before_identity = cap_identity(&before);
    let mut file = open_leaf_nofollow(parent, leaf)
        .map_err(|error| format!("open controlled target without following links: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect opened controlled target: {error}"))?;
    let (identity, links) = opened_identity_and_links(&file, &metadata)
        .map_err(|error| format!("identify opened controlled target: {error}"))?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) || !metadata.is_file() {
        return Ok(ObservedTarget {
            state: if metadata.is_dir() {
                PathState::Directory
            } else {
                PathState::Other
            },
            bytes: None,
            unsupported_reason: Some("opened target is not a plain regular file".to_string()),
            identity: Some(identity),
            permissions: None,
        });
    }
    let after = parent
        .symlink_metadata(leaf)
        .map_err(|error| format!("revalidate opened controlled target: {error}"))?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || cap_identity(&after) != before_identity
        || identity != before_identity
    {
        return Err("controlled target identity changed while it was opened".to_string());
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek controlled target: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read controlled target: {error}"))?;
    Ok(ObservedTarget {
        state: state_for_bytes(&bytes),
        bytes: Some(bytes),
        unsupported_reason: (links != 1).then(|| {
            format!("regular file has {links} hard links; controlled mutation requires exactly one")
        }),
        identity: Some(identity),
        permissions: Some(metadata.permissions()),
    })
}

fn open_leaf_nofollow(parent: &Dir, leaf: &OsString) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    parent
        .open_with(leaf, &options)
        .map(cap_std::fs::File::into_std)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        (directory, workspace)
    }

    #[test]
    fn partial_candidate_write_failure_preserves_target_and_leaves_no_temp_effect() {
        let (directory, workspace) = workspace();
        std::fs::write(directory.path().join("fault.txt"), b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "fault.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"after-data\n".to_vec(),
            success: "unused".to_string(),
        };

        let result =
            commit_candidate_with(&workspace, operation, WriteFault::AfterPrefix(3), |_| {});

        assert!(result.is_error);
        assert_eq!(
            std::fs::read(directory.path().join("fault.txt")).unwrap(),
            b"before\n"
        );
        let WorkspaceEffectReport::Measured(effects) = result.effects else {
            panic!("expected measured effects")
        };
        assert!(effects.is_empty());
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".ferric-candidate-")
        }));
        assert_eq!(result.failure.unwrap().kind, ControlFailureKind::Io);
    }

    #[test]
    fn appearance_between_ancestor_check_and_exclusive_open_is_stale_without_overwrite() {
        let (directory, workspace) = workspace();
        let operation = FileMutationCandidate {
            path: "race.txt".to_string(),
            expected: PathState::Absent,
            candidate: b"candidate\n".to_vec(),
            success: "unused".to_string(),
        };

        let result = commit_candidate_with(&workspace, operation, WriteFault::None, |path| {
            std::fs::write(path, b"racer\n").unwrap()
        });

        assert!(result.is_error);
        assert_eq!(
            std::fs::read(directory.path().join("race.txt")).unwrap(),
            b"racer\n"
        );
        assert!(matches!(
            result.failure,
            Some(ControlFailure {
                kind: ControlFailureKind::StalePrecondition,
                witness: Some(ControlFailureWitness::StaleObservation(
                    StaleObservationWitness {
                        expected: PathState::Absent,
                        observed: PathState::File { .. },
                        ..
                    }
                )),
                ..
            })
        ));
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
        ));
    }
}
