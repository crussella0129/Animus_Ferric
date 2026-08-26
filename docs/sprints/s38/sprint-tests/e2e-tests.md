# Sprint 38 E2E Tests

- **Status:** possible (via `--mock`, no real GGUF model required) — and built. `cli.rs`'s
  `config_only_model_still_resolves_profile`, `animus_md_folds_into_prompt`, and
  `animus_md_present_traces_note` (see `integration-tests.md`) ARE end-to-end in the sense the
  test-plan meant: a real `ferric` subprocess, a real `.ferric/config.toml`/`Animus.md` on disk, a
  real trace file read back — no mocked filesystem or config layer. Filed under
  `integration-tests.md` rather than duplicated here since the harness draws that line at
  "subprocess + real disk" vs. "in-process," and these already satisfy the stronger bar.
- A real-backend config/`Animus.md` smoke (a live llama-server, confirming the resolved
  model/backend actually loads and the folded system prompt reaches the model) is a **manual
  verification step**, not automated — matches the project's established no-live-backend-CI
  position (ADR-045).
