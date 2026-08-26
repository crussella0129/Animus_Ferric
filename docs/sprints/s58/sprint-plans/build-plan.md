Finalized - DO NOT EDIT

## Build Plan

### Increment 1: GitRead Tool
- Create `crates/ferric-tools/src/builtin/git_read.rs` implementing `GitRead` struct.
- Map it to `PermissionLevel::Read` and `ring: 1`.
- Expose commands: `status`, `diff`, `log`, `branch`.

### Increment 2: GitWrite Tool
- Create `crates/ferric-tools/src/builtin/git_write.rs` implementing `GitWrite` struct.
- Map it to `PermissionLevel::Write` and `ring: 2`.
- Expose commands: `add`, `commit`, `checkout`.
- Ensure `-m` is enforced for `commit` to prevent hanging in the editor.

### Increment 3: Registry Integration
- Modify `crates/ferric-tools/src/builtin/mod.rs`.
- Export and register `GitRead` and `GitWrite`.
