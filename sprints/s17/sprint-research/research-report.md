# Sprint 17 Research Report — Durable promotion: read the model profile back

> Sprint 16 *measures* the ring a model earns (`--calibrate-rings`) but only
> *recommends* it. This sprint makes the promotion **durable**: persist the earned
> ring + read the profile back at `query` time so a calibrated model automatically
> runs at its proven tier and ring — "demonstrated reliability → durable promotion."

## Decisions Reviewed
- **ADR-019** — `ferric bench` is the SOLE producer of `measured_level`; tier changes only with a committed measurement. This sprint adds a *consumer* (read-back) without changing the producer, and persists `calibrated_ring` alongside it.
- **ADR-028** — rings: `ring_for_tier` ceiling + `RunPolicy.max_ring` (restrict-only). The read-back applies the persisted ring as the `max_ring` default; the operator flag still wins.
- **ADR-008** — deterministic, key-stable store (`model_profiles.json` merged by (model, protocol)); the new writer follows the same merge discipline.
- **New: ADR-029** — the profile is a real input to `query` (read-back of `measured_level` + `calibrated_ring`); safe no-op when absent.

## The gap found (grounded in code)
`model_profiles.json` is **written but never read.** `ferric bench` calls
`ferric_bench::calibrate()` → `write_profile()` to store a `ModelProfileRecord`
{model, params_b, protocol, **measured_level**, tiers} keyed by (model, protocol)
(`crates/ferric-bench/src/calibrate.rs`). But a `grep` for any reader returns
nothing: **every** `ModelProfile{}` construction site hardcodes
`measured_level: None` — including `query` (`crates/ferric-cli/src/query.rs:142`).
So the `measured_level` override (ADR-019, applied in `policy_for`,
`scale.rs:172`) is computed, stored… and ignored at run time. The persistence
loop is half-built.

## Existing pieces to reuse
| Piece | Use |
|---|---|
| `ferric_bench::{ModelProfileRecord, calibrate, write_profile}` | the store + writer; `write_profile` already merges by (model, protocol) key, keeping other records. |
| `ferric-cli` already depends on `ferric-bench` (`bench_cmd.rs`) | `query`/`toolbench` can call a new `read_profile` with no new dependency. |
| `policy_for` honours `ModelProfile.measured_level` (`scale.rs:172`) | applying a read-back `measured_level` needs no scale change. |
| `RunPolicy.max_ring` + `tools_for_policy` ceiling (s15) | applying a read-back `calibrated_ring` is just `policy.max_ring = ring` (operator `--max-ring` still wins). |
| `toolbench --calibrate-rings` + `recommend_max_ring` (s16) | the producer of the ring to persist. |

## Design (settled)
1. **Extend `ModelProfileRecord`** with `calibrated_ring: Option<u8>` — additive
   (`#[serde(default)]`), so existing records deserialize unchanged.
2. **`read_profile(dir, model, protocol) -> Option<ModelProfileRecord>`** in
   ferric-bench (exact (model, protocol) match; missing file/record → `None`).
   Plus a merge-writer `write_calibrated_ring(dir, model, protocol, params_b, ring)`
   that loads-or-creates the record and sets only the ring (preserving any
   `measured_level` the L0–L6 bench wrote).
3. **`toolbench --calibrate-rings` persists** each model's recommended ring via
   the merge-writer (to `--profile-dir`, default `benchmarks`).
4. **`query` reads the profile back**: a `--profile-dir` (default `benchmarks`);
   resolve the model name (from `backend_opts`) + protocol label; if a record
   exists, seed `ModelProfile.measured_level` from it (→ tier via `policy_for`)
   **and** default `policy.max_ring` to its `calibrated_ring` (explicit
   `--max-ring` overrides). **Safe:** no file ⇒ `None` ⇒ byte-identical to today.

So: `bench` proves the tier, `--calibrate-rings` proves the ring, both land in
`model_profiles.json`, and `query` auto-applies them. The operator still overrides
via `--max-ring`; widening past proven capability stays earned.

## Risks
- **(model, protocol) key match** — `query` must form the same model string +
  protocol label the writers used. Mitigation: resolve the protocol label from
  `--protocol`/backend default and key on it; a miss is a safe `None` (no worse
  than today). Documented.
- **Behavior change when a profile exists** — intended (that's the feature); gated
  to no-op without the file, and a `--mock` test pins the before/after.

## Recommended approach
T-1701: `calibrated_ring` field + `read_profile` + `write_calibrated_ring` in
ferric-bench (unit-tested: round-trip, missing→None, ring-merge preserves
measured_level). T-1702: `toolbench --calibrate-rings` writes the ring; `query`
applies the read-back profile (measured_level + ring); a `--mock` CLI test (a
written profile lifts the tier / sets the ring in the trace) + docs (the durable
promotion workflow) + ADR-029.
