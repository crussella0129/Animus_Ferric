use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GuardError {
    #[error("path escapes workspace boundary: {0}")]
    BoundaryViolation(PathBuf),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// The workspace boundary. Every tool path must pass through [`Workspace::resolve`].
///
/// Containment is decided on canonicalized `Component` sequences, never on
/// string prefixes — `project-evil` is NOT inside `project` (the lineage's
/// documented CRITICAL prefix-collision bug), and Windows `\\?\` verbatim
/// prefixes and case normalization come out consistently because both sides
/// are canonicalized the same way. Symlinks are resolved before the check, so
/// an in-workspace link pointing outside is rejected.
pub struct Workspace {
    root: PathBuf,
    ignore: crate::ignore::IgnoreList,
}

impl Workspace {
    /// Create a boundary rooted at `root` (which must exist). Also loads the
    /// workspace's `.ferricignore` (ADR-068) — an absent file is a no-op.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, GuardError> {
        let root = std::fs::canonicalize(root.as_ref())?;
        let ignore = crate::ignore::IgnoreList::load(&root);
        Ok(Self { root, ignore })
    }

    /// The canonical workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The workspace's additive `.ferricignore` denials (ADR-068).
    pub fn ignore(&self) -> &crate::ignore::IgnoreList {
        &self.ignore
    }

    /// Resolve `candidate` (absolute, or relative to the root) to a canonical
    /// path, erroring if it falls outside the workspace. The path itself need
    /// not exist (write targets); its deepest existing ancestor is
    /// canonicalized and the non-existent tail re-appended.
    pub fn resolve(&self, candidate: impl AsRef<Path>) -> Result<PathBuf, GuardError> {
        let candidate = candidate.as_ref();
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.root.join(candidate)
        };
        let normalized = lexical_normalize(&joined).ok_or_else(|| boundary_violation(candidate))?;
        let resolved = canonicalize_existing_prefix(&normalized)?;
        if is_contained(&self.root, &resolved) {
            Ok(resolved)
        } else {
            Err(boundary_violation(candidate))
        }
    }
}

fn boundary_violation(candidate: &Path) -> GuardError {
    GuardError::BoundaryViolation(candidate.to_path_buf())
}

/// Lexically remove `.` and resolve `..` against preceding components.
/// Returns `None` when `..` would climb past the path's prefix/root.
fn lexical_normalize(path: &Path) -> Option<PathBuf> {
    let mut out: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => return None,
            },
            other => out.push(other),
        }
    }
    Some(out.iter().collect())
}

/// Canonicalize the deepest existing ancestor (resolving symlinks), then
/// re-append the non-existent tail components unchanged.
fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, GuardError> {
    let mut existing = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut resolved = std::fs::canonicalize(&existing)?;
    for name in tail.iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

fn is_contained(root: &Path, path: &Path) -> bool {
    let root: Vec<Component> = root.components().collect();
    let path: Vec<Component> = path.components().collect();
    path.len() >= root.len() && path[..root.len()] == root[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_in(dir: &tempfile::TempDir) -> (PathBuf, Workspace) {
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let ws = Workspace::new(&root).unwrap();
        (root, ws)
    }

    #[test]
    fn rejects_dotdot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let (_, ws) = workspace_in(&dir);
        let err = ws.resolve("../outside.txt").unwrap_err();
        assert!(matches!(err, GuardError::BoundaryViolation(_)));
    }

    #[test]
    fn rejects_absolute_outside() {
        let dir = tempfile::tempdir().unwrap();
        let (_, ws) = workspace_in(&dir);
        let outside = dir.path().join("elsewhere").join("file.txt");
        let err = ws.resolve(&outside).unwrap_err();
        assert!(matches!(err, GuardError::BoundaryViolation(_)));
    }

    #[test]
    fn rejects_prefix_collision() {
        let dir = tempfile::tempdir().unwrap();
        let (_, ws) = workspace_in(&dir);
        let evil = dir.path().join("project-evil");
        std::fs::create_dir(&evil).unwrap();
        let err = ws.resolve(evil.join("x")).unwrap_err();
        assert!(matches!(err, GuardError::BoundaryViolation(_)));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let (root, ws) = workspace_in(&dir);
        let outside = dir.path().join("secret");
        std::fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let err = ws.resolve("link/data.txt").unwrap_err();
        assert!(matches!(err, GuardError::BoundaryViolation(_)));
    }

    #[test]
    fn accepts_nested_valid_path() {
        let dir = tempfile::tempdir().unwrap();
        let (root, ws) = workspace_in(&dir);
        // Non-existent nested write target inside the workspace is fine.
        let resolved = ws.resolve("src/a/b.rs").unwrap();
        assert!(is_contained(ws.root(), &resolved));
        assert!(resolved.ends_with(Path::new("src").join("a").join("b.rs")));
        // Existing file too.
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();
        let existing = ws.resolve("src/main.rs").unwrap();
        assert!(is_contained(ws.root(), &existing));
    }

    #[test]
    fn dotdot_within_workspace_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let (root, ws) = workspace_in(&dir);
        std::fs::create_dir(root.join("src")).unwrap();
        let resolved = ws.resolve("src/../notes.md").unwrap();
        assert!(is_contained(ws.root(), &resolved));
        assert!(resolved.ends_with("notes.md"));
    }
}
