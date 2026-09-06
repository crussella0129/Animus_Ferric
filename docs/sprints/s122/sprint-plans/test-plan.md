Finalized - DO NOT EDIT

# Sprint 122 Test Plan

## Intent Traceability
| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 hardware-informed recommendation (measured, non-fabricated reading) | T-12201 / WHEN standard `/proc/meminfo` THEN return total+available | `parse_meminfo_reads_total_and_available` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (unmeasured stays unmeasured) | T-12201 / WHEN no `MemAvailable` THEN `None` | `parse_meminfo_missing_available_is_none` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (real host reading) | T-12201 / WHEN native probe on host THEN `Some(total>0)` | `native_probe_reports_positive_total` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (honest estimate) | T-12202 / WHEN estimate THEN ≥ weights + context | `estimate_covers_weights_and_context` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (fits/tight/wontfit) | T-12202 / WHEN estimate vs available THEN class | `classify_fits_tight_and_wontfit` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (unmeasured stays unmeasured) | T-12202 / WHEN available None THEN Unknown | `classify_none_is_unknown` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 (the 27B-on-CPU trap) | T-12202 / WHEN 15.3 GiB vs small RAM THEN WontFit | `human_test_27b_on_small_ram_is_wontfit` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 recommendation surfaced | T-12203 / WHEN picker + known memory THEN annotate each | `picker_annotates_each_model_fit` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 no silent bad start | T-12203 / WHEN selected WontFit THEN extra confirm naming numbers | `wontfit_requires_extra_confirmation` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | AC-13 unmeasured stays unmeasured | T-12203 / WHEN Unknown THEN preserve current behavior | `unknown_memory_preserves_current_behavior` |

## Unit Tests
### T-12201 unit tests
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `parse_meminfo_reads_total_and_available`: standard `/proc/meminfo` text → `SystemMemory { total_bytes, available_bytes }` (kB×1024)
- `parse_meminfo_missing_available_is_none`: text with `MemTotal` but no `MemAvailable` → `None`
- Stubs: none (pure string parser)

### T-12202 unit tests
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `estimate_covers_weights_and_context`: `estimate(f, ctx) ≥ f + positive allowance`, monotonic in context
- `classify_fits_tight_and_wontfit`: the three bands over `(estimate, available)`
- `classify_none_is_unknown`: `available = None` → `Unknown`
- `human_test_27b_on_small_ram_is_wontfit`: `estimate(15.3 GiB) vs available ≈ small` → `WontFit`
- Stubs: none (pure functions)

### T-12203 unit tests
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `picker_annotates_each_model_fit`: `choose_model` with an injected `Some(SystemMemory)` and two models → output contains each model's fit label
- `wontfit_requires_extra_confirmation`: selected model `WontFit` + interactive → an extra `io.read` confirmation whose prompt names estimate and available; declining does not start the engine
- `unknown_memory_preserves_current_behavior`: injected `None` → the existing caution line, no fabricated numbers, no extra prompt
- Stubs: existing `ScriptedIo`; injected `Option<SystemMemory>`

## Integration Tests
### Probe + fit on the real host
- **Intents:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `native_probe_reports_positive_total`: `NativeMemoryProbe` on the CI host (Windows + Linux gates) returns `Some(total_bytes > 0)` — proves the FFI/`/proc` path links and reads, complementing the pure `parse_meminfo` test.

## End-to-End Tests
- **Status:** possible
- `wontfit_session_gate_end_to_end`: drive `session_with` (or the picker+gate path) with `ScriptedIo`, an injected small-memory probe, and a large-model `Startup`; assert the picker annotation appears AND the engine-start path demands the won't-fit confirmation before any engine start — the exact human-test failure, now gated. Pass/fail: confirmation prompt present and honored; no engine start on decline.
