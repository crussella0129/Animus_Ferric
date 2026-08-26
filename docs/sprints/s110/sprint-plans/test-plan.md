Finalized - DO NOT EDIT

# Sprint 110 Test Plan

Test outward from the changed seams:

1. Focused unit tests:
   - `StopReason::is_success`;
   - `shell_exec` nonzero exit and model-ring exclusion;
   - attachment size and workspace/read-policy rejection;
   - pure trace normalization/replay behavior.
2. CLI/MCP integration tests:
   - a real mock trace still verifies;
   - verify cannot alter a sentinel in the source workspace;
   - MCP rejects outside-workspace and sensitive attachments;
   - repeated mock MCP calls remain independent if that surface changes;
   - incomplete loop outcomes surface as errors.
3. Deterministic offline smoke:
   - version/help;
   - `query --mock` creates the expected artifact and successful trace;
   - `trace cat` (and safe `trace verify`);
   - launch, ICM init/plan/run mock, skills, and cron dry-run as time permits.
4. Repository gates:
   - `cargo fmt --all --check`;
   - `cargo clippy --workspace --all-targets -- -D warnings`;
   - `cargo test --workspace`;
   - `cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings`;
   - release build with `backend-openai`;
   - aarch64 check if the target is installed.
5. Record Docker/live-server checks as not validated unless the external
   daemon/model is actually available; skipped tests are not passes.
