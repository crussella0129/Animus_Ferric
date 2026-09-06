# Sprint 123 End-to-End Tests

Tested head: `d038ec6`.

## Status: possible — verified

- **The warning is gone (T-12028).** `cargo build -p ferric-cli` on the extracted
  tree emits **zero** "found to be present in multiple build targets" lines
  (grep count 0) — the exact warning from the human terminal trial. Verified on
  the real build.
- **Behavior preserved across the whole product.** `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`, and
  `cargo test --workspace --locked` are green, and the two extra CI feature
  shapes compile clean under `-D warnings`:
  `-p ferric-cli --features lifecycle-fixture` (the gate path) and
  `-p ferric-cli --no-default-features` (which exercises the `bin_identity`
  dead-code guards). The aarch64 `cargo check` runs at the Loop-phase CI.
- For a behavior-preserving extraction, the whole existing suite passing
  unchanged — including the process-spawning lifecycle fixture — is the E2E:
  the binaries do exactly what they did, now over a shared library.
