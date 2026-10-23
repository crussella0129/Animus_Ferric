# Sprint 128 — Test plan

| Level | What | Where |
|---|---|---|
| Unit | Catalog pins well formed; pinned URL; RAM tiers; data-dir resolution; https/loopback-only sources; part-file path; per-platform asset names; digest required; managed-engine record; offer wording; Enter/yes acceptance; plan logic; models-folder fallback | `crates/ferric-setup/src/*`, `crates/ferric-cli/src/{setup_cmd,startup/tests}.rs` |
| Integration | Local mirror: verified download lands, mismatch discarded, resume via `Range`, existing file kept, engine installed and recorded, wrong engine digest refused | `crates/ferric-setup/tests/mirror.rs` |
| Guards | Primary help includes `setup`; getting started requires the one-liners, `ferric setup` and SHA-256; charter live intents include INT-0016 | `crates/ferric-cli/tests/{human_cli,getting_started_doc,charter_docs}.rs` |
| End to end | Empty home: install from a release mirror, a non-interactive refusal, `ferric setup --yes --model 1.5b` (engine via a release-API mirror, the real model from Hugging Face), the answer from the installed engine and model | `sprint-tests/e2e-tests.md` |
| Cross-platform | Clippy for `x86_64-pc-windows-gnu`; CI on Windows, Linux and aarch64 | CI |
