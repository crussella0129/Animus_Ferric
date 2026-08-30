//! Lossless server-registration storage.
//!
//! The lifecycle layer needs to distinguish an absent registration from one
//! which exists but cannot safely authorize an action.  This module therefore
//! captures each configured slot independently, preserves the exact bytes used
//! for later compare-and-remove, and never silently falls through a blocked or
//! conflicting slot.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::{Builder, NamedTempFile};

use crate::server::{ServerRunfile, runfile_path};

const LEGACY_SCHEMA_VERSION: u8 = 1;
const IDENTITY_SCHEMA_VERSION: u8 = 2;

/// The role a registration path plays in lifecycle resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistrationScope {
    Local,
    Global,
    /// The local alias promised by a schema-v2 global registration.
    Origin,
}

impl fmt::Display for RegistrationScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Local => "local",
            Self::Global => "global",
            Self::Origin => "origin",
        })
    }
}

/// Why a configured registration slot could not be captured safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationBlock {
    Unreadable(String),
    Symlink,
    NonRegular,
    Malformed(String),
    InvalidSchema(String),
}

impl fmt::Display for RegistrationBlock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(detail) => write!(formatter, "unreadable: {detail}"),
            Self::Symlink => formatter.write_str("is a symbolic link or reparse point"),
            Self::NonRegular => formatter.write_str("is not a regular file"),
            Self::Malformed(detail) => write!(formatter, "contains malformed JSON: {detail}"),
            Self::InvalidSchema(detail) => write!(formatter, "has invalid schema: {detail}"),
        }
    }
}

/// One immutable registration snapshot, including the bytes later used for
/// compare-and-remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapturedRegistration {
    pub(crate) scope: RegistrationScope,
    pub(crate) path: PathBuf,
    pub(crate) raw: Vec<u8>,
    pub(crate) runfile: ServerRunfile,
}

/// State of one configured registration slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistrationSlot {
    Absent {
        scope: RegistrationScope,
        path: PathBuf,
    },
    Blocked {
        scope: RegistrationScope,
        path: PathBuf,
        reason: RegistrationBlock,
    },
    Captured(Box<CapturedRegistration>),
}

/// Independent local/global inventory. `global == None` means that no global
/// slot was configured; it is intentionally distinct from a configured but
/// absent global slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationInventory {
    pub(crate) local: RegistrationSlot,
    pub(crate) global: Option<RegistrationSlot>,
}

/// Capture both configured runfile locations without local-first fallback.
pub(crate) fn inventory_runfiles(
    workspace: &Path,
    global_path: Option<PathBuf>,
) -> RegistrationInventory {
    let local = capture_registration_path(RegistrationScope::Local, &runfile_path(workspace));
    let global = global_path
        .as_deref()
        .map(|path| capture_registration_path(RegistrationScope::Global, path));
    RegistrationInventory { local, global }
}

/// Capture an arbitrary registration path. This is also used to expand the
/// origin alias named by a selected schema-v2 global record before teardown.
pub(crate) fn capture_registration_path(scope: RegistrationScope, path: &Path) -> RegistrationSlot {
    let path = match absolute_path(path) {
        Ok(path) => path,
        Err(error) => {
            return RegistrationSlot::Blocked {
                scope,
                path: path.to_path_buf(),
                reason: RegistrationBlock::Unreadable(format!("resolve absolute path: {error}")),
            };
        }
    };

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return RegistrationSlot::Absent { scope, path };
        }
        Err(error) => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::Unreadable(error.to_string()),
            };
        }
    };
    if metadata.file_type().is_symlink() {
        return RegistrationSlot::Blocked {
            scope,
            path,
            reason: RegistrationBlock::Symlink,
        };
    }
    if !metadata.is_file() {
        return RegistrationSlot::Blocked {
            scope,
            path,
            reason: RegistrationBlock::NonRegular,
        };
    }

    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::Unreadable(error.to_string()),
            };
        }
    };

    // Recheck the path after reading so a simple replace-with-link race cannot
    // leave a link classified as a captured regular file.
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::Symlink,
            };
        }
        Ok(metadata) if !metadata.is_file() => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::NonRegular,
            };
        }
        Ok(_) => {}
        Err(error) => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::Unreadable(format!("reinspect after read: {error}")),
            };
        }
    }

    let runfile = match serde_json::from_slice::<ServerRunfile>(&raw) {
        Ok(runfile) => runfile,
        Err(error) => {
            return RegistrationSlot::Blocked {
                scope,
                path,
                reason: RegistrationBlock::Malformed(error.to_string()),
            };
        }
    };
    if let Err(detail) = validate_runfile(scope, &path, &runfile) {
        return RegistrationSlot::Blocked {
            scope,
            path,
            reason: RegistrationBlock::InvalidSchema(detail),
        };
    }

    RegistrationSlot::Captured(Box::new(CapturedRegistration {
        scope,
        path,
        raw,
        runfile,
    }))
}

pub(crate) fn validate_runfile(
    scope: RegistrationScope,
    capture_path: &Path,
    runfile: &ServerRunfile,
) -> Result<(), String> {
    match runfile.schema_version {
        LEGACY_SCHEMA_VERSION => {
            if runfile.process_identity.is_some() || runfile.origin_local_runfile.is_some() {
                return Err(
                    "schema 1 must not carry process_identity or origin_local_runfile".to_string(),
                );
            }
        }
        IDENTITY_SCHEMA_VERSION => {
            if runfile.pid == 0 {
                return Err("schema 2 requires a nonzero pid".to_string());
            }
            if runfile.port == 0 {
                return Err("schema 2 requires a nonzero port".to_string());
            }
            if !runfile.tailscale {
                let expected_base_url = format!("http://127.0.0.1:{}/v1", runfile.port);
                if runfile.base_url != expected_base_url {
                    return Err(format!(
                        "schema 2 non-Tailscale base_url must be {expected_base_url}"
                    ));
                }
            }
            let identity = runfile
                .process_identity
                .as_ref()
                .ok_or_else(|| "schema 2 requires process_identity".to_string())?;
            if identity.start_token.trim().is_empty() {
                return Err("schema 2 process_identity requires a start_token".to_string());
            }
            if !identity.executable.is_absolute() {
                return Err(
                    "schema 2 process_identity executable must be an absolute path".to_string(),
                );
            }
            if identity.argv.is_empty() {
                return Err("schema 2 process_identity requires observed argv".to_string());
            }
            if identity.argv.iter().any(|argument| argument.is_empty()) {
                return Err("schema 2 process_identity argv elements must not be empty".to_string());
            }
            let origin = runfile
                .origin_local_runfile
                .as_deref()
                .ok_or_else(|| "schema 2 requires origin_local_runfile".to_string())?;
            if !origin.is_absolute() {
                return Err("schema 2 origin_local_runfile must be absolute".to_string());
            }
            if !has_local_runfile_suffix(origin) {
                return Err(
                    "schema 2 origin_local_runfile must end in .ferric/server.json".to_string(),
                );
            }
            if matches!(scope, RegistrationScope::Local | RegistrationScope::Origin)
                && !paths_match(origin, capture_path)
            {
                return Err(format!(
                    "schema 2 local origin {} does not name its own registration {}",
                    origin.display(),
                    capture_path.display()
                ));
            }
        }
        version => return Err(format!("unsupported schema version {version}")),
    }
    Ok(())
}

fn has_local_runfile_suffix(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "server.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == ".ferric")
}

fn absolute_path(path: &Path) -> io::Result<PathBuf> {
    std::path::absolute(path).map(|path| lexical_normalize(&path))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                // Callers pass an absolute path. At its root, `..` is a no-op.
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
        }
    }
    normalized
}

fn paths_match(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => absolute_path(left).ok() == absolute_path(right).ok(),
    }
}

/// Why the inventory cannot be reduced to one legacy-compatible record.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectionError {
    Blocked {
        scope: RegistrationScope,
        path: PathBuf,
        reason: RegistrationBlock,
    },
    Conflict {
        local: PathBuf,
        global: PathBuf,
    },
}

#[cfg(test)]
impl fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked {
                scope,
                path,
                reason,
            } => write!(
                formatter,
                "{scope} server registration at {} is blocked: {reason}",
                path.display()
            ),
            Self::Conflict { local, global } => write!(
                formatter,
                "local registration {} conflicts with global registration {}",
                local.display(),
                global.display()
            ),
        }
    }
}

#[cfg(test)]
impl std::error::Error for SelectionError {}

/// Select a unique record for non-destructive legacy consumers.
///
/// A blocked configured slot fails closed. Two captures are the same logical
/// registration only when their exact bytes match or both are schema-v2 and
/// parse to the same complete record. Identity equality alone is insufficient:
/// different ports, URLs, or launch metadata remain a discovery conflict.
#[cfg(test)]
pub(crate) fn select_unique(
    inventory: &RegistrationInventory,
) -> Result<Option<&CapturedRegistration>, SelectionError> {
    fn classify(slot: &RegistrationSlot) -> Result<Option<&CapturedRegistration>, SelectionError> {
        match slot {
            RegistrationSlot::Absent { .. } => Ok(None),
            RegistrationSlot::Blocked {
                scope,
                path,
                reason,
            } => Err(SelectionError::Blocked {
                scope: *scope,
                path: path.clone(),
                reason: reason.clone(),
            }),
            RegistrationSlot::Captured(captured) => Ok(Some(captured)),
        }
    }

    let local = classify(&inventory.local)?;
    let global = inventory
        .global
        .as_ref()
        .map(classify)
        .transpose()?
        .flatten();
    match (local, global) {
        (None, None) => Ok(None),
        (Some(captured), None) | (None, Some(captured)) => Ok(Some(captured)),
        (Some(local), Some(global)) => {
            let exact_duplicate = local.raw == global.raw;
            let same_v2_record = local.runfile.schema_version == IDENTITY_SCHEMA_VERSION
                && global.runfile.schema_version == IDENTITY_SCHEMA_VERSION
                && local.runfile.process_identity.is_some()
                && local.runfile == global.runfile;
            if exact_duplicate || same_v2_record {
                Ok(Some(local))
            } else {
                Err(SelectionError::Conflict {
                    local: local.path.clone(),
                    global: global.path.clone(),
                })
            }
        }
    }
}

/// Successful atomic publication of one or two identical registrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishedRegistrations {
    pub(crate) local: CapturedRegistration,
    pub(crate) global: Option<CapturedRegistration>,
}

/// Atomic publication failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishError {
    Invalid {
        scope: RegistrationScope,
        path: PathBuf,
        detail: String,
    },
    Serialize(String),
    Write {
        scope: RegistrationScope,
        path: PathBuf,
        detail: String,
    },
    Mirror {
        path: PathBuf,
        detail: String,
        /// Exact local capture which the process-owning caller must pass to
        /// `remove_if_unchanged` only after stopping and waiting for its child.
        local: Box<CapturedRegistration>,
    },
    /// A final path was committed, but syncing its parent directory failed.
    /// The process-owning caller must stop/wait its child before conditionally
    /// removing every exact capture in `published`.
    Durability {
        path: PathBuf,
        detail: String,
        published: Box<PublishedRegistrations>,
    },
}

impl fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid {
                scope,
                path,
                detail,
            } => write!(
                formatter,
                "invalid {scope} registration for {}: {detail}",
                path.display()
            ),
            Self::Serialize(detail) => write!(formatter, "serialize server registration: {detail}"),
            Self::Write {
                scope,
                path,
                detail,
            } => write!(
                formatter,
                "publish {scope} registration at {}: {detail}",
                path.display()
            ),
            Self::Mirror {
                path,
                detail,
                local,
            } => write!(
                formatter,
                "publish global registration at {}: {detail}; local registration at {} requires caller-owned rollback after child shutdown",
                path.display(),
                local.path.display()
            ),
            Self::Durability {
                path,
                detail,
                published,
            } => write!(
                formatter,
                "registration was committed at {} but its parent-directory durability check failed: {detail}; {} published scope(s) require caller-owned rollback after child shutdown",
                path.display(),
                1 + usize::from(published.global.is_some())
            ),
        }
    }
}

impl std::error::Error for PublishError {}

/// Serialize once and atomically publish the same bytes to the local slot and,
/// when configured, the global slot. Existing destinations are never replaced.
pub(crate) fn publish_mirrored(
    workspace: &Path,
    global_path: Option<&Path>,
    runfile: &ServerRunfile,
) -> Result<PublishedRegistrations, PublishError> {
    let local_path =
        absolute_path(&runfile_path(workspace)).map_err(|error| PublishError::Invalid {
            scope: RegistrationScope::Local,
            path: runfile_path(workspace),
            detail: format!("resolve absolute path: {error}"),
        })?;
    let global_path = global_path
        .map(|path| {
            absolute_path(path).map_err(|error| PublishError::Invalid {
                scope: RegistrationScope::Global,
                path: path.to_path_buf(),
                detail: format!("resolve absolute path: {error}"),
            })
        })
        .transpose()?;

    if let Some(global_path) = &global_path
        && paths_match(&local_path, global_path)
    {
        return Err(PublishError::Invalid {
            scope: RegistrationScope::Global,
            path: global_path.clone(),
            detail: "global and local registration paths alias each other".to_string(),
        });
    }
    validate_runfile(RegistrationScope::Local, &local_path, runfile).map_err(|detail| {
        PublishError::Invalid {
            scope: RegistrationScope::Local,
            path: local_path.clone(),
            detail,
        }
    })?;
    if let Some(global_path) = &global_path {
        validate_runfile(RegistrationScope::Global, global_path, runfile).map_err(|detail| {
            PublishError::Invalid {
                scope: RegistrationScope::Global,
                path: global_path.clone(),
                detail,
            }
        })?;
    }

    // This is deliberately the sole serialization call. Both scopes receive
    // clones of this exact byte vector.
    let raw = serde_json::to_vec_pretty(runfile)
        .map_err(|error| PublishError::Serialize(error.to_string()))?;
    if let Err(error) = persist_bytes_noclobber(&local_path, &raw) {
        if !error.committed {
            return Err(PublishError::Write {
                scope: RegistrationScope::Local,
                path: local_path,
                detail: error.to_string(),
            });
        }
        let local = CapturedRegistration {
            scope: RegistrationScope::Local,
            path: local_path.clone(),
            raw,
            runfile: runfile.clone(),
        };
        return Err(PublishError::Durability {
            path: local_path,
            detail: error.to_string(),
            published: Box::new(PublishedRegistrations {
                local,
                global: None,
            }),
        });
    }
    let local = CapturedRegistration {
        scope: RegistrationScope::Local,
        path: local_path,
        raw: raw.clone(),
        runfile: runfile.clone(),
    };

    let global = if let Some(global_path) = global_path {
        if let Err(error) = persist_bytes_noclobber(&global_path, &raw) {
            if error.committed {
                let global = CapturedRegistration {
                    scope: RegistrationScope::Global,
                    path: global_path.clone(),
                    raw,
                    runfile: runfile.clone(),
                };
                return Err(PublishError::Durability {
                    path: global_path,
                    detail: error.to_string(),
                    published: Box::new(PublishedRegistrations {
                        local,
                        global: Some(global),
                    }),
                });
            }
            return Err(PublishError::Mirror {
                path: global_path,
                detail: error.to_string(),
                local: Box::new(local),
            });
        }
        Some(CapturedRegistration {
            scope: RegistrationScope::Global,
            path: global_path,
            raw,
            runfile: runfile.clone(),
        })
    } else {
        None
    };

    Ok(PublishedRegistrations { local, global })
}

#[derive(Debug)]
struct PersistFailure {
    kind: io::ErrorKind,
    detail: String,
    committed: bool,
}

impl fmt::Display for PersistFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

fn persist_failure(context: impl fmt::Display, error: io::Error) -> PersistFailure {
    PersistFailure {
        kind: error.kind(),
        detail: format!("{context}: {error}"),
        committed: false,
    }
}

fn persist_bytes_noclobber(path: &Path, raw: &[u8]) -> Result<(), PersistFailure> {
    let parent = path.parent().ok_or_else(|| PersistFailure {
        kind: io::ErrorKind::InvalidInput,
        detail: format!("path {} has no parent", path.display()),
        committed: false,
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        persist_failure(format_args!("create parent {}", parent.display()), error)
    })?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
        persist_failure(format_args!("inspect parent {}", parent.display()), error)
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(PersistFailure {
            kind: io::ErrorKind::InvalidInput,
            detail: format!("parent {} is not a regular directory", parent.display()),
            committed: false,
        });
    }

    let mut temporary: NamedTempFile = Builder::new()
        .prefix(".server-registration-")
        .tempfile_in(parent)
        .map_err(|error| {
            persist_failure(
                format_args!("create temporary file in {}", parent.display()),
                error,
            )
        })?;
    temporary
        .write_all(raw)
        .map_err(|error| persist_failure("write temporary registration", error))?;
    temporary
        .as_file_mut()
        .flush()
        .map_err(|error| persist_failure("flush temporary registration", error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| persist_failure("sync temporary registration", error))?;
    temporary.persist_noclobber(path).map_err(|error| {
        persist_failure("persist without replacing an existing path", error.error)
    })?;
    sync_parent_directory(parent).map_err(|error| PersistFailure {
        kind: error.kind(),
        detail: format!("sync parent directory {}: {error}", parent.display()),
        committed: true,
    })?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    fs::File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    // Rust's portable File API cannot open a Windows directory with
    // FILE_FLAG_BACKUP_SEMANTICS. The stage file itself is flushed before the
    // no-clobber commit; Windows publication claims that file-level boundary,
    // not a directory-metadata flush.
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

/// Result of an exact-byte conditional removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    Removed,
    Absent,
    /// A different/non-regular entry was moved aside and retained here because
    /// deleting it was not authorized. When possible its exact bytes are also
    /// restored at the original name without clobbering another entry.
    ReplacementPreserved {
        path: PathBuf,
        detail: String,
    },
}

/// Result of replacing one captured registration without ever overwriting a
/// concurrent entry at the original name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplacementOutcome {
    Replaced,
    Absent,
    /// The entry moved for comparison did not match the capture. Its bytes are
    /// retained at `path` and, when the name remained free, restored there too.
    ReplacementPreserved {
        path: PathBuf,
        detail: String,
    },
}

/// Conditional-replacement failure. `preserved_at`, when present, retains the
/// atomically isolated entry so an I/O failure cannot destroy recovery state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplacementError {
    pub(crate) path: PathBuf,
    pub(crate) detail: String,
    pub(crate) preserved_at: Option<PathBuf>,
    /// True only when the replacement reached the final name before a later
    /// durability operation failed.
    pub(crate) replacement_committed: bool,
}

impl fmt::Display for ReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "replace registration at {} only if unchanged: {}",
            self.path.display(),
            self.detail
        )?;
        if let Some(preserved) = &self.preserved_at {
            write!(
                formatter,
                "; isolated entry retained at {}",
                preserved.display()
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for ReplacementError {}

/// Replace a captured registration only when the entry atomically moved out
/// of its name still has the exact captured bytes. The replacement is
/// published with the same no-clobber primitive as a new registration, so an
/// entry created after isolation is never overwritten.
pub(crate) fn replace_if_unchanged(
    captured: &CapturedRegistration,
    replacement: &[u8],
) -> Result<ReplacementOutcome, ReplacementError> {
    replace_if_unchanged_impl(
        captured,
        replacement,
        |_| {},
        persist_bytes_noclobber,
        persist_bytes_noclobber,
    )
}

fn replace_if_unchanged_impl<F, P, R>(
    captured: &CapturedRegistration,
    replacement: &[u8],
    after_rename: F,
    publish_replacement: P,
    restore_original: R,
) -> Result<ReplacementOutcome, ReplacementError>
where
    F: FnOnce(&Path),
    P: FnOnce(&Path, &[u8]) -> Result<(), PersistFailure>,
    R: FnOnce(&Path, &[u8]) -> Result<(), PersistFailure>,
{
    let original = &captured.path;
    let metadata = match fs::symlink_metadata(original) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ReplacementOutcome::Absent);
        }
        Err(error) => {
            return Err(ReplacementError {
                path: original.clone(),
                detail: format!("inspect current entry: {error}"),
                preserved_at: None,
                replacement_committed: false,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ReplacementError {
            path: original.clone(),
            detail: "current entry is not a regular non-symlink file".to_string(),
            preserved_at: None,
            replacement_committed: false,
        });
    }

    let parent = original.parent().ok_or_else(|| ReplacementError {
        path: original.clone(),
        detail: "registration path has no parent".to_string(),
        preserved_at: None,
        replacement_committed: false,
    })?;
    let holding_dir = Builder::new()
        .prefix(".server-registration-replace-")
        .tempdir_in(parent)
        .map_err(|error| ReplacementError {
            path: original.clone(),
            detail: format!("create same-parent holding directory: {error}"),
            preserved_at: None,
            replacement_committed: false,
        })?;
    let moved = holding_dir.path().join("registration");
    match fs::rename(original, &moved) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ReplacementOutcome::Absent);
        }
        Err(error) => {
            return Err(ReplacementError {
                path: original.clone(),
                detail: format!("atomically move into holding directory: {error}"),
                preserved_at: None,
                replacement_committed: false,
            });
        }
    }

    after_rename(original);

    let moved_metadata = match fs::symlink_metadata(&moved) {
        Ok(metadata) => metadata,
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(ReplacementError {
                path: original.clone(),
                detail: format!("inspect atomically moved entry: {error}"),
                preserved_at: Some(preserved),
                replacement_committed: false,
            });
        }
    };
    if moved_metadata.file_type().is_symlink() || !moved_metadata.is_file() {
        let preserved = keep_holding_dir(holding_dir, "registration");
        return Ok(ReplacementOutcome::ReplacementPreserved {
            path: preserved,
            detail: "atomically moved entry is not a regular non-symlink file".to_string(),
        });
    }
    let moved_raw = match fs::read(&moved) {
        Ok(raw) => raw,
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(ReplacementError {
                path: original.clone(),
                detail: format!("read atomically moved entry: {error}"),
                preserved_at: Some(preserved),
                replacement_committed: false,
            });
        }
    };

    if moved_raw != captured.raw {
        let detail = match restore_original(original, &moved_raw) {
            Ok(()) => {
                "changed entry was restored without clobbering and retained in the holding directory"
                    .to_string()
            }
            Err(error) if error.kind == io::ErrorKind::AlreadyExists => format!(
                "a concurrent entry occupies the original name; moved replacement retained: {error}"
            ),
            Err(error) => {
                let preserved = keep_holding_dir(holding_dir, "registration");
                return Err(ReplacementError {
                    path: original.clone(),
                    detail: format!("could not restore changed entry: {error}"),
                    preserved_at: Some(preserved),
                    replacement_committed: false,
                });
            }
        };
        let preserved = keep_holding_dir(holding_dir, "registration");
        return Ok(ReplacementOutcome::ReplacementPreserved {
            path: preserved,
            detail,
        });
    }

    if let Err(error) = publish_replacement(original, replacement) {
        let committed = error.committed;
        let restore_detail = if committed {
            String::new()
        } else {
            match restore_original(original, &moved_raw) {
                Ok(()) => "; original bytes restored".to_string(),
                Err(restore_error) => {
                    format!("; original restore was not completed: {restore_error}")
                }
            }
        };
        let preserved = keep_holding_dir(holding_dir, "registration");
        return Err(ReplacementError {
            path: original.clone(),
            detail: format!("publish replacement: {error}{restore_detail}"),
            preserved_at: Some(preserved),
            replacement_committed: committed,
        });
    }

    if let Err(error) = fs::remove_file(&moved) {
        let preserved = keep_holding_dir(holding_dir, "registration");
        return Err(ReplacementError {
            path: original.clone(),
            detail: format!(
                "replacement committed but old isolated entry could not be removed: {error}"
            ),
            preserved_at: Some(preserved),
            replacement_committed: true,
        });
    }
    holding_dir.close().map_err(|error| ReplacementError {
        path: original.clone(),
        detail: format!(
            "replacement committed but empty holding directory could not be removed: {error}"
        ),
        preserved_at: None,
        replacement_committed: true,
    })?;
    Ok(ReplacementOutcome::Replaced)
}

/// Conditional-removal failure. `preserved_at`, when present, is an isolated
/// same-parent location retaining the entry which was moved for inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovalError {
    pub(crate) path: PathBuf,
    pub(crate) detail: String,
    pub(crate) preserved_at: Option<PathBuf>,
}

impl fmt::Display for RemovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "remove registration at {} only if unchanged: {}",
            self.path.display(),
            self.detail
        )?;
        if let Some(preserved) = &self.preserved_at {
            write!(
                formatter,
                "; moved entry retained at {}",
                preserved.display()
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for RemovalError {}

/// Remove a captured registration only when the entry atomically moved out of
/// its name still has the exact captured bytes. A replacement created after
/// the rename is never opened or removed.
pub(crate) fn remove_if_unchanged(
    captured: &CapturedRegistration,
) -> Result<RemovalOutcome, RemovalError> {
    remove_if_unchanged_impl(captured, |_| {}, persist_bytes_noclobber)
}

fn remove_if_unchanged_impl<F, R>(
    captured: &CapturedRegistration,
    after_rename: F,
    restore_changed: R,
) -> Result<RemovalOutcome, RemovalError>
where
    F: FnOnce(&Path),
    R: FnOnce(&Path, &[u8]) -> Result<(), PersistFailure>,
{
    let original = &captured.path;
    let metadata = match fs::symlink_metadata(original) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RemovalOutcome::Absent);
        }
        Err(error) => {
            return Err(RemovalError {
                path: original.clone(),
                detail: format!("inspect current entry: {error}"),
                preserved_at: None,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RemovalError {
            path: original.clone(),
            detail: "current entry is not a regular non-symlink file".to_string(),
            preserved_at: None,
        });
    }

    let parent = original.parent().ok_or_else(|| RemovalError {
        path: original.clone(),
        detail: "registration path has no parent".to_string(),
        preserved_at: None,
    })?;
    let holding_dir = Builder::new()
        .prefix(".server-registration-remove-")
        .tempdir_in(parent)
        .map_err(|error| RemovalError {
            path: original.clone(),
            detail: format!("create same-parent holding directory: {error}"),
            preserved_at: None,
        })?;
    let moved = holding_dir.path().join("registration");
    match fs::rename(original, &moved) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RemovalOutcome::Absent);
        }
        Err(error) => {
            return Err(RemovalError {
                path: original.clone(),
                detail: format!("atomically move into holding directory: {error}"),
                preserved_at: None,
            });
        }
    }

    after_rename(original);

    let moved_metadata = match fs::symlink_metadata(&moved) {
        Ok(metadata) => metadata,
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(RemovalError {
                path: original.clone(),
                detail: format!("inspect atomically moved entry: {error}"),
                preserved_at: Some(preserved),
            });
        }
    };
    if moved_metadata.file_type().is_symlink() || !moved_metadata.is_file() {
        let preserved = keep_holding_dir(holding_dir, "registration");
        return Ok(RemovalOutcome::ReplacementPreserved {
            path: preserved,
            detail: "atomically moved entry is not a regular non-symlink file".to_string(),
        });
    }
    let moved_raw = match fs::read(&moved) {
        Ok(raw) => raw,
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(RemovalError {
                path: original.clone(),
                detail: format!("read atomically moved entry: {error}"),
                preserved_at: Some(preserved),
            });
        }
    };

    if moved_raw == captured.raw {
        if let Err(error) = fs::remove_file(&moved) {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(RemovalError {
                path: original.clone(),
                detail: format!("remove unchanged moved entry: {error}"),
                preserved_at: Some(preserved),
            });
        }
        holding_dir.close().map_err(|error| RemovalError {
            path: original.clone(),
            detail: format!("remove empty holding directory: {error}"),
            preserved_at: None,
        })?;
        return match fs::symlink_metadata(original) {
            Ok(_) => Ok(RemovalOutcome::ReplacementPreserved {
                path: original.clone(),
                detail:
                    "a replacement appeared at the original name after the captured entry was moved"
                        .to_string(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RemovalOutcome::Removed),
            Err(error) => Err(RemovalError {
                path: original.clone(),
                detail: format!("inspect original name after removing captured entry: {error}"),
                preserved_at: None,
            }),
        };
    }

    // Restore the exact moved bytes through the same atomic no-clobber path
    // used for publication. This works on filesystems without hard-link
    // support and fails safely if another process has already recreated the
    // original name. The holding copy remains in either case, so a concurrent
    // unlink of the restored name still cannot destroy the replacement.
    match restore_changed(original, &moved_raw) {
        Ok(()) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            Ok(RemovalOutcome::ReplacementPreserved {
                path: preserved,
                detail: "changed entry was restored without clobbering and retained in the holding directory"
                    .to_string(),
            })
        }
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            if error.kind == io::ErrorKind::AlreadyExists {
                Ok(RemovalOutcome::ReplacementPreserved {
                    path: preserved,
                    detail: format!(
                        "a concurrent entry occupies the original name; moved replacement retained: {error}"
                    ),
                })
            } else {
                Err(RemovalError {
                    path: original.clone(),
                    detail: format!("could not restore changed entry: {error}"),
                    preserved_at: Some(preserved),
                })
            }
        }
    }
}

fn keep_holding_dir(directory: tempfile::TempDir, file_name: &str) -> PathBuf {
    directory.keep().join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Engine;
    use crate::server_process::ProcessIdentity;

    fn legacy_runfile(pid: u32) -> ServerRunfile {
        ServerRunfile {
            schema_version: LEGACY_SCHEMA_VERSION,
            engine: Engine::LlamaServer,
            pid,
            port: 8080,
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            tailscale: false,
            model: Some("model.gguf".to_string()),
            context_size: Some(4096),
            sampling_seed: Some(42),
            parallel_slots: Some(1),
            process_identity: None,
            origin_local_runfile: None,
        }
    }

    fn identity(token: &str) -> ProcessIdentity {
        ProcessIdentity {
            start_token: token.to_string(),
            executable: absolute_path(Path::new("llama-server")).unwrap(),
            argv: vec![
                "llama-server".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ],
        }
    }

    fn v2_runfile(origin: &Path, token: &str) -> ServerRunfile {
        let mut runfile = legacy_runfile(1234);
        runfile.schema_version = IDENTITY_SCHEMA_VERSION;
        runfile.process_identity = Some(identity(token));
        runfile.origin_local_runfile = Some(origin.to_path_buf());
        runfile
    }

    fn write_runfile(path: &Path, runfile: &ServerRunfile) -> Vec<u8> {
        let raw = serde_json::to_vec_pretty(runfile).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &raw).unwrap();
        raw
    }

    fn captured(slot: &RegistrationSlot) -> &CapturedRegistration {
        match slot {
            RegistrationSlot::Captured(captured) => captured.as_ref(),
            other => panic!("expected captured registration, got {other:?}"),
        }
    }

    #[test]
    fn inventory_preserves_independent_absent_slots() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let global = root.path().join("config").join("server.json");
        fs::create_dir_all(&workspace).unwrap();

        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert!(matches!(
            inventory.local,
            RegistrationSlot::Absent {
                scope: RegistrationScope::Local,
                ..
            }
        ));
        assert!(matches!(
            inventory.global,
            Some(RegistrationSlot::Absent {
                scope: RegistrationScope::Global,
                ref path
            }) if *path == absolute_path(&global).unwrap()
        ));
        assert_eq!(select_unique(&inventory).unwrap(), None);

        let without_global = inventory_runfiles(&workspace, None);
        assert!(without_global.global.is_none());
    }

    #[test]
    fn capture_retains_exact_bytes_and_legacy_defaults() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let raw = br#"{
  "engine": "llama-server",
  "pid": 9,
  "port": 8080,
  "base_url": "http://127.0.0.1:8080/v1"
}"#;
        fs::write(&path, raw).unwrap();

        let slot = capture_registration_path(RegistrationScope::Local, &path);
        let capture = captured(&slot);
        assert_eq!(capture.raw, raw);
        assert_eq!(capture.runfile.schema_version, LEGACY_SCHEMA_VERSION);
        assert!(capture.runfile.process_identity.is_none());
    }

    #[test]
    fn capture_distinguishes_nonregular_malformed_and_invalid_schema() {
        let root = tempfile::tempdir().unwrap();
        let unreadable = PathBuf::from(format!(
            "{}\0server.json",
            root.path().join("invalid-component").display()
        ));
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &unreadable),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::Unreadable(_),
                ..
            }
        ));

        let directory = root.path().join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &directory),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::NonRegular,
                ..
            }
        ));

        let malformed = root.path().join("malformed.json");
        fs::write(&malformed, b"not-json").unwrap();
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &malformed),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::Malformed(_),
                ..
            }
        ));

        let unknown = root.path().join("unknown.json");
        let mut unknown_runfile = legacy_runfile(1);
        unknown_runfile.schema_version = 99;
        write_runfile(&unknown, &unknown_runfile);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &unknown),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("unsupported schema version 99")
        ));

        let incomplete = root.path().join("incomplete.json");
        let mut incomplete_runfile = legacy_runfile(2);
        incomplete_runfile.schema_version = IDENTITY_SCHEMA_VERSION;
        incomplete_runfile.origin_local_runfile =
            Some(absolute_path(&root.path().join("workspace/.ferric/server.json")).unwrap());
        write_runfile(&incomplete, &incomplete_runfile);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &incomplete),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("requires process_identity")
        ));

        let v1_with_identity = root.path().join("v1-with-identity.json");
        let mut downgraded = legacy_runfile(3);
        downgraded.process_identity = Some(identity("unexpected"));
        write_runfile(&v1_with_identity, &downgraded);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &v1_with_identity),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("schema 1 must not carry")
        ));
    }

    #[test]
    fn capture_rejects_symlink_without_following_it() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target.json");
        write_runfile(&target, &legacy_runfile(1));
        let link = root.path().join("link.json");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            // Windows developer-mode/elevation policy can forbid symlink
            // creation; the production classification remains compiled.
            return;
        }

        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &link),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::Symlink,
                ..
            }
        ));
    }

    #[test]
    fn global_v2_may_name_another_workspace_but_local_must_name_itself() {
        let root = tempfile::tempdir().unwrap();
        let workspace_a = root.path().join("workspace-a");
        let workspace_b = root.path().join("workspace-b");
        let local_a = absolute_path(&runfile_path(&workspace_a)).unwrap();
        let local_b = absolute_path(&runfile_path(&workspace_b)).unwrap();
        let global = root.path().join("config").join("server.json");
        let runfile = v2_runfile(&local_b, "same-process");

        write_runfile(&global, &runfile);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &global),
            RegistrationSlot::Captured(_)
        ));

        write_runfile(&local_a, &runfile);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Local, &local_a),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("does not name its own registration")
        ));
        assert!(matches!(
            capture_registration_path(RegistrationScope::Origin, &local_a),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("does not name its own registration")
        ));

        let origin_raw = write_runfile(&local_b, &runfile);
        let global_capture = captured(&capture_registration_path(
            RegistrationScope::Global,
            &global,
        ))
        .clone();
        let origin_capture = captured(&capture_registration_path(
            RegistrationScope::Origin,
            &local_b,
        ))
        .clone();
        assert_eq!(origin_capture.scope, RegistrationScope::Origin);
        assert_eq!(origin_capture.raw, origin_raw);
        assert_eq!(origin_capture.runfile, global_capture.runfile);
    }

    #[test]
    fn v2_origin_must_be_absolute_and_have_local_runfile_shape() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.json");
        let relative = v2_runfile(Path::new("workspace/.ferric/server.json"), "token");
        write_runfile(&global, &relative);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &global),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("must be absolute")
        ));

        let wrong_shape = v2_runfile(&root.path().join("server.json"), "token");
        write_runfile(&global, &wrong_shape);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &global),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("must end in .ferric/server.json")
        ));
    }

    #[test]
    fn v2_identity_values_must_be_authoritative() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.json");
        let origin = absolute_path(&root.path().join("workspace/.ferric/server.json")).unwrap();

        let mut cases = Vec::new();
        let mut zero_pid = v2_runfile(&origin, "token");
        zero_pid.pid = 0;
        cases.push((zero_pid, "nonzero pid"));
        let mut zero_port = v2_runfile(&origin, "token");
        zero_port.port = 0;
        cases.push((zero_port, "nonzero port"));
        let empty_token = v2_runfile(&origin, "   ");
        cases.push((empty_token, "start_token"));
        let mut relative_executable = v2_runfile(&origin, "token");
        relative_executable
            .process_identity
            .as_mut()
            .unwrap()
            .executable = PathBuf::from("llama-server");
        cases.push((relative_executable, "absolute path"));
        let mut empty_argv = v2_runfile(&origin, "token");
        empty_argv.process_identity.as_mut().unwrap().argv.clear();
        cases.push((empty_argv, "observed argv"));
        let mut empty_argv_element = v2_runfile(&origin, "token");
        empty_argv_element
            .process_identity
            .as_mut()
            .unwrap()
            .argv
            .push(String::new());
        cases.push((empty_argv_element, "argv elements"));
        let mut divergent_base_url = v2_runfile(&origin, "token");
        divergent_base_url.base_url = "http://127.0.0.1:9090/v1".to_string();
        cases.push((divergent_base_url, "non-Tailscale base_url"));

        for (runfile, expected) in cases {
            write_runfile(&global, &runfile);
            assert!(matches!(
                capture_registration_path(RegistrationScope::Global, &global),
                RegistrationSlot::Blocked {
                    reason: RegistrationBlock::InvalidSchema(detail),
                    ..
                } if detail.contains(expected)
            ));
        }
    }

    #[test]
    fn unique_selection_accepts_single_duplicate_and_same_v2_record() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let global = root.path().join("config").join("server.json");

        write_runfile(&local, &legacy_runfile(1));
        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert_eq!(
            select_unique(&inventory).unwrap().unwrap().scope,
            RegistrationScope::Local
        );

        let duplicate = fs::read(&local).unwrap();
        fs::create_dir_all(global.parent().unwrap()).unwrap();
        fs::write(&global, duplicate).unwrap();
        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert_eq!(
            select_unique(&inventory).unwrap().unwrap().scope,
            RegistrationScope::Local
        );

        let local_v2 = v2_runfile(&local, "shared-token");
        write_runfile(&local, &local_v2);
        fs::write(&global, serde_json::to_vec(&local_v2).unwrap()).unwrap();
        let inventory = inventory_runfiles(&workspace, Some(global));
        assert_ne!(
            captured(&inventory.local).raw,
            captured(inventory.global.as_ref().unwrap()).raw
        );
        assert_eq!(
            select_unique(&inventory).unwrap().unwrap().scope,
            RegistrationScope::Local
        );
    }

    #[test]
    fn unique_selection_fails_closed_on_conflict_or_blocker() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = runfile_path(&workspace);
        let global = root.path().join("config").join("server.json");
        write_runfile(&local, &legacy_runfile(1));
        write_runfile(&global, &legacy_runfile(2));
        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert!(matches!(
            select_unique(&inventory),
            Err(SelectionError::Conflict { .. })
        ));

        let local_path = absolute_path(&local).unwrap();
        let local_v2 = v2_runfile(&local_path, "same-identity");
        let mut contradictory_global = local_v2.clone();
        contradictory_global.port = 9090;
        contradictory_global.base_url = "http://127.0.0.1:9090/v1".to_string();
        write_runfile(&local, &local_v2);
        write_runfile(&global, &contradictory_global);
        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert!(matches!(
            select_unique(&inventory),
            Err(SelectionError::Conflict { .. })
        ));

        fs::write(&global, b"malformed").unwrap();
        let inventory = inventory_runfiles(&workspace, Some(global));
        assert!(matches!(
            select_unique(&inventory),
            Err(SelectionError::Blocked {
                scope: RegistrationScope::Global,
                reason: RegistrationBlock::Malformed(_),
                ..
            })
        ));
    }

    #[test]
    fn mirrored_publish_is_identical_and_never_clobbers() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let global = root.path().join("config").join("server.json");
        let runfile = v2_runfile(&local, "publish-token");

        let published = publish_mirrored(&workspace, Some(&global), &runfile).unwrap();
        assert_eq!(fs::read(&local).unwrap(), fs::read(&global).unwrap());
        assert_eq!(published.local.raw, published.global.unwrap().raw);

        let before = fs::read(&local).unwrap();
        let error = publish_mirrored(&workspace, Some(&global), &runfile).unwrap_err();
        assert!(matches!(
            error,
            PublishError::Write {
                scope: RegistrationScope::Local,
                ..
            }
        ));
        assert_eq!(fs::read(&local).unwrap(), before);

        let local_only_root = tempfile::tempdir().unwrap();
        let local_only_workspace = local_only_root.path().join("workspace");
        let local_only_path = absolute_path(&runfile_path(&local_only_workspace)).unwrap();
        let local_only_runfile = v2_runfile(&local_only_path, "local-only-token");
        let local_only =
            publish_mirrored(&local_only_workspace, None, &local_only_runfile).unwrap();
        assert!(local_only.global.is_none());
        assert_eq!(fs::read(local_only_path).unwrap(), local_only.local.raw);
    }

    #[test]
    fn failed_second_publish_returns_exact_first_capture_for_ordered_rollback() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let global = root.path().join("config").join("server.json");
        fs::create_dir_all(global.parent().unwrap()).unwrap();
        fs::write(&global, b"existing-global").unwrap();
        let runfile = v2_runfile(&local, "rollback-token");

        let error = publish_mirrored(&workspace, Some(&global), &runfile).unwrap_err();
        let rollback_capture = match error {
            PublishError::Mirror { local, .. } => local,
            other => panic!("expected deferred mirrored-publish rollback, got {other:?}"),
        };
        assert_eq!(rollback_capture.path, local);
        assert_eq!(rollback_capture.raw, fs::read(&local).unwrap());
        assert_eq!(
            remove_if_unchanged(&rollback_capture).unwrap(),
            RemovalOutcome::Removed
        );
        assert!(!local.exists());
        assert_eq!(fs::read(global).unwrap(), b"existing-global");
    }

    #[test]
    fn remove_if_unchanged_deletes_only_exact_capture() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();

        assert_eq!(
            remove_if_unchanged(&capture).unwrap(),
            RemovalOutcome::Removed
        );
        assert!(!path.exists());
        assert_eq!(
            remove_if_unchanged(&capture).unwrap(),
            RemovalOutcome::Absent
        );
    }

    #[test]
    fn replace_if_unchanged_commits_only_over_exact_capture() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = b"replacement-v2";

        assert_eq!(
            replace_if_unchanged(&capture, replacement).unwrap(),
            ReplacementOutcome::Replaced
        );
        assert_eq!(fs::read(&path).unwrap(), replacement);
    }

    #[test]
    fn conditional_replace_restores_and_preserves_changed_entry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let changed = write_runfile(&path, &legacy_runfile(2));

        let preserved = match replace_if_unchanged(&capture, b"replacement-v2").unwrap() {
            ReplacementOutcome::ReplacementPreserved { path, .. } => path,
            outcome => panic!("changed bytes must be preserved, got {outcome:?}"),
        };
        assert_eq!(fs::read(&path).unwrap(), changed);
        assert_eq!(fs::read(preserved).unwrap(), changed);
    }

    #[test]
    fn conditional_replace_never_clobbers_entry_created_after_isolation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let concurrent = b"concurrent-registration".to_vec();

        let error = replace_if_unchanged_impl(
            &capture,
            b"replacement-v2",
            |original| fs::write(original, &concurrent).unwrap(),
            persist_bytes_noclobber,
            persist_bytes_noclobber,
        )
        .unwrap_err();
        assert!(!error.replacement_committed);
        assert_eq!(fs::read(&path).unwrap(), concurrent);
        assert_eq!(
            fs::read(error.preserved_at.expect("isolated capture retained")).unwrap(),
            capture.raw
        );
    }

    #[test]
    fn conditional_replace_publish_failure_restores_original_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();

        let error = replace_if_unchanged_impl(
            &capture,
            b"replacement-v2",
            |_| {},
            |_, _| {
                Err(PersistFailure {
                    kind: io::ErrorKind::PermissionDenied,
                    detail: "injected replacement failure".to_string(),
                    committed: false,
                })
            },
            persist_bytes_noclobber,
        )
        .unwrap_err();
        assert!(!error.replacement_committed);
        assert_eq!(fs::read(&path).unwrap(), capture.raw);
        assert_eq!(
            fs::read(error.preserved_at.expect("isolated capture retained")).unwrap(),
            capture.raw
        );
    }

    #[test]
    fn changed_entry_is_restored_or_retained_with_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = write_runfile(&path, &legacy_runfile(2));

        let preserved = match remove_if_unchanged(&capture).unwrap() {
            RemovalOutcome::ReplacementPreserved { path, .. } => path,
            outcome => panic!("changed bytes must be preserved, got {outcome:?}"),
        };
        assert_eq!(fs::read(&path).unwrap(), replacement);
        assert_eq!(fs::read(preserved).unwrap(), replacement);
    }

    #[test]
    fn replacement_created_after_atomic_move_is_never_deleted() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = serde_json::to_vec_pretty(&legacy_runfile(2)).unwrap();

        let outcome = remove_if_unchanged_impl(
            &capture,
            |original| {
                fs::write(original, &replacement).unwrap();
            },
            persist_bytes_noclobber,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            RemovalOutcome::ReplacementPreserved { ref path, .. } if path == &capture.path
        ));
        assert_eq!(fs::read(&path).unwrap(), replacement);
    }

    #[test]
    fn changed_entry_restore_io_failure_is_error_with_preserved_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = write_runfile(&path, &legacy_runfile(2));

        let error = remove_if_unchanged_impl(
            &capture,
            |_| {},
            |_, _| {
                Err(PersistFailure {
                    kind: io::ErrorKind::PermissionDenied,
                    detail: "injected restore failure".to_string(),
                    committed: false,
                })
            },
        )
        .unwrap_err();
        assert!(error.detail.contains("could not restore changed entry"));
        let preserved = error.preserved_at.expect("moved bytes must be retained");
        assert_eq!(fs::read(preserved).unwrap(), replacement);
        assert!(!path.exists());
    }
}
