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
    /// The workspace is not itself a git worktree root. Git discovery walks
    /// *upward*, so without this check a workspace at `~/scratch/foo` resolves
    /// to whatever repo owns `~` and we would stage — and mutate the index of —
    /// a repository the user never pointed us at.
    #[error("workspace {workspace} is not a git repository root (git resolved to {toplevel})")]
    NotWorkspaceRoot { workspace: String, toplevel: String },
}

/// A lightweight wrapper for Git-based workspace snapshotting.
///
/// Every method here is **blocking** — each shells out to `git` via
/// `std::process::Command`. They used to be `async fn` with not a single
/// `.await` in either body (ADR-074): the signature promised a yield point that
/// did not exist, so under tokio it silently blocked a reactor thread once per
/// turn while looking like it didn't. `ferric-loop` is deliberately
/// executor-agnostic (ADR-010), so the honest fix is to be synchronous and say
/// so; a caller that cares can offload with its own executor's blocking pool.
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
    ///
    /// The snapshot is staged in a **private index file**, never the repository's
    /// real one (ADR-073). `run.rs` calls this once per turn, so anything that
    /// touches `.git/index` silently destroys the user's staging area on turn 1
    /// and every turn after. Note that `git reset` and `git read-tree HEAD` — the
    /// two options the previous implementation weighed — are equally destructive:
    /// both reset the index to HEAD, discarding whatever the user had staged.
    /// Not touching the real index is the only correct answer.
    pub fn snapshot(&self, session_id: &str, turn_id: u32) -> Result<String, VcsError> {
        self.ensure_workspace_is_repo_root()?;
        let git_dir = std::path::PathBuf::from(self.run_git(&["rev-parse", "--absolute-git-dir"])?);
        let temp_index = git_dir.join(format!(
            "ferric-snapshot-index-{}-{}",
            sanitize_for_filename(session_id),
            turn_id
        ));

        // Seed the private index from the real one when it exists. `add -A` then
        // only re-hashes what actually changed, because the stat cache comes
        // along — this runs every turn, so a cold index would mean re-hashing the
        // whole tree each time. (Copying is read-only w.r.t. the real index.)
        let real_index = git_dir.join("index");
        if real_index.exists() {
            std::fs::copy(&real_index, &temp_index)?;
        }

        let result = self.snapshot_into(&temp_index, session_id, turn_id);
        // Best-effort cleanup: a leaked temp index is inert (git only reads it
        // when GIT_INDEX_FILE points at it), so this must not mask a real error.
        let _ = std::fs::remove_file(&temp_index);
        result
    }

    fn snapshot_into(
        &self,
        index: &Path,
        session_id: &str,
        turn_id: u32,
    ) -> Result<String, VcsError> {
        let tag_name = snapshot_ref(session_id, turn_id);

        // 1. Stage the whole working tree — into `index`, not `.git/index`.
        self.run_git_with_index(&["add", "-A"], Some(index))?;

        // 2. Write the tree object from that private index.
        let tree_hash = self.run_git_with_index(&["write-tree"], Some(index))?;

        // 3. Create an orphan commit (no parents). Indexless.
        let commit_message = format!("Snapshot for session {}, turn {}", session_id, turn_id);
        let commit_hash = self.run_git(&["commit-tree", &tree_hash, "-m", &commit_message])?;

        // 4. Point the ref at it. Indexless.
        self.run_git(&["update-ref", &tag_name, &commit_hash])?;

        Ok(commit_hash)
    }

    /// Reverts the workspace exactly to the snapshot associated with `session_id` and `turn_id`.
    pub fn revert(&self, session_id: &str, turn_id: u32) -> Result<(), VcsError> {
        // Same containment check as `snapshot`, and it matters more here: this
        // runs `git clean -fd`, so pointing it at an ancestor repo would delete
        // untracked files across the user's whole home directory.
        self.ensure_workspace_is_repo_root()?;
        let tag_name = snapshot_ref(session_id, turn_id);

        // Use `git restore`
        self.run_git(&[
            "restore",
            "--source",
            &tag_name,
            "--worktree",
            "--staged",
            ".",
        ])?;
        // Clean untracked files (since restore leaves them). Deliberately NOT
        // `-x`: gitignored paths (`target/`, `.env`, model caches) are never
        // part of a snapshot and must survive a revert. Callers are expected to
        // have confirmed this with the user — see `untracked_to_be_removed`.
        self.run_git(&["clean", "-fd"])?;

        Ok(())
    }

    /// The untracked paths a `revert` would delete, as a dry run. Lets a caller
    /// show the user exactly what is about to be destroyed instead of asking
    /// them to approve an abstraction.
    pub fn untracked_to_be_removed(&self) -> Result<Vec<String>, VcsError> {
        self.ensure_workspace_is_repo_root()?;
        let out = self.run_git(&["clean", "-nd"])?;
        Ok(out
            .lines()
            .filter_map(|l| l.strip_prefix("Would remove "))
            .map(|s| s.trim().to_string())
            .collect())
    }

    /// Refuse to operate unless the workspace root *is* the git worktree root.
    ///
    /// Git discovery walks upward, so a workspace that is not itself a repo
    /// resolves to the nearest ancestor repo. On a machine where `~` is a git
    /// repo — which is common, and true on the machine this was found on — every
    /// non-git workspace under `~` resolves to the home repo, and a per-turn
    /// `git add -A` would then scan and stage the user's entire home directory
    /// into a repository they never pointed Ferric at.
    fn ensure_workspace_is_repo_root(&self) -> Result<(), VcsError> {
        let toplevel = self.run_git(&["rev-parse", "--show-toplevel"])?;
        let same = std::fs::canonicalize(&toplevel)
            .ok()
            .zip(std::fs::canonicalize(&self.workspace_root).ok())
            .map(|(a, b)| a == b)
            .unwrap_or(false);

        if same {
            Ok(())
        } else {
            Err(VcsError::NotWorkspaceRoot {
                workspace: self.workspace_root.display().to_string(),
                toplevel,
            })
        }
    }

    fn run_git(&self, args: &[&str]) -> Result<String, VcsError> {
        self.run_git_with_index(args, None)
    }

    /// `index`: when set, git operates on that index file via `GIT_INDEX_FILE`
    /// and leaves the repository's real `.git/index` untouched.
    fn run_git_with_index(&self, args: &[&str], index: Option<&Path>) -> Result<String, VcsError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.workspace_root).args(args);
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        let output = cmd.output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(VcsError::GitError(err));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Session ids reach us from the trace and end up in both a filename and a git
/// ref, so keep them to a conservative charset rather than trusting them. Real
/// ids are `q-<millis>`, but `JsonlSink::open` accepts any string, and git
/// hard-refuses ref names containing `:`, spaces, `..`, and friends.
fn sanitize_for_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The single definition of a snapshot's ref name. `snapshot` and `revert` MUST
/// derive it identically — if they drift, every revert silently fails to find
/// its snapshot.
fn snapshot_ref(session_id: &str, turn_id: u32) -> String {
    format!(
        "refs/ferric/{}/{}",
        sanitize_for_filename(session_id),
        turn_id
    )
}
