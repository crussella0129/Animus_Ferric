//! T-4304 (sprint 43): `scaffold` integration tests against a real temp dir.
//! These run `git` on PATH (GitHub Actions has it; the aarch64 CI gate is
//! `cargo check`, so it never runs these).

use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use animus_launch::{LaunchError, LaunchSpec, ProjectType, ScaffoldReport, scaffold};

static GIT_ENV_LOCK: Mutex<()> = Mutex::new(());

fn git_env_lock() -> MutexGuard<'static, ()> {
    GIT_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn scaffold_for_test(spec: &LaunchSpec) -> Result<ScaffoldReport, LaunchError> {
    let _guard = git_env_lock();
    scaffold(spec)
}

struct EnvironmentRestore {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvironmentRestore {
    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: every scaffold invocation in this integration-test process
        // holds GIT_ENV_LOCK, so no peer thread can observe this temporary Git
        // configuration through a child process.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvironmentRestore {
    fn drop(&mut self) {
        // SAFETY: the caller still holds GIT_ENV_LOCK while this guard drops.
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

fn spec(path: PathBuf) -> LaunchSpec {
    spec_with_type(path, ProjectType::Empty)
}

fn spec_with_type(path: PathBuf, project_type: ProjectType) -> LaunchSpec {
    LaunchSpec {
        name: "demo".to_string(),
        path,
        goal: "build a tiny parser; add tests".to_string(),
        project_type,
    }
}

fn git_out(cwd: &Path, args: &[&str]) -> (bool, String) {
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

fn tracked_files(cwd: &Path) -> Vec<String> {
    let (ok, output) = git_out(cwd, &["ls-files"]);
    assert!(ok, "git ls-files failed in {}", cwd.display());
    output.lines().map(str::to_string).collect()
}

fn expected_files(project_type: &ProjectType) -> Vec<String> {
    let mut files = vec![
        ".gitignore",
        "README.md",
        "docs/.sprint-loop-book",
        "docs/README.md",
        "docs/SUMMARY.md",
        "docs/history/README.md",
        "docs/intents/INT-0001-initial-project-goal.md",
        "docs/intents/README.md",
        "docs/sprints/README.md",
        "docs/work/completed-tasks.md",
        "docs/work/confidence.txt",
        "docs/work/tasks.md",
    ];
    match project_type {
        ProjectType::Rust => files.extend(["Cargo.toml", "src/main.rs"]),
        ProjectType::Python => files.extend(["requirements.txt", "src/main.py"]),
        ProjectType::Web => files.extend(["app.js", "index.html", "style.css"]),
        ProjectType::Empty => {}
    }
    files.sort_unstable();
    files.into_iter().map(str::to_string).collect()
}

const BOOK_IGNORES: &str = "# >>> sprint-loops >>>\n# The Book is tracked. Ignore only transient helper output.\n*.tmp\n/guards-report.ndjson\n# <<< sprint-loops <<<\n";

fn expected_gitignore(project_type: &ProjectType) -> String {
    match project_type {
        ProjectType::Rust => format!("# Rust build output.\n/target/\n\n{BOOK_IGNORES}"),
        ProjectType::Python => {
            format!("__pycache__/\n*.pyc\n.venv/\nvenv/\n\n{BOOK_IGNORES}")
        }
        ProjectType::Web | ProjectType::Empty => BOOK_IGNORES.to_string(),
    }
}

fn markdown_links(markdown: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut remaining = markdown;
    while let Some(start) = remaining.find("](") {
        let target_and_rest = &remaining[start + 2..];
        let Some(end) = target_and_rest.find(')') else {
            break;
        };
        links.push(target_and_rest[..end].to_string());
        remaining = &target_and_rest[end + 1..];
    }
    links
}

fn assert_book_markdown_is_reachable(target: &Path, files_created: &[String]) {
    let project_root = std::fs::canonicalize(target).unwrap();
    let expected: BTreeSet<PathBuf> = files_created
        .iter()
        .filter(|path| path.starts_with("docs/") && path.ends_with(".md"))
        .map(|path| std::fs::canonicalize(target.join(path)).unwrap())
        .collect();

    let summary = std::fs::canonicalize(target.join("docs/SUMMARY.md")).unwrap();
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::from([summary]);
    while let Some(markdown_path) = queue.pop_front() {
        if !reachable.insert(markdown_path.clone()) {
            continue;
        }
        let markdown = std::fs::read_to_string(&markdown_path).unwrap();
        for link in markdown_links(&markdown) {
            assert!(
                !link.contains("://") && !link.starts_with('/'),
                "generated Book link must be relative: {link}"
            );
            let relative_path = link.split('#').next().unwrap();
            if relative_path.is_empty() {
                continue;
            }
            let resolved = std::fs::canonicalize(
                markdown_path
                    .parent()
                    .expect("Markdown file has a parent")
                    .join(relative_path),
            )
            .unwrap_or_else(|error| {
                panic!(
                    "unresolved generated link {link} from {}: {error}",
                    markdown_path.display()
                )
            });
            assert!(
                resolved.starts_with(&project_root),
                "generated link escapes project: {link}"
            );
            if resolved
                .extension()
                .is_some_and(|extension| extension == "md")
            {
                queue.push_back(resolved);
            }
        }
    }

    assert_eq!(
        reachable, expected,
        "every generated Book Markdown file must be reachable from docs/SUMMARY.md"
    );
}

#[test]
fn scaffold_creates_git_repo_with_main_and_dev() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    let report = scaffold_for_test(&spec(target.clone())).unwrap();

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

    // The seed project and canonical Sprint Loops Book v2 files, with content
    // asserted by substring where line endings may vary under Windows.
    let readme = std::fs::read_to_string(target.join("README.md")).unwrap();
    assert!(readme.contains("demo") && readme.contains("tiny parser"));
    let gitignore = std::fs::read_to_string(target.join(".gitignore")).unwrap();
    assert_eq!(gitignore, expected_gitignore(&ProjectType::Empty));
    assert!(gitignore.contains("# >>> sprint-loops >>>"));
    assert!(gitignore.lines().all(|line| line.trim() != "sprints/"));
    assert!(gitignore.lines().any(|line| line.trim() == "*.tmp"));
    assert!(!gitignore.contains("/docs/**/*.tmp"));
    assert!(!gitignore.contains("/target/"));

    assert_eq!(
        std::fs::read_to_string(target.join("docs/.sprint-loop-book")).unwrap(),
        "schema-version: 2\n"
    );
    let tasks = std::fs::read_to_string(target.join("docs/work/tasks.md")).unwrap();
    assert!(tasks.contains("T-0001") && tasks.contains("Build a tiny parser"));
    assert!(tasks.contains("T-0002") && tasks.contains("Add tests"));
    assert!(
        tasks.contains("[INT-0001](../intents/INT-0001-initial-project-goal.md)"),
        "every generated work item must link to its intent: {tasks}"
    );
    let intent =
        std::fs::read_to_string(target.join("docs/intents/INT-0001-initial-project-goal.md"))
            .unwrap();
    assert!(intent.contains("<!-- sprint-loop-intent-v2 -->"));
    assert!(intent.contains("- **State:** planned"));
    assert!(intent.contains("[T-0001](../work/tasks.md)"));
    assert!(intent.contains("[T-0002](../work/tasks.md)"));
    assert!(!intent.contains("T-0001 initial backlog"));
    assert!(intent.contains("build a tiny parser; add tests"));

    let summary = std::fs::read_to_string(target.join("docs/SUMMARY.md")).unwrap();
    assert!(summary.contains("(intents/INT-0001-initial-project-goal.md)"));
    let intents_readme = std::fs::read_to_string(target.join("docs/intents/README.md")).unwrap();
    assert!(intents_readme.contains("(INT-0001-initial-project-goal.md)"));
    assert!(!intents_readme.contains("schemas/intent.md"));

    for canonical in [
        "docs/README.md",
        "docs/SUMMARY.md",
        "docs/intents/README.md",
        "docs/work/completed-tasks.md",
        "docs/work/confidence.txt",
        "docs/sprints/README.md",
        "docs/history/README.md",
    ] {
        assert!(target.join(canonical).is_file(), "missing {canonical}");
    }
    for legacy in ["decisions.md", "agent-tasks", "confidence.txt", "sprints"] {
        assert!(
            !target.join(legacy).exists(),
            "legacy live ledger must be absent: {legacy}"
        );
    }

    assert_eq!(report.branches, vec!["main", "dev"]);
    assert_eq!(report.files_created, expected_files(&ProjectType::Empty));
    assert!(
        report
            .files_created
            .contains(&"docs/.sprint-loop-book".to_string())
    );
    assert!(
        report
            .files_created
            .contains(&"docs/intents/INT-0001-initial-project-goal.md".to_string())
    );
    assert_eq!(report.files_created, tracked_files(&target));
    assert_book_markdown_is_reachable(&target, &report.files_created);
}

#[test]
fn reports_and_bytes_are_deterministic_and_complete_for_every_profile() {
    let tmp = tempfile::tempdir().unwrap();
    for (label, project_type) in [
        ("empty", ProjectType::Empty),
        ("rust", ProjectType::Rust),
        ("python", ProjectType::Python),
        ("web", ProjectType::Web),
    ] {
        let first = tmp.path().join(format!("{label}-first"));
        let second = tmp.path().join(format!("{label}-second"));
        let first_report =
            scaffold_for_test(&spec_with_type(first.clone(), project_type.clone())).unwrap();
        let second_report =
            scaffold_for_test(&spec_with_type(second.clone(), project_type.clone())).unwrap();

        let expected = expected_files(&project_type);
        assert_eq!(first_report.files_created, expected, "{label} inventory");
        assert_eq!(second_report.files_created, expected, "{label} inventory");
        assert_eq!(first_report.files_created, tracked_files(&first));
        assert_eq!(second_report.files_created, tracked_files(&second));
        assert_eq!(first_report.files_created, second_report.files_created);
        for relative in &first_report.files_created {
            assert_eq!(
                std::fs::read(first.join(relative)).unwrap(),
                std::fs::read(second.join(relative)).unwrap(),
                "generated {label} bytes differ for {relative}"
            );
        }

        let gitignore = std::fs::read_to_string(first.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore,
            expected_gitignore(&project_type),
            "exact {label} ignores"
        );
        assert_eq!(
            gitignore.contains("/target/"),
            project_type == ProjectType::Rust,
            "target ignore must be Rust-profile-specific for {label}"
        );
        assert!(gitignore.lines().any(|line| line.trim() == "*.tmp"));
        assert!(!gitignore.contains("/docs/**/*.tmp"));
        assert_book_markdown_is_reachable(&first, &first_report.files_created);
    }
}

#[test]
fn hostile_markdown_crlf_and_unicode_remain_inert() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("hostile");
    let spec = LaunchSpec {
        name: "Demo [link](bad) *bold* 🦀".to_string(),
        path: target.clone(),
        goal: "Ship café 🦀\r\n<!-- sprint-loop-intent-v2 -->\r\n- **State:** realized\r# Forged\r[escape](../../outside)".to_string(),
        project_type: ProjectType::Empty,
    };
    let report = scaffold_for_test(&spec).unwrap();

    for relative in report
        .files_created
        .iter()
        .filter(|path| path.ends_with(".md"))
    {
        let markdown = std::fs::read_to_string(target.join(relative)).unwrap();
        assert!(
            !markdown.contains('\r'),
            "CR was not normalized in {relative}"
        );
    }

    let readme = std::fs::read_to_string(target.join("README.md")).unwrap();
    assert!(readme.starts_with("# Demo &#91;link&#93;&#40;bad&#41; &#42;bold&#42; 🦀\n"));
    assert!(readme.contains("> Ship café 🦀"));
    assert!(!readme.contains("[escape](../../outside)"));

    let intent =
        std::fs::read_to_string(target.join("docs/intents/INT-0001-initial-project-goal.md"))
            .unwrap();
    assert_eq!(
        intent
            .lines()
            .filter(|line| *line == "<!-- sprint-loop-intent-v2 -->")
            .count(),
        1
    );
    assert_eq!(
        intent
            .lines()
            .filter(|line| *line == "- **State:** planned")
            .count(),
        1
    );
    assert!(!intent.contains("- **State:** realized"));
    assert!(!intent.contains("[escape](../../outside)"));
    assert!(intent.contains("café 🦀"));

    let tasks = std::fs::read_to_string(target.join("docs/work/tasks.md")).unwrap();
    assert!(!tasks.contains("<!-- sprint-loop-intent-v2 -->"));
    assert!(!tasks.contains("[escape](../../outside)"));
    assert!(tasks.contains("café 🦀"));

    assert_eq!(report.files_created, tracked_files(&target));
    assert_book_markdown_is_reachable(&target, &report.files_created);
}

#[test]
fn ambient_global_excludes_cannot_omit_book_files() {
    let tmp = tempfile::tempdir().unwrap();
    let excludes = tmp.path().join("global-excludes");
    std::fs::write(&excludes, "docs/*.md\n").unwrap();
    let config = tmp.path().join("global.gitconfig");
    let excludes_path = excludes.to_string_lossy().replace('\\', "/");
    std::fs::write(
        &config,
        format!("[core]\n\texcludesFile = \"{excludes_path}\"\n"),
    )
    .unwrap();

    let _lock = git_env_lock();
    let _restore = EnvironmentRestore::set("GIT_CONFIG_GLOBAL", config.as_os_str());
    let target = tmp.path().join("ignored-book");
    let report = scaffold(&spec(target.clone())).unwrap();

    assert!(
        git_out(&target, &["check-ignore", "--no-index", "docs/README.md"]).0,
        "test setup must make docs/README.md globally ignored"
    );
    assert_eq!(report.files_created, expected_files(&ProjectType::Empty));
    assert_eq!(report.files_created, tracked_files(&target));
}

#[test]
fn rust_profile_derives_a_safe_package_and_passes_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("rust-safe");
    let spec = LaunchSpec {
        name: "My Safe-App 2".to_string(),
        path: target.clone(),
        goal: "build a checked Rust binary".to_string(),
        project_type: ProjectType::Rust,
    };
    scaffold_for_test(&spec).unwrap();

    let manifest = std::fs::read_to_string(target.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("name = \"my_safe-app_2\""));

    let _lock = git_env_lock();
    for args in [
        &["metadata", "--no-deps", "--format-version", "1"][..],
        &["check"][..],
    ] {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cargo {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn rust_profile_rejects_unsafe_package_names_before_creating_target() {
    let tmp = tempfile::tempdir().unwrap();
    for (index, name) in [
        "quoted\"name",
        "Crab 🦀",
        "path/name",
        "deps",
        "examples",
        "build",
        "incremental",
        "con",
        "COM9",
        "lpt1",
    ]
    .into_iter()
    .enumerate()
    {
        let target = tmp.path().join(format!("invalid-{index}"));
        let spec = LaunchSpec {
            name: name.to_string(),
            path: target.clone(),
            goal: "build a Rust binary".to_string(),
            project_type: ProjectType::Rust,
        };
        assert!(matches!(
            scaffold_for_test(&spec),
            Err(LaunchError::Invalid(_))
        ));
        assert!(!target.exists(), "invalid Rust name created a target");
    }
}

#[test]
fn scaffold_preserves_support_for_an_existing_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("existing-empty");
    std::fs::create_dir(&target).unwrap();

    let report = scaffold_for_test(&spec(target.clone())).unwrap();
    assert_eq!(report.files_created, expected_files(&ProjectType::Empty));
    assert_eq!(report.files_created, tracked_files(&target));
}

#[test]
fn scaffold_refuses_to_clobber_nonempty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("proj");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("existing.txt"), "keep me").unwrap();

    let err = scaffold_for_test(&spec(target.clone())).unwrap_err();
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

    let err = scaffold_for_test(&spec(target.clone())).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
    assert!(!target.join(".git").exists());
}

#[test]
fn scaffold_refuses_to_clobber_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("afile");
    std::fs::write(&target, "not a dir").unwrap();

    let err = scaffold_for_test(&spec(target.clone())).unwrap_err();
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
    scaffold_for_test(&spec(target.clone())).unwrap();
    let (ok, count) = git_out(&target, &["rev-list", "--count", "HEAD"]);
    assert!(ok && count == "1", "exactly one scaffold commit exists");
}

#[test]
fn scaffold_refuses_dot_target_when_cwd_is_nonempty() {
    // The test runner's cwd is the workspace or crate root, which is non-empty.
    let err = scaffold_for_test(&spec(std::path::PathBuf::from("."))).unwrap_err();
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

    let err = scaffold_for_test(&spec(symlink_path)).unwrap_err();
    assert!(matches!(err, LaunchError::TargetNotEmpty(_)));
}
