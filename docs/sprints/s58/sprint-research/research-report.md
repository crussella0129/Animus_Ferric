# Sprint 58 Research Report: Git Tool

## Objective
Implement a curated `git` tool for Ferric, exposing a safe subset of git commands (status, diff, add, commit, log, branch, checkout) via a subprocess, mapped to `Ring 1` for read commands and `Ring 2` for write commands. Force-push/rebase/reset are rejected. Subprocess execution will be used (not `git2` crate to avoid heavy dependencies).

## Findings
1. **Tool Structure**: Tools are defined by implementing the `Tool` trait (`crates/ferric-tools/src/spec.rs`), returning a `ToolSpec`. 
2. **Permission Model**: The `ferric-guard` system denies write access to `.git` paths via `DENIED_WRITE_SEGMENTS`. However, since `git` executes as a child process, it inherently mutates `.git` without needing to pass `.git` as a target path.
3. **Ring Mapping**: Since each `ToolSpec` defines a single permission level and ring, we must implement two separate tools:
    - `GitRead` (Ring 1, `PermissionLevel::Read`) - Supports `status`, `diff`, `log`, `branch` (list mode).
    - `GitWrite` (Ring 2, `PermissionLevel::Write`) - Supports `add`, `commit`, `checkout`, `branch` (create mode).
4. **Tool Registration**: The tools will be registered in `crates/ferric-tools/src/builtin/mod.rs` via `register_builtin_tools`.
5. **Execution Environment**: We will use `std::process::Command` to invoke the `git` binary, ensuring the `current_dir` is set to `ctx.workspace.root()`.

## Open Questions
- Is `Command::new("git")` sufficient, or do we need special environment variables (like `GIT_AUTHOR_NAME`) for test environments? I'll assume we can pass common git options or use the user's default configuration, but setting basic fallback identity for test suites is a known need (as seen in Sprint 43).

## Conclusion
We have enough context to proceed to the Planning phase.
