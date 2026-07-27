//! `ferric skills` — see what is installed, and whether it is authorized.
//!
//! Listing is deliberately separate from running. The whole point of ADR-091 is
//! that a skill on disk does nothing until a human says so, which means there
//! has to be a way to look at what is on disk *without* that being the act of
//! authorizing it.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum SkillsCommand {
    /// List installed skills and how each would be authorized.
    List(SkillsListArgs),
}

#[derive(Args)]
pub struct SkillsListArgs {
    /// Workspace root (defaults to the current directory).
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

pub fn run(command: SkillsCommand) -> ExitCode {
    match command {
        SkillsCommand::List(args) => list(args),
    }
}

fn list(args: SkillsListArgs) -> ExitCode {
    let root = args.workspace.unwrap_or_else(|| PathBuf::from("."));

    let (skills, errors) = ferric_skills::discover(&root);
    let loaded = crate::config::load_layered(&root);
    let allowed = loaded.config.allowed_skills.unwrap_or_default();

    let dir = ferric_skills::skills_dir(&root);
    if skills.is_empty() && errors.is_empty() {
        println!("No skills installed in {}", dir.display());
        println!(
            "Install one by creating {}/<name>/SKILL.md with `name` and `description` frontmatter.",
            dir.display()
        );
        return ExitCode::SUCCESS;
    }

    if !skills.is_empty() {
        println!("{} skill(s) in {}:\n", skills.len(), dir.display());
        for s in &skills {
            // Say plainly whether this would do anything, because "installed"
            // and "in effect" are exactly the two things ADR-091 separates and
            // a listing that blurred them would undo the point.
            let status = if allowed.contains(&s.name) {
                "authorized (config allowlist)"
            } else {
                "installed — needs `--skill` or an allowlist entry"
            };
            println!("  {:<24} {}", s.name, status);
            println!("  {:<24} {}", "", s.description);
        }
    }

    // Malformed skills are reported, never silently omitted: a skill the user
    // installed and cannot see listed is the confusing case.
    if !errors.is_empty() {
        println!("\n{} skill(s) could not be loaded:", errors.len());
        for e in &errors {
            println!("  {e}");
        }
    }

    ExitCode::SUCCESS
}
