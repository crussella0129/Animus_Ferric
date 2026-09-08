# Sprint 125 — Unit Test Results

Scope: the pure/unit-level EARS clauses for the run-from-anywhere front door
(INT-0008). All tests are `ferric-cli` library tests, deterministic, no real
environment mutated. Runner: `cargo test -p ferric-cli --locked --lib`.

## T-12501 — configured models directory, decoupled from the workspace

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `startup::tests::resolve_models_dir_precedence` | WHEN no flag/env/config THEN default `<workspace>/models`; WHEN set THEN flag > `FERRIC_MODELS_DIR` > config > default | Each layer overrides the ones below; default is `<workspace>/models`. An injected `&dyn Fn(&str)->Option<String>` env accessor stands in for the process environment, so no real env var is set. | ok |

The resolution function is pure (no I/O), and the env layer is injected rather
than read from the process, so there is no shared-state or ordering flake.

## T-12502 — owner's 0/1/>1 picker rule + honest selection

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `human::enabled::tests::model_selection_is_by_count` | WHEN 0 → NoModel; 1 → Auto; >1 → Pick | `model_selection(0) == NoModel`, `model_selection(1) == Auto`, `model_selection(2) == Pick` — the branch is chosen by count, never by a saved preference. | ok |
| `human::enabled::tests::no_model_message_names_the_resolved_dir` | WHEN 0 GGUFs THEN a message naming the resolved dir | `no_model_message(dir)` contains the resolved directory's display path (not a generic "this folder"). | ok |
| `human::enabled::tests::picker_annotates_each_model_fit` | WHEN >1 THEN each model listed with its fit annotation | The rendered picker lines each carry a `fit_annotation` (Fits/Tight/WontFit/Unknown), so an over-large model cannot be selected unseen. | ok |

`choose_model` is driven through the existing `ScriptedIo` seam with an injected
model list; the selection branch is a pure function of the count, so the unit
layer proves the rule without a live engine.

## Execution evidence

```
running 9 tests
test human::enabled::tests::human_repeat_with_multiple_models_always_reasks ... ok
test human::enabled::tests::human_stale_single_model_still_auto_picks ... ok
test human::enabled::tests::model_selection_is_by_count ... ok
test human::enabled::tests::no_model_message_names_the_resolved_dir ... ok
test human::enabled::tests::picker_annotates_each_model_fit ... ok
test startup::models::tests::scan_dir_discovers_gguf_in_an_external_directory ... ok
test startup::models::tests::scan_dir_rejects_a_non_file_gguf_entry ... ok
test startup::models::tests::scan_dir_rejects_a_symlinked_gguf_in_an_external_directory ... ok
test startup::tests::resolve_models_dir_precedence ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 407 filtered out
```

(The two `scan_dir_*` discovery tests are reported under Integration.)
