# Sprint 124 End-to-End Tests

Tested head: `e77f749`.

## Status: possible — verified

For a behavior-preserving file move, the whole suite passing unchanged is the E2E:

- `cargo build -p ferric-cli` — clean; `server.rs` is gone, `server/mod.rs` + `server/tests.rs` present.
- `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean, plus the `-p ferric-cli --features lifecycle-fixture` and `--no-default-features` shapes clean under `-D warnings`.
- `cargo test --workspace --locked` — **0 failures**, including the 11.7K relocated `server::tests::*` and the `server_resolution` cross-module helpers.
- Structural: `server/mod.rs` is 6,561 lines (down from 18,257), and its production region (lines 1-6559) is byte-identical to the old `server.rs`.

The aarch64 `cargo check` and the multi-host lifecycle-fixture / no-default-features CI jobs run at the Loop phase.
