Finalized - DO NOT EDIT

# Sprint 59: Build Plan (`shell_exec` Tool)

1. **`ferric-guard` updates (`crates/ferric-guard/src/checker.rs`)**
   - Implement `check_command(command: &str) -> Decision` which scans the command string for substrings present in `denylist::DENIED_COMMAND_PATTERNS`.

2. **Tool Trait Extension (`crates/ferric-tools/src/spec.rs`)**
   - Add `fn target_commands(&self, args: &serde_json::Value) -> Vec<String>` to the `Tool` trait. Default implementation returns `Vec::new()`.

3. **Registry and Permissions (`crates/ferric-tools/src/registry.rs`)**
   - Update `Registry::execute` to call `ferric_guard::check_command(...)` on all `target_commands()`.
   - Wire the CaMeL `SinkPolicy` checking `args_tainted` if permission is `Write` or `Execute`.

4. **`shell_exec` Tool Implementation (`crates/ferric-tools/src/builtin/shell_exec.rs`)**
   - Implement `ShellExec` tool (Ring 2, `PermissionLevel::Execute`).
   - Use `std::process::Command` with `sh -c` (Unix) or `cmd.exe /C` (Windows).
   - Implement a 60-second execution timeout via synchronous `try_wait` + thread sleep loop.
   - Implement a 10KB output truncation cap.

5. **Tool Registration (`crates/ferric-tools/src/builtin/mod.rs`)**
   - Register `ShellExec` tool.
