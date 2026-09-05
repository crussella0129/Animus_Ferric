//! Cargo-driven process-level front-door checks. Every command uses the shared
//! bounded source owner, which proves child cleanup before returning output.
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

#[path = "../src/test_process_containment.rs"]
mod test_process_containment;

fn command(workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
    command
        .current_dir(workspace)
        .env("APPDATA", workspace.join("unused-user-config"))
        .env("XDG_CONFIG_HOME", workspace.join("unused-user-config"))
        .env("HOME", workspace.join("unused-user-config"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("FERRIC_PROMPTS_DIR")
        .env_remove("FERRIC_LOG")
        .env_remove("RUST_LOG");
    command
}

fn output(command: &mut Command) -> Output {
    test_process_containment::output_bounded(command, Duration::from_secs(30))
        .expect("bounded command finished and all owned children were reaped")
}

#[test]
fn no_args_non_tty_welcome_is_nonmutating() {
    let root = tempfile::tempdir().unwrap();
    for arguments in [vec![], vec!["run"]] {
        let result = output(command(root.path()).args(arguments));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let welcome = String::from_utf8(result.stdout).unwrap();
        assert!(welcome.contains("Ferric"));
        assert!(welcome.contains("Ask mode"));
        assert!(welcome.lines().count() <= 12, "{welcome}");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    // Even a present malformed config is not opened on this noninteractive
    // welcome path. It must not accidentally turn the welcome into preflight.
    std::fs::create_dir(root.path().join(".ferric")).unwrap();
    std::fs::write(root.path().join(".ferric/config.toml"), "malformed").unwrap();
    let result = output(&mut command(root.path()));
    assert!(result.status.success());
    assert!(!root.path().join(".ferric-startup.lock").exists());
    assert_eq!(
        std::fs::read_dir(root.path().join(".ferric"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn malformed_explicit_commands_remain_usage_errors() {
    let root = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["not-a-command"],
        vec!["run", "--not-an-option"],
        vec!["advanced", "not-a-command"],
        vec!["advanced", "--", "-v"],
    ] {
        let result = output(command(root.path()).args(arguments));
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn primary_help_is_compact() {
    let root = tempfile::tempdir().unwrap();
    let result = output(command(root.path()).arg("--help"));
    assert!(result.status.success());
    let help = String::from_utf8(result.stdout).unwrap();
    let commands = help
        .split("Commands:\n")
        .nth(1)
        .expect("command section")
        .split("\nOptions:")
        .next()
        .unwrap();
    let names: Vec<_> = commands
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(names, ["run", "status", "explain", "advanced"]);
    assert!(!help.contains("--params-b"));
    assert!(!help.contains("--gpu-layers"));
}

#[test]
fn advanced_original_commands_compatible() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "query", "bench", "mcp", "chat", "launch", "icm", "cron", "revert", "dream", "server",
        "skills", "trace",
    ] {
        let direct = output(command(root.path()).args([name, "--help"]));
        let advanced = output(command(root.path()).args(["advanced", name, "--help"]));
        assert!(direct.status.success(), "direct {name}");
        assert!(
            advanced.status.success(),
            "advanced {name}: {}",
            String::from_utf8_lossy(&advanced.stderr)
        );
        assert_eq!(
            String::from_utf8(direct.stdout).unwrap(),
            String::from_utf8(advanced.stdout).unwrap(),
            "delegation preserves original parser: {name}"
        );
    }
    let result =
        output(command(root.path()).args(["advanced", "query", "--mock", "--no-config", "hello"]));
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(root.path().join(".ferric/trace").is_dir());
}

#[test]
fn cargo_default_launch_selects_real_cli() {
    let workspace: toml::Value = toml::from_str(include_str!("../../../Cargo.toml")).unwrap();
    assert_eq!(
        workspace["workspace"]["default-members"]
            .as_array()
            .unwrap(),
        &[toml::Value::String("crates/ferric-cli".into())]
    );
    let cli: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert_eq!(cli["package"]["default-run"].as_str(), Some("ferric"));
    assert!(
        cli["features"]["default"]
            .as_array()
            .unwrap()
            .contains(&toml::Value::String("backend-openai".into()))
    );
}

#[cfg(not(feature = "backend-openai"))]
#[test]
fn no_default_features_welcome_and_mock_compatibility() {
    let root = tempfile::tempdir().unwrap();
    let result = output(&mut command(root.path()));
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("no real backend")
    );
    let result = output(command(root.path()).args(["query", "--mock", "--no-config", "hello"]));
    assert!(result.status.success());
}
