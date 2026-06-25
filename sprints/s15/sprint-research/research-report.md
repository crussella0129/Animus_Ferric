# Sprint 15 Research Report — `--max-ring`: control exactly which rings a model uses

> ADR-028 made rings real (gated by `tier`). The user's words were "you can
> **control exactly what rings** your model is using as its grammar." This sprint
> adds that explicit lever: a `--max-ring` that caps the active ring ceiling
> independent of tier — so you can pin a model to, say, just Ring 0.

## Decisions Reviewed
- **ADR-028** (rings) — the `ring` field + `ring_for_tier` ceiling + trim-from-outer `tools_for_policy`. This sprint adds the **explicit override** flagged there as a follow-on.
- **ADR-006** — config-supplied, never inferred: `--max-ring` is explicit operator config (it doesn't sniff). Clean.
- **ADR-019 / measured_level** — *expansion* beyond a model's capability stays earned: `--max-ring` only **caps** (restricts); to widen past the tier ceiling you raise `measured_level` (prove it). So the override is the safe "use fewer rings" knob; reliability is still the gate for "more".

## Existing code survey
| File | Change |
|------|--------|
| `crates/ferric-core/src/scale.rs` | `RunPolicy` gains `max_ring: Option<u8>` (None = use `ring_for_tier(tier)`); `policy_for` sets `None`. |
| `crates/ferric-tools/src/registry.rs` | `tools_for_policy`: ceiling = `ring_for_tier(tier).min(max_ring.unwrap_or(u8::MAX))` — caps the rings; everything else unchanged. **No signature change** ⇒ the loop's `registry_tools` call is untouched. |
| `crates/ferric-loop/src/protocol.rs:52` + `crates/ferric-loop/tests/common/mod.rs:17` | the two test helpers that build `RunPolicy {}` literals add `max_ring: None`. |
| `crates/ferric-cli/src/query.rs` | `--max-ring <u8>` on `QueryArgs`; after `policy_for`, `policy.max_ring = args.max_ring`. (Also `toolbench` — bench a specific ring set.) |
| `crates/ferric-core/tests/tier_table_snapshot.rs` | **untouched** — it field-asserts `policy_for` output, doesn't construct `RunPolicy`, and won't reference the new optional field. |

## Design (settled)
- **`RunPolicy.max_ring: Option<u8>`** — `None` ⇒ tier-derived ceiling; `Some(n)` ⇒ cap at `min(tier_ceiling, n)`. **Cap-only** (cannot raise above what the tier/`measured_level` allows — that path is reliability-earned).
- **`tools_for_policy` ceiling** = `ring_for_tier(policy.tier).min(policy.max_ring.unwrap_or(u8::MAX))`. The trim-from-outer logic (ADR-028) is unchanged; this only lowers the admit threshold.
- **CLI:** `ferric query --max-ring 0` pins the model to the Ring-0 core (smallest grammar) regardless of size; `ferric toolbench --max-ring N` benches exactly rings `0..=N`.
- **Semantics doc:** the override RESTRICTS; to EXPAND beyond the tier ceiling, bench the model and let `measured_level` promote it (ADR-019). This keeps "small grammar by choice" easy and "bigger grammar" reliability-gated.

## Risks / unknowns
- **Adding a `RunPolicy` field** touches the 2 test helpers + `policy_for` — mechanical, compiler-enumerated. The snapshot test is safe (field-assert style).
- **`u8::MAX` sentinel** for "no cap" is fine (rings are small); `min` makes a too-high `--max-ring` a no-op (capped by tier), which is the correct safe behaviour.

## Recommended approach
T-1501: `RunPolicy.max_ring` + the `tools_for_policy` cap + the 2 helper updates + a unit test (override caps the admitted rings; `None` = tier default). T-1502: `--max-ring` on `query` (+ `toolbench`) wiring + a `--mock` CLI test (`--max-ring 0` → only the 6 core in the grammar) + docs + an ADR-028 amendment noting the override shipped.
