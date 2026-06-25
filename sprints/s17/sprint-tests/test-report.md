# Sprint 17 Test Report — Durable promotion (profile read-back)

**Date:** 2026-06-25. The read/merge contract is proven by ferric-bench units; the
write→read handoff is proven end-to-end against ollama with a real persisted file.

## Unit (`ferric-bench` — green)
- `read_profile_round_trips_and_misses_are_none` — read-after-write == the record; wrong (model,protocol) key → `None`; missing file → `None`.
- `write_calibrated_ring_preserves_measured_level` — on a record the L0–L6 bench measured to `Some(4)`, writing the ring keeps `measured_level Some(4)` and sets `calibrated_ring Some(1)`.
- `write_calibrated_ring_creates_record_when_absent` — fresh model → record created with the ring, `measured_level None`.
- `old_json_without_ring_field_defaults_none` — a pre-field record deserializes with `calibrated_ring: None` (additive serde).

## Integration (`ferric-cli`, `--mock` — green)
- `persisted_calibrated_ring_caps_the_offered_tools` — a written `calibrated_ring: 0` for the model caps the trace's `PromptAssembled.offered_tools` to the Ring-0 core (no `search_files`, `write_file` present) **with no `--max-ring` flag**; an **empty** profile-dir leaves Small's Ring 1 (`search_files`) intact (no-op safety).

## Build / Lint
- `cargo test --workspace` green; `clippy -p ferric-cli --features backend-openai --all-targets -D warnings` clean; `fmt --check` clean.

## End-to-End — RAN it (ollama, real file handoff)
**1. Persist** — `toolbench --calibrate-rings --profile-dir /tmp/s17prof` for `llama3.2:1b`:
```
  ring | tools |   rate | verdict
     0 |     6 | 100.0% | solid
     1 |     8 | 100.0% | solid
  → Recommended --max-ring 1 (solid through ring 1)
    saved calibrated_ring 1 → …/s17prof/model_profiles.json
```
The written record: `{"model":"llama3.2:1b","protocol":"ConstrainedJson",…,"calibrated_ring":1}`.

**2. Read back** — `query --model llama3.2:1b --protocol grammar --profile-dir /tmp/s17prof` prints:
```
profile llama3.2:1b: measured_level None, calibrated_ring Some(1) (…/s17prof)
```
The profile written by one command is loaded and applied by the next — the durable
promotion handoff, end-to-end.

## Verdict
The model profile is now a real input to `query`. `bench` proves the tier,
`--calibrate-rings` proves the ring, both persist to `model_profiles.json`, and
`query` auto-applies them — closing the write-without-read gap (ADR-029). Safe by
default (no file ⇒ unchanged). No human-verification checkpoint.
