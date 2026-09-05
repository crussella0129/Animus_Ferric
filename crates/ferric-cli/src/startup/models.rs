//! Bounded local discovery. A GGUF header is format metadata, not a fit claim.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, Metadata, OpenOptions};

use super::storage::Preference;
use super::{ModelChoice, StartupError};

const ENTRY_LIMIT: usize = 256;
const MODEL_LIMIT: usize = 128;

pub(super) struct LocalModel {
    pub(super) choice: ModelChoice,
    // Retain the file so inode reuse cannot conceal a replacement.
    file: cap_std::fs::File,
    identity: (u64, u64),
    modified_nanos: Option<u128>,
    discovered: Option<Arc<DiscoveredDirectory>>,
}

impl LocalModel {
    pub(super) fn open(path: &Path) -> Result<Self, StartupError> {
        let original = std::fs::symlink_metadata(path).map_err(|_| {
            StartupError::resource(
                "The selected model cannot be read. Select an existing GGUF file.",
            )
        })?;
        if !original.is_file() || original.file_type().is_symlink() {
            return Err(StartupError::resource(
                "The selected model must be a regular, non-symlink GGUF file.",
            ));
        }
        // Resolve only the caller-authorized parent. Canonicalizing the leaf
        // could follow a symlink substituted after the preliminary type check.
        let path = std::path::absolute(path).map_err(|_| changed())?;
        let parent = path
            .parent()
            .ok_or_else(changed)?
            .canonicalize()
            .map_err(|_| changed())?;
        let path = parent.join(path.file_name().ok_or_else(changed)?);
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        {
            return Err(StartupError::resource(
                "Select a model with the .gguf extension.",
            ));
        }
        let directory = Dir::open_ambient_dir(
            path.parent()
                .ok_or_else(|| StartupError::resource("The model has no parent directory."))?,
            cap_std::ambient_authority(),
        )
        .map_err(|_| StartupError::resource("The model directory cannot be opened."))?;
        Self::open_in(&directory, path, None)
    }

    fn open_in(
        directory: &Dir,
        path: PathBuf,
        discovered: Option<Arc<DiscoveredDirectory>>,
    ) -> Result<Self, StartupError> {
        if let Some(binding) = &discovered {
            binding.validate()?;
        }
        let name = path
            .file_name()
            .ok_or_else(|| StartupError::resource("The model filename is invalid."))?;
        let before = directory.symlink_metadata(name).map_err(|_| changed())?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_fs_ext::OpenOptionsExt as _;
            options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use cap_fs_ext::OpenOptionsExt as _;
            // Keep the selected bytes/path stable while this foreground session
            // owns them. Existing writers make admission fail closed.
            options.share_mode(1); // FILE_SHARE_READ
        }
        let mut file = directory.open_with(name, &options).map_err(|_| changed())?;
        let opened = file.metadata().map_err(|_| changed())?;
        let after = directory.symlink_metadata(name).map_err(|_| changed())?;
        if !same(&before, &opened) || !same(&opened, &after) || opened.len() < 24 {
            return Err(changed());
        }
        let mut header = [0_u8; 24];
        file.read_exact(&mut header)
            .map_err(|_| StartupError::resource("The selected GGUF header is incomplete."))?;
        let version = u32::from_le_bytes(header[4..8].try_into().expect("four bytes"));
        if &header[..4] != b"GGUF" || !matches!(version, 2 | 3) {
            return Err(StartupError::resource(
                "The selected file is not a supported GGUF version 2 or 3 model.",
            ));
        }
        let label = name
            .to_str()
            .filter(|name| !name.chars().any(char::is_control))
            .ok_or_else(|| {
                StartupError::resource(
                    "The model filename must be valid text without control characters.",
                )
            })?
            .to_owned();
        let model = Self {
            choice: ModelChoice {
                label,
                bytes: Some(opened.len()),
                path: Some(path),
            },
            identity: identity(&opened),
            modified_nanos: modified(&opened),
            file,
            discovered,
        };
        model.validate()?;
        Ok(model)
    }

    pub(super) fn validate(&self) -> Result<(), StartupError> {
        let path = self.choice.path.as_ref().expect("local path");
        let explicit_directory;
        let directory = if let Some(binding) = &self.discovered {
            binding.validate()?;
            &binding.directory
        } else {
            explicit_directory =
                Dir::open_ambient_dir(path.parent().expect("parent"), cap_std::ambient_authority())
                    .map_err(|_| changed())?;
            &explicit_directory
        };
        let current = directory
            .symlink_metadata(path.file_name().expect("filename"))
            .map_err(|_| changed())?;
        let held = self.file.metadata().map_err(|_| changed())?;
        if !same(&current, &held)
            || identity(&held) != self.identity
            || Some(held.len()) != self.choice.bytes
            || modified(&held) != self.modified_nanos
        {
            return Err(changed());
        }
        if let Some(binding) = &self.discovered {
            binding.validate()?;
        }
        Ok(())
    }

    pub(super) fn preference(&self) -> Preference {
        Preference {
            schema_version: 1,
            model_path: self.choice.path.clone(),
            model_bytes: self.choice.bytes,
            modified_nanos: self.modified_nanos,
            endpoint: None,
            model_id: None,
        }
    }

    pub(super) fn matches(&self, preference: &Preference) -> bool {
        self.preference() == *preference
    }
}

pub(super) fn scan(
    workspace: &Path,
    explicit: Option<&Path>,
) -> Result<Vec<LocalModel>, StartupError> {
    if let Some(path) = explicit {
        let selected = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        };
        return Ok(vec![LocalModel::open(&selected)?]);
    }
    scan_discovered(workspace, |_| Ok(()))
}

fn scan_discovered(
    workspace: &Path,
    checkpoint: impl FnOnce(&DiscoveredDirectory) -> Result<(), StartupError>,
) -> Result<Vec<LocalModel>, StartupError> {
    let Some(binding) = DiscoveredDirectory::open(workspace)? else {
        return Ok(Vec::new());
    };
    scan_binding(binding, checkpoint)
}

fn scan_binding(
    binding: DiscoveredDirectory,
    checkpoint: impl FnOnce(&DiscoveredDirectory) -> Result<(), StartupError>,
) -> Result<Vec<LocalModel>, StartupError> {
    let binding = Arc::new(binding);
    let mut names = Vec::new();
    for (index, entry) in binding
        .directory
        .entries()
        .map_err(|_| changed())?
        .enumerate()
    {
        if index >= ENTRY_LIMIT {
            return Err(StartupError::resource(
                "The models directory exceeds 256 entries. Select one model explicitly.",
            ));
        }
        let entry = entry.map_err(|_| changed())?;
        let name = entry.file_name();
        if !Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        {
            continue;
        }
        let metadata = binding
            .directory
            .symlink_metadata(&name)
            .map_err(|_| changed())?;
        if !metadata.is_file() || is_link(&metadata) {
            return Err(StartupError::resource(
                "A discovered GGUF is not a regular, non-symlink file. Select a safe model explicitly.",
            ));
        }
        names.push(name);
        if names.len() > MODEL_LIMIT {
            return Err(StartupError::resource(
                "The models directory exceeds 128 GGUF files. Select one model explicitly.",
            ));
        }
    }
    names.sort();
    checkpoint(&binding)?;
    binding.validate()?;
    let models = names
        .into_iter()
        .map(|name| {
            // The absolute path is informational/engine argv only. Admission opens
            // this single-component leaf through the retained directory, never by
            // canonicalizing an ambient path assembled from a stale enumeration.
            LocalModel::open_in(
                &binding.directory,
                binding.root_path.join("models").join(name),
                Some(Arc::clone(&binding)),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    binding.validate()?;
    Ok(models)
}

/// Automatic discovery retains the exact workspace and models directory for
/// every selected file's lifetime. A directory replacement is neither a new
/// search root nor authorization to admit a model outside the selected root.
struct DiscoveredDirectory {
    root_path: PathBuf,
    root: Dir,
    root_identity: (u64, u64),
    directory: Dir,
    directory_identity: (u64, u64),
}

impl DiscoveredDirectory {
    fn open(workspace: &Path) -> Result<Option<Self>, StartupError> {
        let root_path = workspace.canonicalize().map_err(|_| changed())?;
        let root = open_root(&root_path)?;
        let root_identity = identity(&plain_directory_metadata(&root)?);
        match root.symlink_metadata("models") {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(changed()),
            Ok(_) => {}
        }
        let directory = open_plain_directory(&root, Path::new("models"))?;
        let directory_identity = identity(&plain_directory_metadata(&directory)?);
        let binding = Self {
            root_path,
            root,
            root_identity,
            directory,
            directory_identity,
        };
        binding.validate()?;
        Ok(Some(binding))
    }

    fn validate(&self) -> Result<(), StartupError> {
        let current_root = open_root(&self.root_path)?;
        if identity(&plain_directory_metadata(&current_root)?) != self.root_identity
            || identity(&plain_directory_metadata(&self.root)?) != self.root_identity
        {
            return Err(changed());
        }
        let current = open_plain_directory(&self.root, Path::new("models"))?;
        if identity(&plain_directory_metadata(&current)?) != self.directory_identity
            || identity(&plain_directory_metadata(&self.directory)?) != self.directory_identity
        {
            return Err(changed());
        }
        Ok(())
    }
}

fn open_root(path: &Path) -> Result<Dir, StartupError> {
    if path.canonicalize().map_err(|_| changed())? != path {
        return Err(changed());
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(leaf)) => {
            let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority())
                .map_err(|_| changed())?;
            open_plain_directory(&parent, Path::new(leaf))
        }
        _ => Dir::open_ambient_dir(path, cap_std::ambient_authority()).map_err(|_| changed()),
    }
}

fn open_plain_directory(parent: &Dir, name: &Path) -> Result<Dir, StartupError> {
    let before = parent.symlink_metadata(name).map_err(|_| changed())?;
    if !before.is_dir() || is_link(&before) {
        return Err(changed());
    }
    let directory = parent.open_dir_nofollow(name).map_err(|_| changed())?;
    let after = parent.symlink_metadata(name).map_err(|_| changed())?;
    let opened = plain_directory_metadata(&directory)?;
    if !after.is_dir()
        || is_link(&after)
        || identity(&before) != identity(&opened)
        || identity(&opened) != identity(&after)
    {
        return Err(changed());
    }
    Ok(directory)
}

fn plain_directory_metadata(directory: &Dir) -> Result<Metadata, StartupError> {
    let metadata = directory.dir_metadata().map_err(|_| changed())?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(changed());
    }
    Ok(metadata)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    if cap_fs_ext::OsMetadataExt::file_attributes(metadata) & 0x400 != 0 {
        return true;
    }
    metadata.file_type().is_symlink()
}

fn identity(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}
fn modified(metadata: &Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .into_std()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_nanos())
}
fn same(left: &Metadata, right: &Metadata) -> bool {
    left.is_file()
        && right.is_file()
        && !is_link(left)
        && !is_link(right)
        && identity(left) == identity(right)
        && left.len() == right.len()
        && modified(left) == modified(right)
}
fn changed() -> StartupError {
    StartupError::resource("The selected model changed or cannot be inspected. Select it again.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gguf(directory: &Path, extra_bytes: usize) -> PathBuf {
        std::fs::create_dir_all(directory).unwrap();
        let path = directory.join("choice.gguf");
        let mut header = vec![0_u8; 24 + extra_bytes];
        header[..4].copy_from_slice(b"GGUF");
        header[4..8].copy_from_slice(&3_u32.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        path
    }

    fn symlink_directory(target: &Path, path: &Path) {
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(target, path).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, path).unwrap();
    }

    #[test]
    fn discovered_models_directory_swap_cannot_admit_external_model() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let models_path = workspace.path().join("models");
        gguf(&models_path, 0);
        gguf(outside.path(), 777);
        let binding = DiscoveredDirectory::open(workspace.path())
            .unwrap()
            .unwrap();
        #[cfg(windows)]
        let binding = {
            use std::os::windows::fs::OpenOptionsExt as _;
            // Production cap-std directory handles additionally deny rename
            // on Windows. Deliberately allow delete-sharing in this test-only
            // handle so the cross-platform revalidation fallback is exercised
            // by an actual swap, not merely by the native rename prevention.
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(7)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(&models_path)
                .unwrap();
            let mut binding = binding;
            binding.directory = Dir::from_std_file(file);
            binding
        };
        let result = scan_binding(binding, |_| {
            std::fs::rename(&models_path, workspace.path().join("old-models")).unwrap();
            symlink_directory(outside.path(), &models_path);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::metadata(outside.path().join("choice.gguf"))
                .unwrap()
                .len(),
            801
        );
    }

    #[test]
    fn automatic_symlink_root_is_refused_but_explicit_external_model_is_allowed() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let selected = gguf(outside.path(), 99);
        symlink_directory(outside.path(), &workspace.path().join("models"));
        assert!(scan(workspace.path(), None).is_err());
        let models = scan(workspace.path(), Some(&selected)).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].choice.bytes, Some(123));
        models[0].validate().unwrap();
    }

    #[test]
    fn discovered_binding_is_retained_for_later_model_validation() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let models_path = workspace.path().join("models");
        gguf(&models_path, 0);
        gguf(outside.path(), 777);
        let models = scan(workspace.path(), None).unwrap();
        let replacement = std::fs::rename(&models_path, workspace.path().join("old-models"));
        match replacement {
            Ok(()) => {
                symlink_directory(outside.path(), &models_path);
                assert!(models[0].validate().is_err());
            }
            Err(error) => {
                #[cfg(windows)]
                {
                    assert!(
                        error.kind() == std::io::ErrorKind::PermissionDenied
                            || error.raw_os_error() == Some(32),
                        "unexpected directory rename failure: {error}"
                    );
                    models[0].validate().unwrap();
                    assert_eq!(models[0].choice.bytes, Some(24));
                }
                #[cfg(not(windows))]
                panic!("unexpected directory rename failure: {error}");
            }
        }
    }

    #[test]
    fn local_directory_entry_and_model_count_limits_are_exact() {
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("models");
        std::fs::create_dir(&directory).unwrap();
        for index in 0..ENTRY_LIMIT {
            std::fs::write(directory.join(format!("entry-{index}.txt")), b"").unwrap();
        }
        assert!(scan(workspace.path(), None).unwrap().is_empty());
        std::fs::write(directory.join("one-too-many.txt"), b"").unwrap();
        let error = match scan(workspace.path(), None) {
            Err(error) => error,
            Ok(_) => panic!("directory entry cap was not enforced"),
        };
        assert!(error.to_string().contains("exceeds 256 entries"));

        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join("models");
        let model = gguf(&directory, 0);
        for index in 1..MODEL_LIMIT {
            std::fs::copy(&model, directory.join(format!("choice-{index}.gguf"))).unwrap();
        }
        let admitted = scan(workspace.path(), None).unwrap();
        assert_eq!(admitted.len(), MODEL_LIMIT);
        drop(admitted);
        std::fs::copy(&model, directory.join("one-too-many.gguf")).unwrap();
        let error = match scan(workspace.path(), None) {
            Err(error) => error,
            Ok(_) => panic!("model count cap was not enforced"),
        };
        assert!(error.to_string().contains("exceeds 128 GGUF files"));
    }
}
