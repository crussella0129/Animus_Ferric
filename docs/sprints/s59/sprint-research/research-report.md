# Research Report for Sprint 59: `shell_exec` Tool

## Goal
Implement a `shell_exec` tool (Ring 2) that allows the agent to run arbitrary shell commands with:
1. Workspace cwd restriction
2. Command timeout
3. stdout/stderr capture
4. Output caps
5. Ornstein command screening for destructive/exfil/privesc patterns.

## Current State
- The `GitRead` and `GitWrite` tools were successfully added in Sprint 58, operating as simple subprocess execution wrappers.
- The `Registry` is the chokepoint. It checks permissions via `ferric_guard::check()`.
- The `ferric-guard` module currently implements `PermissionLevel::{Read, Write, Execute}`.
- `Execute` is available as a `PermissionLevel` but currently checks against `DENIED_WRITE_SEGMENTS` and `DENIED_WRITE_FILES` in the same way `Write` does (`checker.rs:79-103`).
- `DENIED_COMMAND_PATTERNS` exists in `ferric-guard/src/denylist.rs` containing `["rm -rf /", "mkfs", "dd if=", "git push --force", "shutdown", "reboot"]`.
- There is a `TaintSet` and `SinkPolicy` in `ferric-guard/src/sink.rs` that checks if arguments contain any tainted strings and can block `Write`/`Execute` actions.
- The `ADR-045` reference in `agent-tasks.md` notes that we need to "extend Ornstein to screen commands for destructive/exfil/privesc patterns before exec. (Needs the real permission-model extension flagged in ADR-045, not a quick add.)"

## Requirements Breakdown

### 1. The Tool Implementation (`shell_exec.rs`)
- It will be a `Ring 2` tool.
- It will require a `PermissionLevel::Execute`.
- Its arguments will be `command` (the command string to run) and `args` (an optional array of string arguments). Or possibly just a single `command` string that we run via `sh -c` or `cmd /C`. Let's use standard subprocess execution (e.g. `sh -c "..."` on unix, `cmd.exe /C "..."` on Windows).
- It needs a **command timeout**. `std::process::Command` does not have a built-in timeout. We might need a small thread loop or `wait_timeout` logic if we stick to `std`. But since this tool executes synchronously and blocks the agent's turn, we must implement a timeout to prevent hanging the agent (e.g., if a command waits for stdin). We can spawn the child process, then loop with `try_wait()` and thread sleeps, killing the child if the timeout is reached.
- **Output caps**: The output (stdout + stderr) must be capped to prevent massive dumps from blowing up the context window.
- **Workspace cwd restriction**: Easy, set `cmd.current_dir(ctx.workspace.root())`.

### 2. The Permission Model Extension (ADR-045)
- Currently `checker::check_write_target` checks path segments. How does this apply to arbitrary commands? A command like `curl ...` might not provide paths to `target_paths()`.
- If a tool has `PermissionLevel::Execute`, what should `ferric-guard` check?
- We need to screen the *command string itself* against `DENIED_COMMAND_PATTERNS`.
- But `target_paths` returns `Vec<String>` which `Registry::execute` tries to resolve as paths relative to the workspace. If the tool yields the command string as a target path, `Workspace::resolve` will fail if it's not a valid path.
- Therefore, we need a new concept in `ferric-guard` to check commands directly, or we update the `Tool` trait to expose `target_commands()` or similar, OR we do the command screening inside the `shell_exec` tool's `run()` method (or its `target_paths()` returning empty, and relying on `sink.rs` for screening).
- **Wait, ADR-045 says**: "Needs the real permission-model extension flagged in ADR-045, not a quick add." What is that extension? Let's look at `research-report.md` from sprint 35. It says: "a shell/exec tool (needs a real permission-model extension, likely a new `PermissionLevel` or a heavily sandboxed variant — not a quick add)".
- So we should probably add a new check function in `ferric-guard::checker` for commands: `pub fn check_command(command: &str) -> Decision`. This function will check the command string against `DENIED_COMMAND_PATTERNS`.
- To integrate this with the `Registry`, we can expand the `Tool` trait or change how `Registry::execute` handles `PermissionLevel::Execute`. If `PermissionLevel::Execute`, does it still yield paths? A shell command might touch paths but it's fundamentally a command execution.
- Actually, changing the `Tool` trait affects 14 existing tools. Instead, we can add `target_commands(&self, args: &serde_json::Value) -> Vec<String>` to the `Tool` trait with a default implementation returning empty. `Registry::execute` will then loop over `target_commands` and pass them to `ferric_guard::check_command()`.

### 3. Taint / CaMeL-lite Screening
- `sink.rs` already has `args_tainted(args)` which walks the JSON and checks if ANY string contains a tainted string.
- If we wire `SinkPolicy` into `Registry::execute` (which is already a stated goal from Sprint 35 but was deferred), it will automatically block `Execute` tools if their args are tainted (under `SinkAction::Deny`).
- Wait, the sprint 35 report says "Wire the CaMeL sink policy into Registry::execute (closes the confirmed dangling primitive)." We should definitely do this wiring as part of this sprint, as it implements the "extend Ornstein to screen commands" requirement.

### Open Questions for Implementation Plan
- Should `shell_exec` run via `sh -c` / `cmd.exe /C` to allow shell features (pipes, redirections), or should it strictly run the executable as arg 0 to avoid shell escape vulnerabilities? The goal is "run arbitrary shell commands" which implies `sh -c`. We will use `sh -c` on Unix and `cmd.exe /c` on Windows.
- How to handle the timeout in pure `std` Rust? We can spawn the child, then use a loop with `std::thread::sleep(Duration::from_millis(100))` and `child.try_wait()`. If `elapsed > timeout`, `child.kill()`.
- What should the output cap be? Let's use 10,000 bytes. If it exceeds, we truncate and append a truncation notice.
