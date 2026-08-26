Finalized - DO NOT EDIT

# Sprint 110 Build Plan

1. Refactor `trace verify` into a side-effect-free drift check and add a
   regression proving the source workspace cannot be changed.
2. Make the attachment pipeline take a `Workspace`, enforce the read guard,
   reject outside/sensitive paths, and cap per-file plus aggregate input size.
3. Remove `shell_exec` and `manage_task` from all model-visible rings while
   retaining human chat passthrough; propagate nonzero foreground exits as tool
   errors.
4. Centralize successful `StopReason` classification and use it at the CLI/MCP
   boundaries.
5. Repair the demo/E2E runners so native failures or missing artifacts fail the
   script, remove obsolete CLI arguments, and add a deterministic offline smoke
   path.
6. Correct demo/reference documentation (`--no-stream`, trace directory,
   visible `Animus.md` verification, and the explicitly safe Monday scope).
7. Record the decision and remaining live-only risks in the durable project
   ledger.
