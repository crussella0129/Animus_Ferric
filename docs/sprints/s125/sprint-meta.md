# Sprint 125 Meta

- **Sprint number:** 125
- **Book schema version:** 2
- **Start timestamp:** 2026-09-07T13:24:59Z
- **End timestamp:** 2026-09-07T16:05:01Z
- **Model:** claude-opus-4-8
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Deployable, run-from-anywhere front door (INT-0008): decouple model discovery from the working folder via a configured models dir (--models-dir > FERRIC_MODELS_DIR > config > default <workspace>/models), replace the silent picker with the owner's 0/1/>1 rule + fit warnings, and document the install-once/run-in-your-project deploy story. No auto-download.
- **Intents:** [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md) — active; human entry point (run-from-anywhere + configured discovery + honest selection; AC-13 acquisition stays deferred by owner decision).
- **Completion evidence:** INT-0008 run-from-anywhere front door: configured models-dir discovery (T-12501), owner 0/1/>1 picker (T-12502), install-once deploy docs (T-12503); full workspace green at 39659d7, proceed-with-caveats critique.
- **Checkpoint:** https://github.com/crussella0129/Animus_Ferric/pull/119
