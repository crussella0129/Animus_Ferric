//! T-4304 (sprint 43): `scaffold` integration tests against a real temp dir.
//! These run `git` on PATH (GitHub Actions has it; the aarch64 CI gate is
//! `cargo check`, so it never runs these).

use std::process::Command;

use animus_launch::{LaunchError, LaunchSpec, scaffold};

fn spec(path: std::path::PathBuf) -> LaunchSpec {
    LaunchSpec {
        name: "demo".to_string(),
        path,
        goal: "build a tiny parser; add tests".to_string(),
        project_type: animus_launch::ProjectType::Empty,
    }
}

fn git_out(cwd: &std::path::Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    )
}

#[test]
fn scaffold_creates_git_repo_with_main_and_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    let report = scaffold(&spec(target.clone())).unwrap();

    // A real git repo.
    assert!(target.join(".git").is_dir(), ".git must exist");
    // Both branches exist.
    assert!(
        git_out(&target, &["rev-parse", "--verify", "main"]).0,
        "main branch must exist"
    );
    assert!(
        git_out(&target, &["rev-parse", "--verify", "dev"]).0,
        "dev branch must exist"
    );
    // HEAD is on main with the scaffold commit.
    let (_ok, branch) = git_out(&target, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(branch, "main", "HEAD should be on main");
    let (_ok, log) = git_out(&target, &["log", "--oneline"]);
    assert!(
        log.contains("Initial scaffold"),
        "the scaffold commit: {log}"
    );

    // The four seed files, content asserted by SUBSTRING (Windows autocrlf-safe).
    let readme = std::fs::read_to_string(target.join("README.md")).unwrap();
    assert!(readme.contains("demo") && readme.contains("tiny parser"));
    let gitignore = std::fs::read_to_string(target.join(".gitignore")).unwrap();
    assert!(gitignore.contains("sprints/"));
    let tasks = std::fs::read_to_string(target.join("agent-tasks").join("agent-tasks.md")).unwrap();
    assert!(tasks.contains("parser") && tasks.contains("- [ ]"));
    assert!(target.join("decisions.md").exists());

    assert_eq!(report.branches, vec!["main", "dev"]);
    assert_eq!(report.files_created.len(), 4);
}

#[test]
fn scaffold_refuses_to_clobber_nonempty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("existing.txt"), "keep me").unwrap();

    let err = scaffold(&spec(target.clone())).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
    // Nothing was touched.
    assert_eq!(
        std::fs::read_to_string(target.join("existing.txt")).unwrap(),
        "keep me"
    );
    assert!(!target.join(".git").exists());
}

#[test]
fn scaffold_refuses_to_clobber_hidden_only_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join(".keep"), "x").unwrap();

    let err = scaffold(&spec(target.clone())).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
    assert!(!target.join(".git").exists());
}

#[test]
fn scaffold_refuses_to_clobber_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("afile");
    std::fs::write(&target, "not a dir").unwrap();

    let err = scaffold(&spec(target.clone())).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "not a dir");
}

/// The fixed `-c user.name`/`-c user.email` identity makes the initial commit
/// succeed regardless of the ambient git identity (proven by the commit
/// existing in the temp repo).
#[test]
fn scaffold_commit_works_without_global_git_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    scaffold(&spec(target.clone())).unwrap();
    let (ok, count) = git_out(&target, &["rev-list", "--count", "HEAD"]);
    assert!(ok && count == "1", "exactly one scaffold commit exists");
}

#[test]
fn scaffold_refuses_dot_target_when_cwd_is_nonempty() {
    // The test runner's cwd is the workspace or crate root, which is non-empty.
    let err = scaffold(&spec(std::path::PathBuf::from("."))).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
}

#[test]
#[cfg(unix)]
fn scaffold_refuses_symlink_to_nonempty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let real_dir = tmp.path().join("real");
    std::fs::create_dir(&real_dir).unwrap();
    std::fs::write(real_dir.join("existing.txt"), "keep me").unwrap();

    let symlink_path = tmp.path().join("symlink");
    std::os::unix::fs::symlink(&real_dir, &symlink_path).unwrap();

    let err = scaffold(&spec(symlink_path)).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
}
