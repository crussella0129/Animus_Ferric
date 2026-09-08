# Test Critique — Sprint 125

Adversarial read-only pass over the locked `build-plan.md` / `test-plan.md`, the
`sprint-tests/*` result artifacts, the `T-12501/2/3` completed-task entries, and
`INT-0008`. Intent acceptance criteria are the oracle; EARS clauses are the
sprint promises; the result files are provenance.

## Concerns

### C-001: ">1 in an external dir → picker" is proven by parts, not one E2E
- **Where:** `e2e-tests.md` / `human_external_models_dir_end_to_end` (asserts the **1**-model external case) vs. `human_repeat_with_multiple_models_always_reasks` (the **>1** case, on the **default** `<workspace>/models`).
- **Quote:** "the single external model auto-picked with no picker prompt."
- **Failure mode:** e2e-cop-out (partial)
- **Why it matters:** The end-to-end external-dir test exercises only the 1→auto branch; the >1→picker branch is exercised end-to-end only on the default models dir. A reader could ask whether ">1 GGUFs in an *external* dir" reaches the picker through the full session.
- **Suggested response:** defer-with-rationale. `model_selection(count)` is a pure function of the **count** and is independent of where the directory came from; `scan_dir_discovers_gguf_in_an_external_directory` proves an external dir yields all its GGUFs (returns 2), and `human_repeat_with_multiple_models_always_reasks` proves count>1 reaches the picker through the real session. The composition of those two covers the strand; a dedicated >1-external E2E would only re-assert the count branch already isolated. Left as a named caveat rather than adding a near-duplicate.

### C-002: symlink negative path depends on runner symlink privilege
- **Where:** `integration-tests.md` / `scan_dir_rejects_a_symlinked_gguf_in_an_external_directory`.
- **Quote:** "A `link.gguf` symlink inside the resolved dir → `scan_dir(...).is_err()`."
- **Failure mode:** flake-risk
- **Why it matters:** Symlink creation needs a privilege on Windows; a runner without it would error at `symlink_file`, not at the assertion, which would read as an infra failure.
- **Suggested response:** reject (the concern is real but already mitigated). The pre-existing `symlink_directory` tests in the same module (`automatic_symlink_root_is_refused_...`, `discovered_models_directory_swap_...`) already create symlinks unconditionally and are green on both the Windows and Linux CI runners; the new file-symlink test adds no new platform dependency beyond what the suite already requires. If those pass, this passes; if the runner lacked the privilege, the suite was already red.

### C-003: heavier feature/target shapes are deferred to CI
- **Where:** `e2e-tests.md` "Behavior preservation and platform matrix"; the plan's Verification lists `lifecycle-fixture`, `--no-default-features`, and the `aarch64` `cargo check`.
- **Failure mode:** evidence-drift
- **Why it matters:** The local run proves default-feature `--workspace` green; the alternate feature/target shapes are only asserted, not shown, until CI runs them on the PR.
- **Suggested response:** defer-with-rationale, then close in Loop. These three shapes are exactly the CI matrix's jobs; `test-report.md` records the tested head SHA and will carry the authoritative CI conclusion once the sprint PR runs. The refactor is additive and feature-agnostic (no `cfg(feature)` branches were added), so the risk of a shape-specific regression is low, and the Loop phase confirms CI green before the sprint closes.

## Confidence
proceed-with-caveats
