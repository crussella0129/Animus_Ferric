//! The first-run docs must describe the source-selected binary and its real
//! parser. Help-only children use the same bounded, reaping owner as journeys.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[path = "../src/test_process_containment.rs"]
mod test_process_containment;

fn help(workspace: &Path, arguments: &[&str]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferric"));
    command
        .current_dir(workspace)
        .env("APPDATA", workspace.join("unused-user-config"))
        .env("XDG_CONFIG_HOME", workspace.join("unused-user-config"))
        .env("HOME", workspace.join("unused-user-config"))
        .env_remove("OPENAI_API_KEY")
        .env_remove("FERRIC_PROMPTS_DIR")
        .env_remove("FERRIC_LOG")
        .env_remove("RUST_LOG")
        .args(arguments)
        .arg("--help");
    let output = test_process_containment::output_bounded(&mut command, Duration::from_secs(30))
        .expect("help finished and all owned children were reaped");
    assert!(
        output.status.success(),
        "help for {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_dir(workspace).unwrap().count(),
        0,
        "help must not prepare resources or write session state"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

fn documented_actions(markdown: &str) -> Vec<&str> {
    markdown
        .lines()
        .filter_map(|line| line.strip_prefix("| `ferric "))
        .filter_map(|line| line.split([' ', '`']).next())
        .collect()
}

#[test]
fn first_run_docs_match_cli() {
    let readme = include_str!("../../../README.md");
    assert!(
        !readme
            .lines()
            .any(|line| line.starts_with('#') && line.to_ascii_lowercase().contains("sprint")),
        "README must not grow a sprint-history ledger"
    );
    let commands = include_str!("../../../docs/commands.md");
    for (name, markdown) in [("README", readme), ("commands", commands)] {
        let mut lines = markdown.lines();
        assert_eq!(
            lines.find(|line| line.starts_with("```")),
            Some("```sh"),
            "{name}: first example is the source-driven launch"
        );
        assert_eq!(lines.next(), Some("cargo r"), "{name}: first command");
        assert_eq!(lines.next(), Some("```"), "{name}: no setup checklist");
    }

    let workspace: toml::Value = toml::from_str(include_str!("../../../Cargo.toml")).unwrap();
    assert_eq!(
        workspace["workspace"]["default-members"]
            .as_array()
            .unwrap(),
        &[toml::Value::String("crates/ferric-cli".into())]
    );
    let cli: toml::Value = toml::from_str(include_str!("../Cargo.toml")).unwrap();
    assert_eq!(cli["package"]["name"].as_str(), Some("ferric-cli"));
    assert_eq!(cli["package"]["default-run"].as_str(), Some("ferric"));
    assert!(cli["bin"].as_array().unwrap().iter().any(|binary| {
        binary["name"].as_str() == Some("ferric") && binary["path"].as_str() == Some("src/main.rs")
    }));
    assert!(
        cli["features"]["default"]
            .as_array()
            .unwrap()
            .contains(&toml::Value::String("backend-openai".into()))
    );

    let root = tempfile::tempdir().unwrap();
    let primary_help = help(root.path(), &[]);
    let primary_actions: Vec<_> = primary_help
        .split_once("Commands:\n")
        .expect("primary command list")
        .1
        .split("\nOptions:")
        .next()
        .unwrap()
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(primary_actions, ["run", "status", "explain", "advanced"]);
    assert_eq!(documented_actions(readme), primary_actions);
    assert_eq!(documented_actions(commands), primary_actions);

    for (action, options) in [
        ("run", &["[PROMPT]", "--workspace", "--allow-edits"][..]),
        ("status", &["--workspace", "--json"][..]),
        ("explain", &["--workspace", "--json"][..]),
    ] {
        let action_help = help(root.path(), &[action]);
        let documented = commands
            .lines()
            .find(|line| line.starts_with(&format!("| `ferric {action} ")))
            .expect("documented primary action");
        for option in options {
            assert!(action_help.contains(option), "{action} help omits {option}");
            assert!(documented.contains(option), "{action} docs omit {option}");
        }
    }

    let advanced_help = help(root.path(), &["advanced"]);
    for expert in ["query", "chat", "server", "bench", "trace"] {
        assert!(advanced_help.contains(expert), "expert command {expert}");
    }
    assert_eq!(
        help(root.path(), &["advanced", "query"]),
        help(root.path(), &["query"]),
        "documented advanced spelling preserves the expert parser"
    );
}
