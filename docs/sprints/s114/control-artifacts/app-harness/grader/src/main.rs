use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mh_rs01_grader::grade_candidate;

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("mh-rs01-grader: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: impl Iterator<Item = std::ffi::OsString>) -> io::Result<ExitCode> {
    let mut candidate = None::<PathBuf>;
    let mut seed = None::<PathBuf>;
    let mut results = None::<PathBuf>;
    let arguments: Vec<_> = arguments.collect();
    let mut index = 0usize;
    while index < arguments.len() {
        match arguments[index].to_str() {
            Some("--candidate") => candidate = Some(option_path(&arguments, &mut index)?),
            Some("--seed") => seed = Some(option_path(&arguments, &mut index)?),
            Some("--results") => results = Some(option_path(&arguments, &mut index)?),
            Some("--help" | "-h") => {
                println!(
                    "Usage: mh-rs01-grader --candidate <path> [--seed <path>] [--results <path>]"
                );
                return Ok(ExitCode::SUCCESS);
            }
            Some(other) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown argument: {other}"),
                ));
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "arguments must be valid Unicode",
                ));
            }
        }
        index += 1;
    }

    let candidate = candidate
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--candidate is required"))?;
    let seed = seed.unwrap_or_else(default_seed);
    if let Some(results_path) = results.as_deref()
        && path_is_within(results_path, &candidate)?
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--results must be outside the candidate workspace",
        ));
    }

    let report = grade_candidate(&candidate, &seed)?;
    let jsonl = report.to_jsonl();
    print!("{jsonl}");
    io::stdout().flush()?;
    if let Some(path) = results {
        write_atomic(&path, jsonl.as_bytes())?;
    }

    Ok(if report.execution_allowed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn option_path(arguments: &[std::ffi::OsString], index: &mut usize) -> io::Result<PathBuf> {
    *index += 1;
    arguments
        .get(*index)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "option requires a path"))
}

fn default_seed() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("grader has app-harness parent")
        .join("seed")
}

fn path_is_within(path: &Path, directory: &Path) -> io::Result<bool> {
    let absolute_directory = fs::canonicalize(directory)?;
    let absolute_path = if path.exists() {
        fs::canonicalize(path)?
    } else {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "results path has no filename")
        })?;
        fs::canonicalize(parent)?.join(file_name)
    };
    Ok(absolute_path.starts_with(absolute_directory))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "results path has no filename")
    })?;
    let temporary = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));
    fs::write(&temporary, bytes)?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}
