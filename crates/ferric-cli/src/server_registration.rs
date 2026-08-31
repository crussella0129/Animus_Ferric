//! Lossless server-registration storage.
//!
//! The lifecycle layer needs to distinguish an absent registration from one
//! which exists but cannot safely authorize an action.  This module therefore
//! captures each configured slot independently, preserves the exact bytes used
//! for later compare-and-remove, and never silently falls through a blocked or
//! conflicting slot.

use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
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

/// One typed registration coordinate. Keeping the scope beside the exact
/// absolute path prevents later diagnostics from collapsing an origin into a
/// same-path local capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationCoordinate {
    pub(crate) scope: RegistrationScope,
    pub(crate) path: PathBuf,
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
    /// Every origin promised by a valid captured global v2 registration.
    ///
    /// The origin is captured independently even when it names the configured
    /// local path. `expected_runfile` is the source record against which later
    /// resolution can diagnose a changed-but-valid origin without discarding
    /// either capture's exact bytes.
    pub(crate) promised_origins: Vec<PromisedOriginRegistration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromisedOriginRegistration {
    pub(crate) source: RegistrationCoordinate,
    pub(crate) expected_runfile: ServerRunfile,
    pub(crate) slot: RegistrationSlot,
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
    let promised_origins = global
        .iter()
        .filter_map(|slot| match slot {
            RegistrationSlot::Captured(captured)
                if captured.runfile.schema_version == IDENTITY_SCHEMA_VERSION =>
            {
                let origin = captured
                    .runfile
                    .origin_local_runfile
                    .as_deref()
                    .expect("validated schema-v2 registration has an origin");
                Some(PromisedOriginRegistration {
                    source: RegistrationCoordinate {
                        scope: captured.scope,
                        path: captured.path.clone(),
                    },
                    expected_runfile: captured.runfile.clone(),
                    slot: capture_registration_path(RegistrationScope::Origin, origin),
                })
            }
            _ => None,
        })
        .collect();
    RegistrationInventory {
        local,
        global,
        promised_origins,
    }
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
            if let Some(detail) = schema_envelope_invalidity(&raw) {
                return RegistrationSlot::Blocked {
                    scope,
                    path,
                    reason: RegistrationBlock::InvalidSchema(detail),
                };
            }
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

/// A structural mirror of `ServerRunfile` whose authority coordinates retain
/// their full JSON `u64` range. Deriving `Deserialize` keeps Serde's ordinary
/// missing-field, wrong-type, nested-shape, and duplicate-field rejection; a
/// `Value` intermediary would erase duplicate known fields before we could
/// distinguish them from a pure numeric-range violation.
#[derive(serde::Deserialize)]
struct WideRunfileEnvelope {
    #[serde(default = "legacy_schema_version_u64")]
    schema_version: u64,
    #[serde(rename = "engine")]
    _engine: crate::server::Engine,
    pid: u64,
    port: u64,
    #[serde(rename = "base_url")]
    _base_url: String,
    #[serde(default, rename = "tailscale")]
    _tailscale: bool,
    #[serde(default, rename = "tailscale_serve")]
    _tailscale_serve: Option<crate::tailscale_serve::TailscaleServeOwnership>,
    #[serde(default, rename = "model")]
    _model: Option<String>,
    #[serde(default, rename = "context_size")]
    _context_size: Option<u32>,
    #[serde(default, rename = "sampling_seed")]
    _sampling_seed: Option<i64>,
    #[serde(default, rename = "parallel_slots")]
    _parallel_slots: Option<u32>,
    #[serde(default, rename = "process_identity")]
    _process_identity: Option<crate::server_process::ProcessIdentity>,
    #[serde(default, rename = "origin_local_runfile")]
    _origin_local_runfile: Option<PathBuf>,
}

const fn legacy_schema_version_u64() -> u64 {
    LEGACY_SCHEMA_VERSION as u64
}

/// Recover schema-level numeric range violations from an otherwise
/// structurally valid `ServerRunfile` envelope.
///
/// Serde cannot construct `ServerRunfile` when a JSON coordinate exceeds its
/// Rust storage type, but that is an invalid claimed schema coordinate rather
/// than malformed JSON. Keep all other deserialization errors classified as
/// `Malformed` and promote only the explicit version/authority overflows.
fn schema_envelope_invalidity(raw: &[u8]) -> Option<String> {
    let envelope = serde_json::from_slice::<WideRunfileEnvelope>(raw).ok()?;
    let version = envelope.schema_version;
    if version > u64::from(u8::MAX) {
        return Some(format!("unsupported schema version {version}"));
    }
    if version != u64::from(IDENTITY_SCHEMA_VERSION) {
        return None;
    }
    for (field, coordinate, maximum) in [
        ("pid", envelope.pid, u64::from(u32::MAX)),
        ("port", envelope.port, u64::from(u16::MAX)),
    ] {
        if coordinate > maximum {
            return Some(format!(
                "schema 2 {field} coordinate {coordinate} exceeds maximum {maximum}"
            ));
        }
    }
    None
}

pub(crate) fn validate_runfile(
    scope: RegistrationScope,
    capture_path: &Path,
    runfile: &ServerRunfile,
) -> Result<(), String> {
    match runfile.schema_version {
        LEGACY_SCHEMA_VERSION => {
            if runfile.process_identity.is_some()
                || runfile.origin_local_runfile.is_some()
                || runfile.tailscale_serve.is_some()
            {
                return Err(
                    "schema 1 must not carry process_identity, origin_local_runfile, or tailscale_serve"
                        .to_string(),
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
            let expected_base_url = format!("http://127.0.0.1:{}/v1", runfile.port);
            if runfile.base_url != expected_base_url {
                return Err(format!(
                    "schema 2 base_url must remain the local endpoint {expected_base_url}"
                ));
            }
            if !runfile.tailscale && runfile.tailscale_serve.is_some() {
                return Err(
                    "schema 2 non-Tailscale registration must not carry tailscale_serve ownership"
                        .to_string(),
                );
            }
            if let Some(ownership) = &runfile.tailscale_serve {
                ownership
                    .validate_for_port(runfile.port)
                    .map_err(|error| format!("schema 2 tailscale_serve is invalid: {error}"))?;
            }
            let identity = runfile
                .process_identity
                .as_ref()
                .ok_or_else(|| "schema 2 requires process_identity".to_string())?;
            crate::server_process::validate_start_token(&identity.start_token).map_err(
                |detail| format!("schema 2 process_identity has invalid start_token: {detail}"),
            )?;
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

/// One persistence boundary reached by a publication attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistencePhase {
    CreateStage,
    WriteAll,
    Flush,
    FileSync,
    PersistNoClobber,
    StageCleanup,
    ParentSync,
}

/// An exclusive same-parent stage retained after a failed publication.
/// Missing raw bytes or file identity means the stage path is known but
/// automated cleanup is not authorized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicationStage {
    pub(crate) scope: RegistrationScope,
    pub(crate) final_path: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) raw: Option<Vec<u8>>,
    pub(crate) identity: Option<PublicationStageIdentity>,
}

/// Stable identity of the open stage file. On Unix this is `(device, inode)`;
/// on Windows it is `(volume serial number, file index)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicationStageIdentity {
    first: u64,
    second: u64,
}

#[cfg(unix)]
fn publication_stage_identity(file: &fs::File) -> io::Result<PublicationStageIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    Ok(PublicationStageIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}

#[cfg(windows)]
fn publication_stage_identity(file: &fs::File) -> io::Result<PublicationStageIdentity> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct FileTime {
        _low_date_time: u32,
        _high_date_time: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        _file_attributes: u32,
        _creation_time: FileTime,
        _last_access_time: FileTime,
        _last_write_time: FileTime,
        volume_serial_number: u32,
        _file_size_high: u32,
        _file_size_low: u32,
        _number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetFileInformationByHandle"]
        fn get_file_information_by_handle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    // SAFETY: the output structure is plain data and the borrowed file handle
    // remains live for the duration of the call.
    let mut information: ByHandleFileInformation = unsafe { std::mem::zeroed() };
    let ok = unsafe { get_file_information_by_handle(file.as_raw_handle(), &mut information) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PublicationStageIdentity {
        first: u64::from(information.volume_serial_number),
        second: u64::from(information.file_index_high) << 32
            | u64::from(information.file_index_low),
    })
}

#[cfg(not(any(unix, windows)))]
fn publication_stage_identity(_file: &fs::File) -> io::Result<PublicationStageIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stable file identity is unavailable on this platform",
    ))
}

/// Exact recovery state produced by a failed publication attempt. The launch
/// coordinator owns this value until retained-child exit and listener release
/// are proven; only then may it conditionally remove these finals and stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublicationAttempt {
    pub(crate) finals: Vec<CapturedRegistration>,
    pub(crate) stages: Vec<PublicationStage>,
    pub(crate) terminal_phase: PersistencePhase,
    pub(crate) final_committed: bool,
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
        attempt: Box<PublicationAttempt>,
    },
    Mirror {
        path: PathBuf,
        detail: String,
        /// Exact local capture which the process-owning caller must pass to
        /// `remove_if_unchanged` only after stopping and waiting for its child.
        local: Box<CapturedRegistration>,
        attempt: Box<PublicationAttempt>,
    },
    /// A final path was committed, but syncing its parent directory failed.
    /// The process-owning caller must stop/wait its child before conditionally
    /// removing every exact capture in `published`.
    Durability {
        path: PathBuf,
        detail: String,
        published: Box<PublishedRegistrations>,
        attempt: Box<PublicationAttempt>,
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
                ..
            } => write!(
                formatter,
                "publish {scope} registration at {}: {detail}",
                path.display()
            ),
            Self::Mirror {
                path,
                detail,
                local,
                ..
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
                ..
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
    publish_mirrored_with(
        workspace,
        global_path,
        runfile,
        &mut NativePersistenceEffects,
    )
}

/// Persistence boundary used by publication fault matrices. The production
/// implementation below still performs every filesystem operation; scripted
/// implementations can fail one exact phase without replacing the atomic
/// no-clobber algorithm with an in-memory imitation.
pub(crate) trait PersistenceEffects {
    fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>>;

    fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile>;

    fn write_all(
        &mut self,
        final_path: &Path,
        stage: &mut NamedTempFile,
        raw: &[u8],
    ) -> io::Result<()>;

    fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()>;

    fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()>;

    fn persist_noclobber(
        &mut self,
        final_path: &Path,
        stage: NamedTempFile,
    ) -> Result<(), StagePersistError>;

    fn sync_parent(&mut self, final_path: &Path, parent: &Path) -> io::Result<()>;
}

#[derive(Debug)]
pub(crate) struct StagePersistError {
    pub(crate) error: io::Error,
    pub(crate) stage: NamedTempFile,
}

struct NativePersistenceEffects;

impl PersistenceEffects for NativePersistenceEffects {
    fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec_pretty(runfile)
    }

    fn create_stage(&mut self, _final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
        Builder::new()
            .prefix(".server-registration-")
            .tempfile_in(parent)
    }

    fn write_all(
        &mut self,
        _final_path: &Path,
        stage: &mut NamedTempFile,
        raw: &[u8],
    ) -> io::Result<()> {
        stage.write_all(raw)
    }

    fn flush(&mut self, _final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
        stage.as_file_mut().flush()
    }

    fn sync_file(&mut self, _final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
        stage.as_file().sync_all()
    }

    fn persist_noclobber(
        &mut self,
        final_path: &Path,
        stage: NamedTempFile,
    ) -> Result<(), StagePersistError> {
        stage
            .persist_noclobber(final_path)
            .map(drop)
            .map_err(|error| StagePersistError {
                error: error.error,
                stage: error.file,
            })
    }

    fn sync_parent(&mut self, _final_path: &Path, parent: &Path) -> io::Result<()> {
        sync_parent_directory(parent)
    }
}

/// Serialize once and publish through an injectable phase boundary. This is
/// crate-visible so launch compensation tests can use the real publication
/// state machine with deterministic filesystem faults.
pub(crate) fn publish_mirrored_with<E: PersistenceEffects>(
    workspace: &Path,
    global_path: Option<&Path>,
    runfile: &ServerRunfile,
    effects: &mut E,
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
    let raw = effects
        .serialize(runfile)
        .map_err(|error| PublishError::Serialize(error.to_string()))?;
    if let Err(error) = persist_bytes_noclobber_with(&local_path, &raw, effects, true) {
        let detail = error.to_string();
        if !error.committed {
            let attempt = error.into_attempt(RegistrationScope::Local, Vec::new());
            return Err(PublishError::Write {
                scope: RegistrationScope::Local,
                path: local_path,
                detail,
                attempt: Box::new(attempt),
            });
        }
        let local = CapturedRegistration {
            scope: RegistrationScope::Local,
            path: local_path.clone(),
            raw,
            runfile: runfile.clone(),
        };
        let attempt = error.into_attempt(RegistrationScope::Local, vec![local.clone()]);
        return Err(PublishError::Durability {
            path: local_path,
            detail,
            published: Box::new(PublishedRegistrations {
                local,
                global: None,
            }),
            attempt: Box::new(attempt),
        });
    }
    let local = CapturedRegistration {
        scope: RegistrationScope::Local,
        path: local_path,
        raw: raw.clone(),
        runfile: runfile.clone(),
    };

    let global = if let Some(global_path) = global_path {
        if let Err(error) = persist_bytes_noclobber_with(&global_path, &raw, effects, true) {
            let detail = error.to_string();
            if error.committed {
                let global = CapturedRegistration {
                    scope: RegistrationScope::Global,
                    path: global_path.clone(),
                    raw,
                    runfile: runfile.clone(),
                };
                let attempt = error.into_attempt(
                    RegistrationScope::Global,
                    vec![local.clone(), global.clone()],
                );
                return Err(PublishError::Durability {
                    path: global_path,
                    detail,
                    published: Box::new(PublishedRegistrations {
                        local,
                        global: Some(global),
                    }),
                    attempt: Box::new(attempt),
                });
            }
            let attempt = error.into_attempt(RegistrationScope::Global, vec![local.clone()]);
            return Err(PublishError::Mirror {
                path: global_path,
                detail,
                local: Box::new(local),
                attempt: Box::new(attempt),
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
    phase: PersistencePhase,
    stage: Option<Box<RetainedStage>>,
}

#[derive(Debug)]
struct RetainedStage {
    final_path: PathBuf,
    path: PathBuf,
    raw: Option<Vec<u8>>,
    identity: Option<PublicationStageIdentity>,
}

impl PersistFailure {
    fn into_attempt(
        self,
        scope: RegistrationScope,
        finals: Vec<CapturedRegistration>,
    ) -> PublicationAttempt {
        let stages = self
            .stage
            .into_iter()
            .map(|stage| {
                let stage = *stage;
                PublicationStage {
                    scope,
                    final_path: stage.final_path,
                    path: stage.path,
                    raw: stage.raw,
                    identity: stage.identity,
                }
            })
            .collect();
        PublicationAttempt {
            finals,
            stages,
            terminal_phase: self.phase,
            final_committed: self.committed,
        }
    }
}

impl fmt::Display for PersistFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

fn persist_failure(
    phase: PersistencePhase,
    context: impl fmt::Display,
    error: io::Error,
) -> PersistFailure {
    PersistFailure {
        kind: error.kind(),
        detail: format!("{context}: {error}"),
        committed: false,
        phase,
        stage: None,
    }
}

fn persist_bytes_noclobber(path: &Path, raw: &[u8]) -> Result<(), PersistFailure> {
    persist_bytes_noclobber_with(path, raw, &mut NativePersistenceEffects, false)
}

struct OpenStageSnapshot {
    raw: Option<Vec<u8>>,
    identity: Option<PublicationStageIdentity>,
    diagnostics: Vec<String>,
}

fn snapshot_open_stage(stage: &NamedTempFile) -> OpenStageSnapshot {
    let mut diagnostics = Vec::new();
    let raw = (|| {
        let mut open_stage = stage.as_file().try_clone()?;
        open_stage.seek(SeekFrom::Start(0))?;
        let mut raw = Vec::new();
        open_stage.read_to_end(&mut raw)?;
        Ok::<_, io::Error>(raw)
    })()
    .map(Some)
    .unwrap_or_else(|error| {
        diagnostics.push(format!("capture bytes through open stage handle: {error}"));
        None
    });
    let identity = match publication_stage_identity(stage.as_file()) {
        Ok(identity) => Some(identity),
        Err(error) => {
            diagnostics.push(format!(
                "capture identity through open stage handle: {error}"
            ));
            None
        }
    };
    OpenStageSnapshot {
        raw,
        identity,
        diagnostics,
    }
}

fn finish_uncommitted_stage(
    final_path: &Path,
    stage: NamedTempFile,
    mut failure: PersistFailure,
    retain_for_coordinator: bool,
) -> PersistFailure {
    if retain_for_coordinator {
        return retain_uncommitted_stage(final_path, stage, failure);
    }

    let stage_path = stage.path().to_path_buf();
    let snapshot = snapshot_open_stage(&stage);
    if let Err(error) = stage.close() {
        for diagnostic in snapshot.diagnostics {
            failure.detail.push_str(&format!("; {diagnostic}"));
        }
        failure.detail.push_str(&format!(
            "; cleanup temporary stage {}: {error}; stage preserved for recovery",
            stage_path.display()
        ));
        failure.stage = Some(Box::new(RetainedStage {
            final_path: final_path.to_path_buf(),
            path: stage_path,
            raw: snapshot.raw,
            identity: snapshot.identity,
        }));
    }
    failure
}

fn retain_uncommitted_stage(
    final_path: &Path,
    mut stage: NamedTempFile,
    mut failure: PersistFailure,
) -> PersistFailure {
    let stage_path = stage.path().to_path_buf();
    let snapshot = snapshot_open_stage(&stage);
    // Publication may fail while its retained child is still live. Keep the
    // exclusive stage intact; the coordinator may remove it only after proving
    // that exact generation exited and released its listener.
    stage.disable_cleanup(true);
    drop(stage);
    for diagnostic in snapshot.diagnostics {
        failure.detail.push_str(&format!("; {diagnostic}"));
    }
    failure.detail.push_str(&format!(
        "; stage retained for post-exit cleanup at {}",
        stage_path.display()
    ));
    failure.stage = Some(Box::new(RetainedStage {
        final_path: final_path.to_path_buf(),
        path: stage_path,
        raw: snapshot.raw,
        identity: snapshot.identity,
    }));
    failure
}

fn retained_committed_stage(
    final_path: &Path,
    stage_path: &Path,
    raw: &[u8],
    identity: Option<PublicationStageIdentity>,
    mut detail: String,
) -> PersistFailure {
    if identity.is_none() {
        detail.push_str("; original open stage identity was unavailable");
    }
    PersistFailure {
        kind: io::ErrorKind::Other,
        detail,
        committed: true,
        phase: PersistencePhase::StageCleanup,
        stage: Some(Box::new(RetainedStage {
            final_path: final_path.to_path_buf(),
            path: stage_path.to_path_buf(),
            raw: Some(raw.to_vec()),
            identity,
        })),
    }
}

fn persist_bytes_noclobber_with<E: PersistenceEffects>(
    path: &Path,
    raw: &[u8],
    effects: &mut E,
    retain_failed_stages: bool,
) -> Result<(), PersistFailure> {
    let parent = path.parent().ok_or_else(|| PersistFailure {
        kind: io::ErrorKind::InvalidInput,
        detail: format!("path {} has no parent", path.display()),
        committed: false,
        phase: PersistencePhase::CreateStage,
        stage: None,
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        persist_failure(
            PersistencePhase::CreateStage,
            format_args!("create parent {}", parent.display()),
            error,
        )
    })?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|error| {
        persist_failure(
            PersistencePhase::CreateStage,
            format_args!("inspect parent {}", parent.display()),
            error,
        )
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(PersistFailure {
            kind: io::ErrorKind::InvalidInput,
            detail: format!("parent {} is not a regular directory", parent.display()),
            committed: false,
            phase: PersistencePhase::CreateStage,
            stage: None,
        });
    }

    let mut temporary = effects.create_stage(path, parent).map_err(|error| {
        persist_failure(
            PersistencePhase::CreateStage,
            format_args!("create temporary file in {}", parent.display()),
            error,
        )
    })?;
    let stage_path = temporary.path().to_path_buf();
    let stage_parent = stage_path.parent();
    if stage_parent.is_none_or(|stage_parent| !paths_match(stage_parent, parent)) {
        let failure = PersistFailure {
            kind: io::ErrorKind::InvalidInput,
            detail: format!(
                "temporary registration stage {} is not in destination parent {}",
                stage_path.display(),
                parent.display()
            ),
            committed: false,
            phase: PersistencePhase::CreateStage,
            stage: None,
        };
        return Err(finish_uncommitted_stage(
            path,
            temporary,
            failure,
            retain_failed_stages,
        ));
    }

    if let Err(error) = effects.write_all(path, &mut temporary, raw) {
        let failure = persist_failure(
            PersistencePhase::WriteAll,
            "write temporary registration",
            error,
        );
        return Err(finish_uncommitted_stage(
            path,
            temporary,
            failure,
            retain_failed_stages,
        ));
    }
    if let Err(error) = effects.flush(path, &mut temporary) {
        let failure = persist_failure(
            PersistencePhase::Flush,
            "flush temporary registration",
            error,
        );
        return Err(finish_uncommitted_stage(
            path,
            temporary,
            failure,
            retain_failed_stages,
        ));
    }
    if let Err(error) = effects.sync_file(path, &temporary) {
        let failure = persist_failure(
            PersistencePhase::FileSync,
            "sync temporary registration",
            error,
        );
        return Err(finish_uncommitted_stage(
            path,
            temporary,
            failure,
            retain_failed_stages,
        ));
    }
    let committed_stage_identity = publication_stage_identity(temporary.as_file()).ok();
    if let Err(error) = effects.persist_noclobber(path, temporary) {
        let failure = persist_failure(
            PersistencePhase::PersistNoClobber,
            "persist without replacing an existing path",
            error.error,
        );
        return Err(finish_uncommitted_stage(
            path,
            error.stage,
            failure,
            retain_failed_stages,
        ));
    }
    // `tempfile::persist_noclobber` can leave its original hard link behind
    // on an interrupted/unlink-denied path. Never unlink a path after losing
    // the stage handle: retain and report it for post-exit exact-byte cleanup.
    match fs::symlink_metadata(&stage_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => {
            return Err(retained_committed_stage(
                path,
                &stage_path,
                raw,
                committed_stage_identity.clone(),
                format!(
                    "publication committed at {} but stage {} remained; stage retained for post-exit cleanup",
                    path.display(),
                    stage_path.display()
                ),
            ));
        }
        Err(error) => {
            return Err(retained_committed_stage(
                path,
                &stage_path,
                raw,
                committed_stage_identity,
                format!(
                    "publication committed at {} but stage cleanup could not be verified: {error}",
                    path.display()
                ),
            ));
        }
    }
    effects
        .sync_parent(path, parent)
        .map_err(|error| PersistFailure {
            kind: error.kind(),
            detail: format!("sync parent directory {}: {error}", parent.display()),
            committed: true,
            phase: PersistencePhase::ParentSync,
            stage: None,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemovalFailureKind {
    Restore,
    Remove,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemovalError {
    pub(crate) path: PathBuf,
    pub(crate) kind: RemovalFailureKind,
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
    remove_if_unchanged_impl(
        captured,
        |_| {},
        |path| fs::read(path),
        |path| fs::remove_file(path),
        persist_bytes_noclobber,
    )
}

/// Conditionally remove one retained publication stage after the launch
/// coordinator has proved retained-child exit and listener release. A stage
/// whose exact bytes or stable open-handle identity could not be captured is
/// recovery evidence only and is never deleted automatically.
pub(crate) fn remove_publication_stage_if_unchanged(
    stage: &PublicationStage,
) -> Result<RemovalOutcome, RemovalError> {
    let (raw, identity) = match (stage.raw.as_deref(), stage.identity.as_ref()) {
        (Some(raw), Some(identity)) => (raw, identity),
        _ => {
            let missing = match (stage.raw.is_none(), stage.identity.is_none()) {
                (true, true) => "bytes and file identity were not captured",
                (true, false) => "bytes were not captured",
                (false, true) => "file identity was not captured",
                (false, false) => unreachable!(),
            };
            return match fs::symlink_metadata(&stage.path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(RemovalOutcome::Absent),
                Ok(_) => Err(RemovalError {
                    path: stage.path.clone(),
                    kind: RemovalFailureKind::Other,
                    detail: format!(
                        "retained publication stage {missing}; automatic cleanup is unauthorized"
                    ),
                    preserved_at: Some(stage.path.clone()),
                }),
                Err(error) => Err(RemovalError {
                    path: stage.path.clone(),
                    kind: RemovalFailureKind::Other,
                    detail: format!(
                        "retained publication stage {missing}, and its path is uninspectable: {error}"
                    ),
                    preserved_at: Some(stage.path.clone()),
                }),
            };
        }
    };
    let stage_parent = stage.path.parent();
    let final_parent = stage.final_path.parent();
    if stage_parent.is_none()
        || final_parent.is_none()
        || !paths_match(stage_parent.unwrap(), final_parent.unwrap())
    {
        return Err(RemovalError {
            path: stage.path.clone(),
            kind: RemovalFailureKind::Other,
            detail: format!(
                "retained publication stage is not in final-path parent {}",
                stage.final_path.display()
            ),
            preserved_at: Some(stage.path.clone()),
        });
    }
    remove_exact_bytes_if_unchanged_impl(
        &stage.path,
        raw,
        Some(identity),
        |_| {},
        |path| fs::read(path),
        |path| fs::remove_file(path),
        persist_bytes_noclobber,
    )
}

fn remove_if_unchanged_impl<F, Q, D, R>(
    captured: &CapturedRegistration,
    after_rename: F,
    read_moved: Q,
    remove_moved: D,
    restore_changed: R,
) -> Result<RemovalOutcome, RemovalError>
where
    F: FnOnce(&Path),
    Q: FnOnce(&Path) -> io::Result<Vec<u8>>,
    D: FnOnce(&Path) -> io::Result<()>,
    R: FnOnce(&Path, &[u8]) -> Result<(), PersistFailure>,
{
    remove_exact_bytes_if_unchanged_impl(
        &captured.path,
        &captured.raw,
        None,
        after_rename,
        read_moved,
        remove_moved,
        restore_changed,
    )
}

fn remove_exact_bytes_if_unchanged_impl<F, Q, D, R>(
    original: &Path,
    captured_raw: &[u8],
    expected_identity: Option<&PublicationStageIdentity>,
    after_rename: F,
    read_moved: Q,
    remove_moved: D,
    restore_changed: R,
) -> Result<RemovalOutcome, RemovalError>
where
    F: FnOnce(&Path),
    Q: FnOnce(&Path) -> io::Result<Vec<u8>>,
    D: FnOnce(&Path) -> io::Result<()>,
    R: FnOnce(&Path, &[u8]) -> Result<(), PersistFailure>,
{
    let original_path = original.to_path_buf();
    let original = &original_path;
    let metadata = match fs::symlink_metadata(original) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RemovalOutcome::Absent);
        }
        Err(error) => {
            return Err(RemovalError {
                path: original.clone(),
                kind: RemovalFailureKind::Other,
                detail: format!("inspect current entry: {error}"),
                preserved_at: None,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(RemovalError {
            path: original.clone(),
            kind: RemovalFailureKind::Other,
            detail: "current entry is not a regular non-symlink file".to_string(),
            preserved_at: None,
        });
    }

    let parent = original.parent().ok_or_else(|| RemovalError {
        path: original.clone(),
        kind: RemovalFailureKind::Other,
        detail: "registration path has no parent".to_string(),
        preserved_at: None,
    })?;
    let holding_dir = Builder::new()
        .prefix(".server-registration-remove-")
        .tempdir_in(parent)
        .map_err(|error| RemovalError {
            path: original.clone(),
            kind: RemovalFailureKind::Other,
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
                kind: RemovalFailureKind::Other,
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
                kind: RemovalFailureKind::Other,
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
    let identity_matches = if let Some(expected_identity) = expected_identity {
        let moved_identity =
            match fs::File::open(&moved).and_then(|file| publication_stage_identity(&file)) {
                Ok(identity) => identity,
                Err(error) => {
                    let preserved = keep_holding_dir(holding_dir, "registration");
                    return Err(RemovalError {
                        path: original.clone(),
                        kind: RemovalFailureKind::Other,
                        detail: format!("inspect atomically moved file identity: {error}"),
                        preserved_at: Some(preserved),
                    });
                }
            };
        &moved_identity == expected_identity
    } else {
        true
    };
    let moved_raw = match read_moved(&moved) {
        Ok(raw) => raw,
        Err(error) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(RemovalError {
                path: original.clone(),
                kind: RemovalFailureKind::Other,
                detail: format!("read atomically moved entry: {error}"),
                preserved_at: Some(preserved),
            });
        }
    };

    if moved_raw == captured_raw && identity_matches {
        if let Err(error) = remove_moved(&moved) {
            let preserved = keep_holding_dir(holding_dir, "registration");
            return Err(RemovalError {
                path: original.clone(),
                kind: RemovalFailureKind::Remove,
                detail: format!("remove unchanged moved entry: {error}"),
                preserved_at: Some(preserved),
            });
        }
        holding_dir.close().map_err(|error| RemovalError {
            path: original.clone(),
            kind: RemovalFailureKind::Other,
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
                kind: RemovalFailureKind::Other,
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
    let replacement_reason = if !identity_matches {
        "entry at the retained stage path has a different file identity"
    } else {
        "entry at the retained stage path has different bytes"
    };
    match restore_changed(original, &moved_raw) {
        Ok(()) => {
            let preserved = keep_holding_dir(holding_dir, "registration");
            Ok(RemovalOutcome::ReplacementPreserved {
                path: preserved,
                detail: format!(
                    "{replacement_reason}; changed entry was restored without clobbering and retained in the holding directory"
                ),
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
                    kind: RemovalFailureKind::Restore,
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
    use crate::server_process::{
        ListenerState, ProcessError, ProcessFacts, ProcessIdentity, ProcessRuntime, RetainedProcess,
    };

    fn legacy_runfile(pid: u32) -> ServerRunfile {
        ServerRunfile {
            schema_version: LEGACY_SCHEMA_VERSION,
            engine: Engine::LlamaServer,
            pid,
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
        }
    }

    fn canonical_start_token(discriminator: u64) -> String {
        #[cfg(windows)]
        {
            format!(
                "windows-filetime:{}",
                133_999_123_456_789_000 + discriminator
            )
        }
        #[cfg(target_os = "linux")]
        {
            format!(
                "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:{}",
                987_000 + discriminator
            )
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            panic!("schema-v2 registration authority is supported only on Windows and Linux");
        }
    }

    fn identity(discriminator: u64) -> ProcessIdentity {
        ProcessIdentity {
            start_token: canonical_start_token(discriminator),
            executable: absolute_path(Path::new("llama-server")).unwrap(),
            argv: vec![
                "llama-server".to_string(),
                "--port".to_string(),
                "8080".to_string(),
            ],
        }
    }

    fn v2_runfile(origin: &Path, discriminator: u64) -> ServerRunfile {
        let mut runfile = legacy_runfile(1234);
        runfile.schema_version = IDENTITY_SCHEMA_VERSION;
        runfile.process_identity = Some(identity(discriminator));
        runfile.origin_local_runfile = Some(origin.to_path_buf());
        runfile
    }

    fn tailscale_ownership() -> crate::tailscale_serve::TailscaleServeOwnership {
        let token = "00112233445566778899aabbccddeeff";
        let fqdn = "example-host.tailnet-example.ts.net";
        crate::tailscale_serve::TailscaleServeOwnership {
            version: crate::tailscale_serve::OWNERSHIP_VERSION,
            token: token.to_string(),
            fqdn: fqdn.to_string(),
            https_port: crate::tailscale_serve::HTTPS_PORT,
            mount_path: format!("/_ferric/{token}"),
            proxy_target: "http://127.0.0.1:8080".to_string(),
            remote_base_url: format!("https://{fqdn}/_ferric/{token}/v1"),
            before_status_sha256: "a".repeat(64),
        }
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

    fn assert_invalid_schema(
        scope: RegistrationScope,
        path: &Path,
        runfile: &ServerRunfile,
        expected: &str,
    ) {
        write_runfile(path, runfile);
        assert!(
            matches!(
                capture_registration_path(scope, path),
                RegistrationSlot::Blocked {
                    reason: RegistrationBlock::InvalidSchema(detail),
                    ..
                } if detail.contains(expected)
            ),
            "{scope} row at {} did not report InvalidSchema containing {expected:?}",
            path.display()
        );
    }

    fn assert_raw_invalid_schema(
        scope: RegistrationScope,
        path: &Path,
        raw: &[u8],
        expected: &str,
    ) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, raw).unwrap();
        assert!(
            matches!(
                capture_registration_path(scope, path),
                RegistrationSlot::Blocked {
                    reason: RegistrationBlock::InvalidSchema(detail),
                    ..
                } if detail.contains(expected)
            ),
            "raw {scope} row at {} did not report InvalidSchema containing {expected:?}",
            path.display()
        );
    }

    fn assert_raw_malformed(scope: RegistrationScope, path: &Path, raw: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, raw).unwrap();
        match capture_registration_path(scope, path) {
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::Malformed(_),
                ..
            } => {}
            other => panic!(
                "raw {scope} row at {} was {other:?}, expected Malformed",
                path.display()
            ),
        }
    }

    #[cfg(windows)]
    fn invalid_schema_start_tokens() -> &'static [&'static str] {
        &[
            "",
            "token",
            "opaque",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1",
            "windows-filetime:0",
            "windows-filetime:01",
            "windows-filetime:+1",
            "windows-filetime:1extra",
            "windows-filetime:1;trailing",
            "windows-filetime:18446744073709551616",
            " windows-filetime:1",
            "windows-filetime:1 ",
        ]
    }

    #[cfg(target_os = "linux")]
    fn invalid_schema_start_tokens() -> &'static [&'static str] {
        &[
            "",
            "token",
            "opaque",
            "windows-filetime:1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:0",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:01",
            "linux-boot-id:00000000-1111-4222-8333-44444444444;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-44444444444A;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;ticks-start:1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1;extra",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:18446744073709551616",
            " linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1",
            "linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:1 ",
        ]
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct PersistenceEvent {
        phase: PersistencePhase,
        final_path: PathBuf,
        stage_path: Option<PathBuf>,
        byte_len: Option<usize>,
    }

    #[derive(Default)]
    struct ScriptedPersistenceEffects {
        events: Vec<PersistenceEvent>,
        serializations: usize,
        failure: Option<(PathBuf, PersistencePhase)>,
        retain_stage_after_persist: Option<PathBuf>,
    }

    impl ScriptedPersistenceEffects {
        fn failing(final_path: &Path, phase: PersistencePhase) -> Self {
            Self {
                failure: Some((final_path.to_path_buf(), phase)),
                ..Self::default()
            }
        }

        fn retaining_committed_stage(final_path: &Path) -> Self {
            Self {
                retain_stage_after_persist: Some(final_path.to_path_buf()),
                ..Self::default()
            }
        }

        fn fails(&self, final_path: &Path, phase: PersistencePhase) -> bool {
            self.failure.as_ref().is_some_and(|(target, target_phase)| {
                target == final_path && *target_phase == phase
            })
        }

        fn record(
            &mut self,
            phase: PersistencePhase,
            final_path: &Path,
            stage_path: Option<&Path>,
            byte_len: Option<usize>,
        ) {
            self.events.push(PersistenceEvent {
                phase,
                final_path: final_path.to_path_buf(),
                stage_path: stage_path.map(Path::to_path_buf),
                byte_len,
            });
        }

        fn injected(phase: PersistencePhase) -> io::Error {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("injected {phase:?} failure"),
            )
        }
    }

    impl PersistenceEffects for ScriptedPersistenceEffects {
        fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
            self.serializations += 1;
            serde_json::to_vec_pretty(runfile)
        }

        fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
            if self.fails(final_path, PersistencePhase::CreateStage) {
                self.record(PersistencePhase::CreateStage, final_path, None, None);
                return Err(Self::injected(PersistencePhase::CreateStage));
            }
            let stage = NativePersistenceEffects.create_stage(final_path, parent)?;
            self.record(
                PersistencePhase::CreateStage,
                final_path,
                Some(stage.path()),
                None,
            );
            Ok(stage)
        }

        fn write_all(
            &mut self,
            final_path: &Path,
            stage: &mut NamedTempFile,
            raw: &[u8],
        ) -> io::Result<()> {
            self.record(
                PersistencePhase::WriteAll,
                final_path,
                Some(stage.path()),
                Some(raw.len()),
            );
            if self.fails(final_path, PersistencePhase::WriteAll) {
                let short = raw.len().min(7);
                stage.write_all(&raw[..short])?;
                return Err(Self::injected(PersistencePhase::WriteAll));
            }
            NativePersistenceEffects.write_all(final_path, stage, raw)
        }

        fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
            self.record(
                PersistencePhase::Flush,
                final_path,
                Some(stage.path()),
                None,
            );
            if self.fails(final_path, PersistencePhase::Flush) {
                return Err(Self::injected(PersistencePhase::Flush));
            }
            NativePersistenceEffects.flush(final_path, stage)
        }

        fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
            self.record(
                PersistencePhase::FileSync,
                final_path,
                Some(stage.path()),
                None,
            );
            if self.fails(final_path, PersistencePhase::FileSync) {
                return Err(Self::injected(PersistencePhase::FileSync));
            }
            NativePersistenceEffects.sync_file(final_path, stage)
        }

        fn persist_noclobber(
            &mut self,
            final_path: &Path,
            mut stage: NamedTempFile,
        ) -> Result<(), StagePersistError> {
            let stage_path = stage.path().to_path_buf();
            self.record(
                PersistencePhase::PersistNoClobber,
                final_path,
                Some(&stage_path),
                None,
            );
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
                if let Err(error) = fs::hard_link(&stage_path, final_path) {
                    return Err(StagePersistError { error, stage });
                }
                stage.disable_cleanup(true);
                drop(stage);
                return Ok(());
            }
            NativePersistenceEffects.persist_noclobber(final_path, stage)
        }

        fn sync_parent(&mut self, final_path: &Path, parent: &Path) -> io::Result<()> {
            self.record(PersistencePhase::ParentSync, final_path, None, None);
            if self.fails(final_path, PersistencePhase::ParentSync) {
                return Err(Self::injected(PersistencePhase::ParentSync));
            }
            NativePersistenceEffects.sync_parent(final_path, parent)
        }
    }

    const PROCESS_CLIENT_MODE: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_MODE";
    const PROCESS_CLIENT_ROOT: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_ROOT";
    const PROCESS_CLIENT_ID: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_ID";
    const PROCESS_CLIENT_PATH: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_PATH";
    const PROCESS_CLIENT_WORKSPACE: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_WORKSPACE";
    const PROCESS_CLIENT_GLOBAL: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_GLOBAL";
    const PROCESS_CLIENT_DISCRIMINATOR: &str = "FERRIC_REGISTRATION_PROCESS_CLIENT_DISCRIMINATOR";

    #[derive(Debug, serde::Serialize, serde::Deserialize)]
    struct ProcessClientOutcome {
        kind: String,
        preserved_at: Option<PathBuf>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FullAdoptionEvent {
        Acquire(u32),
        Inspect(u32),
        Wait(u32),
        Terminate(u32),
    }

    #[derive(Clone)]
    struct FullAdoptionProcess {
        pid: u32,
        facts: ProcessFacts,
        events: std::sync::Arc<std::sync::Mutex<Vec<FullAdoptionEvent>>>,
    }

    impl RetainedProcess for FullAdoptionProcess {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn inspect(&self, _port: u16) -> Result<ProcessFacts, ProcessError> {
            self.events
                .lock()
                .unwrap()
                .push(FullAdoptionEvent::Inspect(self.pid));
            Ok(self.facts.clone())
        }

        fn terminate(&self) -> Result<bool, ProcessError> {
            self.events
                .lock()
                .unwrap()
                .push(FullAdoptionEvent::Terminate(self.pid));
            Err(ProcessError::Operation(
                "legacy adoption must never signal".to_string(),
            ))
        }

        fn wait(&self, _timeout: std::time::Duration) -> Result<bool, ProcessError> {
            self.events
                .lock()
                .unwrap()
                .push(FullAdoptionEvent::Wait(self.pid));
            Ok(false)
        }
    }

    struct FullAdoptionRuntime {
        process: FullAdoptionProcess,
    }

    impl ProcessRuntime for FullAdoptionRuntime {
        type Process = FullAdoptionProcess;

        fn acquire(&self, pid: u32) -> Result<Self::Process, ProcessError> {
            assert_eq!(pid, self.process.pid);
            self.process
                .events
                .lock()
                .unwrap()
                .push(FullAdoptionEvent::Acquire(pid));
            Ok(self.process.clone())
        }
    }

    fn full_adoption_runtime(
        runfile: &ServerRunfile,
    ) -> (
        FullAdoptionRuntime,
        std::sync::Arc<std::sync::Mutex<Vec<FullAdoptionEvent>>>,
    ) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let facts = ProcessFacts {
            identity: ProcessIdentity {
                start_token: canonical_start_token(501),
                executable: absolute_path(Path::new("llama-server")).unwrap(),
                argv: vec![
                    "llama-server".to_string(),
                    "--host".to_string(),
                    "127.0.0.1".to_string(),
                    "--port".to_string(),
                    runfile.port.to_string(),
                    "--model".to_string(),
                    runfile.model.clone().unwrap(),
                    "--ctx-size".to_string(),
                    runfile.context_size.unwrap().to_string(),
                    "--seed".to_string(),
                    runfile.sampling_seed.unwrap().to_string(),
                    "--parallel".to_string(),
                    runfile.parallel_slots.unwrap().to_string(),
                ],
            },
            listener: ListenerState::OwnedByTarget,
        };
        (
            FullAdoptionRuntime {
                process: FullAdoptionProcess {
                    pid: runfile.pid,
                    facts,
                    events: std::sync::Arc::clone(&events),
                },
            },
            events,
        )
    }

    fn process_client_coordinate(root: &Path, phase: &str, id: &str, state: &str) -> PathBuf {
        root.join(format!("{phase}-{id}.{state}"))
    }

    fn wait_for_process_coordinate(path: &Path, timeout: std::time::Duration) -> io::Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match fs::symlink_metadata(path) {
                Ok(_) => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "timed out waiting for process coordinate {}",
                        path.display()
                    ),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn process_client_barrier(root: &Path, phase: &str, id: &str) -> io::Result<()> {
        fs::create_dir_all(root)?;
        let ready = process_client_coordinate(root, phase, id, "ready");
        let release = process_client_coordinate(root, phase, id, "release");
        fs::write(&ready, format!("pid={}\n", std::process::id()))?;
        wait_for_process_coordinate(&release, std::time::Duration::from_secs(15))
    }

    struct ProcessBarrierPersistenceEffects {
        local: PathBuf,
        global: PathBuf,
        root: PathBuf,
        id: String,
    }

    impl PersistenceEffects for ProcessBarrierPersistenceEffects {
        fn serialize(&mut self, runfile: &ServerRunfile) -> serde_json::Result<Vec<u8>> {
            NativePersistenceEffects.serialize(runfile)
        }

        fn create_stage(&mut self, final_path: &Path, parent: &Path) -> io::Result<NamedTempFile> {
            NativePersistenceEffects.create_stage(final_path, parent)
        }

        fn write_all(
            &mut self,
            final_path: &Path,
            stage: &mut NamedTempFile,
            raw: &[u8],
        ) -> io::Result<()> {
            let phase = if paths_match(final_path, &self.local) {
                "publish-local-stage"
            } else if paths_match(final_path, &self.global) {
                "publish-global-stage"
            } else {
                panic!("unexpected publication path {}", final_path.display());
            };
            process_client_barrier(&self.root, phase, &self.id)?;
            NativePersistenceEffects.write_all(final_path, stage, raw)
        }

        fn flush(&mut self, final_path: &Path, stage: &mut NamedTempFile) -> io::Result<()> {
            NativePersistenceEffects.flush(final_path, stage)
        }

        fn sync_file(&mut self, final_path: &Path, stage: &NamedTempFile) -> io::Result<()> {
            NativePersistenceEffects.sync_file(final_path, stage)
        }

        fn persist_noclobber(
            &mut self,
            final_path: &Path,
            stage: NamedTempFile,
        ) -> Result<(), StagePersistError> {
            NativePersistenceEffects.persist_noclobber(final_path, stage)
        }

        fn sync_parent(&mut self, final_path: &Path, parent: &Path) -> io::Result<()> {
            let phase = if paths_match(final_path, &self.local) {
                "publish-local-committed"
            } else if paths_match(final_path, &self.global) {
                "publish-global-committed"
            } else {
                panic!("unexpected publication path {}", final_path.display());
            };
            process_client_barrier(&self.root, phase, &self.id)?;
            NativePersistenceEffects.sync_parent(final_path, parent)
        }
    }

    struct ProcessClientGuard {
        label: String,
        child: Option<std::process::Child>,
    }

    impl ProcessClientGuard {
        fn spawn(
            label: impl Into<String>,
            mode: &str,
            root: &Path,
            id: &str,
            environment: &[(&str, String)],
        ) -> Self {
            let label = label.into();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "server_registration::tests::lifecycle_interleaving_process_client",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(PROCESS_CLIENT_MODE, mode)
                .env(PROCESS_CLIENT_ROOT, root)
                .env(PROCESS_CLIENT_ID, id)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            for (name, value) in environment {
                command.env(name, value);
            }
            let child = command
                .spawn()
                .unwrap_or_else(|error| panic!("spawn {label}: {error}"));
            Self {
                label,
                child: Some(child),
            }
        }

        fn finish(mut self) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                let status = self
                    .child
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .unwrap_or_else(|error| panic!("poll {} process client: {error}", self.label));
                if status.is_some() {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    let child = self.child.as_mut().unwrap();
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "{} process client exceeded its 20 second watchdog",
                        self.label
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = self.child.take().unwrap().wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{} process client failed ({}):\nstdout:\n{}\nstderr:\n{}",
                self.label,
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for ProcessClientGuard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn process_client_outcome_path(root: &Path, id: &str) -> PathBuf {
        root.join(format!("outcome-{id}.json"))
    }

    fn write_process_client_outcome(root: &Path, id: &str, outcome: ProcessClientOutcome) {
        let raw = serde_json::to_vec_pretty(&outcome).unwrap();
        fs::write(process_client_outcome_path(root, id), raw).unwrap();
    }

    fn read_process_client_outcome(root: &Path, id: &str) -> ProcessClientOutcome {
        serde_json::from_slice(&fs::read(process_client_outcome_path(root, id)).unwrap()).unwrap()
    }

    fn release_process_client(root: &Path, phase: &str, id: &str) {
        fs::write(
            process_client_coordinate(root, phase, id, "release"),
            b"released\n",
        )
        .unwrap();
    }

    fn await_process_client(root: &Path, phase: &str, id: &str) {
        wait_for_process_coordinate(
            &process_client_coordinate(root, phase, id, "ready"),
            std::time::Duration::from_secs(15),
        )
        .unwrap();
    }

    fn await_either_process_client(root: &Path, phase: &str, ids: [&str; 2]) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            for id in ids {
                let ready = process_client_coordinate(root, phase, id, "ready");
                match fs::symlink_metadata(&ready) {
                    Ok(_) => return id.to_string(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => panic!("inspect process coordinate {}: {error}", ready.display()),
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for either process client at phase {phase}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn persistence_phases(
        effects: &ScriptedPersistenceEffects,
        final_path: &Path,
    ) -> Vec<PersistencePhase> {
        effects
            .events
            .iter()
            .filter(|event| event.final_path == final_path)
            .map(|event| event.phase)
            .collect()
    }

    fn publication_stage_paths(root: &Path) -> Vec<PathBuf> {
        fn visit(path: &Path, stages: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    visit(&path, stages);
                } else if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".server-registration-"))
                {
                    stages.push(path);
                }
            }
        }
        let mut stages = Vec::new();
        visit(root, &mut stages);
        stages.sort();
        stages
    }

    fn replace_with_same_bytes_and_new_identity(
        path: &Path,
        raw: &[u8],
    ) -> PublicationStageIdentity {
        let mut replacement = Builder::new()
            .prefix(".same-bytes-replacement-")
            .tempfile_in(path.parent().unwrap())
            .unwrap();
        replacement.write_all(raw).unwrap();
        replacement.as_file_mut().flush().unwrap();
        replacement.as_file().sync_all().unwrap();
        let identity = publication_stage_identity(replacement.as_file())
            .expect("test filesystem exposes stable file identity");
        let replacement_path = replacement.path().to_path_buf();
        replacement.disable_cleanup(true);
        drop(replacement);
        fs::remove_file(path).unwrap();
        fs::rename(replacement_path, path).unwrap();
        assert_eq!(
            publication_stage_identity(&fs::File::open(path).unwrap()).unwrap(),
            identity
        );
        identity
    }

    fn publication_attempt(error: &PublishError) -> &PublicationAttempt {
        match error {
            PublishError::Write { attempt, .. }
            | PublishError::Mirror { attempt, .. }
            | PublishError::Durability { attempt, .. } => attempt,
            other => panic!("expected persistence attempt, got {other:?}"),
        }
    }

    fn process_client_path(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("process client is missing {name}"))
    }

    fn process_client_discriminator() -> u64 {
        std::env::var(PROCESS_CLIENT_DISCRIMINATOR)
            .unwrap_or_else(|_| panic!("process client is missing {PROCESS_CLIENT_DISCRIMINATOR}"))
            .parse()
            .expect("process client discriminator is a u64")
    }

    fn captured_path(path: &Path) -> CapturedRegistration {
        let slot = capture_registration_path(RegistrationScope::Local, path);
        captured(&slot).clone()
    }

    #[test]
    fn lifecycle_interleaving_process_client() {
        let Ok(mode) = std::env::var(PROCESS_CLIENT_MODE) else {
            return;
        };
        let root = process_client_path(PROCESS_CLIENT_ROOT);
        let id = std::env::var(PROCESS_CLIENT_ID)
            .unwrap_or_else(|_| panic!("process client is missing {PROCESS_CLIENT_ID}"));

        match mode.as_str() {
            "publish" => {
                let workspace = process_client_path(PROCESS_CLIENT_WORKSPACE);
                let global = process_client_path(PROCESS_CLIENT_GLOBAL);
                let local = absolute_path(&runfile_path(&workspace)).unwrap();
                let runfile = v2_runfile(&local, process_client_discriminator());
                let mut effects = ProcessBarrierPersistenceEffects {
                    local: local.clone(),
                    global: global.clone(),
                    root: root.clone(),
                    id: id.clone(),
                };
                let (kind, preserved_at) = match publish_mirrored_with(
                    &workspace,
                    Some(&global),
                    &runfile,
                    &mut effects,
                ) {
                    Ok(published) => {
                        assert_eq!(published.local.raw, published.global.unwrap().raw);
                        ("published", None)
                    }
                    Err(error) => {
                        process_client_barrier(&root, "publish-compensation", &id).unwrap();
                        let attempt = publication_attempt(&error);
                        let mut preserved_at = None;
                        for final_registration in &attempt.finals {
                            match remove_if_unchanged(final_registration).unwrap() {
                                RemovalOutcome::Removed | RemovalOutcome::Absent => {}
                                RemovalOutcome::ReplacementPreserved { path, .. } => {
                                    assert!(
                                        preserved_at.replace(path).is_none(),
                                        "only one losing local final can be replacement-preserved"
                                    );
                                }
                            }
                        }
                        for stage in &attempt.stages {
                            assert!(matches!(
                                remove_publication_stage_if_unchanged(stage).unwrap(),
                                RemovalOutcome::Removed | RemovalOutcome::Absent
                            ));
                        }
                        if preserved_at.is_some() {
                            ("compensated-preserved", preserved_at)
                        } else {
                            ("compensated", None)
                        }
                    }
                };
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: kind.to_string(),
                        preserved_at,
                    },
                );
            }
            "publish-replacement" => {
                let path = process_client_path(PROCESS_CLIENT_PATH);
                let raw =
                    serde_json::to_vec_pretty(&v2_runfile(&path, process_client_discriminator()))
                        .unwrap();
                persist_bytes_noclobber(&path, &raw).unwrap();
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: "published-replacement".to_string(),
                        preserved_at: None,
                    },
                );
            }
            "remove" => {
                let path = process_client_path(PROCESS_CLIENT_PATH);
                let capture = captured_path(&path);
                process_client_barrier(&root, "remove-start", &id).unwrap();
                let outcome = remove_if_unchanged(&capture).unwrap();
                let (kind, preserved_at) = match outcome {
                    RemovalOutcome::Removed => ("removed", None),
                    RemovalOutcome::Absent => ("absent", None),
                    RemovalOutcome::ReplacementPreserved { path, .. } => {
                        ("replacement-preserved", Some(path))
                    }
                };
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: kind.to_string(),
                        preserved_at,
                    },
                );
            }
            "remove-after-isolation" => {
                let path = process_client_path(PROCESS_CLIENT_PATH);
                let capture = captured_path(&path);
                let outcome = remove_if_unchanged_impl(
                    &capture,
                    |_| process_client_barrier(&root, "remove-isolated", &id).unwrap(),
                    |path| fs::read(path),
                    |path| fs::remove_file(path),
                    persist_bytes_noclobber,
                )
                .unwrap();
                let (kind, preserved_at) = match outcome {
                    RemovalOutcome::Removed => ("removed", None),
                    RemovalOutcome::Absent => ("absent", None),
                    RemovalOutcome::ReplacementPreserved { path, .. } => {
                        ("replacement-preserved", Some(path))
                    }
                };
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: kind.to_string(),
                        preserved_at,
                    },
                );
            }
            "adopt-full" => {
                let path = process_client_path(PROCESS_CLIENT_PATH);
                let capture = captured_path(&path);
                let pid = capture.runfile.pid;
                let (runtime, events) = full_adoption_runtime(&capture.runfile);
                process_client_barrier(&root, "adopt-full-start", &id).unwrap();
                let summary =
                    crate::server::execute_legacy_adoption_for_test(vec![capture], pid, &runtime);
                assert!(summary.identity_validated);
                assert!(summary.listener_validated);
                let events = events.lock().unwrap().clone();
                assert!(
                    events
                        .iter()
                        .all(|event| !matches!(event, FullAdoptionEvent::Terminate(_))),
                    "legacy adoption signalled unexpectedly: {events:?}"
                );
                let (kind, preserved_at) = if summary.success {
                    assert!(summary.final_generation_revalidated);
                    assert_eq!(
                        events,
                        vec![
                            FullAdoptionEvent::Acquire(pid),
                            FullAdoptionEvent::Inspect(pid),
                            FullAdoptionEvent::Wait(pid),
                            FullAdoptionEvent::Inspect(pid),
                        ]
                    );
                    assert!(summary.replacement_preserved_at.is_none());
                    ("adopted", None)
                } else {
                    assert!(!summary.final_generation_revalidated);
                    assert_eq!(
                        events,
                        vec![
                            FullAdoptionEvent::Acquire(pid),
                            FullAdoptionEvent::Inspect(pid),
                            FullAdoptionEvent::Wait(pid),
                        ]
                    );
                    (
                        "adoption-replacement-preserved",
                        Some(
                            summary
                                .replacement_preserved_at
                                .expect("losing full adoption must preserve the winner"),
                        ),
                    )
                };
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: kind.to_string(),
                        preserved_at,
                    },
                );
            }
            "replace" => {
                let path = process_client_path(PROCESS_CLIENT_PATH);
                let capture = captured_path(&path);
                let replacement =
                    serde_json::to_vec_pretty(&v2_runfile(&path, process_client_discriminator()))
                        .unwrap();
                process_client_barrier(&root, "replace-start", &id).unwrap();
                let outcome = replace_if_unchanged(&capture, &replacement).unwrap();
                let (kind, preserved_at) = match outcome {
                    ReplacementOutcome::Replaced => ("replaced", None),
                    ReplacementOutcome::Absent => ("absent", None),
                    ReplacementOutcome::ReplacementPreserved { path, .. } => {
                        ("replacement-preserved", Some(path))
                    }
                };
                write_process_client_outcome(
                    &root,
                    &id,
                    ProcessClientOutcome {
                        kind: kind.to_string(),
                        preserved_at,
                    },
                );
            }
            other => panic!("unknown registration process-client mode {other}"),
        }
    }

    fn assert_two_process_lifecycle_interleaving_is_per_path_safe() {
        let root = tempfile::tempdir().unwrap();
        let coordinates = root.path().join("coordinates");
        fs::create_dir_all(&coordinates).unwrap();
        let workspace_a = root.path().join("workspace-a");
        let workspace_b = root.path().join("workspace-b");
        let global = absolute_path(&root.path().join("config").join("server.json")).unwrap();
        let local_a = absolute_path(&runfile_path(&workspace_a)).unwrap();
        let local_b = absolute_path(&runfile_path(&workspace_b)).unwrap();

        let publisher_a = ProcessClientGuard::spawn(
            "publisher A",
            "publish",
            &coordinates,
            "publisher-a",
            &[
                (
                    PROCESS_CLIENT_WORKSPACE,
                    workspace_a.to_string_lossy().into_owned(),
                ),
                (PROCESS_CLIENT_GLOBAL, global.to_string_lossy().into_owned()),
                (PROCESS_CLIENT_DISCRIMINATOR, "101".to_string()),
            ],
        );
        let publisher_b = ProcessClientGuard::spawn(
            "publisher B",
            "publish",
            &coordinates,
            "publisher-b",
            &[
                (
                    PROCESS_CLIENT_WORKSPACE,
                    workspace_b.to_string_lossy().into_owned(),
                ),
                (PROCESS_CLIENT_GLOBAL, global.to_string_lossy().into_owned()),
                (PROCESS_CLIENT_DISCRIMINATOR, "102".to_string()),
            ],
        );
        for phase in [
            "publish-local-stage",
            "publish-local-committed",
            "publish-global-stage",
        ] {
            await_process_client(&coordinates, phase, "publisher-a");
            await_process_client(&coordinates, phase, "publisher-b");
            for workspace in [&workspace_a, &workspace_b] {
                let inventory = inventory_runfiles(workspace, Some(global.clone()));
                match phase {
                    "publish-local-stage" => {
                        assert!(matches!(inventory.local, RegistrationSlot::Absent { .. }));
                    }
                    "publish-local-committed" | "publish-global-stage" => {
                        assert!(matches!(inventory.local, RegistrationSlot::Captured(_)));
                    }
                    _ => unreachable!(),
                }
                assert!(matches!(
                    inventory.global,
                    Some(RegistrationSlot::Absent { .. })
                ));
            }
            assert_eq!(
                publication_stage_paths(root.path()).len(),
                if matches!(phase, "publish-local-stage" | "publish-global-stage") {
                    2
                } else {
                    0
                },
                "unexpected retained stages at publication checkpoint {phase}"
            );
            release_process_client(&coordinates, phase, "publisher-a");
            release_process_client(&coordinates, phase, "publisher-b");
        }

        let winner_id = await_either_process_client(
            &coordinates,
            "publish-global-committed",
            ["publisher-a", "publisher-b"],
        );
        let loser_id = if winner_id == "publisher-a" {
            "publisher-b"
        } else {
            "publisher-a"
        };
        await_process_client(&coordinates, "publish-compensation", loser_id);
        let (winner_workspace, winner_local, loser_workspace, loser_local) =
            if winner_id == "publisher-a" {
                (&workspace_a, &local_a, &workspace_b, &local_b)
            } else {
                (&workspace_b, &local_b, &workspace_a, &local_a)
            };

        let winner_inventory = inventory_runfiles(winner_workspace, Some(global.clone()));
        assert!(matches!(
            winner_inventory.local,
            RegistrationSlot::Captured(_)
        ));
        assert!(matches!(
            winner_inventory.global,
            Some(RegistrationSlot::Captured(_))
        ));
        assert!(select_unique(&winner_inventory).is_ok());

        let loser_inventory = inventory_runfiles(loser_workspace, Some(global.clone()));
        assert!(matches!(
            loser_inventory.local,
            RegistrationSlot::Captured(_)
        ));
        assert!(matches!(
            loser_inventory.global,
            Some(RegistrationSlot::Captured(_))
        ));
        assert!(matches!(
            select_unique(&loser_inventory),
            Err(SelectionError::Conflict { .. })
        ));
        assert_eq!(publication_stage_paths(root.path()).len(), 1);

        let concurrent_loser = v2_runfile(loser_local, 103);
        let concurrent_loser_raw = write_runfile(loser_local, &concurrent_loser);
        release_process_client(&coordinates, "publish-global-committed", &winner_id);
        release_process_client(&coordinates, "publish-compensation", loser_id);
        publisher_a.finish();
        publisher_b.finish();
        let publisher_a_outcome = read_process_client_outcome(&coordinates, "publisher-a");
        let publisher_b_outcome = read_process_client_outcome(&coordinates, "publisher-b");
        let mut publisher_kinds = [
            publisher_a_outcome.kind.as_str(),
            publisher_b_outcome.kind.as_str(),
        ];
        publisher_kinds.sort_unstable();
        assert_eq!(publisher_kinds, ["compensated-preserved", "published"]);
        let loser_outcome = if loser_id == "publisher-a" {
            &publisher_a_outcome
        } else {
            &publisher_b_outcome
        };
        assert_eq!(fs::read(winner_local).unwrap(), fs::read(&global).unwrap());
        assert_eq!(fs::read(loser_local).unwrap(), concurrent_loser_raw);
        assert_eq!(captured_path(loser_local).runfile, concurrent_loser);
        let held_loser = loser_outcome
            .preserved_at
            .as_ref()
            .expect("loser compensation must report the preserved concurrent replacement");
        assert_eq!(fs::read(held_loser).unwrap(), concurrent_loser_raw);
        fs::remove_file(held_loser).unwrap();
        fs::remove_dir(held_loser.parent().unwrap()).unwrap();
        assert!(publication_stage_paths(root.path()).is_empty());

        let winner_capture = captured_path(winner_local);
        let global_slot = capture_registration_path(RegistrationScope::Global, &global);
        let global_capture = captured(&global_slot).clone();
        assert_eq!(
            remove_if_unchanged(&winner_capture).unwrap(),
            RemovalOutcome::Removed
        );
        assert_eq!(
            remove_if_unchanged(&global_capture).unwrap(),
            RemovalOutcome::Removed
        );
        assert_eq!(
            remove_if_unchanged(&captured_path(loser_local)).unwrap(),
            RemovalOutcome::Removed
        );

        // A remover atomically isolates its capture while a different process
        // publishes a replacement at the original name. The remover may clean
        // only its moved capture and must preserve the publisher's bytes.
        let operation_workspace = root.path().join("operation-workspace");
        let operation_path = absolute_path(&runfile_path(&operation_workspace)).unwrap();
        write_runfile(&operation_path, &v2_runfile(&operation_path, 301));
        let remover = ProcessClientGuard::spawn(
            "isolating remover",
            "remove-after-isolation",
            &coordinates,
            "isolating-remover",
            &[(
                PROCESS_CLIENT_PATH,
                operation_path.to_string_lossy().into_owned(),
            )],
        );
        await_process_client(&coordinates, "remove-isolated", "isolating-remover");
        let isolated = inventory_runfiles(&operation_workspace, None);
        assert!(matches!(
            isolated.local,
            RegistrationSlot::Absent {
                scope: RegistrationScope::Local,
                ref path,
            } if path == &operation_path
        ));
        let replacement_publisher = ProcessClientGuard::spawn(
            "replacement publisher",
            "publish-replacement",
            &coordinates,
            "replacement-publisher",
            &[
                (
                    PROCESS_CLIENT_PATH,
                    operation_path.to_string_lossy().into_owned(),
                ),
                (PROCESS_CLIENT_DISCRIMINATOR, "302".to_string()),
            ],
        );
        replacement_publisher.finish();
        let replacement_inventory = inventory_runfiles(&operation_workspace, None);
        assert_eq!(
            captured(&replacement_inventory.local).runfile,
            v2_runfile(&operation_path, 302)
        );
        release_process_client(&coordinates, "remove-isolated", "isolating-remover");
        remover.finish();
        assert_eq!(
            read_process_client_outcome(&coordinates, "isolating-remover").kind,
            "replacement-preserved"
        );
        assert_eq!(
            captured_path(&operation_path).runfile,
            v2_runfile(&operation_path, 302)
        );

        // Two explicit adoption clients capture one live v1 record before
        // either replaces it. The first wins; the second preserves the winner
        // because its captured bytes are stale. This registration-only path
        // has no process signal operation at all.
        write_runfile(&operation_path, &legacy_runfile(1234));
        let adopter_a = ProcessClientGuard::spawn(
            "adopter A",
            "replace",
            &coordinates,
            "adopter-a",
            &[
                (
                    PROCESS_CLIENT_PATH,
                    operation_path.to_string_lossy().into_owned(),
                ),
                (PROCESS_CLIENT_DISCRIMINATOR, "401".to_string()),
            ],
        );
        let adopter_b = ProcessClientGuard::spawn(
            "adopter B",
            "replace",
            &coordinates,
            "adopter-b",
            &[
                (
                    PROCESS_CLIENT_PATH,
                    operation_path.to_string_lossy().into_owned(),
                ),
                (PROCESS_CLIENT_DISCRIMINATOR, "402".to_string()),
            ],
        );
        await_process_client(&coordinates, "replace-start", "adopter-a");
        await_process_client(&coordinates, "replace-start", "adopter-b");
        assert_eq!(
            captured(&inventory_runfiles(&operation_workspace, None).local)
                .runfile
                .schema_version,
            LEGACY_SCHEMA_VERSION
        );
        release_process_client(&coordinates, "replace-start", "adopter-a");
        adopter_a.finish();
        assert_eq!(
            captured(&inventory_runfiles(&operation_workspace, None).local).runfile,
            v2_runfile(&operation_path, 401)
        );
        release_process_client(&coordinates, "replace-start", "adopter-b");
        adopter_b.finish();
        let adopter_a = read_process_client_outcome(&coordinates, "adopter-a");
        let adopter_b = read_process_client_outcome(&coordinates, "adopter-b");
        assert_eq!(adopter_a.kind, "replaced");
        assert_eq!(adopter_b.kind, "replacement-preserved");
        assert_eq!(
            captured_path(&operation_path).runfile,
            v2_runfile(&operation_path, 401)
        );
        let held_winner = adopter_b
            .preserved_at
            .expect("losing adopter retains the concurrently published winner");
        assert_eq!(
            fs::read(&held_winner).unwrap(),
            fs::read(&operation_path).unwrap()
        );
        fs::remove_file(&held_winner).unwrap();
        fs::remove_dir(held_winner.parent().unwrap()).unwrap();

        // Repeat the race through the complete adoption orchestration, not
        // just its compare-and-replace primitive. Both child processes capture
        // schema v1 before either acquires and inspects its retained process;
        // the winner validates identity/listener/argv and revalidates the same
        // generation after publication, while the loser preserves that winner.
        write_runfile(&operation_path, &legacy_runfile(1234));
        let full_adopter_a = ProcessClientGuard::spawn(
            "full adopter A",
            "adopt-full",
            &coordinates,
            "full-adopter-a",
            &[(
                PROCESS_CLIENT_PATH,
                operation_path.to_string_lossy().into_owned(),
            )],
        );
        let full_adopter_b = ProcessClientGuard::spawn(
            "full adopter B",
            "adopt-full",
            &coordinates,
            "full-adopter-b",
            &[(
                PROCESS_CLIENT_PATH,
                operation_path.to_string_lossy().into_owned(),
            )],
        );
        await_process_client(&coordinates, "adopt-full-start", "full-adopter-a");
        await_process_client(&coordinates, "adopt-full-start", "full-adopter-b");
        assert_eq!(
            captured(&inventory_runfiles(&operation_workspace, None).local)
                .runfile
                .schema_version,
            LEGACY_SCHEMA_VERSION
        );
        release_process_client(&coordinates, "adopt-full-start", "full-adopter-a");
        full_adopter_a.finish();
        let adopted = captured_path(&operation_path);
        assert_eq!(adopted.runfile.schema_version, IDENTITY_SCHEMA_VERSION);
        assert_eq!(
            adopted.runfile.origin_local_runfile,
            Some(operation_path.clone())
        );
        assert_eq!(
            adopted
                .runfile
                .process_identity
                .as_ref()
                .unwrap()
                .start_token,
            canonical_start_token(501)
        );
        release_process_client(&coordinates, "adopt-full-start", "full-adopter-b");
        full_adopter_b.finish();
        let full_adopter_a = read_process_client_outcome(&coordinates, "full-adopter-a");
        let full_adopter_b = read_process_client_outcome(&coordinates, "full-adopter-b");
        assert_eq!(full_adopter_a.kind, "adopted");
        assert_eq!(full_adopter_b.kind, "adoption-replacement-preserved");
        let held_full_winner = full_adopter_b
            .preserved_at
            .expect("losing full adoption retains the concurrently adopted winner");
        assert_eq!(
            fs::read(&held_full_winner).unwrap(),
            fs::read(&operation_path).unwrap()
        );
        fs::remove_file(&held_full_winner).unwrap();
        fs::remove_dir(held_full_winner.parent().unwrap()).unwrap();

        // Both remover processes capture the same final before either runs.
        // One exact-byte removal succeeds and the other observes absence.
        let remover_a = ProcessClientGuard::spawn(
            "remover A",
            "remove",
            &coordinates,
            "remover-a",
            &[(
                PROCESS_CLIENT_PATH,
                operation_path.to_string_lossy().into_owned(),
            )],
        );
        let remover_b = ProcessClientGuard::spawn(
            "remover B",
            "remove",
            &coordinates,
            "remover-b",
            &[(
                PROCESS_CLIENT_PATH,
                operation_path.to_string_lossy().into_owned(),
            )],
        );
        await_process_client(&coordinates, "remove-start", "remover-a");
        await_process_client(&coordinates, "remove-start", "remover-b");
        assert!(matches!(
            inventory_runfiles(&operation_workspace, None).local,
            RegistrationSlot::Captured(_)
        ));
        release_process_client(&coordinates, "remove-start", "remover-a");
        remover_a.finish();
        assert!(matches!(
            inventory_runfiles(&operation_workspace, None).local,
            RegistrationSlot::Absent { .. }
        ));
        release_process_client(&coordinates, "remove-start", "remover-b");
        remover_b.finish();
        assert_eq!(
            read_process_client_outcome(&coordinates, "remover-a").kind,
            "removed"
        );
        assert_eq!(
            read_process_client_outcome(&coordinates, "remover-b").kind,
            "absent"
        );
        assert!(!operation_path.exists());
        assert!(publication_stage_paths(root.path()).is_empty());
    }

    #[test]
    fn two_process_lifecycle_interleaving_is_per_path_safe() {
        assert_two_process_lifecycle_interleaving_is_per_path_safe();
    }

    fn clean_attempt_stages(attempt: &PublicationAttempt) {
        for stage in &attempt.stages {
            assert_eq!(
                remove_publication_stage_if_unchanged(stage).unwrap(),
                RemovalOutcome::Removed
            );
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum InventorySlotCase {
        Absent,
        Unreadable,
        Malformed,
        NonRegular,
        V1,
        V2,
    }

    fn assert_inventory_slot_case(scope: RegistrationScope, case: InventorySlotCase) {
        let root = tempfile::tempdir().unwrap();
        let mut workspace = root.path().join("workspace");
        let mut global = root.path().join("config/server.json");
        if case == InventorySlotCase::Unreadable {
            match scope {
                RegistrationScope::Local => {
                    workspace =
                        PathBuf::from(format!("{}\0unreadable-workspace", root.path().display()));
                }
                RegistrationScope::Global => {
                    global =
                        PathBuf::from(format!("{}\0unreadable-global.json", root.path().display()));
                }
                RegistrationScope::Origin => unreachable!("origin has its own matrix"),
            }
        }

        let local = runfile_path(&workspace);
        let target = match scope {
            RegistrationScope::Local => local.clone(),
            RegistrationScope::Global => global.clone(),
            RegistrationScope::Origin => unreachable!("origin has its own matrix"),
        };
        let expected_path = absolute_path(&target).unwrap_or_else(|_| target.clone());
        let mut expected_capture = None;
        match case {
            InventorySlotCase::Absent | InventorySlotCase::Unreadable => {}
            InventorySlotCase::Malformed => {
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, b"{not-json").unwrap();
            }
            InventorySlotCase::NonRegular => fs::create_dir_all(&target).unwrap(),
            InventorySlotCase::V1 => {
                let runfile = legacy_runfile(101);
                let raw = write_runfile(&target, &runfile);
                expected_capture = Some((raw, runfile));
            }
            InventorySlotCase::V2 => {
                let origin = if scope == RegistrationScope::Local {
                    expected_path.clone()
                } else {
                    absolute_path(&runfile_path(&root.path().join("promised-workspace"))).unwrap()
                };
                let runfile = v2_runfile(&origin, 102);
                let raw = write_runfile(&target, &runfile);
                expected_capture = Some((raw, runfile));
            }
        }

        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        let slot = match scope {
            RegistrationScope::Local => &inventory.local,
            RegistrationScope::Global => inventory.global.as_ref().unwrap(),
            RegistrationScope::Origin => unreachable!("origin has its own matrix"),
        };
        match case {
            InventorySlotCase::Absent => assert!(matches!(
                slot,
                RegistrationSlot::Absent {
                    scope: actual_scope,
                    path,
                } if *actual_scope == scope && path == &expected_path
            )),
            InventorySlotCase::Unreadable => assert!(matches!(
                slot,
                RegistrationSlot::Blocked {
                    scope: actual_scope,
                    path,
                    reason: RegistrationBlock::Unreadable(_),
                } if *actual_scope == scope && path == &expected_path
            )),
            InventorySlotCase::Malformed => assert!(matches!(
                slot,
                RegistrationSlot::Blocked {
                    scope: actual_scope,
                    path,
                    reason: RegistrationBlock::Malformed(_),
                } if *actual_scope == scope && path == &expected_path
            )),
            InventorySlotCase::NonRegular => assert!(matches!(
                slot,
                RegistrationSlot::Blocked {
                    scope: actual_scope,
                    path,
                    reason: RegistrationBlock::NonRegular,
                } if *actual_scope == scope && path == &expected_path
            )),
            InventorySlotCase::V1 | InventorySlotCase::V2 => {
                let (raw, runfile) = expected_capture.as_ref().unwrap();
                let capture = captured(slot);
                assert_eq!(capture.scope, scope);
                assert_eq!(capture.path, expected_path);
                assert_eq!(&capture.raw, raw);
                assert_eq!(&capture.runfile, runfile);
            }
        }

        let other = match scope {
            RegistrationScope::Local => inventory.global.as_ref().unwrap(),
            RegistrationScope::Global => &inventory.local,
            RegistrationScope::Origin => unreachable!("origin has its own matrix"),
        };
        let (other_scope, other_path) = match scope {
            RegistrationScope::Local => {
                (RegistrationScope::Global, absolute_path(&global).unwrap())
            }
            RegistrationScope::Global => (RegistrationScope::Local, absolute_path(&local).unwrap()),
            RegistrationScope::Origin => unreachable!("origin has its own matrix"),
        };
        assert!(matches!(
            other,
            RegistrationSlot::Absent { scope, path }
                if *scope == other_scope && path == &other_path
        ));

        match case {
            InventorySlotCase::Absent => assert_eq!(select_unique(&inventory).unwrap(), None),
            InventorySlotCase::V1 | InventorySlotCase::V2 => {
                let selected = select_unique(&inventory).unwrap().unwrap();
                assert_eq!(selected.scope, scope);
                assert_eq!(selected.path, expected_path);
            }
            InventorySlotCase::Unreadable
            | InventorySlotCase::Malformed
            | InventorySlotCase::NonRegular => assert!(matches!(
                select_unique(&inventory),
                Err(SelectionError::Blocked {
                    scope: blocked_scope,
                    path,
                    ..
                }) if blocked_scope == scope && path == expected_path
            )),
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum PromisedOriginCase {
        Matching,
        Absent,
        Changed,
        Malformed,
        Unreadable,
        NonRegular,
    }

    fn assert_promised_origin_case(case: PromisedOriginCase) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("inventory-workspace");
        let global = root.path().join("config/server.json");
        let origin = if matches!(case, PromisedOriginCase::Unreadable) {
            PathBuf::from(format!("{}\0unreadable-origin", root.path().display()))
                .join(".ferric/server.json")
        } else {
            absolute_path(&runfile_path(&root.path().join("origin-workspace"))).unwrap()
        };
        let expected = v2_runfile(&origin, 201);
        let global_raw = write_runfile(&global, &expected);

        let mut changed_capture = None;
        match case {
            PromisedOriginCase::Matching => {
                let raw = write_runfile(&origin, &expected);
                changed_capture = Some((raw, expected.clone()));
            }
            PromisedOriginCase::Absent | PromisedOriginCase::Unreadable => {}
            PromisedOriginCase::Changed => {
                let changed = v2_runfile(&origin, 202);
                let raw = write_runfile(&origin, &changed);
                changed_capture = Some((raw, changed));
            }
            PromisedOriginCase::Malformed => {
                fs::create_dir_all(origin.parent().unwrap()).unwrap();
                fs::write(&origin, b"{malformed-origin").unwrap();
            }
            PromisedOriginCase::NonRegular => fs::create_dir_all(&origin).unwrap(),
        }

        let inventory = inventory_runfiles(&workspace, Some(global.clone()));
        assert!(matches!(
            &inventory.local,
            RegistrationSlot::Absent {
                scope: RegistrationScope::Local,
                path,
            } if path == &absolute_path(&runfile_path(&workspace)).unwrap()
        ));
        let global_capture = captured(inventory.global.as_ref().unwrap());
        assert_eq!(global_capture.scope, RegistrationScope::Global);
        assert_eq!(global_capture.path, absolute_path(&global).unwrap());
        assert_eq!(global_capture.raw, global_raw);
        assert_eq!(global_capture.runfile, expected);

        assert_eq!(inventory.promised_origins.len(), 1);
        let promise = &inventory.promised_origins[0];
        assert_eq!(promise.source.scope, RegistrationScope::Global);
        assert_eq!(promise.source.path, absolute_path(&global).unwrap());
        assert_eq!(promise.expected_runfile, expected);
        match case {
            PromisedOriginCase::Matching | PromisedOriginCase::Changed => {
                let (raw, runfile) = changed_capture.as_ref().unwrap();
                let capture = captured(&promise.slot);
                assert_eq!(capture.scope, RegistrationScope::Origin);
                assert_eq!(capture.path, origin);
                assert_eq!(&capture.raw, raw);
                assert_eq!(&capture.runfile, runfile);
                assert_eq!(
                    capture.runfile == promise.expected_runfile,
                    matches!(case, PromisedOriginCase::Matching)
                );
            }
            PromisedOriginCase::Absent => assert!(matches!(
                &promise.slot,
                RegistrationSlot::Absent {
                    scope: RegistrationScope::Origin,
                    path,
                } if path == &origin
            )),
            PromisedOriginCase::Malformed => assert!(matches!(
                &promise.slot,
                RegistrationSlot::Blocked {
                    scope: RegistrationScope::Origin,
                    path,
                    reason: RegistrationBlock::Malformed(_),
                } if path == &origin
            )),
            PromisedOriginCase::Unreadable => assert!(matches!(
                &promise.slot,
                RegistrationSlot::Blocked {
                    scope: RegistrationScope::Origin,
                    path,
                    reason: RegistrationBlock::Unreadable(_),
                } if path == &origin
            )),
            PromisedOriginCase::NonRegular => assert!(matches!(
                &promise.slot,
                RegistrationSlot::Blocked {
                    scope: RegistrationScope::Origin,
                    path,
                    reason: RegistrationBlock::NonRegular,
                } if path == &origin
            )),
        }
    }

    #[test]
    fn registration_inventory_retains_both_scopes_and_raw_bytes() {
        for scope in [RegistrationScope::Local, RegistrationScope::Global] {
            for case in [
                InventorySlotCase::Absent,
                InventorySlotCase::Unreadable,
                InventorySlotCase::Malformed,
                InventorySlotCase::NonRegular,
                InventorySlotCase::V1,
                InventorySlotCase::V2,
            ] {
                assert_inventory_slot_case(scope, case);
            }
        }

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace-without-global");
        let without_global = inventory_runfiles(&workspace, None);
        assert!(matches!(
            without_global.local,
            RegistrationSlot::Absent {
                scope: RegistrationScope::Local,
                ref path,
            } if *path == absolute_path(&runfile_path(&workspace)).unwrap()
        ));
        assert!(without_global.global.is_none());
        assert!(without_global.promised_origins.is_empty());
        assert_eq!(select_unique(&without_global).unwrap(), None);

        for case in [
            PromisedOriginCase::Matching,
            PromisedOriginCase::Absent,
            PromisedOriginCase::Changed,
            PromisedOriginCase::Malformed,
            PromisedOriginCase::Unreadable,
            PromisedOriginCase::NonRegular,
        ] {
            assert_promised_origin_case(case);
        }
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
        downgraded.process_identity = Some(identity(1));
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
        let runfile = v2_runfile(&local_b, 2);

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
        let relative = v2_runfile(Path::new("workspace/.ferric/server.json"), 3);
        write_runfile(&global, &relative);
        assert!(matches!(
            capture_registration_path(RegistrationScope::Global, &global),
            RegistrationSlot::Blocked {
                reason: RegistrationBlock::InvalidSchema(detail),
                ..
            } if detail.contains("must be absolute")
        ));

        let wrong_shape = v2_runfile(&root.path().join("server.json"), 4);
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
    fn runfile_schema_authority_matrix() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.json");
        let origin = absolute_path(&root.path().join("workspace/.ferric/server.json")).unwrap();

        let legacy_raw = write_runfile(&global, &legacy_runfile(1));
        let legacy = captured(&capture_registration_path(
            RegistrationScope::Global,
            &global,
        ))
        .clone();
        assert_eq!(legacy.raw, legacy_raw);
        assert_eq!(legacy.runfile.schema_version, LEGACY_SCHEMA_VERSION);
        assert!(legacy.runfile.process_identity.is_none());

        let valid_v2 = v2_runfile(&origin, 5);
        write_runfile(&global, &valid_v2);
        assert_eq!(
            captured(&capture_registration_path(
                RegistrationScope::Global,
                &global,
            ))
            .runfile,
            valid_v2
        );

        for (field, coordinate, expected) in [
            (
                "pid",
                u64::from(u32::MAX) + 1,
                "pid coordinate 4294967296 exceeds maximum 4294967295",
            ),
            (
                "port",
                u64::from(u16::MAX) + 1,
                "port coordinate 65536 exceeds maximum 65535",
            ),
        ] {
            let mut envelope = serde_json::to_value(&valid_v2).unwrap();
            envelope[field] = serde_json::Value::from(coordinate);
            let raw = serde_json::to_vec_pretty(&envelope).unwrap();
            assert_raw_invalid_schema(RegistrationScope::Global, &global, &raw, expected);
        }

        let mut version_overflow = serde_json::to_value(&valid_v2).unwrap();
        version_overflow["schema_version"] = serde_json::Value::from(u64::from(u8::MAX) + 1);
        assert_raw_invalid_schema(
            RegistrationScope::Global,
            &global,
            &serde_json::to_vec_pretty(&version_overflow).unwrap(),
            "unsupported schema version 256",
        );

        let mut wrong_type_with_pid_overflow = serde_json::to_value(&valid_v2).unwrap();
        wrong_type_with_pid_overflow["pid"] = serde_json::Value::from(u64::from(u32::MAX) + 1);
        wrong_type_with_pid_overflow["base_url"] = serde_json::Value::from(7);
        assert_raw_malformed(
            RegistrationScope::Global,
            &global,
            &serde_json::to_vec_pretty(&wrong_type_with_pid_overflow).unwrap(),
        );

        let mut missing_with_port_overflow = serde_json::to_value(&valid_v2).unwrap();
        missing_with_port_overflow["port"] = serde_json::Value::from(u64::from(u16::MAX) + 1);
        missing_with_port_overflow
            .as_object_mut()
            .unwrap()
            .remove("base_url");
        assert_raw_malformed(
            RegistrationScope::Global,
            &global,
            &serde_json::to_vec_pretty(&missing_with_port_overflow).unwrap(),
        );

        let mut duplicate_with_pid_overflow = serde_json::to_value(&valid_v2).unwrap();
        duplicate_with_pid_overflow["pid"] = serde_json::Value::from(u64::from(u32::MAX) + 1);
        let duplicate_with_pid_overflow =
            serde_json::to_string(&duplicate_with_pid_overflow).unwrap();
        let duplicate_with_pid_overflow = format!(
            "{{\"pid\":1,{}",
            duplicate_with_pid_overflow
                .strip_prefix('{')
                .expect("serialized runfile is a JSON object")
        );
        assert_raw_malformed(
            RegistrationScope::Global,
            &global,
            duplicate_with_pid_overflow.as_bytes(),
        );

        let mut cases = Vec::new();
        let mut zero_pid = v2_runfile(&origin, 6);
        zero_pid.pid = 0;
        cases.push((zero_pid, "nonzero pid"));
        let mut zero_port = v2_runfile(&origin, 7);
        zero_port.port = 0;
        cases.push((zero_port, "nonzero port"));
        let mut empty_token = v2_runfile(&origin, 8);
        empty_token.process_identity.as_mut().unwrap().start_token = "   ".to_string();
        cases.push((empty_token, "start_token"));
        let mut missing_identity = v2_runfile(&origin, 9);
        missing_identity.process_identity = None;
        cases.push((missing_identity, "requires process_identity"));
        let mut relative_executable = v2_runfile(&origin, 10);
        relative_executable
            .process_identity
            .as_mut()
            .unwrap()
            .executable = PathBuf::from("llama-server");
        cases.push((relative_executable, "absolute path"));
        let mut empty_executable = v2_runfile(&origin, 11);
        empty_executable
            .process_identity
            .as_mut()
            .unwrap()
            .executable = PathBuf::new();
        cases.push((empty_executable, "absolute path"));
        let mut empty_argv = v2_runfile(&origin, 12);
        empty_argv.process_identity.as_mut().unwrap().argv.clear();
        cases.push((empty_argv, "observed argv"));
        let mut empty_argv_element = v2_runfile(&origin, 13);
        empty_argv_element
            .process_identity
            .as_mut()
            .unwrap()
            .argv
            .push(String::new());
        cases.push((empty_argv_element, "argv elements"));
        let mut missing_origin = v2_runfile(&origin, 14);
        missing_origin.origin_local_runfile = None;
        cases.push((missing_origin, "requires origin_local_runfile"));
        let relative_origin = v2_runfile(Path::new("workspace/.ferric/server.json"), 15);
        cases.push((relative_origin, "must be absolute"));
        let wrong_suffix_origin = v2_runfile(&root.path().join("workspace/server.json"), 16);
        cases.push((wrong_suffix_origin, "must end in .ferric/server.json"));
        for (index, base_url) in [
            "https://127.0.0.1:8080/v1",
            "http://localhost:8080/v1",
            "http://127.0.0.1:9090/v1",
            "http://127.0.0.1:8080/health",
        ]
        .into_iter()
        .enumerate()
        {
            let mut divergent_base_url = v2_runfile(&origin, 20 + index as u64);
            divergent_base_url.base_url = base_url.to_string();
            cases.push((divergent_base_url, "base_url must remain"));
        }
        let mut unknown_version = v2_runfile(&origin, 30);
        unknown_version.schema_version = 99;
        cases.push((unknown_version, "unsupported schema version"));

        #[cfg(any(windows, target_os = "linux"))]
        for (index, token) in invalid_schema_start_tokens().iter().enumerate() {
            let mut runfile = v2_runfile(&origin, 40 + index as u64);
            runfile.process_identity.as_mut().unwrap().start_token = (*token).to_string();
            cases.push((runfile, "start_token"));
        }

        for (runfile, expected) in cases {
            assert_invalid_schema(RegistrationScope::Global, &global, &runfile, expected);
        }

        let local = absolute_path(&runfile_path(&root.path().join("local-workspace"))).unwrap();
        let other_local =
            absolute_path(&runfile_path(&root.path().join("different-workspace"))).unwrap();
        let self_mismatched = v2_runfile(&other_local, 90);
        assert_invalid_schema(
            RegistrationScope::Local,
            &local,
            &self_mismatched,
            "does not name its own registration",
        );
    }

    #[test]
    fn runfile_schema_is_additive_and_validated() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.json");
        let origin = absolute_path(&root.path().join("workspace/.ferric/server.json")).unwrap();

        let old_false = br#"{"schema_version":2,"engine":"llama-server","pid":1234,"port":8080,"base_url":"http://127.0.0.1:8080/v1","tailscale":false,"process_identity":{"start_token":"linux-boot-id:00000000-1111-4222-8333-444444444444;start-ticks:987005","executable":"/fixture/llama-server","argv":["llama-server"]},"origin_local_runfile":"/fixture/.ferric/server.json"}"#;
        let decoded = serde_json::from_slice::<ServerRunfile>(old_false).unwrap();
        assert!(decoded.tailscale_serve.is_none());

        let mut owned = v2_runfile(&origin, 100);
        owned.tailscale = true;
        owned.tailscale_serve = Some(tailscale_ownership());
        let raw = write_runfile(&global, &owned);
        let slot = capture_registration_path(RegistrationScope::Global, &global);
        let captured = captured(&slot);
        assert_eq!(captured.runfile, owned);
        assert_eq!(captured.raw, raw);

        let mut boolean_only = owned.clone();
        boolean_only.tailscale_serve = None;
        validate_runfile(RegistrationScope::Global, &global, &boolean_only)
            .expect("historical boolean-only records remain structurally readable");

        let mut cases = Vec::new();
        let mut invalid_token = owned.clone();
        invalid_token.tailscale_serve.as_mut().unwrap().token = "0".repeat(31);
        cases.push(invalid_token);
        let mut invalid_path = owned.clone();
        invalid_path.tailscale_serve.as_mut().unwrap().mount_path = "/_ferric/wrong".to_string();
        cases.push(invalid_path);
        let mut invalid_target = owned.clone();
        invalid_target
            .tailscale_serve
            .as_mut()
            .unwrap()
            .proxy_target = "http://127.0.0.1:9090".to_string();
        cases.push(invalid_target);
        let mut invalid_port = owned.clone();
        invalid_port.tailscale_serve.as_mut().unwrap().https_port = 8443;
        cases.push(invalid_port);
        let mut invalid_digest = owned.clone();
        invalid_digest
            .tailscale_serve
            .as_mut()
            .unwrap()
            .before_status_sha256 = "not-a-digest".to_string();
        cases.push(invalid_digest);
        let mut invalid_remote = owned.clone();
        invalid_remote
            .tailscale_serve
            .as_mut()
            .unwrap()
            .remote_base_url = "https://example.invalid/v1".to_string();
        cases.push(invalid_remote);
        let mut metadata_disagreement = owned.clone();
        metadata_disagreement.tailscale = false;
        cases.push(metadata_disagreement);

        for invalid in cases {
            assert!(
                validate_runfile(RegistrationScope::Global, &global, &invalid).is_err(),
                "invalid ownership shape authorized schema-v2 registration: {invalid:?}"
            );
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn runfile_schema_rejects_untagged_foreign_or_noncanonical_start_tokens() {
        let root = tempfile::tempdir().unwrap();
        let global = root.path().join("global.json");
        let origin = absolute_path(&root.path().join("workspace/.ferric/server.json")).unwrap();

        for (index, token) in invalid_schema_start_tokens().iter().enumerate() {
            let mut runfile = v2_runfile(&origin, 40 + index as u64);
            runfile.process_identity.as_mut().unwrap().start_token = (*token).to_string();
            assert_invalid_schema(RegistrationScope::Global, &global, &runfile, "start_token");
        }
    }

    #[test]
    fn identical_and_parse_equal_mirrors_keep_scope_tokens() {
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

        let local_v2 = v2_runfile(&local, 12);
        write_runfile(&local, &local_v2);
        fs::write(&global, serde_json::to_vec(&local_v2).unwrap()).unwrap();
        let inventory = inventory_runfiles(&workspace, Some(global));
        assert_ne!(
            captured(&inventory.local).raw,
            captured(inventory.global.as_ref().unwrap()).raw
        );
        assert_eq!(captured(&inventory.local).scope, RegistrationScope::Local);
        assert_eq!(
            captured(inventory.global.as_ref().unwrap()).scope,
            RegistrationScope::Global
        );
        assert_eq!(inventory.promised_origins.len(), 1);
        let origin_capture = captured(&inventory.promised_origins[0].slot);
        assert_eq!(origin_capture.scope, RegistrationScope::Origin);
        assert_eq!(origin_capture.raw, captured(&inventory.local).raw);
        assert_eq!(origin_capture.runfile, local_v2);
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
        let local_v2 = v2_runfile(&local_path, 13);
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
    fn registration_publication_is_complete_synced_and_no_clobber() {
        let expected_phases = vec![
            PersistencePhase::CreateStage,
            PersistencePhase::WriteAll,
            PersistencePhase::Flush,
            PersistencePhase::FileSync,
            PersistencePhase::PersistNoClobber,
            PersistencePhase::ParentSync,
        ];

        // Local-only and mirrored success both flow the sole serialized byte
        // vector through complete same-parent stages before exposing finals.
        for mirrored in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let global = root.path().join("config/server.json");
            let runfile = v2_runfile(&local, 70 + u64::from(mirrored));
            let expected_raw = serde_json::to_vec_pretty(&runfile).unwrap();
            let mut effects = ScriptedPersistenceEffects::default();

            let published = publish_mirrored_with(
                &workspace,
                mirrored.then_some(global.as_path()),
                &runfile,
                &mut effects,
            )
            .unwrap();

            assert_eq!(effects.serializations, 1);
            assert_eq!(published.local.raw, expected_raw);
            assert_eq!(fs::read(&local).unwrap(), expected_raw);
            assert_eq!(
                serde_json::from_slice::<ServerRunfile>(&published.local.raw).unwrap(),
                runfile
            );
            assert_eq!(persistence_phases(&effects, &local), expected_phases);
            if mirrored {
                let global_capture = published.global.as_ref().unwrap();
                assert_eq!(global_capture.raw, published.local.raw);
                assert_eq!(fs::read(&global).unwrap(), published.local.raw);
                assert_eq!(
                    serde_json::from_slice::<ServerRunfile>(&global_capture.raw).unwrap(),
                    runfile
                );
                assert_eq!(persistence_phases(&effects, &global), expected_phases);
            } else {
                assert!(published.global.is_none());
            }
            let created = effects
                .events
                .iter()
                .filter(|event| event.phase == PersistencePhase::CreateStage)
                .collect::<Vec<_>>();
            assert_eq!(created.len(), 1 + usize::from(mirrored));
            let mut unique_stages = created
                .iter()
                .map(|event| event.stage_path.as_ref().unwrap())
                .collect::<Vec<_>>();
            unique_stages.sort();
            unique_stages.dedup();
            assert_eq!(unique_stages.len(), created.len());
            for event in effects
                .events
                .iter()
                .filter(|event| event.stage_path.is_some())
            {
                assert_eq!(
                    event.stage_path.as_ref().unwrap().parent(),
                    event.final_path.parent(),
                    "every stage must be in its final's parent: {event:?}"
                );
            }
            let write_lengths = effects
                .events
                .iter()
                .filter(|event| event.phase == PersistencePhase::WriteAll)
                .map(|event| event.byte_len.unwrap())
                .collect::<Vec<_>>();
            assert_eq!(
                write_lengths,
                vec![expected_raw.len(); 1 + usize::from(mirrored)]
            );
            assert!(publication_stage_paths(root.path()).is_empty());
        }

        // Every precommit phase retains one explained exact stage. The launch
        // coordinator can then remove it only after retained-child exit proof.
        for phase in [
            PersistencePhase::CreateStage,
            PersistencePhase::WriteAll,
            PersistencePhase::Flush,
            PersistencePhase::FileSync,
            PersistencePhase::PersistNoClobber,
        ] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(format!("local-{phase:?}"));
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let runfile = v2_runfile(&local, 80);
            let mut effects = ScriptedPersistenceEffects::failing(&local, phase);
            let error =
                publish_mirrored_with(&workspace, None, &runfile, &mut effects).unwrap_err();
            assert_eq!(effects.serializations, 1);
            assert!(matches!(
                &error,
                PublishError::Write {
                    scope: RegistrationScope::Local,
                    ..
                }
            ));
            let attempt = publication_attempt(&error);
            assert_eq!(attempt.terminal_phase, phase);
            assert!(!attempt.final_committed);
            assert!(attempt.finals.is_empty());
            assert!(!local.exists());
            if phase == PersistencePhase::CreateStage {
                assert!(attempt.stages.is_empty());
                assert!(publication_stage_paths(root.path()).is_empty());
            } else {
                assert_eq!(attempt.stages.len(), 1);
                let stage = &attempt.stages[0];
                assert_eq!(stage.scope, RegistrationScope::Local);
                assert_eq!(stage.final_path, local);
                assert_eq!(stage.path.parent(), local.parent());
                assert_eq!(
                    publication_stage_paths(root.path()),
                    vec![stage.path.clone()]
                );
                if phase == PersistencePhase::WriteAll {
                    assert_eq!(stage.raw.as_ref().unwrap().len(), 7);
                } else {
                    assert_eq!(
                        stage.raw.as_ref().unwrap(),
                        &serde_json::to_vec_pretty(&runfile).unwrap()
                    );
                }
                clean_attempt_stages(attempt);
                assert!(publication_stage_paths(root.path()).is_empty());
            }

            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(format!("global-{phase:?}"));
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let global = root.path().join("config/server.json");
            let runfile = v2_runfile(&local, 81);
            let mut effects = ScriptedPersistenceEffects::failing(&global, phase);
            let error = publish_mirrored_with(&workspace, Some(&global), &runfile, &mut effects)
                .unwrap_err();
            assert_eq!(effects.serializations, 1);
            assert!(matches!(&error, PublishError::Mirror { .. }));
            let attempt = publication_attempt(&error);
            assert_eq!(attempt.terminal_phase, phase);
            assert!(!attempt.final_committed);
            assert_eq!(attempt.finals.len(), 1);
            assert_eq!(attempt.finals[0].path, local);
            assert!(local.exists());
            assert!(!global.exists());
            if phase == PersistencePhase::CreateStage {
                assert!(attempt.stages.is_empty());
            } else {
                assert_eq!(attempt.stages.len(), 1);
                assert_eq!(attempt.stages[0].scope, RegistrationScope::Global);
                assert_eq!(attempt.stages[0].final_path, global);
                clean_attempt_stages(attempt);
            }
            assert!(publication_stage_paths(root.path()).is_empty());
        }

        // A parent-sync fault is committed durability failure for either
        // scope, and the attempt names every final already exposed.
        for fail_global in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("durability");
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let global = root.path().join("config/server.json");
            let runfile = v2_runfile(&local, 90 + u64::from(fail_global));
            let target = if fail_global { &global } else { &local };
            let mut effects =
                ScriptedPersistenceEffects::failing(target, PersistencePhase::ParentSync);
            let error = publish_mirrored_with(
                &workspace,
                fail_global.then_some(global.as_path()),
                &runfile,
                &mut effects,
            )
            .unwrap_err();
            assert!(matches!(&error, PublishError::Durability { .. }));
            let attempt = publication_attempt(&error);
            assert_eq!(attempt.terminal_phase, PersistencePhase::ParentSync);
            assert!(attempt.final_committed);
            assert_eq!(attempt.finals.len(), 1 + usize::from(fail_global));
            assert!(attempt.stages.is_empty());
            assert!(local.exists());
            assert_eq!(global.exists(), fail_global);
            assert!(publication_stage_paths(root.path()).is_empty());
        }

        // A rare committed no-clobber operation which retains its original
        // hard link becomes an explained StageCleanup durability failure at
        // either scope. The global row must retain both already-committed
        // finals plus the exact global recovery stage.
        for fail_global in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join(if fail_global {
                "retained-global-stage"
            } else {
                "retained-local-stage"
            });
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let global = root.path().join("config/server.json");
            let target = if fail_global { &global } else { &local };
            let runfile = v2_runfile(&local, 92 + u64::from(fail_global));
            let expected_raw = serde_json::to_vec_pretty(&runfile).unwrap();
            let mut effects = ScriptedPersistenceEffects::retaining_committed_stage(target);
            let error = publish_mirrored_with(
                &workspace,
                fail_global.then_some(global.as_path()),
                &runfile,
                &mut effects,
            )
            .unwrap_err();
            assert!(matches!(&error, PublishError::Durability { .. }));
            let attempt = publication_attempt(&error);
            assert_eq!(attempt.terminal_phase, PersistencePhase::StageCleanup);
            assert!(attempt.final_committed);
            assert_eq!(attempt.finals.len(), 1 + usize::from(fail_global));
            assert_eq!(attempt.finals[0].scope, RegistrationScope::Local);
            assert_eq!(attempt.finals[0].path, local);
            assert_eq!(attempt.finals[0].raw, expected_raw);
            assert_eq!(fs::read(&local).unwrap(), expected_raw);
            if fail_global {
                assert_eq!(attempt.finals[1].scope, RegistrationScope::Global);
                assert_eq!(attempt.finals[1].path, global);
                assert_eq!(attempt.finals[1].raw, expected_raw);
                assert_eq!(fs::read(&global).unwrap(), expected_raw);
            } else {
                assert!(!global.exists());
            }
            assert_eq!(attempt.stages.len(), 1);
            let stage = &attempt.stages[0];
            assert_eq!(
                stage.scope,
                if fail_global {
                    RegistrationScope::Global
                } else {
                    RegistrationScope::Local
                }
            );
            assert_eq!(stage.final_path, *target);
            assert_eq!(stage.path.parent(), target.parent());
            assert_eq!(stage.raw.as_deref(), Some(expected_raw.as_slice()));
            assert_eq!(fs::read(&stage.path).unwrap(), expected_raw);
            assert_eq!(
                publication_stage_paths(root.path()),
                vec![stage.path.clone()]
            );
            assert!(
                error
                    .to_string()
                    .contains(&stage.path.display().to_string())
            );
            clean_attempt_stages(attempt);
            assert!(local.exists());
            assert_eq!(global.exists(), fail_global);
            assert!(publication_stage_paths(root.path()).is_empty());
        }

        // Real destination appearance exercises tempfile's atomic no-replace
        // failure rather than a scripted substitute, for both final scopes.
        for existing_global in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("occupied");
            let local = absolute_path(&runfile_path(&workspace)).unwrap();
            let global = root.path().join("config/server.json");
            let occupied = if existing_global { &global } else { &local };
            fs::create_dir_all(occupied.parent().unwrap()).unwrap();
            fs::write(occupied, b"external-winner").unwrap();
            let runfile = v2_runfile(&local, 93 + u64::from(existing_global));
            let mut effects = ScriptedPersistenceEffects::default();
            let error = publish_mirrored_with(
                &workspace,
                existing_global.then_some(global.as_path()),
                &runfile,
                &mut effects,
            )
            .unwrap_err();
            assert_eq!(fs::read(occupied).unwrap(), b"external-winner");
            assert!(
                matches!(
                    &error,
                    PublishError::Mirror { .. } if existing_global
                ) || matches!(
                    &error,
                    PublishError::Write {
                        scope: RegistrationScope::Local,
                        ..
                    } if !existing_global
                )
            );
            let attempt = publication_attempt(&error);
            assert_eq!(attempt.terminal_phase, PersistencePhase::PersistNoClobber);
            assert!(!attempt.final_committed);
            assert_eq!(attempt.stages.len(), 1);
            clean_attempt_stages(attempt);
            assert!(publication_stage_paths(root.path()).is_empty());
        }

        // Even a byte-identical file at the retained pathname is not
        // attempt-owned when its stable file identity changed.
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("same-byte-stage-race");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let runfile = v2_runfile(&local, 94);
        let mut effects = ScriptedPersistenceEffects::failing(&local, PersistencePhase::FileSync);
        let error = publish_mirrored_with(&workspace, None, &runfile, &mut effects).unwrap_err();
        let stage = publication_attempt(&error).stages[0].clone();
        let replacement_identity =
            replace_with_same_bytes_and_new_identity(&stage.path, stage.raw.as_ref().unwrap());
        assert_ne!(stage.identity, Some(replacement_identity));
        let preserved = match remove_publication_stage_if_unchanged(&stage).unwrap() {
            RemovalOutcome::ReplacementPreserved { path, detail } => {
                assert!(detail.contains("different file identity"), "{detail}");
                path
            }
            outcome => panic!("same-byte replacement must be preserved, got {outcome:?}"),
        };
        assert_eq!(
            fs::read(&stage.path).unwrap(),
            stage.raw.as_ref().unwrap().as_slice()
        );
        assert_eq!(
            fs::read(preserved).unwrap(),
            stage.raw.as_ref().unwrap().as_slice()
        );

        // Lexical aliases are rejected before serialization or stage creation.
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("alias");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let alias = local.parent().unwrap().join(".").join("server.json");
        let runfile = v2_runfile(&local, 95);
        let mut effects = ScriptedPersistenceEffects::default();
        assert!(matches!(
            publish_mirrored_with(&workspace, Some(&alias), &runfile, &mut effects),
            Err(PublishError::Invalid {
                scope: RegistrationScope::Global,
                ..
            })
        ));
        assert_eq!(effects.serializations, 0);
        assert!(effects.events.is_empty());
        assert!(publication_stage_paths(root.path()).is_empty());
    }

    #[test]
    fn publication_stage_cleanup_is_exact_and_failure_preserving() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("cleanup");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let runfile = v2_runfile(&local, 96);
        let mut effects = ScriptedPersistenceEffects::failing(&local, PersistencePhase::FileSync);
        let publication_error =
            publish_mirrored_with(&workspace, None, &runfile, &mut effects).unwrap_err();
        let stage = publication_attempt(&publication_error).stages[0].clone();

        let cleanup_error = remove_exact_bytes_if_unchanged_impl(
            &stage.path,
            stage.raw.as_ref().unwrap(),
            stage.identity.as_ref(),
            |_| {},
            |path| fs::read(path),
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "injected stage cleanup failure",
                ))
            },
            persist_bytes_noclobber,
        )
        .unwrap_err();
        assert_eq!(cleanup_error.kind, RemovalFailureKind::Remove);
        let preserved = cleanup_error
            .preserved_at
            .expect("failed cleanup retains an exact recovery path");
        assert_eq!(fs::read(&preserved).unwrap(), stage.raw.unwrap());
        assert!(!stage.path.exists());

        let uncaptured = PublicationStage {
            scope: RegistrationScope::Local,
            final_path: local.clone(),
            path: local
                .parent()
                .unwrap()
                .join(".server-registration-uncaptured"),
            raw: None,
            identity: None,
        };
        assert_eq!(
            remove_publication_stage_if_unchanged(&uncaptured).unwrap(),
            RemovalOutcome::Absent
        );
        assert!(!uncaptured.path.exists());
        fs::write(&uncaptured.path, b"unknown-stage").unwrap();
        let error = remove_publication_stage_if_unchanged(&uncaptured).unwrap_err();
        assert_eq!(error.preserved_at, Some(uncaptured.path.clone()));
        assert_eq!(fs::read(&uncaptured.path).unwrap(), b"unknown-stage");
    }

    #[test]
    fn mirrored_publish_is_identical_and_never_clobbers() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let local = absolute_path(&runfile_path(&workspace)).unwrap();
        let global = root.path().join("config").join("server.json");
        let runfile = v2_runfile(&local, 14);

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
        let local_only_runfile = v2_runfile(&local_only_path, 15);
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
        let runfile = v2_runfile(&local, 16);

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
    fn concurrent_lifecycle_operations_are_per_path_safe() {
        let root = tempfile::tempdir().unwrap();
        let workspace_a = root.path().join("workspace-a");
        let workspace_b = root.path().join("workspace-b");
        let local_a = absolute_path(&runfile_path(&workspace_a)).unwrap();
        let local_b = absolute_path(&runfile_path(&workspace_b)).unwrap();
        let global = root.path().join("config/server.json");
        let runfile_a = v2_runfile(&local_a, 60);
        let runfile_b = v2_runfile(&local_b, 61);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));

        let (result_a, result_b) = std::thread::scope(|scope| {
            let barrier_a = barrier.clone();
            let barrier_b = barrier.clone();
            let first_workspace = workspace_a.clone();
            let first_global = global.clone();
            let first_runfile = runfile_a.clone();
            let second_workspace = workspace_b.clone();
            let second_global = global.clone();
            let second_runfile = runfile_b.clone();
            let first = scope.spawn(move || {
                barrier_a.wait();
                publish_mirrored(&first_workspace, Some(&first_global), &first_runfile)
            });
            let second = scope.spawn(move || {
                barrier_b.wait();
                publish_mirrored(&second_workspace, Some(&second_global), &second_runfile)
            });
            barrier.wait();
            (first.join().unwrap(), second.join().unwrap())
        });

        let (winner, loser_workspace, loser_local) = match (result_a, result_b) {
            (Ok(winner), Err(PublishError::Mirror { local, .. })) => (winner, workspace_b, *local),
            (Err(PublishError::Mirror { local, .. }), Ok(winner)) => (winner, workspace_a, *local),
            outcomes => panic!("exactly one shared-global publisher must win: {outcomes:?}"),
        };
        assert_eq!(fs::read(&global).unwrap(), winner.local.raw);

        // Before attempt-owned compensation, the losing workspace observes a
        // typed local/global split rather than a local-first winner.
        let split = inventory_runfiles(&loser_workspace, Some(global.clone()));
        assert!(matches!(split.local, RegistrationSlot::Captured(_)));
        assert!(matches!(split.global, Some(RegistrationSlot::Captured(_))));
        assert!(matches!(
            select_unique(&split),
            Err(SelectionError::Conflict { .. })
        ));
        assert_eq!(
            remove_if_unchanged(&loser_local).unwrap(),
            RemovalOutcome::Removed
        );
        assert!(!loser_local.path.exists());
        assert_eq!(fs::read(&global).unwrap(), winner.local.raw);

        // Script a second cleanup while the first client holds the atomically
        // isolated entry. It must observe typed absence; the first remains the
        // sole remover. Real simultaneous process races remain for T-11706.
        let cleanup_path = root.path().join("cleanup/.ferric/server.json");
        write_runfile(&cleanup_path, &legacy_runfile(90));
        let cleanup_capture = captured(&capture_registration_path(
            RegistrationScope::Local,
            &cleanup_path,
        ))
        .clone();
        let nested_outcome = std::cell::RefCell::new(None);
        assert_eq!(
            remove_if_unchanged_impl(
                &cleanup_capture,
                |_| {
                    nested_outcome.replace(Some(remove_if_unchanged(&cleanup_capture).unwrap()));
                },
                |path| fs::read(path),
                |path| fs::remove_file(path),
                persist_bytes_noclobber,
            )
            .unwrap(),
            RemovalOutcome::Removed
        );
        assert_eq!(nested_outcome.into_inner(), Some(RemovalOutcome::Absent));

        assert_two_process_lifecycle_interleaving_is_per_path_safe();
    }

    fn assert_changed_entry_restore_success() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = write_runfile(&path, &legacy_runfile(2));

        let preserved = match remove_if_unchanged(&capture).unwrap() {
            RemovalOutcome::ReplacementPreserved { path, detail } => {
                assert!(detail.contains("restored without clobbering"), "{detail}");
                path
            }
            outcome => panic!("changed bytes must be preserved, got {outcome:?}"),
        };
        assert_eq!(fs::read(&path).unwrap(), replacement);
        assert_eq!(fs::read(preserved).unwrap(), replacement);
    }

    fn assert_replacement_after_atomic_move_is_preserved() {
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
            |path| fs::read(path),
            |path| fs::remove_file(path),
            persist_bytes_noclobber,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            RemovalOutcome::ReplacementPreserved { ref path, ref detail }
                if path == &capture.path && detail.contains("replacement appeared")
        ));
        assert_eq!(fs::read(&path).unwrap(), replacement);
    }

    fn assert_occupied_original_preserves_both_entries() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let changed = write_runfile(&path, &legacy_runfile(2));
        let concurrent = b"concurrent-original-winner".to_vec();

        let preserved = match remove_if_unchanged_impl(
            &capture,
            |original| fs::write(original, &concurrent).unwrap(),
            |path| fs::read(path),
            |path| fs::remove_file(path),
            persist_bytes_noclobber,
        )
        .unwrap()
        {
            RemovalOutcome::ReplacementPreserved { path, detail } => {
                assert!(detail.contains("concurrent entry occupies"), "{detail}");
                path
            }
            outcome => panic!("occupied original must preserve both entries: {outcome:?}"),
        };
        assert_eq!(fs::read(&path).unwrap(), concurrent);
        assert_eq!(fs::read(preserved).unwrap(), changed);
    }

    fn assert_changed_entry_restore_failure_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".ferric").join("server.json");
        write_runfile(&path, &legacy_runfile(1));
        let capture = captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
        let replacement = write_runfile(&path, &legacy_runfile(2));

        let error = remove_if_unchanged_impl(
            &capture,
            |_| {},
            |path| fs::read(path),
            |path| fs::remove_file(path),
            |_, _| {
                Err(PersistFailure {
                    kind: io::ErrorKind::PermissionDenied,
                    detail: "injected restore failure".to_string(),
                    committed: false,
                    phase: PersistencePhase::PersistNoClobber,
                    stage: None,
                })
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, RemovalFailureKind::Restore);
        assert!(error.detail.contains("could not restore changed entry"));
        let preserved = error.preserved_at.expect("moved bytes must be retained");
        assert_eq!(fs::read(preserved).unwrap(), replacement);
        assert!(!path.exists());
    }

    #[test]
    fn atomic_conditional_removal_matrix() {
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

        let parse_equal = root.path().join("parse-equal/.ferric/server.json");
        let record = legacy_runfile(2);
        write_runfile(&parse_equal, &record);
        let capture = captured(&capture_registration_path(
            RegistrationScope::Local,
            &parse_equal,
        ))
        .clone();
        let compact = serde_json::to_vec(&record).unwrap();
        fs::write(&parse_equal, &compact).unwrap();
        let preserved = match remove_if_unchanged(&capture).unwrap() {
            RemovalOutcome::ReplacementPreserved { path, .. } => path,
            outcome => panic!("parse-equal changed bytes must be preserved: {outcome:?}"),
        };
        assert_eq!(fs::read(&parse_equal).unwrap(), compact);
        assert_eq!(fs::read(preserved).unwrap(), compact);

        let nonregular = root.path().join("nonregular/.ferric/server.json");
        write_runfile(&nonregular, &legacy_runfile(3));
        let capture = captured(&capture_registration_path(
            RegistrationScope::Local,
            &nonregular,
        ))
        .clone();
        fs::remove_file(&nonregular).unwrap();
        fs::create_dir(&nonregular).unwrap();
        let error = remove_if_unchanged(&capture).unwrap_err();
        assert_eq!(error.kind, RemovalFailureKind::Other);
        assert_eq!(error.path, capture.path);
        assert!(error.detail.contains("not a regular non-symlink file"));
        assert!(error.preserved_at.is_none());
        assert!(nonregular.is_dir());

        let unreadable_path = PathBuf::from(format!(
            "{}\0unreadable/.ferric/server.json",
            root.path().display()
        ));
        let unreadable_runfile = legacy_runfile(4);
        let unreadable_capture = CapturedRegistration {
            scope: RegistrationScope::Local,
            path: unreadable_path.clone(),
            raw: serde_json::to_vec_pretty(&unreadable_runfile).unwrap(),
            runfile: unreadable_runfile,
        };
        let error = remove_if_unchanged(&unreadable_capture).unwrap_err();
        assert_eq!(error.kind, RemovalFailureKind::Other);
        assert_eq!(error.path, unreadable_path);
        assert!(error.detail.contains("inspect current entry"));
        assert!(error.preserved_at.is_none());

        for (label, fail_read) in [("read", true), ("remove", false)] {
            let path = root
                .path()
                .join(format!("{label}-failure/.ferric/server.json"));
            write_runfile(&path, &legacy_runfile(3));
            let capture =
                captured(&capture_registration_path(RegistrationScope::Local, &path)).clone();
            let result = if fail_read {
                remove_if_unchanged_impl(
                    &capture,
                    |_| {},
                    |_| {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "injected read",
                        ))
                    },
                    |path| fs::remove_file(path),
                    persist_bytes_noclobber,
                )
            } else {
                remove_if_unchanged_impl(
                    &capture,
                    |_| {},
                    |path| fs::read(path),
                    |_| {
                        Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "injected remove",
                        ))
                    },
                    persist_bytes_noclobber,
                )
            };
            let error = result.unwrap_err();
            assert_eq!(
                error.kind,
                if fail_read {
                    RemovalFailureKind::Other
                } else {
                    RemovalFailureKind::Remove
                }
            );
            assert!(error.detail.contains(if fail_read {
                "read atomically moved entry"
            } else {
                "remove unchanged moved entry"
            }));
            let holding = error
                .preserved_at
                .expect("failed entry remains recoverable");
            assert_eq!(fs::read(holding).unwrap(), capture.raw);
            assert!(!path.exists());
        }

        assert_replacement_after_atomic_move_is_preserved();
        assert_changed_entry_restore_success();
        assert_occupied_original_preserves_both_entries();
        assert_changed_entry_restore_failure_is_preserved();
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
                    phase: PersistencePhase::PersistNoClobber,
                    stage: None,
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
        assert_changed_entry_restore_success();
    }

    #[test]
    fn replacement_created_after_atomic_move_is_never_deleted() {
        assert_replacement_after_atomic_move_is_preserved();
    }

    #[test]
    fn changed_entry_restore_io_failure_is_error_with_preserved_bytes() {
        assert_changed_entry_restore_failure_is_preserved();
    }
}
