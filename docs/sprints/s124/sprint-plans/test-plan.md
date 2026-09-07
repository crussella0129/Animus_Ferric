# Sprint 124 Test Plan

## Intent Traceability
| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-4 — file split at a real seam, builds | T-12401 / WHEN built THEN server/ exists, server.rs gone, build ok | `cargo build -p ferric-cli` + file presence check |
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-4 — tests run/pass from the new location | T-12401 / WHEN suite runs THEN passes unchanged incl. cross-module helpers | `cargo test --workspace` (server::tests::* + server_resolution) |
| [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md) | AC-4 — no production drift | T-12401 / WHEN production region diffed THEN only mod-tests line changed | `git diff` production-region inspection + clippy `-D warnings` |

## Unit Tests
### T-12401
- **Intent:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- No new unit test. This is a behavior-preserving file move; the ~11.7K lines of `server::tests::*` that move are the assertion — they must compile and pass from `server/tests.rs` exactly as they did inline. A hand-written test here would test the move, not any product behavior.

## Integration Tests
### Cross-module test-helper resolution
- **Intents:** [INT-0009](../../../intents/INT-0009-lean-decomposed-architecture.md)
- `crates/ferric-cli/src/server_resolution.rs` (existing) calls `crate::server::tests::legacy_tailscale_registration_remains_unowned()` and `…::promised_origin_static_matrix_precedes_process_inspection()` from its own tests. These compiling and passing after the move proves the `pub(crate) mod tests;` submodule kept the path resolving — the one non-obvious risk of the move.

## End-to-End Tests
- **Status:** possible
- The whole `cargo test --workspace` suite passing unchanged — plus the `lifecycle-fixture` and `no-default-features` feature shapes compiling under `-D warnings` and the aarch64 `cargo check` — is the behavior-preservation E2E for a file move of this kind.
- Structural check: after the split, `server.rs` is absent, `server/mod.rs` + `server/tests.rs` are present, and `git diff` shows the production region byte-identical except the `mod tests { … }` → `mod tests;` edit.
