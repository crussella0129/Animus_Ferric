# Sprint 125 Test Report — Deployable run-from-anywhere front door (INT-0008)

## Verdict: PASS (proceed-with-caveats)

Tested head: `39659d7`. Local gates (dev host, Windows):

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean (exit 0).
- `cargo test --workspace --locked -- --test-threads=1` — **0 failed**, every suite ok.
- The changed crate specifically: `ferric-cli` lib now **417 tests** (was 407; +9 sprint-125 lib tests across resolution, selection, and external-dir discovery), plus the new `getting_started_doc` integration test (1/1).

Authoritative multi-host CI (Linux + Windows, the `lifecycle-fixture` and
`--no-default-features` shapes, and the `aarch64-unknown-linux-gnu` `cargo
check`) runs on push at the Loop phase and is confirmed green before the sprint
PR closes.

## What was proven

Every INT-0008 clause in this sprint's scope maps to a named, executed,
tightly-asserted test (full traceability in `test-plan.md`):

| Acceptance outcome | Test(s) | Result |
|---|---|---|
| Models dir resolves flag > `FERRIC_MODELS_DIR` > config > `<workspace>/models` | `resolve_models_dir_precedence` | PASS |
| Default path is byte-identical to today | existing `startup::models` suite on the `scan(workspace)` wrapper | PASS |
| External dir enumerates its GGUFs, confined to it | `scan_dir_discovers_gguf_in_an_external_directory` | PASS |
| …and still rejects non-file entries | `scan_dir_rejects_a_non_file_gguf_entry` | PASS |
| …and still rejects symlinks | `scan_dir_rejects_a_symlinked_gguf_in_an_external_directory` | PASS |
| 0 GGUFs → stop, message names the resolved dir, no engine | `no_model_message_names_the_resolved_dir`, `model_selection_is_by_count`, `human_journey_e2e_matrix` (absent-dir case) | PASS |
| 1 GGUF → auto-pick with fit, no picker, even when the saved preference is stale | `model_selection_is_by_count`, `human_stale_single_model_still_auto_picks` | PASS |
| >1 GGUFs → picker with a fit annotation per model, required choice, no silent saved-preference skip | `model_selection_is_by_count`, `picker_annotates_each_model_fit`, `human_repeat_with_multiple_models_always_reasks` | PASS |
| Full session discovers from an external dir decoupled from the working folder | `human_external_models_dir_end_to_end` | PASS |
| Documented entry point is install-once / run-in-your-project / `FERRIC_MODELS_DIR` (no in-repo `cargo run`) | `getting_started_describes_installed_flow` | PASS |

**The second human use test's exact failures now cannot recur.** `ferric` no
longer has to run inside the repo: an installed binary resolves a configured
models directory and, per the owner's rule, tells you to add a GGUF when there
are none, auto-picks when there is one, and always shows the fit-annotated
picker when there is more than one — a stale saved preference is at most a
highlight, never a silent 27B auto-start.

## Caveats (from `critique.md`)

- **C-001 (e2e-cop-out, partial — deferred with rationale):** the end-to-end
  external-dir test exercises the 1→auto branch; the >1→picker branch is
  end-to-end only on the default models dir. `model_selection(count)` is a pure
  function of the count (dir-source-independent), external discovery of 2 models
  is proven by `scan_dir_discovers_gguf_in_an_external_directory`, and >1→picker
  by `human_repeat_with_multiple_models_always_reasks`; the composition covers
  the strand. A dedicated >1-external E2E would only re-assert the count branch.
- **C-002 (flake-risk, rejected):** the new symlink test needs the runner's
  symlink privilege, but the module's pre-existing `symlink_directory` tests
  already require it and are green on both CI runners — no new dependency.
- **C-003 (evidence-drift, deferred to Loop):** the `lifecycle-fixture`,
  `--no-default-features`, and `aarch64` shapes are the CI matrix's jobs; the
  refactor added no `cfg(feature)` branches, and Loop confirms CI green before
  close.

## Intent status

INT-0008 remains **active**. This sprint realizes the human entry point's
run-from-anywhere / configured-discovery / honest-0-1-many-selection clause. The
owner-deferred AC-13 in-product **acquisition/download** clause and GPU/VRAM
calibration remain active follow-on work, so the intent is **not** marked
realized.
