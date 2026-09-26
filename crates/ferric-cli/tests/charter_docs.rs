//! INT-0010 (T-12610): the landing documents, the project instructions and the
//! work ledger state Ferric's role as the Iron of Animus Amalgam, and the
//! triage that froze standalone work lost no task.

const README: &str = include_str!("../../../README.md");
const DOCS_README: &str = include_str!("../../../docs/README.md");
const INTRODUCTION: &str = include_str!("../../../docs/introduction.md");
const CLAUDE: &str = include_str!("../../../CLAUDE.md");
const AGENTS: &str = include_str!("../../../AGENTS.md");
const TASKS: &str = include_str!("../../../docs/work/tasks.md");
const COMPLETED: &str = include_str!("../../../docs/work/completed-tasks.md");

/// Task lines plus completion entries at the pre-triage baseline (`51af84e`):
/// 122 lines in `tasks.md` and 215 `## T-` entries in `completed-tasks.md`.
/// A task only ever moves from the first ledger to the second, so this sum
/// can grow but never shrink. A shrink means a task was deleted.
const LEDGER_BASELINE: usize = 122 + 215;

fn live_intent(line: &str) -> bool {
    ["INT-0010", "INT-0011", "INT-0012", "INT-0013", "INT-0014"]
        .iter()
        .any(|id| line.contains(id))
}

#[test]
fn landing_docs_state_iron_role() {
    for (name, text) in [
        ("README.md", README),
        ("docs/README.md", DOCS_README),
        ("docs/introduction.md", INTRODUCTION),
    ] {
        assert!(
            text.contains("Animus Amalgam"),
            "{name} names Animus Amalgam"
        );
        assert!(
            text.contains("constrained decoding"),
            "{name} states the constrained-decoding role"
        );
        assert!(
            text.contains("maintenance"),
            "{name} states the maintenance status"
        );
        assert!(text.contains("20–35B"), "{name} states the model envelope");
    }
    assert!(
        README.contains("| Ferric owns | Amalgam owns |"),
        "README carries the ownership split"
    );
}

#[test]
fn open_tasks_are_triaged() {
    let mut heading = "";
    let mut untriaged = Vec::new();
    for line in TASKS.lines() {
        if line.starts_with('#') {
            heading = line;
        } else if line.starts_with("- [ ] ") && !live_intent(line) {
            let named = heading
                .split_once("Maintenance —")
                .is_some_and(|(_, rest)| !rest.trim().is_empty());
            if !named {
                untriaged.push(format!("{heading} :: {}", &line[..line.len().min(80)]));
            }
        }
    }
    assert!(
        untriaged.is_empty(),
        "open tasks neither on a live intent nor under a named Maintenance heading:\n{}",
        untriaged.join("\n")
    );
}

#[test]
fn task_line_count_not_reduced() {
    let task_lines = TASKS
        .lines()
        .filter(|line| line.starts_with("- [ ] ") || line.starts_with("- [x] "))
        .count();
    let completed = COMPLETED
        .lines()
        .filter(|line| line.starts_with("## T-"))
        .count();
    assert!(
        task_lines + completed >= LEDGER_BASELINE,
        "the work ledgers lost tasks: {task_lines} + {completed} < {LEDGER_BASELINE}"
    );
}

#[test]
fn project_instructions_state_direction() {
    for (name, text) in [("CLAUDE.md", CLAUDE), ("AGENTS.md", AGENTS)] {
        assert!(
            text.contains("## Direction: Ferric is the Iron of Animus Amalgam"),
            "{name} has the direction section"
        );
        assert!(
            text.contains("Iron work first"),
            "{name} puts Iron work first"
        );
        assert!(
            text.contains("Standalone surfaces are maintenance-only"),
            "{name} freezes the standalone surfaces"
        );
    }
}
