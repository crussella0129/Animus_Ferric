//! Animus Launch — the deterministic, LLM-free project bootstrapper (ADR-053).
//!
//! `scaffold()` takes a [`LaunchSpec`] and creates a new git repository at a
//! target path: `main` + `dev` branches, an initial commit, and a
//! sprint-loop-ready skeleton (README / `.gitignore` / `agent-tasks/` /
//! `decisions.md`). It is **user-run, deterministic, and no LLM is involved** —
//! so `ferric-guard`'s workspace-containment (which confines an *agent*) does
//! not apply. The one real safety property is **refuse-to-clobber**: never
//! scaffold into a path that already exists and is non-empty (or is not a
//! directory). Git is invoked as a closed set of subcommands via
//! `std::process::Command` (a named subprocess boundary, ADR-013), each step's
//! exit status checked and stderr captured on failure.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use thiserror::Error;

/// Supported project profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Python,
    Web,
    Empty,
}

impl FromStr for ProjectType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "rust" => Ok(ProjectType::Rust),
            "python" | "py" => Ok(ProjectType::Python),
            "web" | "html" => Ok(ProjectType::Web),
            "empty" | "none" | "" => Ok(ProjectType::Empty),
            _ => Err(format!("Unknown project type: {}", s)),
        }
    }
}

/// Everything Launch needs to bootstrap a project. Built by the CLI (from flags
/// and/or an interview) and handed to [`scaffold`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub name: String,
    pub path: PathBuf,
    pub goal: String,
    /// A defined project-type profile to scaffold. Defaults to Empty.
    pub project_type: ProjectType,
}

/// What `scaffold` created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldReport {
    pub path: PathBuf,
    pub branches: Vec<String>,
    pub files_created: Vec<String>,
}

#[derive(Debug, Error)]
pub enum LaunchError {
    /// The target already exists and is non-empty, or is not a directory —
    /// refuse-to-clobber (ADR-053, the sole safety property).
    #[error("refusing to scaffold into {0}: it already exists and is not an empty directory")]
    TargetNotEmpty(PathBuf),
    #[error("invalid launch spec: {0}")]
    Invalid(String),
    #[error("git failed: {0}")]
    Git(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// A project name is valid if it is non-empty after trimming.
pub fn validate_project_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("project name must not be empty".to_string());
    }
    Ok(())
}

/// A goal is valid if it is non-empty after trimming.
pub fn validate_goal(goal: &str) -> Result<(), String> {
    if goal.trim().is_empty() {
        return Err("goal must not be empty".to_string());
    }
    Ok(())
}

/// Derive a few seed backlog bullets from the goal (echoing GECK's
/// `_derive_initial_tasks`/`_parse_goal_to_bullets`). Splits the goal on
/// sentence/clause separators; always returns at least one task. Pure.
pub fn derive_initial_tasks(goal: &str) -> Vec<String> {
    let clauses: Vec<String> = goal
        .split(['.', ';', ',', '\n'])
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            // Capitalize the first character for a task-like reading.
            let mut chars = c.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if clauses.is_empty() {
        // A goal with no clause separators becomes one task.
        return vec![format!("Implement: {}", goal.trim())];
    }
    clauses
}

/// Is `path` a safe scaffold target? Safe iff it does not exist, or it exists
/// as a directory that is empty (hidden entries like `.git`/`.DS_Store` COUNT
/// as non-empty — the stricter, safer rule). A path that exists but is not a
/// directory is NOT safe.
fn target_is_clobber_safe(path: &Path) -> Result<bool, std::io::Error> {
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    // Directory: empty iff it yields no entries at all.
    Ok(path.read_dir()?.next().is_none())
}

/// Run one `git` subcommand in `cwd`, returning `Ok(())` on success and
/// `LaunchError::Git(stderr)` on any non-zero exit or spawn failure. Git is
/// invoked as a closed set of subcommands, never a shell (ADR-013/ADR-053).
fn git(cwd: &Path, args: &[&str]) -> Result<(), LaunchError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| LaunchError::Git(format!("could not run `git {}`: {e}", args.join(" "))))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(LaunchError::Git(format!(
            "`git {}` failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }
    Ok(())
}

/// Bootstrap a new project repo per `spec`. Deterministic, no LLM. See the
/// module doc for the full contract.
pub fn scaffold(spec: &LaunchSpec) -> Result<ScaffoldReport, LaunchError> {
    // 1. Refuse-to-clobber (the sole safety property).
    if !target_is_clobber_safe(&spec.path)? {
        return Err(LaunchError::TargetNotEmpty(spec.path.clone()));
    }
    // 2. Validate.
    validate_project_name(&spec.name).map_err(LaunchError::Invalid)?;
    validate_goal(&spec.goal).map_err(LaunchError::Invalid)?;

    // 3. Create the root + the nested agent-tasks/ dir (fs::write won't create
    //    parents).
    std::fs::create_dir_all(&spec.path)?;
    std::fs::create_dir_all(spec.path.join("agent-tasks"))?;

    // 4. Seed skeleton.
    let type_line = match spec.project_type {
        ProjectType::Empty => String::new(),
        _ => format!("\n\n**Type:** {:?}", spec.project_type),
    };
    let readme = format!(
        "# {}\n\n{}{}\n",
        spec.name.trim(),
        spec.goal.trim(),
        type_line
    );
    std::fs::write(spec.path.join("README.md"), readme)?;

    let mut gitignore = String::from(
        "# Sprint-loop working memory (ephemeral) + build output.\nsprints/\ntarget/\n*.tmp\n",
    );
    let mut files_created = vec![
        "README.md".to_string(),
        ".gitignore".to_string(),
        "agent-tasks/agent-tasks.md".to_string(),
        "decisions.md".to_string(),
    ];

    // Scaffold based on project profile
    match spec.project_type {
        ProjectType::Rust => {
            // The empty `[workspace]` is load-bearing, not boilerplate. Cargo
            // walks *upward* looking for a workspace root, so a project
            // scaffolded anywhere inside another Rust tree gets adopted by it —
            // and if that ancestor manifest is not a valid workspace, `cargo
            // build` fails while naming a file this scaffold never wrote.
            //
            // Found by launching into a scratch dir under a home directory that
            // happened to contain a stray `Cargo.toml`: the fresh project failed
            // with "no targets specified in the manifest", naming that ancestor
            // manifest — a file the user had never heard of and the scaffold had
            // never written. Declaring its own workspace stops the upward walk,
            // which is the standard idiom for a standalone nested project.
            let cargo_toml = format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n\n[workspace]\n",
                spec.name.to_lowercase().replace(" ", "_")
            );
            std::fs::write(spec.path.join("Cargo.toml"), cargo_toml)?;
            files_created.push("Cargo.toml".to_string());

            std::fs::create_dir_all(spec.path.join("src"))?;
            std::fs::write(
                spec.path.join("src").join("main.rs"),
                "fn main() {\n    println!(\"Hello, world!\");\n}\n",
            )?;
            files_created.push("src/main.rs".to_string());
        }
        ProjectType::Python => {
            std::fs::write(spec.path.join("requirements.txt"), "")?;
            files_created.push("requirements.txt".to_string());

            std::fs::create_dir_all(spec.path.join("src"))?;
            std::fs::write(
                spec.path.join("src").join("main.py"),
                "def main():\n    print(\"Hello, world!\")\n\nif __name__ == \"__main__\":\n    main()\n",
            )?;
            files_created.push("src/main.py".to_string());
            gitignore.push_str("__pycache__/\n*.pyc\n.venv/\nvenv/\n");
        }
        ProjectType::Web => {
            std::fs::write(
                spec.path.join("index.html"),
                "<!DOCTYPE html>\n<html>\n<head>\n    <title>App</title>\n    <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n    <h1>App</h1>\n    <script src=\"app.js\"></script>\n</body>\n</html>\n",
            )?;
            files_created.push("index.html".to_string());
            std::fs::write(
                spec.path.join("style.css"),
                "body {\n    font-family: sans-serif;\n}\n",
            )?;
            files_created.push("style.css".to_string());
            std::fs::write(
                spec.path.join("app.js"),
                "console.log(\"Hello, world!\");\n",
            )?;
            files_created.push("app.js".to_string());
        }
        ProjectType::Empty => {}
    }

    std::fs::write(spec.path.join(".gitignore"), gitignore)?;

    let tasks = derive_initial_tasks(&spec.goal);
    let mut agent_tasks = String::from("# Agent Tasks (Persistent Backlog)\n\n");
    for task in &tasks {
        agent_tasks.push_str(&format!("- [ ] {task}\n"));
    }
    std::fs::write(
        spec.path.join("agent-tasks").join("agent-tasks.md"),
        agent_tasks,
    )?;

    std::fs::write(
        spec.path.join("decisions.md"),
        "# Architectural Decisions\n",
    )?;

    // 5. Git: init → add → commit (fixed identity so CI without a global git
    //    identity still commits) → rename default branch to main → create dev.
    git(&spec.path, &["init"])?;
    git(&spec.path, &["add", "-A"])?;
    git(
        &spec.path,
        &[
            "-c",
            "user.name=Animus Launch",
            "-c",
            "user.email=launch@animus.local",
            "commit",
            "-m",
            "Initial scaffold (Animus Launch)",
        ],
    )?;
    git(&spec.path, &["branch", "-M", "main"])?;
    git(&spec.path, &["branch", "dev"])?;

    Ok(ScaffoldReport {
        path: spec.path.clone(),
        branches: vec!["main".to_string(), "dev".to_string()],
        files_created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_project_name_rejects_empty() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("   ").is_err());
        assert!(validate_project_name("demo").is_ok());
    }

    #[test]
    fn validate_goal_rejects_empty() {
        assert!(validate_goal("").is_err());
        assert!(validate_goal("\t\n ").is_err());
        assert!(validate_goal("build a thing").is_ok());
    }

    #[test]
    fn derive_initial_tasks_nonempty() {
        let tasks = derive_initial_tasks("build a parser; add tests, then document it");
        assert!(!tasks.is_empty());
        // Each clause becomes a task referencing the goal's content.
        assert!(tasks.iter().any(|t| t.contains("parser")));
        assert!(tasks.iter().any(|t| t.contains("tests")));
        assert!(tasks.iter().any(|t| t.contains("document")));
        // A separator-free goal still yields one task.
        let single = derive_initial_tasks("a tiny CLI");
        assert_eq!(single.len(), 1);
        assert!(single[0].contains("tiny CLI"));
    }

    #[test]
    fn target_clobber_safety_rules() {
        let dir = tempfile::tempdir().unwrap();
        // Absent path → safe.
        assert!(target_is_clobber_safe(&dir.path().join("new")).unwrap());
        // Empty dir → safe.
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(target_is_clobber_safe(&empty).unwrap());
        // Dir with a hidden-only entry → NOT safe.
        let hidden = dir.path().join("hidden");
        std::fs::create_dir(&hidden).unwrap();
        std::fs::write(hidden.join(".keep"), "x").unwrap();
        assert!(!target_is_clobber_safe(&hidden).unwrap());
        // Existing FILE → NOT safe.
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(!target_is_clobber_safe(&file).unwrap());
    }

    /// A scaffolded Rust project must declare its own workspace.
    ///
    /// Cargo resolves the workspace root by walking **up** the directory tree,
    /// so without this a fresh project nested anywhere inside another Rust tree
    /// is silently adopted by it — and when that ancestor is not a valid
    /// workspace, `cargo build` fails citing a manifest the user never wrote.
    /// Observed live: launching under a home directory that contains a stray
    /// `Cargo.toml` produced a project that would not compile.
    ///
    /// The assertion is on the emitted manifest rather than on a `cargo build`,
    /// so it holds regardless of what happens to sit above the test's tempdir.
    #[test]
    fn a_scaffolded_rust_project_declares_its_own_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("proj");
        let spec = LaunchSpec {
            name: "proj".to_string(),
            path: path.clone(),
            goal: "verify the scaffold".to_string(),
            project_type: ProjectType::Rust,
        };
        scaffold(&spec).expect("scaffold must succeed");

        let manifest = std::fs::read_to_string(path.join("Cargo.toml")).expect("Cargo.toml");
        assert!(
            manifest.contains("[workspace]"),
            "a standalone scaffold must stop cargo's upward workspace search:
{manifest}"
        );
        assert!(
            path.join("src/main.rs").is_file(),
            "the manifest promises a binary target, so src/main.rs must exist"
        );
    }
}
