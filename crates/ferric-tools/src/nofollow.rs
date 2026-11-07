//! No-follow filesystem primitives shared by every capability-relative path
//! (controlled reads and mutations, navigation, and CLI startup state):
//! `(device, inode)` identity, link and reparse-point classification, the
//! read options that never follow a final symbolic link, and the plain
//! directory open that proves a name still binds what was opened.

use std::path::Path;

use cap_fs_ext::{
    DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, OpenOptionsMaybeDirExt,
};
use cap_std::fs::{Dir, Metadata, OpenOptions};

/// The `(device, inode)` pair that binds a name to one filesystem object.
pub fn identity(metadata: &Metadata) -> (u64, u64) {
    (MetadataExt::dev(metadata), MetadataExt::ino(metadata))
}

/// Windows `FILE_ATTRIBUTE_REPARSE_POINT`, which `std` exposes only as part
/// of the raw attribute word. Always false elsewhere.
pub fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

/// A symbolic link, or on Windows any reparse point (including non-symlink
/// reparse tags).
pub fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    if cap_fs_ext::OsMetadataExt::file_attributes(metadata) & 0x400 != 0 {
        return true;
    }
    metadata.file_type().is_symlink()
}

/// Read-only open options that refuse to follow a final symbolic link. On
/// Unix the descriptor is also close-on-exec and non-blocking, so a FIFO
/// planted at the name cannot stall the open.
pub fn read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::No)
        .maybe_dir(true);
    #[cfg(unix)]
    {
        use cap_fs_ext::OpenOptionsExt as _;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    options
}

/// Why [`open_plain_dir`] or [`plain_dir_metadata`] refused a directory;
/// each caller keeps its own wording.
#[derive(Debug)]
pub enum PlainDirError {
    /// The name could not be inspected.
    Inspect(std::io::Error),
    /// The name, or the opened handle, is not a plain (non-link) directory.
    NotPlain,
    /// The no-follow open failed.
    Open(std::io::Error),
    /// The opened handle could not be inspected.
    InspectOpened(std::io::Error),
    /// The name could not be revalidated, or no longer binds the opened
    /// directory.
    Changed,
}

/// The metadata of an opened directory, refusing anything but a plain
/// (non-link) directory.
pub fn plain_dir_metadata(directory: &Dir) -> Result<Metadata, PlainDirError> {
    let metadata = directory
        .dir_metadata()
        .map_err(PlainDirError::InspectOpened)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(PlainDirError::NotPlain);
    }
    Ok(metadata)
}

/// Open `name` under `parent` as a plain directory without following links,
/// then prove the name still binds the directory that was opened.
pub fn open_plain_dir(parent: &Dir, name: impl AsRef<Path>) -> Result<Dir, PlainDirError> {
    let name = name.as_ref();
    let before = parent
        .symlink_metadata(name)
        .map_err(PlainDirError::Inspect)?;
    if !before.is_dir() || is_link(&before) {
        return Err(PlainDirError::NotPlain);
    }
    let opened = parent
        .open_dir_nofollow(name)
        .map_err(PlainDirError::Open)?;
    let after = parent
        .symlink_metadata(name)
        .map_err(|_| PlainDirError::Changed)?;
    if !after.is_dir()
        || is_link(&after)
        || identity(&before) != identity(&after)
        || identity(&before) != identity(&plain_dir_metadata(&opened)?)
    {
        return Err(PlainDirError::Changed);
    }
    Ok(opened)
}
