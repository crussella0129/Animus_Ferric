# Sprint 127 — Test plan

| Level | What | Where |
|---|---|---|
| Unit | Token shape, CSPRNG uniqueness, redaction, file modes (0600/0700), refusal of open files; each gate check on its own, including the rebinding and browser rows that hold the valid token; open paths (GET only); bind policy; X25519 against the RFC 7748 §6.1 vector; clamping; client reach limited to the host /32; config→UAPI round trip; never-overwritten plan; UAPI status parsing; AllowedIPs collection | `crates/ferric-gate/src/*`, `crates/ferric-cli/src/tunnel_cmd.rs` |
| Integration | Axum layer: credential stripped before handlers, denials marked without CORS, fallback gated, health host-checked. Valve: refusals never reach the upstream, upstream sees only its own key, nothing forwarded without one. Probe: each attack refused by its expected check, every attack reaches a naive server, demo exits 0 | `crates/ferric-gate/tests/layer.rs`, `crates/ferric-valve/tests/gate.rs`, `crates/ferric-cli/src/secure_cmd.rs` |
| End to end | Real `ferric api --mock` probed on loopback. Two network namespaces joined by veth, GotaTun on both ends; `ferric api --tunnel`; health, an authenticated query and the probe from the client namespace; the LAN address unreachable | `sprint-tests/e2e-tests.md` |
| Regression | `cargo fmt`, clippy `-D warnings` (workspace and `--no-default-features`), full workspace tests | CI |
