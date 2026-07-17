//! `ferric-vcs` provides lightweight git wrapping for `Animus_Ferric`.
//! It manages orphan snapshot commits tied to `TurnEnd` events in the trace,
//! enabling time-travel via the `ferric revert` CLI command.

use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VcsError {
    #[error("Git command failed: {0}")]
    GitError(String),
    #[error("Failed to execute git: {0}")]
    IoError(#[from] std::io::Error),
}

/// A lightweight wrapper for Git-based workspace snapshotting.
pub struct Vcs {
    workspace_root: std::path::PathBuf,
}

impl Vcs {
    /// Creates a new `Vcs` instance bound to a workspace root.
    pub fn new<P: AsRef<Path>>(workspace_root: P) -> Self {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
        }
    }

    /// Takes a snapshot of the current working tree and creates an orphan commit
    /// tagged with the `session_id` and `turn_id`.
    pub async fn snapshot(&self, session_id: &str, turn_id: u32) -> Result<String, VcsError> {
        let tag_name = format!("refs/ferric/{}/{}", session_id, turn_id);

        // 1. Add all changes to the index
        self.run_git(&["add", "-A"])?;

        // 2. Write the tree object
        let tree_hash = self.run_git(&["write-tree"])?;

        // 3. Create an orphan commit (no parents)
        let commit_message = format!("Snapshot for session {}, turn {}", session_id, turn_id);
        let commit_hash = self.run_git(&["commit-tree", &tree_hash, "-m", &commit_message])?;

        // 4. Create the tag ref pointing to this commit
        self.run_git(&["update-ref", &tag_name, &commit_hash])?;

        // 5. Restore the index to HEAD to prevent the user's staging area from being polluted,
        // although we assume the agent operates largely independently.
        // Wait, `add -A` pollutes the staging area. We can use `git read-tree HEAD` to reset it.
        // Even better: just leave it or `git reset` (mixed).
        let _ = self.run_git(&["reset"]);

        Ok(commit_hash)
    }

    /// Reverts the workspace exactly to the snapshot associated with `session_id` and `turn_id`.
    pub async fn revert(&self, session_id: &str, turn_id: u32) -> Result<(), VcsError> {
        let tag_name = format!("refs/ferric/{}/{}", session_id, turn_id);

        // Use `git restore`
        self.run_git(&["restore", "--source", &tag_name, "--worktree", "--staged", "."])?;
        // Clean untracked files (since restore leaves them)
        self.run_git(&["clean", "-fd"])?;

        Ok(())
    }

    fn run_git(&self, args: &[&str]) -> Result<String, VcsError> {
        let output = Command::new("git")
            .current_dir(&self.workspace_root)
            .args(args)
            .output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(VcsError::GitError(err));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
