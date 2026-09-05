//! Workspace-scoped coordination and choice metadata, never session authority.
//!
//! All state effects use retained directory capabilities. The root lock is
//! persistent: removing a lock name is not an unlock operation. On Unix a
//! second lock on the workspace directory also prevents replacing the root
//! lock name from admitting another startup in that same directory.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cap_fs_ext::OpenOptionsExt as _;
#[cfg(unix)]
use cap_fs_ext::OpenOptionsMaybeDirExt as _;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, Metadata, OpenOptions};
use serde::{Deserialize, Serialize};

pub(super) const LOCK_FILE: &str = ".ferric-startup.lock";
const STATE_DIR: &str = ".ferric";
const PREFERENCE_FILE: &str = "startup-preference.json";
const MAX_PREFERENCE_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preference {
    pub(super) schema_version: u8,
    pub(super) model_path: Option<PathBuf>,
    pub(super) model_bytes: Option<u64>,
    pub(super) modified_nanos: Option<u128>,
    pub(super) endpoint: Option<String>,
    pub(super) model_id: Option<String>,
}

impl Preference {
    fn validate(&self) -> Result<(), String> {
        let valid = self.schema_version == 1
            && match (&self.model_path, &self.endpoint, &self.model_id) {
                (Some(path), None, None) => {
                    path.is_absolute() && self.model_bytes.is_some_and(|size| size > 0)
                }
                (None, Some(endpoint), Some(model)) => {
                    self.model_bytes.is_none()
                        && self.modified_nanos.is_none()
                        && endpoint.len() <= 2048
                        && ["http://", "https://"].into_iter().any(|scheme| {
                            endpoint
                                .strip_prefix(scheme)
                                .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
                        })
                        && !endpoint.chars().any(|character| {
                            character.is_control()
                                || character.is_whitespace()
                                || matches!(character, '@' | '?' | '#' | '\\')
                        })
                        && !model.trim().is_empty()
                        && model.len() <= 512
                        && !model.chars().any(char::is_control)
                }
                _ => false,
            };
        if valid {
            Ok(())
        } else {
            Err("saved model choice is invalid; select a model again".into())
        }
    }
}

type Identity = (u64, u64);

/// Keep this owner alive until foreground process cleanup has been proved.
/// Dropping it closes crash-released locks; no path is deleted by Drop.
pub(super) struct WorkspaceState {
    root_path: PathBuf,
    root: Dir,
    root_identity: Identity,
    directory: Dir,
    directory_identity: Identity,
    lock: File,
    lock_identity: Identity,
    preference: Mutex<Snapshot>,
    trace_directory: Mutex<Option<(Dir, Identity)>>,
    #[cfg(unix)]
    _directory_lock: File,
}

enum Snapshot {
    Absent,
    Present {
        // Retaining the file prevents inode reuse from hiding replacement.
        file: cap_std::fs::File,
        identity: Identity,
        bytes: Vec<u8>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WritePhase {
    Staged,
    BeforePublish,
    Published,
}

impl WorkspaceState {
    pub(super) fn acquire(workspace: &Path) -> Result<Self, String> {
        let root_path = workspace
            .canonicalize()
            .map_err(|_| "cannot identify the selected workspace")?;
        let root = open_root(&root_path)?;
        let root_identity = identity(&directory_metadata(&root)?);

        // cap-std may retain O_PATH directories on Linux. Open a separate
        // read-capable directory handle for flock, then prove its identity.
        #[cfg(unix)]
        let directory_lock = {
            let file = readable_directory(&root)?;
            if identity(&Metadata::from_file(&file).map_err(|_| "cannot identify workspace lock")?)
                != root_identity
            {
                return Err("selected workspace changed while acquiring its lock".into());
            }
            file.try_lock()
                .map_err(|_| "another startup owns this workspace, or its lock is unavailable")?;
            file
        };

        validate_root(&root_path, &root, root_identity)?;
        let lock = open_lock(&root)?.into_std();
        let lock_identity = identity(
            &Metadata::from_file(&lock)
                .map_err(|_| "cannot identify the workspace startup lock")?,
        );
        validate_root(&root_path, &root, root_identity)?;
        validate_file(&root, LOCK_FILE, &lock, lock_identity)?;
        lock.try_lock()
            .map_err(|_| "another startup owns this workspace, or its lock is unavailable")?;
        validate_root(&root_path, &root, root_identity)?;
        validate_file(&root, LOCK_FILE, &lock, lock_identity)?;

        // Only initial acquisition may create .ferric. Validation must never
        // repair a renamed/deleted directory while the old owner is live.
        match root.symlink_metadata(STATE_DIR) {
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                validate_root(&root_path, &root, root_identity)?;
                validate_file(&root, LOCK_FILE, &lock, lock_identity)?;
                match root.create_dir(STATE_DIR) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(_) => return Err("cannot create workspace startup state".into()),
                }
            }
            Err(_) => return Err("cannot inspect workspace startup state".into()),
        }
        let directory = open_plain_dir(&root, STATE_DIR)?;
        let directory_identity = identity(&directory_metadata(&directory)?);
        let preference = Mutex::new(observe_preference(&directory)?);
        let state = Self {
            root_path,
            root,
            root_identity,
            directory,
            directory_identity,
            lock,
            lock_identity,
            preference,
            trace_directory: Mutex::new(None),
            #[cfg(unix)]
            _directory_lock: directory_lock,
        };
        state.validate()?;
        Ok(state)
    }

    /// Also verifies the exact preference observed at acquisition/last write.
    /// Call immediately before resource launch and other preparation effects.
    pub(super) fn validate(&self) -> Result<(), String> {
        let expected = self
            .preference
            .lock()
            .map_err(|_| "workspace preference coordination is unavailable")?;
        self.validate_snapshot(&expected)
    }

    pub(super) fn read_preference(&self) -> Result<Option<Preference>, String> {
        let expected = self
            .preference
            .lock()
            .map_err(|_| "workspace preference coordination is unavailable")?;
        self.validate_snapshot(&expected)?;
        match &*expected {
            Snapshot::Absent => Ok(None),
            Snapshot::Present { bytes, .. } => {
                let preference: Preference = serde_json::from_slice(bytes)
                    .map_err(|_| "saved model choice is malformed; select a model again")?;
                preference.validate()?;
                Ok(Some(preference))
            }
        }
    }

    pub(super) fn write_preference(&self, preference: &Preference) -> Result<(), String> {
        self.write_preference_with(preference, |_| Ok(()))
    }

    /// Exclusively create one bounded human-session trace basename beneath the
    /// pinned `.ferric/trace` directory. The caller writes via the returned
    /// file handle, never by reopening the informational absolute path.
    pub(super) fn create_trace(&self, name: &str) -> Result<(PathBuf, File), String> {
        if name.len() > 96
            || !(name.starts_with("human-") || name.starts_with("q-"))
            || !name.ends_with(".jsonl")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("session trace requires a bounded human or query basename".into());
        }
        self.validate()?;
        let mut pinned = self
            .trace_directory
            .lock()
            .map_err(|_| "session trace coordination is unavailable")?;
        if pinned.is_none() {
            self.validate_base_bindings()?;
            match self.directory.symlink_metadata("trace") {
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    self.validate_base_bindings()?;
                    match self.directory.create_dir("trace") {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                        Err(_) => return Err("cannot create the session trace directory".into()),
                    }
                }
                Err(_) => return Err("cannot inspect the session trace directory".into()),
            }
            let directory = open_plain_dir(&self.directory, "trace")?;
            let expected = identity(&directory_metadata(&directory)?);
            *pinned = Some((directory, expected));
        }
        let (directory, expected) = pinned
            .as_ref()
            .ok_or("session trace directory is unavailable")?;
        self.validate_base_bindings()?;
        self.validate_trace_directory(directory, *expected)?;
        let mut options = file_options();
        options.write(true).create_new(true);
        let file = directory
            .open_with(name, &options)
            .map_err(|_| "cannot exclusively create the session trace")?
            .into_std();
        let file_identity = identity(
            &Metadata::from_file(&file).map_err(|_| "cannot identify the new session trace")?,
        );
        validate_file(directory, name, &file, file_identity)?;
        self.validate_trace_directory(directory, *expected)?;
        self.validate_base_bindings()?;
        drop(pinned);
        self.validate()?;
        Ok((
            self.root_path.join(STATE_DIR).join("trace").join(name),
            file,
        ))
    }

    fn write_preference_with(
        &self,
        preference: &Preference,
        mut checkpoint: impl FnMut(WritePhase) -> Result<(), String>,
    ) -> Result<(), String> {
        preference.validate()?;
        let bytes = serde_json::to_vec(preference)
            .map_err(|_| "cannot encode the selected model choice")?;
        if bytes.len() > MAX_PREFERENCE_BYTES {
            return Err("selected model choice exceeds the storage limit".into());
        }
        let mut expected = self
            .preference
            .lock()
            .map_err(|_| "workspace preference coordination is unavailable")?;
        self.validate_snapshot(&expected)?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| "cannot allocate model preference staging")?;
        let stage_name = format!(".startup-preference-{}.tmp", hex::encode(random));
        let mut options = file_options();
        options.write(true).create_new(true);
        self.validate_snapshot(&expected)?;
        let mut stage = self
            .directory
            .open_with(&stage_name, &options)
            .map_err(|_| "cannot stage the selected model choice")?
            .into_std();
        let stage_identity = identity(
            &Metadata::from_file(&stage).map_err(|_| "cannot inspect model preference staging")?,
        );
        self.validate_snapshot(&expected)?;
        validate_file(&self.directory, &stage_name, &stage, stage_identity)?;
        stage
            .write_all(&bytes)
            .map_err(|_| "cannot write model preference staging")?;
        self.validate_bindings()?;
        validate_file(&self.directory, &stage_name, &stage, stage_identity)?;
        stage
            .sync_all()
            .map_err(|_| "cannot synchronize model preference staging")?;
        checkpoint(WritePhase::Staged)?;
        checkpoint(WritePhase::BeforePublish)?;
        self.validate_snapshot(&expected)?;
        validate_file(&self.directory, &stage_name, &stage, stage_identity)?;
        let staged = observe_file(&self.directory, &stage_name)?;
        if !matches!(&staged, Snapshot::Present { identity, bytes: current, .. }
            if *identity == stage_identity && current == &bytes)
        {
            return Err("model preference staging changed before publication".into());
        }

        // Both names are single components under the pinned directory. Native
        // rename replaces the leaf itself and never writes through a symlink.
        // The workspace lock serializes cooperating publishers. An interrupted
        // pre-publication attempt may retain one bounded staging file; we do
        // not unlink a name whose identity another writer could replace.
        self.directory
            .rename(&stage_name, &self.directory, PREFERENCE_FILE)
            .map_err(|_| "cannot publish the selected model choice")?;
        checkpoint(WritePhase::Published)?;
        self.validate_bindings()?;
        validate_file(&self.directory, PREFERENCE_FILE, &stage, stage_identity)?;
        let published = observe_preference(&self.directory)?;
        if !matches!(&published, Snapshot::Present { identity, bytes: current, .. }
            if *identity == stage_identity && current == &bytes)
        {
            return Err("model preference changed during publication".into());
        }
        #[cfg(unix)]
        {
            // Unlike the retained O_PATH capability, this is a readable
            // directory handle suitable for synchronizing the rename.
            let sync_dir = readable_directory(&self.directory)?;
            self.validate_bindings()?;
            sync_dir
                .sync_all()
                .map_err(|_| "cannot synchronize the published model choice")?;
        }
        self.validate_snapshot(&published)?;
        validate_file(&self.directory, PREFERENCE_FILE, &stage, stage_identity)?;
        *expected = published;
        Ok(())
    }

    fn validate_bindings(&self) -> Result<(), String> {
        self.validate_base_bindings()?;
        let pinned = self
            .trace_directory
            .lock()
            .map_err(|_| "session trace coordination is unavailable")?;
        if let Some((directory, expected)) = &*pinned {
            self.validate_trace_directory(directory, *expected)?;
        }
        Ok(())
    }

    fn validate_trace_directory(&self, directory: &Dir, expected: Identity) -> Result<(), String> {
        let current = open_plain_dir(&self.directory, "trace")?;
        if identity(&directory_metadata(&current)?) != expected
            || identity(&directory_metadata(directory)?) != expected
        {
            return Err("session trace directory changed; restart setup".into());
        }
        Ok(())
    }

    fn validate_base_bindings(&self) -> Result<(), String> {
        validate_root(&self.root_path, &self.root, self.root_identity)?;
        validate_file(&self.root, LOCK_FILE, &self.lock, self.lock_identity)?;
        let current = open_plain_dir(&self.root, STATE_DIR)?;
        if identity(&directory_metadata(&current)?) != self.directory_identity
            || identity(&directory_metadata(&self.directory)?) != self.directory_identity
        {
            return Err("workspace startup directory changed; restart setup".into());
        }
        Ok(())
    }

    fn validate_snapshot(&self, expected: &Snapshot) -> Result<(), String> {
        self.validate_bindings()?;
        let observed = observe_preference(&self.directory)?;
        let same = match (expected, &observed) {
            (Snapshot::Absent, Snapshot::Absent) => true,
            (
                Snapshot::Present {
                    file,
                    identity: prior,
                    bytes: prior_bytes,
                },
                Snapshot::Present {
                    identity: current,
                    bytes: current_bytes,
                    ..
                },
            ) => {
                let retained = file
                    .metadata()
                    .map_err(|_| "cannot inspect saved model choice")?;
                regular_single_link(&retained)
                    && identity(&retained) == *prior
                    && prior == current
                    && prior_bytes == current_bytes
            }
            _ => false,
        };
        if !same {
            return Err("saved model choice changed during setup; restart setup".into());
        }
        self.validate_bindings()
    }
}

fn identity(metadata: &Metadata) -> Identity {
    (MetadataExt::dev(metadata), MetadataExt::ino(metadata))
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    if cap_fs_ext::OsMetadataExt::file_attributes(metadata) & 0x400 != 0 {
        // FILE_ATTRIBUTE_REPARSE_POINT, including non-symlink reparse tags.
        return true;
    }
    metadata.file_type().is_symlink()
}

fn regular_single_link(metadata: &Metadata) -> bool {
    metadata.is_file() && !is_link(metadata) && MetadataExt::nlink(metadata) == 1
}

fn directory_metadata(directory: &Dir) -> Result<Metadata, String> {
    let metadata = directory
        .dir_metadata()
        .map_err(|_| "cannot identify workspace startup directory")?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err("workspace startup state requires a plain directory".into());
    }
    Ok(metadata)
}

#[cfg(unix)]
fn readable_directory(directory: &Dir) -> Result<File, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    // SAFETY: the retained directory descriptor is live, "." is a static
    // terminated name, and openat returns a new descriptor owned only here.
    // Explicit O_RDONLY avoids cap-std's O_PATH optimization for directories,
    // since flock and fsync require a readable descriptor on Linux.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err("cannot open the retained startup directory for coordination".into());
    }
    // SAFETY: a successful openat returned a uniquely owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata =
        Metadata::from_file(&file).map_err(|_| "cannot identify the reopened startup directory")?;
    if !metadata.is_dir() || identity(&metadata) != identity(&directory_metadata(directory)?) {
        return Err("reopened startup directory has changed identity".into());
    }
    Ok(file)
}

fn open_root(path: &Path) -> Result<Dir, String> {
    if path
        .canonicalize()
        .map_err(|_| "selected workspace is unavailable")?
        != path
    {
        return Err("selected workspace binding changed".into());
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(leaf)) => {
            let parent = Dir::open_ambient_dir(parent, ambient_authority())
                .map_err(|_| "cannot open selected workspace parent")?;
            open_plain_dir(&parent, leaf)
        }
        _ => Dir::open_ambient_dir(path, ambient_authority())
            .map_err(|_| "cannot open selected workspace".into()),
    }
}

fn validate_root(path: &Path, retained: &Dir, expected: Identity) -> Result<(), String> {
    if identity(&directory_metadata(&open_root(path)?)?) != expected
        || identity(&directory_metadata(retained)?) != expected
    {
        return Err("selected workspace identity changed; restart setup".into());
    }
    Ok(())
}

fn open_plain_dir(parent: &Dir, leaf: impl AsRef<Path>) -> Result<Dir, String> {
    let leaf = leaf.as_ref();
    let before = parent
        .symlink_metadata(leaf)
        .map_err(|_| "workspace startup directory is unavailable")?;
    if !before.is_dir() || is_link(&before) {
        return Err("workspace startup state requires a plain directory".into());
    }
    let opened = parent
        .open_dir_nofollow(leaf)
        .map_err(|_| "cannot safely open workspace startup directory")?;
    let after = parent
        .symlink_metadata(leaf)
        .map_err(|_| "workspace startup directory changed while opening")?;
    if !after.is_dir()
        || is_link(&after)
        || identity(&before) != identity(&after)
        || identity(&before) != identity(&directory_metadata(&opened)?)
    {
        return Err("workspace startup directory changed while opening".into());
    }
    Ok(opened)
}

fn file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    options
        .maybe_dir(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .mode(0o600);
    options
}

fn open_lock(root: &Dir) -> Result<cap_std::fs::File, String> {
    let mut options = file_options();
    options.write(true);
    // Windows must refuse rename/unlink of the lock while it is held.
    #[cfg(windows)]
    options.share_mode(0x1 | 0x2); // FILE_SHARE_READ | FILE_SHARE_WRITE
    match root.symlink_metadata(LOCK_FILE) {
        Ok(metadata) if regular_single_link(&metadata) => {}
        Ok(_) => return Err("workspace startup lock must be a regular file with one link".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let mut create = options.clone();
            create.create_new(true);
            match root.open_with(LOCK_FILE, &create) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(_) => return Err("cannot create workspace startup lock".into()),
            }
        }
        Err(_) => return Err("cannot inspect workspace startup lock".into()),
    }
    root.open_with(LOCK_FILE, &options)
        .map_err(|_| "cannot safely open workspace startup lock".into())
}

fn validate_file(dir: &Dir, name: &str, file: &File, expected: Identity) -> Result<(), String> {
    let opened = Metadata::from_file(file).map_err(|_| "cannot inspect retained startup file")?;
    let current = dir
        .symlink_metadata(name)
        .map_err(|_| "startup file binding changed")?;
    if !regular_single_link(&opened)
        || !regular_single_link(&current)
        || identity(&opened) != expected
        || identity(&current) != expected
    {
        return Err("startup file must retain its regular, single-link identity".into());
    }
    Ok(())
}

fn observe_preference(directory: &Dir) -> Result<Snapshot, String> {
    observe_file(directory, PREFERENCE_FILE)
}

fn observe_file(directory: &Dir, name: &str) -> Result<Snapshot, String> {
    let before = match directory.symlink_metadata(name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Snapshot::Absent),
        Err(_) => return Err("cannot inspect saved model choice".into()),
    };
    if !regular_single_link(&before) || before.len() > MAX_PREFERENCE_BYTES as u64 {
        return Err("saved model choice must be a bounded regular file with one link".into());
    }
    let mut file = directory
        .open_with(name, &file_options())
        .map_err(|_| "cannot safely open saved model choice")?
        .into_std();
    let expected = identity(&before);
    validate_file(directory, name, &file, expected)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take((MAX_PREFERENCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read saved model choice")?;
    if bytes.len() > MAX_PREFERENCE_BYTES {
        return Err("saved model choice exceeds the storage limit".into());
    }
    validate_file(directory, name, &file, expected)?;
    let after = Metadata::from_file(&file).map_err(|_| "cannot revalidate saved model choice")?;
    if before.len() != bytes.len() as u64
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err("saved model choice changed while reading".into());
    }
    Ok(Snapshot::Present {
        file: cap_std::fs::File::from_std(file),
        identity: expected,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_preference(workspace: &Path, name: &str) -> Preference {
        Preference {
            schema_version: 1,
            model_path: Some(workspace.canonicalize().unwrap().join(name)),
            model_bytes: Some(4096),
            modified_nanos: Some(123),
            endpoint: None,
            model_id: None,
        }
    }

    #[test]
    fn startup_concurrent_invocations_serialize() {
        let workspace = tempfile::tempdir().unwrap();
        let first = WorkspaceState::acquire(workspace.path()).unwrap();
        let original_lock = first.lock_identity;
        std::thread::scope(|scope| {
            let second = scope.spawn(|| WorkspaceState::acquire(workspace.path()).is_err());
            assert!(second.join().unwrap());
        });
        first.validate().unwrap();
        drop(first);
        let next = WorkspaceState::acquire(workspace.path()).unwrap();
        assert_eq!(next.lock_identity, original_lock);
        drop(next);
        assert!(workspace.path().join(LOCK_FILE).is_file());
    }

    #[test]
    fn preferences_atomic_and_symlink_safe() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let first = local_preference(workspace.path(), "first.gguf");
        let next = local_preference(workspace.path(), "next.gguf");
        assert_eq!(state.read_preference().unwrap(), None);
        state.write_preference(&first).unwrap();
        let result = state.write_preference_with(&next, |phase| {
            if phase == WritePhase::Staged {
                Err("injected pre-publication interruption".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(state.read_preference().unwrap(), Some(first));
        state.write_preference(&next).unwrap();
        assert_eq!(state.read_preference().unwrap(), Some(next.clone()));
        let outside = workspace.path().join("outside.json");
        std::fs::write(&outside, b"outside sentinel").unwrap();
        let preference_path = workspace.path().join(STATE_DIR).join(PREFERENCE_FILE);
        let result = state.write_preference_with(&next, |phase| {
            if phase == WritePhase::BeforePublish {
                std::fs::remove_file(&preference_path).unwrap();
                symlink_file(&outside, &preference_path);
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"outside sentinel");
        assert!(
            std::fs::symlink_metadata(preference_path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn preference_replacement_after_publish_is_refused_without_cleanup_of_replacement() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let preference = local_preference(workspace.path(), "model.gguf");
        let path = workspace.path().join(STATE_DIR).join(PREFERENCE_FILE);
        let result = state.write_preference_with(&preference, |phase| {
            if phase == WritePhase::Published {
                std::fs::remove_file(&path).unwrap();
                std::fs::write(&path, b"concurrent replacement").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"concurrent replacement");
    }

    #[test]
    fn changed_staging_bytes_leave_the_prior_preference_readable() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let first = local_preference(workspace.path(), "first.gguf");
        let next = local_preference(workspace.path(), "next.gguf");
        state.write_preference(&first).unwrap();
        let result = state.write_preference_with(&next, |phase| {
            if phase == WritePhase::Staged {
                let stage = std::fs::read_dir(workspace.path().join(STATE_DIR))
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                    .unwrap();
                std::fs::write(stage.path(), b"changed staged choice").unwrap();
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(state.read_preference().unwrap(), Some(first));
    }

    #[test]
    fn preference_changes_and_hardlinks_are_refused() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let preference = local_preference(workspace.path(), "model.gguf");
        state.write_preference(&preference).unwrap();
        let path = workspace.path().join(STATE_DIR).join(PREFERENCE_FILE);
        let alias = workspace.path().join("preference-alias.json");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(state.validate().is_err());
        assert!(state.write_preference(&preference).is_err());
        std::fs::remove_file(alias).unwrap();
        state.validate().unwrap();
        std::fs::write(path, b"changed without changing the inode").unwrap();
        assert!(state.validate().is_err());
    }

    #[test]
    fn lock_hardlinks_and_symlinks_are_refused() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = workspace.path().join("outside-lock");
        let lock = workspace.path().join(LOCK_FILE);
        std::fs::write(&outside, b"do not truncate").unwrap();
        std::fs::hard_link(&outside, &lock).unwrap();
        assert!(WorkspaceState::acquire(workspace.path()).is_err());
        assert!(!workspace.path().join(STATE_DIR).exists());
        std::fs::remove_file(&lock).unwrap();
        symlink_file(&outside, &lock);
        assert!(WorkspaceState::acquire(workspace.path()).is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"do not truncate");
    }

    #[test]
    fn trace_creation_is_exclusive_and_bound_to_the_pinned_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        assert!(state.create_trace("human-../../outside.jsonl").is_err());
        assert!(!workspace.path().join(STATE_DIR).join("trace").exists());
        let (path, mut file) = state.create_trace("human-test.jsonl").unwrap();
        file.write_all(b"bounded fixture\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"bounded fixture\n");
        assert!(state.create_trace("human-test.jsonl").is_err());
        let outside = workspace.path().join("outside-trace");
        std::fs::write(&outside, b"unchanged").unwrap();
        let alias = path.with_file_name("q-linked.jsonl");
        symlink_file(&outside, &alias);
        assert!(state.create_trace("q-linked.jsonl").is_err());
        assert_eq!(std::fs::read(outside).unwrap(), b"unchanged");

        let directory = workspace.path().join(STATE_DIR).join("trace");
        let result = std::fs::rename(&directory, workspace.path().join("old-trace"));
        #[cfg(windows)]
        assert!(result.is_err());
        #[cfg(unix)]
        {
            result.unwrap();
            assert!(state.validate().is_err());
            assert!(state.create_trace("human-next.jsonl").is_err());
            assert!(!directory.exists());
        }
    }

    #[test]
    fn initial_state_directory_symlink_is_refused() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let directory = workspace.path().join(STATE_DIR);
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), directory).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(outside.path(), directory).unwrap();
        assert!(WorkspaceState::acquire(workspace.path()).is_err());
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[test]
    fn malformed_oversized_and_authority_bearing_choices_are_refused() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let mut preference = local_preference(workspace.path(), "model.gguf");
        preference.endpoint = Some("http://127.0.0.1:1234/v1".into());
        preference.model_id = Some("model".into());
        assert!(state.write_preference(&preference).is_err());
        preference.model_path = None;
        preference.model_bytes = None;
        preference.modified_nanos = None;
        state.write_preference(&preference).unwrap();
        assert_eq!(state.read_preference().unwrap(), Some(preference.clone()));
        preference.endpoint = Some("http://user:secret@127.0.0.1:1234/v1".into());
        assert!(state.write_preference(&preference).is_err());
        drop(state);
        let path = workspace.path().join(STATE_DIR).join(PREFERENCE_FILE);
        std::fs::write(&path, b"secret malformed input").unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let error = state.read_preference().unwrap_err();
        assert!(!error.contains("secret"));
        drop(state);
        std::fs::write(path, vec![b' '; MAX_PREFERENCE_BYTES + 1]).unwrap();
        assert!(WorkspaceState::acquire(workspace.path()).is_err());
    }

    #[test]
    fn startup_state_directory_cannot_be_replaced_during_a_session() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let directory = workspace.path().join(STATE_DIR);
        let moved = workspace.path().join("moved-state");
        let moved_result = std::fs::rename(&directory, moved);
        #[cfg(windows)]
        {
            // The pinned Windows capability excludes FILE_SHARE_DELETE.
            assert!(moved_result.is_err());
            state.validate().unwrap();
        }
        #[cfg(unix)]
        {
            moved_result.unwrap();
            assert!(state.validate().is_err());
            assert!(WorkspaceState::acquire(workspace.path()).is_err());
            assert!(!directory.exists());
            std::fs::create_dir(&directory).unwrap();
            assert!(state.validate().is_err());
            assert!(WorkspaceState::acquire(workspace.path()).is_err());
            assert!(std::fs::read_dir(directory).unwrap().next().is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn replaced_root_lock_cannot_admit_a_second_startup() {
        let workspace = tempfile::tempdir().unwrap();
        let state = WorkspaceState::acquire(workspace.path()).unwrap();
        let lock = workspace.path().join(LOCK_FILE);
        std::fs::remove_file(&lock).unwrap();
        assert!(state.validate().is_err());
        assert!(WorkspaceState::acquire(workspace.path()).is_err());
        assert!(!lock.exists());
        drop(state);
        WorkspaceState::acquire(workspace.path()).unwrap();
    }

    #[cfg(unix)]
    fn symlink_file(target: &Path, path: &Path) {
        std::os::unix::fs::symlink(target, path).unwrap();
    }

    #[cfg(windows)]
    fn symlink_file(target: &Path, path: &Path) {
        std::os::windows::fs::symlink_file(target, path).unwrap();
    }
}
