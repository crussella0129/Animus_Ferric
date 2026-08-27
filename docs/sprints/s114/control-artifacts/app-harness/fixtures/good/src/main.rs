use std::path::PathBuf;

use release_plan::{build_plan, parse_manifest};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "expected exactly one manifest path".to_string())?;
    if arguments.next().is_some() {
        return Err("expected exactly one manifest path".to_string());
    }

    let input = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let jobs = parse_manifest(&input).map_err(|error| error.to_string())?;
    let plan = build_plan(&jobs).map_err(|error| error.to_string())?;
    for id in plan {
        println!("{id}");
    }
    Ok(())
}
