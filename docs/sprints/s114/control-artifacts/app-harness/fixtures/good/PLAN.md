# MH-RS01 implementation plan

## Contract

- [x] Preserve the frozen manifest, library, CLI, and safety contracts.

## File plan

- [x] Implement public data and error types in `src/model.rs`.
- [x] Implement manifest parsing in `src/parser.rs`.
- [x] Implement deterministic scheduling in `src/scheduler.rs`.
- [x] Implement the one-path CLI in `src/main.rs`.
- [x] Add focused coverage in `tests/agent_tests.rs`.

## Verification

- [x] Run the authorized `cargo test --offline --all-targets` check successfully.
