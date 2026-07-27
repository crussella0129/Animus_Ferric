use ferric_vcs::Vcs;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_snapshot_and_revert() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Initialize git repo in the temp directory
    Command::new("git")
        .current_dir(root)
        .args(["init"])
        .output()
        .unwrap();

    // Set config so commits don't fail in CI environments
    Command::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test User"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .unwrap();

    let vcs = Vcs::new(root);

    // Initial state
    let test_file = root.join("test.txt");
    fs::write(&test_file, "initial").unwrap();

    // Take snapshot at turn 1
    vcs.snapshot("test-session", 1).expect("Failed to snapshot");

    // Modify state
    fs::write(&test_file, "modified").unwrap();
    let other_file = root.join("other.txt");
    fs::write(&other_file, "hello").unwrap();

    // Take snapshot at turn 2
    vcs.snapshot("test-session", 2).expect("Failed to snapshot");

    // Revert to turn 1
    vcs.revert("test-session", 1).expect("Failed to revert");

    // Check state is back to turn 1
    assert_eq!(fs::read_to_string(&test_file).unwrap(), "initial");
    assert!(!other_file.exists());

    // Revert to turn 2
    vcs.revert("test-session", 2)
        .expect("Failed to revert to turn 2");

    // Check state is back to turn 2
    assert_eq!(fs::read_to_string(&test_file).unwrap(), "modified");
    assert_eq!(fs::read_to_string(&other_file).unwrap(), "hello");
}

/// Regression (ADR-073): `snapshot` runs once per turn, so if it touches the
/// repository index at all, a user who has carefully staged part of their work
/// loses it on turn 1 and every turn after. Both `git reset` and
/// `git read-tree HEAD` fail this test — they reset the index to HEAD.
#[test]
fn snapshot_preserves_the_users_staged_index() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let git = |args: &[&str]| -> String {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    git(&["init"]);
    git(&["config", "user.name", "Test User"]);
    git(&["config", "user.email", "test@example.com"]);
    fs::write(root.join("base.txt"), "base").unwrap();
    git(&["add", "base.txt"]);
    git(&["commit", "-m", "init"]);

    // The user stages ONE of two changes — a real, common working state.
    fs::write(root.join("staged.txt"), "i am staged").unwrap();
    fs::write(root.join("unstaged.txt"), "i am not").unwrap();
    git(&["add", "staged.txt"]);

    let before = git(&["diff", "--cached", "--name-only"]);
    assert_eq!(before, "staged.txt", "precondition");

    Vcs::new(root).snapshot("probe-session", 1).unwrap();

    assert_eq!(
        git(&["diff", "--cached", "--name-only"]),
        before,
        "snapshot() must leave the user's staging area exactly as it found it"
    );
    // And the working tree is untouched too.
    assert_eq!(
        fs::read_to_string(root.join("unstaged.txt")).unwrap(),
        "i am not"
    );
}

/// The private index must not cost the snapshot any content: untracked files
/// still have to make it into the tree, or `revert` silently loses them.
#[test]
fn snapshot_still_captures_untracked_files() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let git = |args: &[&str]| -> String {
        let out = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    git(&["init"]);
    git(&["config", "user.name", "Test User"]);
    git(&["config", "user.email", "test@example.com"]);
    fs::write(root.join("base.txt"), "base").unwrap();
    git(&["add", "base.txt"]);
    git(&["commit", "-m", "init"]);

    fs::write(root.join("never-added.txt"), "untracked").unwrap();

    let commit = Vcs::new(root).snapshot("s", 7).unwrap();
    let listed = git(&["ls-tree", "--name-only", &commit]);

    assert!(
        listed.contains("never-added.txt"),
        "untracked files must be in the snapshot tree, got: {listed}"
    );
}

/// Regression (ADR-073): git discovery walks *upward*. A workspace that is not
/// itself a repo resolves to the nearest ancestor repo — and on a machine where
/// `~` is a git repo, that is the user's entire home directory. `snapshot` must
/// refuse rather than stage (and `revert` rather than `git clean -fd`) a
/// repository the user never pointed Ferric at.
#[test]
fn refuses_to_operate_on_an_ancestor_repo() {
    let outer = tempdir().unwrap();

    // An ancestor repo...
    Command::new("git")
        .current_dir(outer.path())
        .args(["init"])
        .output()
        .unwrap();

    // ...with a plain, non-repo subdirectory used as the workspace.
    let inner = outer.path().join("workspace");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join("a.txt"), "a").unwrap();

    let vcs = Vcs::new(&inner);

    let err = vcs
        .snapshot("s", 1)
        .expect_err("snapshot must refuse a workspace that is not the repo root");
    assert!(
        matches!(err, ferric_vcs::VcsError::NotWorkspaceRoot { .. }),
        "expected NotWorkspaceRoot, got: {err:?}"
    );

    let err = vcs
        .revert("s", 1)
        .expect_err("revert must refuse too — it runs `git clean -fd`");
    assert!(matches!(err, ferric_vcs::VcsError::NotWorkspaceRoot { .. }));

    // And the ancestor repo's index was never touched.
    let staged = Command::new("git")
        .current_dir(outer.path())
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert!(
        staged.stdout.is_empty(),
        "ancestor repo's index was modified: {}",
        String::from_utf8_lossy(&staged.stdout)
    );
}

/// The temp index is an implementation detail and must not survive the call.
#[test]
fn snapshot_leaves_no_temp_index_behind() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    Command::new("git")
        .current_dir(root)
        .args(["init"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test User"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .unwrap();
    fs::write(root.join("a.txt"), "a").unwrap();

    Vcs::new(root).snapshot("sess/with:odd chars", 3).unwrap();

    let leftovers: Vec<_> = fs::read_dir(root.join(".git"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("ferric-snapshot-index"))
        .collect();

    assert!(
        leftovers.is_empty(),
        "temp index files left behind: {leftovers:?}"
    );
}
