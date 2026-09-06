# Sprint 122 Test Report — Hardware-informed model fit (INT-0008 AC-13)

## Verdict: PASS (proceed-with-caveats)

Tested head: `9eabcbc`. Local gates (dev host, Windows):

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test --workspace --locked -- --test-threads=1` — **40 suites ok, 0 failed.**
- The changed crates specifically: `ferric-core` fit (4/4) and `ferric-cli --bin ferric` (403/0, up 7 from 396).

Authoritative multi-host CI (Linux + Windows, aarch64 check) runs on push at the Loop phase.

## What was proven

Every INT-0008 **AC-13** *hardware-informed recommendation* EARS clause maps to a named, executed, tightly-asserted test (full traceability in `test-plan.md`):

| Acceptance outcome | Test | Result |
|---|---|---|
| Measured, non-fabricated memory reading | `parse_meminfo_reads_total_and_available`, `native_probe_reports_positive_total` | PASS |
| Unmeasured stays unmeasured | `parse_meminfo_missing_available_is_none`, `classify_none_is_unknown`, `unknown_memory_preserves_current_behavior` | PASS |
| Honest estimate ≥ weights + context | `estimate_covers_weights_and_context` | PASS |
| Fits / Tight / WontFit classification (keyed on available) | `classify_fits_tight_and_wontfit`, `human_test_27b_on_small_ram_is_wontfit`, `fit_keys_on_available_not_total` | PASS |
| Recommendation surfaced in the picker | `picker_annotates_each_model_fit` | PASS |
| No silent bad start — won't-fit confirmation naming the numbers | `wontfit_requires_extra_confirmation` | PASS |

The first human use test's exact failure (a 15.3 GiB 27B started on a modest machine under "resource fit is not measured") now classifies `WontFit` and cannot start without a deliberate confirmation that names the estimate and available memory.

## Caveats (from `critique.md`)

- **C-001 (e2e-cop-out, deferred with rationale):** the won't-fit gate is proven over the real production helpers, but the ~4 wiring lines in `choose_model` are not executed by a full picker driver, because model `bytes` come from real on-disk file metadata and `Startup::begin_in` is private — a genuine test-seam gap, not a skipped assertion. The wiring is the same `io.read` y/N pattern already proven by the source-tree guard. Named follow-up recorded under INT-0008 (a `#[cfg(test)]` `Startup` seam or an injectable `MemoryProbe` in `session_with`).

## Intent status

INT-0008 remains **active**. This sprint verifies the hardware-informed-recommendation clause of AC-13 for the RAM path; AC-13's in-product acquisition/download clause and GPU/VRAM calibration remain active follow-on work, so the intent is **not** marked realized.
