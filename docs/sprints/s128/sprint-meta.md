# Sprint 128 Meta

- **Sprint number:** 128
- **Book schema version:** 2
- **Start timestamp:** 2026-10-22T18:45:00Z
- **End timestamp:** 2026-10-22T19:25:14Z
- **Model:** claude-opus-5-5
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Install, run, done (INT-0016, owner direction 2026-10-22): from nothing to a working local model in two steps, as with Hermes, Claude Code or Codex. A one-line installer (`install.sh` / `install.ps1`) puts a prebuilt, checksum-verified `ferric` on the PATH, built by a new tag-triggered release workflow. A first `ferric` run, or `ferric setup`, describes once what it would fetch and, on one yes, installs a pinned llama.cpp engine (verified against GitHub's release digest) and a RAM-fitted Qwen2.5-Coder model (pinned Hugging Face revision and SHA-256). Resumable, verified-before-rename downloads come from the new `ferric-setup` crate. No new archive dependency: extraction uses the system `tar`.
- **Intents:** [INT-0016](../../intents/INT-0016-install-run-done.md), [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md) (AC-13 acquisition, previously deferred, now done with consent), [INT-0010](../../intents/INT-0010-ferric-is-the-iron-of-amalgam.md) (the install and first-run flow is the owner's exception to maintenance-only)
- **Completion evidence:** INT-0016 AC-1 to AC-9. The `ferric-setup` crate (T-12801); `ferric setup` and the first-run offer (T-12802); the release workflow and installers (T-12803); the fresh-machine proof (T-12804): installed into an empty home, a verified engine and the real 1.1 GB model, a real answer, and the interactive offer under a pty. fmt and clippy are clean, including the Windows target. The 19 pre-existing sandbox lifecycle failures are unchanged. Open: the first release tag (T-12805, owner approval), GPU (T-12806), the sandbox launch (T-12810). See the [test report](sprint-tests/test-report.md).
- **Checkpoint:** https://github.com/crussella0129/Animus_Ferric/pull/124
