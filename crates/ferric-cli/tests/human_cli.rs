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

#[cfg(feature = "backend-openai")]
#[test]
fn human_read_only_admission_failure_has_one_safe_action() {
    let ambient = tempfile::tempdir().unwrap();
    let selected = tempfile::tempdir().unwrap();
    let models = selected.path().join("models");
    std::fs::create_dir(&models).unwrap();
    let model = models.join("invalid.gguf");
    let bytes = [b'!'; 24];
    std::fs::write(&model, bytes).unwrap();

    for action in ["status", "explain"] {
        let result = output(
            command(ambient.path())
                .args([action, "--workspace"])
                .arg(selected.path())
                .arg("--json"),
        );
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        let diagnostic = String::from_utf8(result.stderr).unwrap();
        assert_eq!(
            diagnostic.trim(),
            "The selected file is not a supported GGUF version 2 or 3 model. Inspect the model and server configuration for the selected folder."
        );
        assert_eq!(diagnostic.matches("Inspect ").count(), 1);
        assert_eq!(diagnostic.lines().count(), 1);
        assert!(!diagnostic.contains("ferric explain"));
        assert!(!diagnostic.contains("Engine diagnostics"));
        assert!(!diagnostic.contains('\u{1b}'));
        assert_eq!(std::fs::read(&model).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(selected.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(&models).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(ambient.path()).unwrap().count(), 0);
        assert!(!selected.path().join(".ferric").exists());
        assert!(!selected.path().join(".ferric-startup.lock").exists());
    }
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

#[cfg(feature = "backend-openai")]
#[test]
fn human_invalid_config_blocks_before_preparation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".ferric")).unwrap();
    let config = "api_key = 'never-print-this-credential'\ntemperature = 'invalid'\n";
    std::fs::write(root.path().join(".ferric/config.toml"), config).unwrap();
    for arguments in [
        vec!["run", "hello"],
        vec!["status"],
        vec!["explain", "--json"],
    ] {
        let result = output(command(root.path()).args(arguments));
        assert_eq!(result.status.code(), Some(1));
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!diagnostic.contains("never-print-this-credential"));
        assert!(diagnostic.contains("config"), "{diagnostic}");
        assert!(!root.path().join(".ferric-startup.lock").exists());
        assert_eq!(
            std::fs::read_dir(root.path().join(".ferric"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join(".ferric/config.toml")).unwrap(),
            config
        );
    }
}

#[test]
fn advanced_original_commands_compatible() {
    let root = tempfile::tempdir().unwrap();
    let names = [
        "query",
        "bench",
        "mcp",
        "chat",
        "launch",
        "icm",
        "cron",
        "revert",
        "dream",
        "server",
        "skills",
        "trace",
        #[cfg(feature = "backend-openai")]
        "api",
    ];
    for name in names {
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

#[cfg(feature = "backend-openai")]
#[test]
fn human_explain_does_not_contact_endpoint_or_prepare() {
    let root = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::create_dir(root.path().join(".ferric")).unwrap();
    std::fs::write(
        root.path().join(".ferric/config.toml"),
        format!(
            "api_base = 'http://{}/v1'\napi_key = 'private-credential'\n",
            listener.local_addr().unwrap()
        ),
    )
    .unwrap();
    for name in ["status", "explain"] {
        let result = output(command(root.path()).args([name, "--json"]));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(summary["context"], 4096);
        assert!(
            summary["resource_policy"]
                .as_str()
                .unwrap()
                .contains("borrowed-server resources are unverified")
        );
        assert!(summary["ownership"].as_str().unwrap().contains("borrowed"));
        assert!(summary["effects"].as_str().unwrap().contains("no network"));
        assert!(
            summary["qualification"]
                .as_str()
                .unwrap()
                .starts_with("unqualified")
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains("private-credential"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!root.path().join(".ferric-startup.lock").exists());
        assert_eq!(
            std::fs::read_dir(root.path().join(".ferric"))
                .unwrap()
                .count(),
            1
        );
    }
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
