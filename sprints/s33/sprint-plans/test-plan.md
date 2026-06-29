Finalized - DO NOT EDIT

# Sprint 33 Test Plan — Ornstein research orchestrator

## Unit — `retriever.rs` (`ferric-research`; temp dirs + MockProvider, deterministic)
- **multi-plane aggregate:** two `LocalFsRetriever`s over *different* temp dirs (each one matching
  file) + a MockProvider scripted with two digests → `MultiResearch.digests.len() == 2`;
  `.planes` has two entries, both `available == true`, each `digests == 1`; digests are in
  retriever order.
- **dedup across planes (one model call):** two retrievers over the **same** dir (one matching
  file) + a MockProvider scripted with **one** digest → `.digests.len() == 1` (shared `source`
  summarized once); first plane `digests == 1`, second plane `digests == 0`. (If dedup were after
  the model call, the script would need 2 completions — so a 1-completion script passing proves
  dedup happens *before* the quarantine.)
- **unavailable plane:** a `LocalFsRetriever` with a missing root + a good one → the bad plane
  `available == false`, `digests == 0`; the good plane still contributes; no error.
- **all-unavailable:** every plane unavailable → empty `.digests`, all `available == false`.

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean;
  `fmt --check` clean.

## E2E
- Not required: deterministic temp-dir + MockProvider coverage is the right granularity for the
  orchestrator (pure composition of tested planes). A live multi-plane run lands with the
  web/tailnet live planes (gated on Docker / a tailnet sshd).
