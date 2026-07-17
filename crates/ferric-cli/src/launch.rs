//! `ferric launch` — the Animus Launch surface (ADR-053): a deterministic,
//! LLM-free project bootstrapper. Supplied flags skip the matching interview
//! question; anything missing is asked on stdin (prompts to STDERR so stdout
//! stays the final report). The answer→spec logic (`spec_from_answers`) is pure
//! and unit-tested; `animus_launch::scaffold` does the actual git+skeleton work.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use animus_launch::{LaunchSpec, ProjectType, scaffold, validate_goal, validate_project_name};

/// `ferric launch`'s CLI surface. All fields are optional — any not supplied is
/// asked interactively (in the fixed order name → path → goal).
#[derive(Args)]
pub struct LaunchArgs {
    /// Project name.
    #[arg(long)]
    pub name: Option<String>,
    /// Local path to create the project at (must be absent or an empty dir).
    #[arg(long)]
    pub path: Option<PathBuf>,
    /// One-line goal for the project (seeds the README + initial tasks).
    #[arg(long)]
    pub goal: Option<String>,
    /// Optional free-form project-type hint.
    #[arg(long)]
    pub project_type: Option<String>,
}

/// Validate + build a `LaunchSpec` from collected answers. Pure — unit-tested.
pub(crate) fn spec_from_answers(
    name: &str,
    path: &str,
    goal: &str,
    project_type: Option<&str>,
) -> Result<LaunchSpec, String> {
    validate_project_name(name)?;
    validate_goal(goal)?;
    if path.trim().is_empty() {
        return Err("path must not be empty".to_string());
    }
    let pt = match project_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t
            .parse::<ProjectType>()
            .map_err(|e| format!("invalid project type: {}", e))?,
        None => ProjectType::Empty,
    };
    Ok(LaunchSpec {
        name: name.trim().to_string(),
        path: PathBuf::from(path.trim()),
        goal: goal.trim().to_string(),
        project_type: pt,
    })
}

/// Print `question` to STDERR (keeping stdout for the report) and read one line
/// from stdin. Returns the trimmed line.
fn prompt_line(question: &str) -> std::io::Result<String> {
    eprint!("{question} ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

pub fn run_launch(args: LaunchArgs) -> ExitCode {
    // Fill each missing field via a prompt, in the fixed order name → path → goal.
    let name = match args.name {
        Some(n) => n,
        None => match prompt_line("Project name:") {
            Ok(n) => n,
            Err(e) => {
                eprintln!("input error: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let path = match args.path {
        Some(p) => p.display().to_string(),
        None => match prompt_line("Local path:") {
            Ok(p) => p,
            Err(e) => {
                eprintln!("input error: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let goal = match args.goal {
        Some(g) => g,
        None => match prompt_line("Goal (one line):") {
            Ok(g) => g,
            Err(e) => {
                eprintln!("input error: {e}");
                return ExitCode::FAILURE;
            }
        },
    };

    let spec = match spec_from_answers(&name, &path, &goal, args.project_type.as_deref()) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("cannot launch: {e}");
            return ExitCode::FAILURE;
        }
    };

    match scaffold(&spec) {
        Ok(report) => {
            println!(
                "Scaffolded '{}' at {} (branches: {}).",
                spec.name,
                report.path.display(),
                report.branches.join(", ")
            );
            println!("Files: {}", report.files_created.join(", "));

            // Increment 2: Begin Work Auto-Hand-Off
            if let Ok(ans) = prompt_line("Begin work now? (y/N)")
                && (ans.trim().to_lowercase() == "y" || ans.trim().to_lowercase() == "yes")
            {
                println!("Handing off to ferric query...");
                let query_args = crate::query::QueryArgs {
                    prompt: Some(spec.goal.clone()),
                    files: vec![],
                    workspace: Some(report.path.clone()),
                    mock: false, // You could add args or let config handle backend etc.
                    backend_opts: crate::backend::BackendOpts {
                        backend: Some(crate::backend::BackendArg::Openai),
                        model: None,
                        api_base: None,
                        api_key: None,
                    },
                    params_b: Some(7.0),
                    quant: Some("q4".to_string()),
                    family: Some("llama3".to_string()),
                    ctx: Some(8192),
                    temperature: Some(0.0),
                    protocol: None,
                    prompts_dir: None,
                    max_ring: None,
                    profile_dir: Some(PathBuf::from(".ferric/profiles")),
                    stream: false,
                    resume: None,
                    modality: None,
                    research: None,
                    sink_action: "deny".to_string(),
                };
                return crate::query::run_query(query_args);
            }
            println!(
                "Next: cd {} && begin work with the Loop.",
                report.path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("launch failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_from_answers_builds_valid_spec() {
        let spec = spec_from_answers("demo", "/tmp/demo", "build a thing", None).unwrap();
        assert_eq!(spec.name, "demo");
        assert_eq!(spec.path, PathBuf::from("/tmp/demo"));
        assert_eq!(spec.goal, "build a thing");
        assert_eq!(spec.project_type, ProjectType::Empty);
        // Trimming + a project-type passthrough.
        let spec2 = spec_from_answers("  demo  ", " /tmp/x ", " goal ", Some(" rust ")).unwrap();
        assert_eq!(spec2.name, "demo");
        assert_eq!(spec2.project_type, ProjectType::Rust);
    }

    #[test]
    fn spec_from_answers_rejects_invalid() {
        assert!(spec_from_answers("", "/tmp/x", "goal", None).is_err()); // empty name
        assert!(spec_from_answers("demo", "/tmp/x", "", None).is_err()); // empty goal
        assert!(spec_from_answers("demo", "", "goal", None).is_err()); // empty path
    }
}
