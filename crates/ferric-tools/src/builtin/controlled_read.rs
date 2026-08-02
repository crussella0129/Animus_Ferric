use std::ffi::OsString;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use cap_fs_ext::OpenOptionsExt as _;
use cap_fs_ext::{
    DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use ferric_guard::Workspace;

pub(crate) struct ControlledFileRead {
    pub bytes: Vec<u8>,
    pub relative: PathBuf,
}

/// Open an exact requested directory through retained capabilities. The normal
/// workspace resolver still performs its canonical boundary check, while this
/// traversal deliberately uses the lexical request so an in-workspace symlink
/// cannot be hidden by canonicalization.
pub(crate) fn open_controlled_dir(
    workspace: &Workspace,
    requested: &str,
) -> Result<(Dir, PathBuf), String> {
    open_controlled_dir_with(workspace, requested, || {})
}

pub(crate) fn validate_controlled_dir(
    workspace: &Workspace,
    requested: &str,
    expected: &Dir,
) -> Result<(), String> {
    let (current, _) = open_controlled_dir(workspace, requested)?;
    let expected = expected
        .dir_metadata()
        .map_err(|error| format!("inspect retained controlled directory: {error}"))?;
    let current = current
        .dir_metadata()
        .map_err(|error| format!("inspect rebound controlled directory: {error}"))?;
    if metadata_identity(&expected) != metadata_identity(&current) {
        return Err("controlled directory binding changed during observation".to_string());
    }
    Ok(())
}

fn open_controlled_dir_with<F>(
    workspace: &Workspace,
    requested: &str,
    after_boundary_check: F,
) -> Result<(Dir, PathBuf), String>
where
    F: FnOnce(),
{
    workspace
        .resolve(requested)
        .map_err(|error| format!("boundary: {error}"))?;
    after_boundary_check();
    let relative = lexical_relative(workspace, Path::new(requested))?;
    let dir = open_relative_dir(workspace, &relative)?;
    Ok((dir, relative))
}

pub(crate) fn read_controlled_file(
    workspace: &Workspace,
    requested: &str,
) -> Result<ControlledFileRead, String> {
    read_controlled_file_with(workspace, requested, || {})
}

fn read_controlled_file_with<F>(
    workspace: &Workspace,
    requested: &str,
    after_boundary_check: F,
) -> Result<ControlledFileRead, String>
where
    F: FnOnce(),
{
    workspace
        .resolve(requested)
        .map_err(|error| format!("boundary: {error}"))?;
    after_boundary_check();
    let relative = lexical_relative(workspace, Path::new(requested))?;
    let leaf = relative
        .file_name()
        .ok_or_else(|| "controlled read path must name a file".to_string())?
        .to_os_string();
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    let parent = open_relative_dir(workspace, parent_relative)?;
    let before = parent
        .symlink_metadata(&leaf)
        .map_err(|error| format!("inspect controlled read target: {error}"))?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err("controlled read target is not a plain regular file".to_string());
    }
    let identity = metadata_identity(&before);
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true);
    #[cfg(unix)]
    {
        options.custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = parent
        .open_with(&leaf, &options)
        .map_err(|error| format!("open controlled read target without following links: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("inspect opened controlled read target: {error}"))?;
    let after = parent
        .symlink_metadata(&leaf)
        .map_err(|error| format!("revalidate controlled read target: {error}"))?;
    if !opened.is_file()
        || opened.file_type().is_symlink()
        || after.file_type().is_symlink()
        || !after.is_file()
        || metadata_identity(&opened) != identity
        || metadata_identity(&after) != identity
    {
        return Err("controlled read target changed while opening".to_string());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read controlled target: {error}"))?;
    let rebound = open_relative_dir(workspace, parent_relative)?;
    let retained_parent = parent
        .dir_metadata()
        .map_err(|error| format!("inspect retained controlled parent: {error}"))?;
    let rebound_parent = rebound
        .dir_metadata()
        .map_err(|error| format!("inspect rebound controlled parent: {error}"))?;
    let rebound_leaf = rebound
        .symlink_metadata(&leaf)
        .map_err(|error| format!("revalidate controlled read path: {error}"))?;
    if metadata_identity(&retained_parent) != metadata_identity(&rebound_parent)
        || rebound_leaf.file_type().is_symlink()
        || !rebound_leaf.is_file()
        || metadata_identity(&rebound_leaf) != identity
    {
        return Err("controlled read path changed during observation".to_string());
    }
    Ok(ControlledFileRead { bytes, relative })
}

fn open_relative_dir(workspace: &Workspace, relative: &Path) -> Result<Dir, String> {
    let mut current = Dir::open_ambient_dir(workspace.root(), ambient_authority())
        .map_err(|error| format!("open controlled workspace root: {error}"))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::CurDir) {
                continue;
            }
            return Err("controlled path contains an unsupported component".to_string());
        };
        let before = current.symlink_metadata(name).map_err(|error| {
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) {
                "controlled directory does not exist".to_string()
            } else {
                format!("inspect controlled directory: {error}")
            }
        })?;
        if before.file_type().is_symlink() || !before.is_dir() {
            return Err("controlled path ancestor is not a plain directory".to_string());
        }
        let identity = metadata_identity(&before);
        let child = current.open_dir_nofollow(name).map_err(|error| {
            format!("open controlled directory without following links: {error}")
        })?;
        let opened = child
            .dir_metadata()
            .map_err(|error| format!("inspect opened controlled directory: {error}"))?;
        let after = current
            .symlink_metadata(name)
            .map_err(|error| format!("revalidate controlled directory: {error}"))?;
        if !opened.is_dir()
            || after.file_type().is_symlink()
            || !after.is_dir()
            || metadata_identity(&opened) != identity
            || metadata_identity(&after) != identity
        {
            return Err("controlled directory changed while opening".to_string());
        }
        current = child;
    }
    Ok(current)
}

fn lexical_relative(workspace: &Workspace, requested: &Path) -> Result<PathBuf, String> {
    let candidate = if requested.is_absolute() {
        requested
            .strip_prefix(workspace.root())
            .map_err(|_| "controlled path is not rooted in the workspace".to_string())?
            .to_path_buf()
    } else {
        requested.to_path_buf()
    };
    let mut components: Vec<OsString> = Vec::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::ParentDir => {
                if components.pop().is_none() {
                    return Err("controlled path escapes the workspace lexically".to_string());
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("controlled path contains an unsupported root".to_string());
            }
        }
    }
    Ok(components.iter().collect())
}

fn metadata_identity(metadata: &cap_std::fs::Metadata) -> (u64, u64) {
    (MetadataExt::dev(metadata), MetadataExt::ino(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(unix, windows))]
    #[test]
    fn ancestor_link_race_after_boundary_check_is_not_followed() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("workspace");
        let nested = root.join("nested");
        let held = root.join("held");
        let outside = outer.path().join("outside");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let workspace = Workspace::new(&root).unwrap();

        let result = open_controlled_dir_with(&workspace, "nested", || {
            std::fs::rename(&nested, &held).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &nested).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&outside, &nested).unwrap();
        });
        assert!(result.is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn leaf_link_race_after_boundary_check_is_not_read() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let target = root.join("target.txt");
        let outside = outer.path().join("outside.txt");
        std::fs::write(&target, b"inside\n").unwrap();
        std::fs::write(&outside, b"outside-secret\n").unwrap();
        let workspace = Workspace::new(&root).unwrap();

        let result = read_controlled_file_with(&workspace, "target.txt", || {
            std::fs::remove_file(&target).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &target).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&outside, &target).unwrap();
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside-secret\n");
    }
}
