//! Expert budget guidance must not enlarge the ordinary human front door.
//! Cargo drives each help child through the shared checked process owner.

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
        .expect("help completed and every owned child was reaped");
    assert!(
        output.status.success(),
        "help for {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_dir(workspace).unwrap().count(),
        0,
        "help is not resource preparation"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

fn normalized(text: &str) -> String {
    text.replace("//!", " ")
        .replace("///", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn option_description(help: &str, flag: &str) -> String {
    let mut lines = help
        .lines()
        .skip_while(|line| !line.trim_start().starts_with(flag));
    let heading = lines
        .next()
        .unwrap_or_else(|| panic!("missing {flag} option"));
    assert!(heading.contains(flag));
    normalized(
        &lines
            .take_while(|line| !line.trim_start().starts_with('-'))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn requires(text: &str, phrases: &[&str], surface: &str) {
    for phrase in phrases {
        assert!(
            text.contains(phrase),
            "{surface} must explain {phrase:?}: {text}"
        );
    }
}

#[test]
fn budget_docs_preserve_human_front_door() {
    let root = tempfile::tempdir().unwrap();
    let primary = help(root.path(), &[]);
    let run = help(root.path(), &["run"]);
    let commands_section = primary
        .split_once("Commands:\n")
        .unwrap()
        .1
        .split("\nOptions:")
        .next()
        .unwrap();
    let actions: Vec<_> = commands_section
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert_eq!(actions, ["run", "status", "explain", "advanced"]);
    for human_help in [&primary, &run] {
        for expert_flag in ["--max-output-tokens", "--timeout-scale"] {
            assert!(
                !human_help.contains(expert_flag),
                "ordinary launch gained an expert decision: {human_help}"
            );
        }
    }

    let query = help(root.path(), &["query"]);
    let cap = option_description(&query, "--max-output-tokens");
    requires(
        &cap,
        &[
            "main-action",
            "declared context",
            "prompt",
            "invocation-scoped",
            "resume",
            "compaction",
        ],
        "query output cap help",
    );
    let bench = help(root.path(), &["bench", "full"]);
    let scale = option_description(&bench, "--timeout-scale");
    requires(
        &scale,
        &[
            "agent execution",
            "positive",
            "finite",
            "diagnostic",
            "profile",
            "cleanup",
        ],
        "benchmark timeout scale help",
    );
    let cap = option_description(&bench, "--max-output-tokens");
    requires(
        &cap,
        &["main-action", "context", "diagnostic", "profile"],
        "benchmark output cap help",
    );
    requires(
        &normalized(&bench),
        &["results.jsonl", "sidecar"],
        "benchmark evidence help",
    );

    let readme = include_str!("../../../README.md");
    let commands = include_str!("../../../docs/commands.md");
    let testbench = include_str!("../../../docs/testbench.md");
    for (name, markdown) in [("README", readme), ("commands", commands)] {
        let mut first_example = markdown.lines().skip_while(|line| !line.starts_with("```"));
        assert_eq!(
            first_example.next(),
            Some("```sh"),
            "{name}: first example remains source-driven"
        );
        assert_eq!(
            first_example.next(),
            Some("cargo r"),
            "{name}: no preflight checklist"
        );
        assert_eq!(
            first_example.next(),
            Some("```"),
            "{name}: no new mandatory choices"
        );
    }
    let ordinary_docs = commands
        .split_once("## `ferric query`")
        .expect("expert query boundary")
        .0;
    assert!(!ordinary_docs.contains("--max-output-tokens"));
    assert!(!ordinary_docs.contains("--timeout-scale"));
    for (name, markdown) in [("commands", commands), ("testbench", testbench)] {
        requires(
            &normalized(markdown),
            &[
                "--max-output-tokens",
                "--timeout-scale",
                "diagnostic",
                "model_profiles.json",
                "sidecar",
                "sha-256",
            ],
            name,
        );
    }
    requires(
        &normalized(commands),
        &["invocation-scoped", "resume"],
        "query resume documentation",
    );

    // Ratchet the removed cross-process speed claim, not all discussion of
    // build profiles. External inference has its own runtime and hardware.
    for (name, text) in [
        ("benchmark CLI", include_str!("../src/bench_cmd.rs")),
        (
            "benchmark runner",
            include_str!("../../ferric-bench/src/runner.rs"),
        ),
        ("commands", commands),
        ("testbench", testbench),
    ] {
        let text = normalized(text);
        for false_claim in [
            "inference will be ~1 tok/s",
            "debug candle is ~1 tok/s",
            "release profile required for usable speed",
            "release-profile children are required for usable speed",
        ] {
            assert!(
                !text.contains(false_claim),
                "{name} revived unsupported external-inference advice: {false_claim}"
            );
        }
    }
}
