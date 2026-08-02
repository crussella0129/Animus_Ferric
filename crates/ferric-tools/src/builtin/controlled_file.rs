use std::ffi::OsString;
use std::fs::{File, Metadata, Permissions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

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
    #[cfg(unix)]
    unix_metadata: Option<UnixMetadata>,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnixMetadata {
    mode: u32,
    uid: u32,
    gid: u32,
}

#[derive(Debug)]
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
/// brackets publication because POSIX permits renaming an open directory. A
/// same-UID process can still move that directory or add a hard link in the
/// last syscall window; those residual POSIX races are detected afterward and
/// reported as opaque rather than being claimed as measured success.
struct PinnedParent {
    chain: Vec<Dir>,
    identities: Vec<FileIdentity>,
    leaf: OsString,
    absolute: PathBuf,
}

impl PinnedParent {
    fn dir(&self) -> &Dir {
        self.chain.last().expect("pinned chain always has its root")
    }
}

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
    commit_candidate_with(
        workspace,
        operation,
        WriteFault::None,
        |_| {},
        |_| {},
        |_, _| {},
        |_| {},
    )
}

fn commit_candidate_with<F, G, H, I>(
    workspace: &Workspace,
    operation: FileMutationCandidate,
    fault: WriteFault,
    before_open: F,
    during_write: G,
    before_publish: H,
    before_displaced_cleanup: I,
) -> FileCommitResult
where
    F: FnOnce(&Path),
    G: FnOnce(Option<&Path>),
    H: FnOnce(&Path, Option<&Path>),
    I: FnOnce(Option<&Path>),
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

    let parent = match validate_parent_binding(workspace, &absolute, &parent) {
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

    let (candidate_file, mut temporary) = match create_temporary(&parent, &operation.path) {
        Ok(value) => value,
        Err(message) => {
            return failure_result(ControlFailureKind::Io, message, None, Vec::new());
        }
    };
    let mut candidate_file = Some(candidate_file);
    let visible_candidate = temporary
        .visible_name()
        .map(|name| absolute.with_file_name(name));
    if let Err(error) = write_candidate(
        candidate_file.as_mut().expect("candidate handle"),
        &operation.candidate,
        fault,
        visible_candidate.as_deref(),
        during_write,
    ) {
        return cleanup_failure(
            &mut temporary,
            &mut candidate_file,
            &operation,
            format!("write candidate for {}: {error}", operation.path),
        );
    }
    if let Some(permissions) = first.permissions.clone()
        && let Err(error) = candidate_file
            .as_ref()
            .expect("candidate handle")
            .set_permissions(permissions)
    {
        return cleanup_failure(
            &mut temporary,
            &mut candidate_file,
            &operation,
            format!("set candidate permissions for {}: {error}", operation.path),
        );
    }
    if let Err(message) =
        verify_candidate_metadata(candidate_file.as_ref().expect("candidate handle"), &first)
    {
        return cleanup_failure(
            &mut temporary,
            &mut candidate_file,
            &operation,
            format!(
                "cannot preserve controlled target metadata for {}: {message}",
                operation.path
            ),
        );
    }
    if let Err(error) = candidate_file
        .as_ref()
        .expect("candidate handle")
        .sync_all()
    {
        return cleanup_failure(
            &mut temporary,
            &mut candidate_file,
            &operation,
            format!("sync candidate for {}: {error}", operation.path),
        );
    }

    let final_parent = match validate_parent_binding(workspace, &absolute, &parent) {
        Ok(parent) => parent,
        Err(failure) => {
            return cleanup_stale(
                &mut temporary,
                &mut candidate_file,
                &operation,
                failure.observed,
                failure.message,
            );
        }
    };
    let second = match observe_target(&final_parent) {
        Ok(observed) => observed,
        Err(message) => {
            return cleanup_failure(&mut temporary, &mut candidate_file, &operation, message);
        }
    };
    if second.state != operation.expected
        || second.unsupported_reason.is_some()
        || second.identity != first.identity
        || !observed_metadata_matches(&first, &second)
    {
        return cleanup_stale(
            &mut temporary,
            &mut candidate_file,
            &operation,
            second.state,
            second
                .unsupported_reason
                .unwrap_or_else(|| "target identity changed before atomic publication".to_string()),
        );
    }

    let candidate_state = state_for_bytes(&operation.candidate);
    let expected_candidate_links = u64::from(temporary.named);
    if let Err(message) = verify_candidate_handle(
        candidate_file.as_mut().expect("candidate handle"),
        temporary.identity,
        &candidate_state,
        expected_candidate_links,
    ) {
        return cleanup_failure(
            &mut temporary,
            &mut candidate_file,
            &operation,
            format!(
                "controlled candidate handle changed before publishing {}: {message}",
                operation.path
            ),
        );
    }
    if let Some(name) = temporary.visible_name() {
        let temporary_observation = match observe_leaf(final_parent.dir(), name) {
            Ok(observed) => observed,
            Err(message) => {
                return cleanup_failure(&mut temporary, &mut candidate_file, &operation, message);
            }
        };
        if temporary_observation.state != candidate_state
            || temporary_observation.identity != Some(temporary.identity)
            || temporary_observation.unsupported_reason.is_some()
        {
            return cleanup_failure(
                &mut temporary,
                &mut candidate_file,
                &operation,
                format!(
                    "controlled candidate identity changed before publishing {}",
                    operation.path
                ),
            );
        }
    }

    let visible_candidate = temporary
        .visible_name()
        .map(|name| absolute.with_file_name(name));
    before_publish(&absolute, visible_candidate.as_deref());

    match publish_temporary(
        &final_parent,
        &mut temporary,
        &mut candidate_file,
        &second,
        &candidate_state,
        before_displaced_cleanup,
    ) {
        PublishDecision::Published => {}
        PublishDecision::StaleRolledBack { observed, detail } => {
            return cleanup_stale(
                &mut temporary,
                &mut candidate_file,
                &operation,
                observed,
                detail,
            );
        }
        PublishDecision::FailedClean(message) => {
            return cleanup_failure(&mut temporary, &mut candidate_file, &operation, message);
        }
        PublishDecision::FailedOpaque(message) => {
            temporary.abandon_cleanup();
            let observed = observe_target(&final_parent)
                .map(|value| value.state)
                .unwrap_or(PathState::Other);
            let mut effects = vec![opaque_effect(&operation.path, operation.expected, observed)];
            effects.extend(temporary.measured_effects());
            return failure_result(ControlFailureKind::Io, message, None, effects);
        }
    }

    let verified_parent = match pin_parent(workspace, &absolute) {
        Ok(value) if value.identities == final_parent.identities => value,
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
    if after.state != candidate_state
        || after.unsupported_reason.is_some()
        || after.identity != Some(temporary.identity)
        || !observed_metadata_matches(&first, &after)
    {
        let witness = StaleObservationWitness {
            path: operation.path.clone(),
            expected: candidate_state,
            observed: after.state.clone(),
        };
        return failure_result(
            ControlFailureKind::Io,
            format!("post-publication identity mismatch for {}", operation.path),
            Some(ControlFailureWitness::StaleObservation(witness)),
            vec![opaque_effect(
                &operation.path,
                operation.expected.clone(),
                after.state,
            )],
        );
    }
    if let Err(message) = verify_published_candidate_handle(
        candidate_file.as_mut().expect("candidate identity handle"),
        temporary.identity,
        &candidate_state,
        1,
    ) {
        return failure_result(
            ControlFailureKind::Io,
            format!(
                "candidate gained an unknown hard link after publishing {}: {message}",
                operation.path
            ),
            None,
            vec![opaque_effect(
                &operation.path,
                operation.expected,
                after.state,
            )],
        );
    }

    FileCommitResult {
        full: operation.success,
        is_error: false,
        effects: WorkspaceEffectReport::Measured(effects),
        failure: None,
    }
}

fn write_candidate<F>(
    file: &mut File,
    candidate: &[u8],
    fault: WriteFault,
    visible_candidate: Option<&Path>,
    during_write: F,
) -> Result<(), String>
where
    F: FnOnce(Option<&Path>),
{
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    match fault {
        WriteFault::None => {
            let midpoint = candidate.len() / 2;
            file.write_all(&candidate[..midpoint])
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())?;
            during_write(visible_candidate);
            file.write_all(&candidate[midpoint..])
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())
        }
        #[cfg(test)]
        WriteFault::AfterPrefix(length) => {
            let length = length.min(candidate.len());
            file.write_all(&candidate[..length])
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())?;
            during_write(visible_candidate);
            Err("injected failure after partial controlled write".to_string())
        }
    }
}

struct TemporaryPath<'a> {
    parent: &'a PinnedParent,
    name: OsString,
    path: String,
    identity: FileIdentity,
    named: bool,
    active: bool,
    owned: bool,
    reported_paths: Vec<(OsString, String)>,
}

impl TemporaryPath<'_> {
    fn cleanup(&mut self) -> std::io::Result<()> {
        if !self.active {
            return Ok(());
        }
        if !self.owned {
            return Err(std::io::Error::other(
                "private path ownership is no longer provable",
            ));
        }
        if !self.named {
            self.active = false;
            self.owned = false;
            return Ok(());
        }
        self.parent.dir().remove_file(&self.name)?;
        self.active = false;
        self.owned = false;
        Ok(())
    }

    #[cfg(windows)]
    fn disarm(&mut self) {
        self.active = false;
        self.owned = false;
    }

    #[cfg(windows)]
    fn track_replacement_name(&mut self, name: OsString) {
        self.path = temporary_display_path(&self.path, &name);
        self.name = name;
        self.named = true;
        self.active = true;
        self.owned = true;
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn mark_named(&mut self) {
        self.named = true;
        self.active = true;
        self.owned = true;
    }

    fn abandon_cleanup(&mut self) {
        self.owned = false;
    }

    #[cfg(windows)]
    fn report_additional_path(&mut self, name: OsString) {
        let path = temporary_display_path(&self.path, &name);
        self.reported_paths.push((name, path));
    }

    fn visible_name(&self) -> Option<&OsString> {
        self.named.then_some(&self.name)
    }

    fn measured_effects(&self) -> Vec<WorkspaceEffect> {
        if !self.active {
            return Vec::new();
        }
        if !self.named {
            return Vec::new();
        }
        let after = observe_leaf(self.parent.dir(), &self.name)
            .map(|observed| observed.state)
            .unwrap_or(PathState::Other);
        let mut effects = measured_effects(&self.path, PathState::Absent, after);
        for (name, path) in &self.reported_paths {
            let after = observe_leaf(self.parent.dir(), name)
                .map(|observed| observed.state)
                .unwrap_or(PathState::Other);
            effects.extend(measured_effects(path, PathState::Absent, after));
        }
        effects
    }
}

fn create_temporary<'a>(
    parent: &'a PinnedParent,
    target_path: &str,
) -> Result<(File, TemporaryPath<'a>), String> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(candidate) = create_unnamed_temporary(parent, target_path)? {
        return Ok(candidate);
    }

    for _ in 0..64 {
        let name = random_private_name("candidate")?;
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
                        named: true,
                        active: true,
                        owned: true,
                        reported_paths: Vec::new(),
                    },
                ));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create controlled candidate: {error}")),
        }
    }
    Err("could not allocate a unique controlled candidate name".to_string())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn create_unnamed_temporary<'a>(
    parent: &'a PinnedParent,
    target_path: &str,
) -> Result<Option<(File, TemporaryPath<'a>)>, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    // SAFETY: the directory fd is live, the path is a static NUL-terminated
    // dot, and O_TMPFILE returns a new owned descriptor on success.
    let descriptor = unsafe {
        libc::openat(
            parent.dir().as_raw_fd(),
            c".".as_ptr(),
            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
            0o666,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error().is_some_and(|code| {
            [libc::EOPNOTSUPP, libc::EINVAL, libc::EISDIR, libc::ENOSYS].contains(&code)
        }) {
            return Ok(None);
        }
        return Err(format!("create unnamed controlled candidate: {error}"));
    }
    // SAFETY: ownership of the successful descriptor transfers to File.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect unnamed controlled candidate: {error}"))?;
    let identity = FileIdentity {
        device: std::os::unix::fs::MetadataExt::dev(&metadata),
        inode: std::os::unix::fs::MetadataExt::ino(&metadata),
    };
    let links = std::os::unix::fs::MetadataExt::nlink(&metadata);
    if !metadata.is_file() || links != 0 {
        return Err("O_TMPFILE candidate was not an unnamed regular inode".to_string());
    }
    let name = random_private_name("candidate")?;
    let path = temporary_display_path(target_path, &name);
    Ok(Some((
        file,
        TemporaryPath {
            parent,
            name,
            path,
            identity,
            named: false,
            active: true,
            owned: true,
            reported_paths: Vec::new(),
        },
    )))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn link_unnamed_candidate(
    parent: &PinnedParent,
    candidate: &File,
    name: &OsString,
) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let name = std::ffi::CString::new(name.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, "candidate name contains NUL"))?;
    // SAFETY: all descriptors and C strings are live for the syscall.
    let direct = unsafe {
        libc::linkat(
            candidate.as_raw_fd(),
            c"".as_ptr(),
            parent.dir().as_raw_fd(),
            name.as_ptr(),
            libc::AT_EMPTY_PATH,
        )
    };
    if direct == 0 {
        return Ok(());
    }
    let direct_error = std::io::Error::last_os_error();
    if !matches!(
        direct_error.kind(),
        ErrorKind::PermissionDenied | ErrorKind::NotFound | ErrorKind::Unsupported
    ) && direct_error.raw_os_error() != Some(libc::EINVAL)
    {
        return Err(direct_error);
    }
    let proc_path = std::ffi::CString::new(format!("/proc/self/fd/{}", candidate.as_raw_fd()))
        .expect("numeric proc fd path contains no NUL");
    // SAFETY: proc_path and name are live C strings; AT_SYMLINK_FOLLOW links
    // the inode referenced by this process's own descriptor symlink.
    let fallback = unsafe {
        libc::linkat(
            libc::AT_FDCWD,
            proc_path.as_ptr(),
            parent.dir().as_raw_fd(),
            name.as_ptr(),
            libc::AT_SYMLINK_FOLLOW,
        )
    };
    if fallback == 0 {
        Ok(())
    } else {
        let fallback_error = std::io::Error::last_os_error();
        Err(std::io::Error::new(
            fallback_error.kind(),
            format!(
                "link unnamed candidate directly ({direct_error}) or through procfs ({fallback_error})"
            ),
        ))
    }
}

fn random_private_name(kind: &str) -> Result<OsString, String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| format!("obtain private-name entropy: {error}"))?;
    Ok(OsString::from(format!(
        ".ferric-{kind}-{:032x}",
        u128::from_ne_bytes(bytes)
    )))
}

fn temporary_display_path(target_path: &str, name: &OsString) -> String {
    let parent = Path::new(target_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    parent.join(name).to_string_lossy().replace('\\', "/")
}

fn cleanup_failure(
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
    operation: &FileMutationCandidate,
    message: String,
) -> FileCommitResult {
    let cleanup_error = cleanup_candidate_artifact(temporary, candidate).err();
    let mut effects = temporary.measured_effects();
    let message = cleanup_error.map_or(message.clone(), |error| {
        effects.push(opaque_effect(
            &operation.path,
            operation.expected.clone(),
            operation.expected.clone(),
        ));
        format!("{message}; candidate cleanup is not provable: {error}")
    });
    failure_result(ControlFailureKind::Io, message, None, effects)
}

fn cleanup_stale(
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
    operation: &FileMutationCandidate,
    observed: PathState,
    detail: String,
) -> FileCommitResult {
    if let Err(error) = cleanup_candidate_artifact(temporary, candidate) {
        let mut effects = temporary.measured_effects();
        effects.push(opaque_effect(
            &operation.path,
            operation.expected.clone(),
            observed,
        ));
        return failure_result(
            ControlFailureKind::Io,
            format!(
                "stale precondition for {}; cleanup is not provable: {error}",
                operation.path
            ),
            None,
            effects,
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

enum PublishDecision {
    Published,
    StaleRolledBack { observed: PathState, detail: String },
    FailedClean(String),
    FailedOpaque(String),
}

fn observed_metadata_matches(expected: &ObservedTarget, actual: &ObservedTarget) -> bool {
    #[cfg(unix)]
    {
        expected
            .unix_metadata
            .is_none_or(|metadata| actual.unix_metadata == Some(metadata))
    }
    #[cfg(windows)]
    {
        expected.permissions.as_ref().is_none_or(|permissions| {
            actual.permissions.as_ref().map(Permissions::readonly) == Some(permissions.readonly())
        })
    }
}

fn verify_candidate_metadata(file: &File, expected: &ObservedTarget) -> Result<(), String> {
    #[cfg(unix)]
    {
        let Some(expected) = expected.unix_metadata else {
            return Ok(());
        };
        let metadata = file
            .metadata()
            .map_err(|error| format!("inspect candidate metadata: {error}"))?;
        let actual = unix_metadata(&metadata);
        if actual != expected {
            return Err(format!(
                "mode/owner/group mismatch (actual={actual:?}, expected={expected:?})"
            ));
        }
    }
    #[cfg(windows)]
    let _ = (file, expected);
    Ok(())
}

fn verify_candidate_identity_and_links(
    file: &File,
    expected_identity: FileIdentity,
    expected_links: u64,
) -> Result<(), String> {
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect retained candidate identity handle: {error}"))?;
    let (identity, links) = opened_identity_and_links(file, &metadata)
        .map_err(|error| format!("identify retained candidate identity handle: {error}"))?;
    if identity != expected_identity || links != expected_links || !metadata.is_file() {
        return Err(format!(
            "candidate identity/link mismatch (identity={identity:?}, links={links}, expected_links={expected_links})"
        ));
    }
    Ok(())
}

fn cleanup_candidate_artifact(
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
) -> Result<(), String> {
    let cleanup = temporary
        .cleanup()
        .map_err(|error| format!("remove private candidate name: {error}"));
    let retained = candidate.as_ref().map_or(Ok(()), |file| {
        verify_candidate_identity_and_links(file, temporary.identity, 0)
            .map_err(|error| format!("candidate may have an unknown hard link: {error}"))
    });
    match (cleanup, retained) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(cleanup), Ok(())) => Err(cleanup),
        (Ok(()), Err(retained)) => Err(retained),
        (Err(cleanup), Err(retained)) => Err(format!("{cleanup}; {retained}")),
    }
}

fn verify_candidate_handle(
    file: &mut File,
    expected_identity: FileIdentity,
    expected_state: &PathState,
    expected_links: u64,
) -> Result<(), String> {
    verify_candidate_identity_and_links(file, expected_identity, expected_links)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek retained candidate handle: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read retained candidate handle: {error}"))?;
    let state = state_for_bytes(&bytes);
    if &state != expected_state {
        return Err("retained candidate bytes changed".to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn verify_published_candidate_handle(
    file: &mut File,
    expected_identity: FileIdentity,
    expected_state: &PathState,
    expected_links: u64,
) -> Result<(), String> {
    verify_candidate_handle(file, expected_identity, expected_state, expected_links)
}

#[cfg(windows)]
fn verify_published_candidate_handle(
    file: &mut File,
    expected_identity: FileIdentity,
    _expected_state: &PathState,
    expected_links: u64,
) -> Result<(), String> {
    verify_candidate_identity_and_links(file, expected_identity, expected_links)
}

fn cleanup_candidate_after_rollback(
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut File,
    candidate_state: &PathState,
) -> Result<(), String> {
    temporary
        .cleanup()
        .map_err(|error| format!("remove rolled-back candidate: {error}"))?;
    verify_published_candidate_handle(candidate, temporary.identity, candidate_state, 0)
        .map_err(|error| format!("candidate may have an unknown hard link: {error}"))
}

fn verify_expected_displaced_handle(
    file: &mut File,
    expected: &ObservedTarget,
    expected_links: u64,
) -> Result<(), String> {
    let identity = expected
        .identity
        .ok_or_else(|| "prepared file target has no retained identity".to_string())?;
    verify_published_candidate_handle(file, identity, &expected.state, expected_links)
        .map_err(|error| format!("displaced original is not exclusively accounted for: {error}"))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn open_expected_displaced_handle(
    parent: &PinnedParent,
    expected: &ObservedTarget,
) -> Result<File, String> {
    let mut file = open_leaf_nofollow(parent.dir(), &parent.leaf)
        .map_err(|error| format!("retain original target before exchange: {error}"))?;
    verify_expected_displaced_handle(&mut file, expected, 1)?;
    Ok(file)
}

#[cfg(windows)]
fn open_expected_displaced_handle(
    parent: &PinnedParent,
    expected: &ObservedTarget,
) -> Result<File, String> {
    let mut file = open_windows_identity_handle(&parent.absolute.join(&parent.leaf))
        .map_err(|error| format!("retain original target before replacement: {error}"))?;
    verify_expected_displaced_handle(&mut file, expected, 1)?;
    Ok(file)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_exchange(
    parent: &PinnedParent,
    left: &OsString,
    right: &OsString,
) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;

    let left = std::ffi::CString::new(left.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, "candidate name contains NUL"))?;
    let right = std::ffi::CString::new(right.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, "target name contains NUL"))?;
    // SAFETY: both C strings are live and both directory descriptors refer to
    // the same retained parent capability.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.dir().as_raw_fd(),
            left.as_ptr(),
            parent.dir().as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn publish_temporary<I>(
    parent: &PinnedParent,
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
    expected: &ObservedTarget,
    candidate_state: &PathState,
    before_displaced_cleanup: I,
) -> PublishDecision
where
    I: FnOnce(Option<&Path>),
{
    let candidate = candidate.as_mut().expect("candidate handle");
    if matches!(expected.state, PathState::Absent) {
        let publication = if temporary.named {
            parent
                .dir()
                .hard_link(&temporary.name, parent.dir(), &parent.leaf)
        } else {
            link_unnamed_candidate(parent, candidate, &parent.leaf)
        };
        if let Err(error) = publication {
            let observed = observe_target(parent).unwrap_or(ObservedTarget {
                state: PathState::Other,
                bytes: None,
                unsupported_reason: Some(error.to_string()),
                identity: None,
                permissions: None,
                #[cfg(unix)]
                unix_metadata: None,
            });
            return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
                Ok(()) if observed.state != PathState::Absent => PublishDecision::StaleRolledBack {
                    observed: observed.state,
                    detail: "target appeared at the final no-clobber publication boundary"
                        .to_string(),
                },
                Ok(()) => PublishDecision::FailedClean(format!(
                    "exclusive candidate publication failed without changing the target: {error}"
                )),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "candidate publication failed and cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
        if let Err(error) = temporary.cleanup() {
            return PublishDecision::FailedOpaque(format!(
                "target was published but the private candidate link could not be removed: {error}"
            ));
        }
        if let Err(error) =
            verify_candidate_handle(candidate, temporary.identity, candidate_state, 1)
        {
            let _ = parent.dir().remove_file(&parent.leaf);
            return PublishDecision::FailedOpaque(format!(
                "published candidate is retained by an unknown link: {error}"
            ));
        }
        return PublishDecision::Published;
    }
    if !matches!(expected.state, PathState::File { .. }) {
        return PublishDecision::FailedClean("unsupported prepared target shape".to_string());
    }

    if !temporary.named {
        if let Err(error) = link_unnamed_candidate(parent, candidate, &temporary.name) {
            return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
                Ok(()) => PublishDecision::FailedClean(format!(
                    "link unnamed candidate for atomic exchange: {error}"
                )),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "link unnamed candidate failed and cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
        temporary.mark_named();
        let linked = observe_leaf(parent.dir(), &temporary.name).ok();
        let linked_exact = linked.as_ref().is_some_and(|value| {
            value.state == *candidate_state
                && value.identity == Some(temporary.identity)
                && value.unsupported_reason.is_none()
        });
        if !linked_exact
            || verify_candidate_handle(candidate, temporary.identity, candidate_state, 1).is_err()
        {
            return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
                Ok(()) => PublishDecision::FailedClean(
                    "linked unnamed candidate failed its retained-handle identity check"
                        .to_string(),
                ),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "linked unnamed candidate is not exclusive: {cleanup}"
                )),
            };
        }
    }

    let mut displaced_handle = match open_expected_displaced_handle(parent, expected) {
        Ok(file) => file,
        Err(error) => {
            let observed = observe_target(parent).ok();
            return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
                Ok(())
                    if observed.as_ref().is_some_and(|value| {
                        value.state != expected.state || value.identity != expected.identity
                    }) =>
                {
                    PublishDecision::StaleRolledBack {
                        observed: observed.expect("checked observation").state,
                        detail: format!(
                            "target changed while retaining the original before exchange: {error}"
                        ),
                    }
                }
                Ok(()) => PublishDecision::FailedClean(error),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "cannot retain the original target and candidate cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
    };

    if let Err(error) = rename_exchange(parent, &temporary.name, &parent.leaf) {
        let observed = observe_target(parent).ok();
        return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
            Ok(())
                if observed.as_ref().is_some_and(|value| {
                    value.state != expected.state || value.identity != expected.identity
                }) =>
            {
                PublishDecision::StaleRolledBack {
                    observed: observed.expect("checked observation").state,
                    detail: "target changed at the final exchange boundary".to_string(),
                }
            }
            Ok(()) => PublishDecision::FailedClean(format!(
                "atomic target exchange failed without a detected target change: {error}"
            )),
            Err(cleanup) => PublishDecision::FailedOpaque(format!(
                "atomic target exchange failed and candidate cleanup is not provable: {error}; {cleanup}"
            )),
        };
    }

    let displaced = observe_leaf(parent.dir(), &temporary.name).ok();
    let displaced_matches =
        displaced.as_ref().is_some_and(|value| {
            value.state == expected.state
                && value.identity == expected.identity
                && value.unsupported_reason.is_none()
        }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
    if !displaced_matches {
        let observed = displaced
            .as_ref()
            .map(|value| value.state.clone())
            .unwrap_or(PathState::Other);
        if let Err(error) = rename_exchange(parent, &temporary.name, &parent.leaf) {
            return PublishDecision::FailedOpaque(format!(
                "raced target was displaced and rollback failed: {error}"
            ));
        }
        let restored = observe_target(parent).ok();
        let restored_exact = restored.as_ref().is_some_and(|value| {
            displaced.as_ref().is_some_and(|displaced| {
                value.state == displaced.state && value.identity == displaced.identity
            })
        });
        let original_unlinked =
            verify_expected_displaced_handle(&mut displaced_handle, expected, 0).is_ok();
        return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
            Ok(()) if restored_exact && original_unlinked => PublishDecision::StaleRolledBack {
                observed,
                detail: "target identity changed at the final exchange boundary; raced target was restored"
                    .to_string(),
            },
            Ok(()) => PublishDecision::FailedOpaque(
                "target race rollback completed but the restored binding is not provable"
                    .to_string(),
            ),
            Err(error) => PublishDecision::FailedOpaque(format!(
                "target race was rolled back but candidate bytes may be retained: {error}"
            )),
        };
    }

    let candidate_path = observe_target(parent).ok();
    let candidate_exact = candidate_path.as_ref().is_some_and(|value| {
        value.state == *candidate_state
            && value.identity == Some(temporary.identity)
            && value.unsupported_reason.is_none()
    });
    if verify_candidate_handle(candidate, temporary.identity, candidate_state, 1).is_err()
        || !candidate_exact
    {
        if let Err(error) = rename_exchange(parent, &temporary.name, &parent.leaf) {
            return PublishDecision::FailedOpaque(format!(
                "candidate integrity failed after exchange and rollback failed: {error}"
            ));
        }
        let cleanup = cleanup_candidate_after_rollback(temporary, candidate, candidate_state);
        let restored = observe_target(parent).ok();
        let original_restored =
            restored.as_ref().is_some_and(|value| {
                value.state == expected.state && value.identity == expected.identity
            }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
        return PublishDecision::FailedOpaque(match cleanup {
            Ok(()) if original_restored => {
                "candidate integrity failed after exchange; original target was restored"
                    .to_string()
            }
            Ok(()) => "candidate integrity failed after exchange and exact original restoration is not provable"
                .to_string(),
            Err(error) => format!(
                "candidate integrity failed after exchange and an unknown hard link may retain bytes: {error}"
            ),
        });
    }
    let displaced_path = parent.absolute.join(&temporary.name);
    before_displaced_cleanup(Some(&displaced_path));
    let revalidated = observe_leaf(parent.dir(), &temporary.name).ok();
    let revalidated_exact =
        revalidated.as_ref().is_some_and(|value| {
            value.state == expected.state
                && value.identity == expected.identity
                && value.unsupported_reason.is_none()
        }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
    if !revalidated_exact {
        return PublishDecision::FailedOpaque(
            "displaced original changed immediately before cleanup; it was retained for recovery"
                .to_string(),
        );
    }
    if let Err(error) = temporary.cleanup() {
        return PublishDecision::FailedOpaque(format!(
            "candidate was published but displaced-target cleanup failed: {error}"
        ));
    }
    if let Err(error) = verify_expected_displaced_handle(&mut displaced_handle, expected, 0) {
        return PublishDecision::FailedOpaque(format!(
            "displaced original may still be linked after cleanup: {error}"
        ));
    }
    PublishDecision::Published
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn publish_temporary<I>(
    parent: &PinnedParent,
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
    expected: &ObservedTarget,
    candidate_state: &PathState,
    _before_displaced_cleanup: I,
) -> PublishDecision
where
    I: FnOnce(Option<&Path>),
{
    let candidate = candidate.as_mut().expect("candidate handle");
    if matches!(expected.state, PathState::Absent) {
        if let Err(error) = parent
            .dir()
            .hard_link(&temporary.name, parent.dir(), &parent.leaf)
        {
            let observed = observe_target(parent).ok();
            return match cleanup_candidate_after_rollback(temporary, candidate, candidate_state) {
                Ok(())
                    if observed.as_ref().is_some_and(|value| {
                        value.state != PathState::Absent || value.identity.is_some()
                    }) =>
                {
                    PublishDecision::StaleRolledBack {
                        observed: observed.expect("checked observation").state,
                        detail: "target appeared at the final no-clobber publication boundary"
                            .to_string(),
                    }
                }
                Ok(()) => PublishDecision::FailedClean(format!(
                    "exclusive candidate publication failed without changing the target: {error}"
                )),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "candidate publication failed and cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
        if let Err(error) = temporary.cleanup() {
            return PublishDecision::FailedOpaque(format!(
                "target was published but the private candidate link could not be removed: {error}"
            ));
        }
        if let Err(error) =
            verify_candidate_handle(candidate, temporary.identity, candidate_state, 1)
        {
            let removal = parent.dir().remove_file(&parent.leaf);
            let unlinked = verify_candidate_identity_and_links(candidate, temporary.identity, 0);
            return PublishDecision::FailedOpaque(format!(
                "published candidate failed retained-handle verification: {error}; rollback={removal:?}; unlinked={unlinked:?}"
            ));
        }
        return PublishDecision::Published;
    }
    let cleanup = cleanup_candidate_after_rollback(temporary, candidate, candidate_state);
    match cleanup {
        Ok(()) => PublishDecision::FailedClean(
            "controlled existing-file replacement requires a kernel atomic-exchange primitive on this Unix platform"
                .to_string(),
        ),
        Err(error) => PublishDecision::FailedOpaque(error),
    }
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn open_windows_identity_handle(path: &Path) -> std::io::Result<File> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let path = wide_path(path);
    // SAFETY: the path is live and NUL-terminated. Desired access is zero so
    // ReplaceFileW's subsequent share-mode-zero open remains compatible; the
    // retained handle is used only for file-ID/link-count queries.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a unique owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

#[cfg(windows)]
fn replace_file_windows(replaced: &Path, replacement: &Path, backup: Option<&Path>) -> bool {
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let replaced = wide_path(replaced);
    let replacement = wide_path(replacement);
    let backup = backup.map(wide_path);
    // SAFETY: every supplied buffer is live and NUL-terminated; optional and
    // reserved pointers follow ReplaceFileW's contract. No ignore flags are
    // used, so metadata/ACL merge failures are not silently accepted.
    unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            backup
                .as_ref()
                .map_or(std::ptr::null(), |path| path.as_ptr()),
            0,
            std::ptr::null(),
            std::ptr::null(),
        ) != 0
    }
}

#[cfg(windows)]
fn publish_temporary<I>(
    parent: &PinnedParent,
    temporary: &mut TemporaryPath<'_>,
    candidate: &mut Option<File>,
    expected: &ObservedTarget,
    candidate_state: &PathState,
    before_displaced_cleanup: I,
) -> PublishDecision
where
    I: FnOnce(Option<&Path>),
{
    let target = parent.absolute.join(&parent.leaf);
    let source = parent.absolute.join(&temporary.name);
    let identity_handle = match open_windows_identity_handle(&source) {
        Ok(handle) => handle,
        Err(error) => {
            return PublishDecision::FailedClean(format!(
                "open retained candidate identity handle: {error}"
            ));
        }
    };
    drop(candidate.take());
    *candidate = Some(identity_handle);
    let candidate_handle = candidate.as_mut().expect("candidate identity handle");
    if let Err(error) =
        verify_published_candidate_handle(candidate_handle, temporary.identity, candidate_state, 1)
    {
        return match cleanup_candidate_after_rollback(temporary, candidate_handle, candidate_state)
        {
            Ok(()) => PublishDecision::FailedClean(format!(
                "candidate identity changed before Windows publication: {error}"
            )),
            Err(cleanup) => PublishDecision::FailedOpaque(format!(
                "candidate identity changed and bytes may be retained: {error}; {cleanup}"
            )),
        };
    }

    if matches!(expected.state, PathState::Absent) {
        if let Err(error) = parent
            .dir()
            .rename(&temporary.name, parent.dir(), &parent.leaf)
        {
            let observed = observe_target(parent).ok();
            return match cleanup_candidate_after_rollback(
                temporary,
                candidate_handle,
                candidate_state,
            ) {
                Ok(())
                    if observed.as_ref().is_some_and(|value| {
                        value.state != PathState::Absent || value.identity.is_some()
                    }) =>
                {
                    PublishDecision::StaleRolledBack {
                        observed: observed.expect("checked observation").state,
                        detail: "target appeared at the final exclusive-move boundary".to_string(),
                    }
                }
                Ok(()) => PublishDecision::FailedClean(format!(
                    "exclusive Windows candidate move failed: {error}"
                )),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "exclusive Windows candidate move failed and cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
        temporary.disarm();
        let path = observe_target(parent).ok();
        let exact = path.as_ref().is_some_and(|value| {
            value.state == *candidate_state
                && value.identity == Some(temporary.identity)
                && value.unsupported_reason.is_none()
        });
        if !exact
            || verify_published_candidate_handle(
                candidate_handle,
                temporary.identity,
                candidate_state,
                1,
            )
            .is_err()
        {
            let _ = parent.dir().remove_file(&parent.leaf);
            return PublishDecision::FailedOpaque(
                "Windows create publication did not retain an exclusive candidate identity"
                    .to_string(),
            );
        }
        return PublishDecision::Published;
    }
    if !matches!(expected.state, PathState::File { .. }) {
        return PublishDecision::FailedClean("unsupported prepared target shape".to_string());
    }

    let mut displaced_handle = match open_expected_displaced_handle(parent, expected) {
        Ok(file) => file,
        Err(error) => {
            let observed = observe_target(parent).ok();
            return match cleanup_candidate_after_rollback(
                temporary,
                candidate_handle,
                candidate_state,
            ) {
                Ok(())
                    if observed.as_ref().is_some_and(|value| {
                        value.state != expected.state || value.identity != expected.identity
                    }) =>
                {
                    PublishDecision::StaleRolledBack {
                        observed: observed.expect("checked observation").state,
                        detail: format!(
                            "target changed while retaining the original before replacement: {error}"
                        ),
                    }
                }
                Ok(()) => PublishDecision::FailedClean(error),
                Err(cleanup) => PublishDecision::FailedOpaque(format!(
                    "cannot retain the original target and candidate cleanup is not provable: {error}; {cleanup}"
                )),
            };
        }
    };

    let backup_name = match random_private_name("backup") {
        Ok(name) => name,
        Err(error) => return PublishDecision::FailedClean(error),
    };
    if parent.dir().symlink_metadata(&backup_name).is_ok() {
        return PublishDecision::FailedClean(
            "random Windows backup name unexpectedly exists".to_string(),
        );
    }
    let backup = parent.absolute.join(&backup_name);
    if !replace_file_windows(&target, &source, Some(&backup)) {
        let error = std::io::Error::last_os_error();
        let backup_exists = parent.dir().symlink_metadata(&backup_name).is_ok();
        let source_exists = parent.dir().symlink_metadata(&temporary.name).is_ok();
        if backup_exists && source_exists {
            temporary.report_additional_path(backup_name);
        } else if backup_exists {
            temporary.track_replacement_name(backup_name);
        } else if !source_exists {
            temporary.disarm();
        }
        return PublishDecision::FailedOpaque(format!(
            "ReplaceFileW failed at an outcome-sensitive boundary: {error}"
        ));
    }
    temporary.track_replacement_name(backup_name);

    let displaced = observe_leaf(parent.dir(), &temporary.name).ok();
    let displaced_matches =
        displaced.as_ref().is_some_and(|value| {
            value.state == expected.state
                && value.identity == expected.identity
                && value.unsupported_reason.is_none()
        }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
    let target_observation = observe_target(parent).ok();
    let candidate_exact = target_observation.as_ref().is_some_and(|value| {
        value.state == *candidate_state
            && value.identity == Some(temporary.identity)
            && value.unsupported_reason.is_none()
    });
    let candidate_links_exact =
        verify_published_candidate_handle(candidate_handle, temporary.identity, candidate_state, 1)
            .is_ok();

    if !displaced_matches || !candidate_exact || !candidate_links_exact {
        let observed = displaced
            .as_ref()
            .map(|value| value.state.clone())
            .unwrap_or(PathState::Other);
        if !replace_file_windows(&target, &backup, None) {
            return PublishDecision::FailedOpaque(format!(
                "Windows replacement identity check failed and rollback failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        temporary.disarm();
        let restored = observe_target(parent).ok();
        let restored_exact = restored.as_ref().is_some_and(|value| {
            displaced.as_ref().is_some_and(|displaced| {
                value.state == displaced.state && value.identity == displaced.identity
            })
        });
        let candidate_unlinked = verify_published_candidate_handle(
            candidate_handle,
            temporary.identity,
            candidate_state,
            0,
        )
        .is_ok();
        let original_restored =
            restored.as_ref().is_some_and(|value| {
                value.state == expected.state && value.identity == expected.identity
            }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
        let original_unlinked =
            verify_expected_displaced_handle(&mut displaced_handle, expected, 0).is_ok();
        if displaced_matches && (!candidate_exact || !candidate_links_exact) {
            return PublishDecision::FailedOpaque(if candidate_unlinked && original_restored {
                "candidate integrity/link count changed at publication; original target restored"
                    .to_string()
            } else {
                "candidate integrity failure was rolled back but an unknown hard link may retain bytes"
                    .to_string()
            });
        }
        return if restored_exact && candidate_unlinked && original_unlinked {
            PublishDecision::StaleRolledBack {
                observed,
                detail: "target identity changed at the final ReplaceFileW boundary; raced target was restored"
                    .to_string(),
            }
        } else {
            PublishDecision::FailedOpaque(
                "Windows race rollback completed but exact restoration is not provable".to_string(),
            )
        };
    }

    before_displaced_cleanup(Some(&backup));
    let revalidated = observe_leaf(parent.dir(), &temporary.name).ok();
    let revalidated_exact =
        revalidated.as_ref().is_some_and(|value| {
            value.state == expected.state
                && value.identity == expected.identity
                && value.unsupported_reason.is_none()
        }) && verify_expected_displaced_handle(&mut displaced_handle, expected, 1).is_ok();
    if !revalidated_exact {
        return PublishDecision::FailedOpaque(
            "displaced Windows original changed immediately before backup cleanup; it was retained for recovery"
                .to_string(),
        );
    }
    if let Err(error) = temporary.cleanup() {
        return PublishDecision::FailedOpaque(format!(
            "Windows candidate published but displaced-target backup cleanup failed: {error}"
        ));
    }
    if let Err(error) = verify_expected_displaced_handle(&mut displaced_handle, expected, 0) {
        return PublishDecision::FailedOpaque(format!(
            "displaced Windows original may still be linked after backup cleanup: {error}"
        ));
    }
    PublishDecision::Published
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
    // A model may emit either path separator regardless of host OS. Treat a
    // backslash as a component separator so Linux resolves `dir\file` the same
    // way Windows already does, matching the forward-slash canonical form this
    // function returns below.
    let requested_path = requested_path.replace('\\', "/");
    let requested = Path::new(&requested_path);
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
        absolute: absolute_parent,
    })
}

fn validate_parent_binding(
    workspace: &Workspace,
    absolute: &Path,
    expected: &PinnedParent,
) -> Result<PinnedParent, ShapeFailure> {
    let current = pin_parent(workspace, absolute)?;
    if current.identities != expected.identities || current.leaf != expected.leaf {
        return Err(ShapeFailure {
            message: "target parent identity changed during controlled commit".to_string(),
            observed: PathState::Other,
        });
    }
    Ok(current)
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

#[cfg(unix)]
fn unix_metadata(metadata: &Metadata) -> UnixMetadata {
    use std::os::unix::fs::MetadataExt;

    UnixMetadata {
        mode: metadata.mode() & 0o7777,
        uid: metadata.uid(),
        gid: metadata.gid(),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn has_extended_metadata(file: &File) -> Result<bool, String> {
    use std::os::fd::AsRawFd;

    // SAFETY: the retained file descriptor is live and a null buffer with a
    // zero size asks flistxattr for the required list length without writing.
    let length = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    if length >= 0 {
        return Ok(length != 0);
    }
    let error = std::io::Error::last_os_error();
    if error
        .raw_os_error()
        .is_some_and(|code| code == libc::ENOTSUP || code == libc::EOPNOTSUPP)
    {
        return Ok(false);
    }
    Err(format!(
        "inspect controlled target extended metadata: {error}"
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
                #[cfg(unix)]
                unix_metadata: None,
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
            #[cfg(unix)]
            unix_metadata: None,
        });
    }
    if before.is_dir() {
        return Ok(ObservedTarget {
            state: PathState::Directory,
            bytes: None,
            unsupported_reason: None,
            identity: Some(cap_identity(&before)),
            permissions: None,
            #[cfg(unix)]
            unix_metadata: None,
        });
    }
    if !before.is_file() {
        return Ok(ObservedTarget {
            state: PathState::Other,
            bytes: None,
            unsupported_reason: Some("target is not a regular file".to_string()),
            identity: Some(cap_identity(&before)),
            permissions: None,
            #[cfg(unix)]
            unix_metadata: None,
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
            #[cfg(unix)]
            unix_metadata: None,
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
    let permissions = metadata.permissions();
    let unsupported_reason = (links != 1).then(|| {
        format!("regular file has {links} hard links; controlled mutation requires exactly one")
    });
    #[cfg(unix)]
    let mut unsupported_reason = unsupported_reason;
    #[cfg(unix)]
    if unsupported_reason.is_none() && permissions.readonly() {
        unsupported_reason =
            Some("read-only POSIX mode is unsupported for controlled replacement".to_string());
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if unsupported_reason.is_none() && has_extended_metadata(&file)? {
        unsupported_reason =
            Some("extended attributes or POSIX ACL metadata cannot yet be preserved".to_string());
    }
    Ok(ObservedTarget {
        state: state_for_bytes(&bytes),
        bytes: Some(bytes),
        unsupported_reason,
        identity: Some(identity),
        permissions: Some(permissions),
        #[cfg(unix)]
        unix_metadata: Some(unix_metadata(&metadata)),
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

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::AfterPrefix(3),
            |_| {},
            |_| {},
            |_, _| {},
            |_| {},
        );

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

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |path| std::fs::write(path, b"racer\n").unwrap(),
            |_| {},
            |_, _| {},
            |_| {},
        );

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

    #[cfg(unix)]
    #[test]
    fn posix_readonly_target_is_rejected_during_preparation() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, workspace) = workspace();
        let target = directory.path().join("readonly.txt");
        std::fs::write(&target, b"before\n").unwrap();
        std::fs::set_permissions(&target, Permissions::from_mode(0o444)).unwrap();
        let ctx = PrepareCtx {
            workspace: &workspace,
            truncation_limit: 1024,
        };

        let error = match inspect_for_prepare(&ctx, "readonly.txt", false) {
            Ok(_) => panic!("read-only POSIX target must not be prepared"),
            Err(error) => error,
        };

        assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
        assert!(error.message.contains("read-only POSIX mode"));
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn posix_mode_owner_and_group_are_preserved_by_replacement() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let (directory, workspace) = workspace();
        let target = directory.path().join("metadata.txt");
        std::fs::write(&target, b"before\n").unwrap();
        std::fs::set_permissions(&target, Permissions::from_mode(0o750)).unwrap();
        let before = std::fs::metadata(&target).unwrap();
        let operation = FileMutationCandidate {
            path: "metadata.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"after\n".to_vec(),
            success: "published".to_string(),
        };

        let result = commit_candidate(&workspace, operation);

        assert!(!result.is_error, "{}", result.full);
        let after = std::fs::metadata(&target).unwrap();
        assert_eq!(after.mode() & 0o7777, before.mode() & 0o7777);
        assert_eq!(after.uid(), before.uid());
        assert_eq!(after.gid(), before.gid());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn extended_metadata_is_rejected_instead_of_silently_dropped() {
        use std::os::fd::AsRawFd;

        let (directory, workspace) = workspace();
        let target = directory.path().join("xattr.txt");
        std::fs::write(&target, b"before\n").unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&target)
            .unwrap();
        // SAFETY: the descriptor and static NUL-terminated name/value buffers
        // remain live for the call. Filesystems without user xattrs skip.
        let set = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                c"user.ferric-test".as_ptr(),
                b"present".as_ptr().cast(),
                b"present".len(),
                0,
            )
        };
        if set != 0 {
            return;
        }
        let ctx = PrepareCtx {
            workspace: &workspace,
            truncation_limit: 1024,
        };

        let error = match inspect_for_prepare(&ctx, "xattr.txt", false) {
            Ok(_) => panic!("extended metadata target must not be prepared"),
            Err(error) => error,
        };

        assert_eq!(error.kind, PrepareErrorKind::UnsupportedOperation);
        assert!(error.message.contains("extended attributes"));
    }

    #[test]
    fn final_boundary_leaf_replacement_is_restored_and_never_reported_as_success() {
        let (directory, workspace) = workspace();
        let path = directory.path().join("final-race.txt");
        std::fs::write(&path, b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "final-race.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"candidate\n".to_vec(),
            success: "must not be returned".to_string(),
        };

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |_| {},
            |target, _candidate| {
                std::fs::remove_file(target).unwrap();
                std::fs::write(target, b"racer\n").unwrap();
            },
            |_| {},
        );

        assert!(result.is_error);
        assert_eq!(
            result.failure.as_ref().map(|failure| failure.kind),
            Some(ControlFailureKind::StalePrecondition)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"racer\n");
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects) if effects.is_empty()
        ));
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            !name.starts_with(".ferric-candidate-") && !name.starts_with(".ferric-backup-")
        }));
    }

    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    #[test]
    fn displaced_original_move_before_cleanup_is_opaque_and_not_deleted() {
        let (directory, workspace) = workspace();
        let target = directory.path().join("cleanup-race.txt");
        let retained = directory.path().join("retained-original.txt");
        std::fs::write(&target, b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "cleanup-race.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"candidate\n".to_vec(),
            success: "must not be returned".to_string(),
        };
        let mut raced_private_path = None;

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |_| {},
            |_, _| {},
            |displaced| {
                let displaced = displaced.expect("existing replacement has displaced path");
                raced_private_path = Some(displaced.to_path_buf());
                std::fs::rename(displaced, &retained).unwrap();
                std::fs::write(displaced, b"racer-private\n").unwrap();
            },
        );

        assert!(result.is_error);
        assert_eq!(std::fs::read(&target).unwrap(), b"candidate\n");
        assert_eq!(std::fs::read(&retained).unwrap(), b"before\n");
        assert_eq!(
            std::fs::read(raced_private_path.expect("cleanup hook ran")).unwrap(),
            b"racer-private\n"
        );
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects)
                if effects.iter().any(|effect| effect.path == "cleanup-race.txt"
                    && effect.kind == WorkspaceEffectKind::Opaque)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn hard_link_created_during_candidate_write_is_reported_opaque() {
        let (directory, workspace) = workspace();
        let target = directory.path().join("during-write.txt");
        let retained = directory.path().join("retained-during-write.txt");
        std::fs::write(&target, b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "during-write.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"candidate-data\n".to_vec(),
            success: "must not be returned".to_string(),
        };

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |candidate| {
                std::fs::hard_link(candidate.expect("visible Windows candidate"), &retained)
                    .unwrap()
            },
            |_, _| panic!("publication hook must not run after candidate link-count failure"),
            |_| {},
        );

        assert!(result.is_error);
        assert_eq!(std::fs::read(&target).unwrap(), b"before\n");
        assert_eq!(std::fs::read(&retained).unwrap(), b"candidate-data\n");
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects)
                if matches!(effects.as_slice(), [effect] if effect.kind == WorkspaceEffectKind::Opaque)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn hard_link_created_after_final_candidate_check_is_reported_opaque() {
        let (directory, workspace) = workspace();
        let target = directory.path().join("after-check.txt");
        let retained = directory.path().join("retained-after-check.txt");
        std::fs::write(&target, b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "after-check.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"candidate-data\n".to_vec(),
            success: "must not be returned".to_string(),
        };

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |_| {},
            |_, candidate| {
                std::fs::hard_link(candidate.expect("visible Windows candidate"), &retained)
                    .unwrap();
            },
            |_| {},
        );

        assert!(result.is_error);
        assert_eq!(std::fs::read(&target).unwrap(), b"before\n");
        assert_eq!(std::fs::read(&retained).unwrap(), b"candidate-data\n");
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects)
                if matches!(effects.as_slice(), [effect] if effect.kind == WorkspaceEffectKind::Opaque)
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn o_tmpfile_candidate_is_not_visible_during_write_or_final_validation() {
        use std::cell::Cell;

        let (directory, workspace) = workspace();
        let target = directory.path().join("unnamed.txt");
        let parent = pin_parent(&workspace, &target).unwrap();
        {
            let Some((file, mut temporary)) =
                create_unnamed_temporary(&parent, "unnamed.txt").unwrap()
            else {
                return;
            };
            assert!(!temporary.named);
            assert!(std::fs::read_dir(directory.path()).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ferric-candidate-")
            }));
            drop(file);
            temporary.cleanup().unwrap();
        }

        let during_write = Cell::new(false);
        let final_check = Cell::new(false);
        let operation = FileMutationCandidate {
            path: "unnamed.txt".to_string(),
            expected: PathState::Absent,
            candidate: b"candidate\n".to_vec(),
            success: "published".to_string(),
        };
        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |candidate| {
                during_write.set(true);
                assert!(candidate.is_none());
            },
            |_, candidate| {
                final_check.set(true);
                assert!(candidate.is_none());
            },
            |_| {},
        );

        assert!(!result.is_error, "{}", result.full);
        assert!(during_write.get());
        assert!(final_check.get());
        assert_eq!(std::fs::read(target).unwrap(), b"candidate\n");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn final_boundary_ancestor_move_is_an_opaque_failure_not_success() {
        let (directory, workspace) = workspace();
        let nested = directory.path().join("nested");
        let held = directory.path().join("held");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("target.txt"), b"before\n").unwrap();
        let operation = FileMutationCandidate {
            path: "nested/target.txt".to_string(),
            expected: state_for_bytes(b"before\n"),
            candidate: b"candidate\n".to_vec(),
            success: "must not be returned".to_string(),
        };

        let result = commit_candidate_with(
            &workspace,
            operation,
            WriteFault::None,
            |_| {},
            |_| {},
            |_, _| {
                std::fs::rename(&nested, &held).unwrap();
                std::fs::create_dir(&nested).unwrap();
                std::fs::write(nested.join("target.txt"), b"racer\n").unwrap();
            },
            |_| {},
        );

        assert!(result.is_error);
        assert_eq!(
            std::fs::read(nested.join("target.txt")).unwrap(),
            b"racer\n"
        );
        assert_eq!(
            std::fs::read(held.join("target.txt")).unwrap(),
            b"candidate\n"
        );
        assert!(matches!(
            result.effects,
            WorkspaceEffectReport::Measured(ref effects)
                if matches!(effects.as_slice(), [effect] if effect.kind == WorkspaceEffectKind::Opaque)
        ));
    }
}
