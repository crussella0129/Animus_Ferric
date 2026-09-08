# Plan Critique — Sprint 125

## Concerns

### C-001: The 0/1/>1 picker tests must key off a pure decision, not `choose_model(&Startup)`
- **Where:** `test-plan.md` T-12502 unit tests / `human.rs::choose_model`
- **Quote:** "`single_model_auto_picks` … `multiple_models_always_list`"
- **Failure mode:** plan-test-mismatch
- **Why it matters:** `Startup` has private fields and no test constructor (the same limit sprint 122 hit; backlog T-12204), so a test cannot build a `Startup` with N models to drive `choose_model` directly.
- **Suggested response:** fix-in-plan. T-12502 extracts the rule into a pure `fn model_selection(count: usize) -> Selection { NoModel, Auto, Pick }` (mirroring sprint 122's `wontfit_confirm_prompt` split), unit-tested over 0/1/2; the ~4 lines of `choose_model` wiring that consume it are trusted, exactly as the source-tree guard's wiring is. The "no-model message names the dir" is likewise a pure `no_model_message(dir)`. This closes the mismatch without needing the `Startup` seam.

### C-002: The full interactive run E2E is bounded by the same `Startup` seam
- **Where:** `test-plan.md` E2E `models_dir_env_end_to_end`
- **Quote:** "a scripted run … reaches the picker"
- **Failure mode:** e2e-cop-out (screened)
- **Why it matters:** the end-to-end "env dir → picker" path runs through `session_with`/`choose_model`, which the `Startup` seam limits.
- **Suggested response:** defer-with-rationale for the *interactive* leg; the observable behavior is proven where it is deterministic: **discovery** end-to-end via a real external temp dir (integration), the **resolution order** and the **0/1/>1 decision** + **no-model message** via pure unit tests. The full scripted interactive run is unlocked by the backlogged front-door `Startup` test seam (T-12204). No SHALL is left unproved.

### C-003: The deploy-doc EARS is a content check
- **Where:** `test-plan.md` T-12503 / `getting_started_describes_installed_flow`
- **Quote:** "no instruction to run `cargo r` inside the repo"
- **Failure mode:** weak-assertion (screened, accepted)
- **Suggested response:** accept — a doc-content assertion (the string is present / the `cargo r`-in-repo instruction is absent) is the right and only test for a documentation change, and matches the existing `human_docs.rs`/`source_execution.rs` doc guards.

## Confidence
proceed-with-caveats

Every EARS clause maps to a named test once T-12502's rule is a pure helper
(C-001, fix-in-plan). The security-relevant change (discovery's confined root
moving to the resolved dir) is covered by an external-dir enumeration test plus a
symlink-rejection test, with the default path proven unchanged by the existing
suite. No intent boundary is crossed; auto-acquisition stays deferred by owner
decision.
