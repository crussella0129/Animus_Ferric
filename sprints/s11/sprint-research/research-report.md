# Sprint 11 Research Report — Re-enable constrained decoding on the mistral.rs backend

> The sprint-10 close left several candidates. This research picks the one that
> is **high-value, AI-verifiable, and Rust-aligned**, and that closes a question
> we left half-answered: does the in-process mistral.rs path enforce a
> JSON-Schema constraint now that the ADR-020 hang is fixed?

## Decisions Reviewed
- **ADR-020** — the s2 llguidance/toktrie hang on GGUF JSON-Schema; the reason `complete()` strips the constraint. The premise being revisited.
- **ADR-022** — honest capabilities: mistral.rs reports `supports_constraint:false` because it strips. This sprint flips that *only if* enforcement is proven.
- **ADR-025** — 0.8.15 "returns but doesn't enforce" — now understood to have measured the *stripped* path; and the hang is fixed upstream.
- **ADR-026** — sprint-10 close; the candidate list this sprint chooses from.
- **ADR-001 / ADR-010** — the HTTP valve is the constrained workhorse; constraint XOR tools (holds — mistralrs strips tools). [[rust-purity-second-to-functionality]] — a working Rust constrained path is preferred if it exists.

## Candidates weighed (from ADR-026 close)
1. **Re-enable constrained decoding on mistral.rs** — wire the `Constraint` we currently *strip* into the engine and test enforcement. **← chosen.** Directly unlocks a pure-Rust constrained path if it works ([[rust-purity-second-to-functionality]]); fully AI-verifiable; bounded.
2. Live-media E2E heartbeat — **human-gated** (needs a multimodal server the machine lacks). Not an autonomous sprint.
3. `ModelProfile.modalities` + trace media descriptor — low value: `--modality` already gates input, and the trace never stores message content (no base64 leak today), so the descriptor is moot.
4. MCP-stdio integration (ADR-012) — high value but large; better as its own multi-sprint effort.

## The key finding (why the question is open)
`MistralRsProvider::complete()` (`crates/ferric-provider/src/mistralrs.rs:133–139`) builds the request via `map_messages` + `apply_sampling` and **deliberately drops the constraint** — the comment says *"No engine-level tools or grammar constraints are passed (s3 pivot)."* That strip dates to the ADR-020 hang.

So the sprint-8/9 `grammar_probe` (ADR-025) — which calls `provider.complete()` with a `Constraint::JsonSchema` — was unknowingly measuring the **stripped** path: mistralrs received *no* constraint, ran free, and returned the freeform "John Doe" JSON. **ADR-025's "doesn't enforce" was the strip, not mistralrs ignoring a constraint it was given.** The real enforcement behaviour is **untested**.

## mistralrs 0.8.15 actually supports constraints (verified in the git checkout)
- `mistralrs::Constraint` is re-exported (`mistralrs/src/lib.rs:288`) from `mistralrs-core`, with variants **`JsonSchema(serde_json::Value)`**, `Lark(String)`, `Regex(String)` (`mistralrs-core/src/request.rs:24–27`) — a **1:1 map** to our `ferric_provider::Constraint`.
- `RequestBuilder::set_constraint(constraint: Constraint) -> Self` (`mistralrs/src/messages.rs:1003`) — *"Apply a generation constraint (regex, JSON schema, or grammar)."*

So we can pass our schema straight through: `builder = builder.set_constraint(mistralrs::Constraint::JsonSchema(schema))`.

## Existing code survey
| File | Relevance |
|------|-----------|
| `crates/ferric-provider/src/mistralrs.rs` | `complete()` (strip site), `capabilities()` (`supports_constraint:false`, ADR-022), `map_messages`/`apply_sampling` (the RequestBuilder pipeline to extend). |
| `crates/ferric-provider/src/types.rs` | our `Constraint` enum (JsonSchema/Lark/Regex) — the source of the 1:1 map. |
| `crates/ferric-provider/tests/grammar_probe.rs` | the bounded-subprocess probe — now it will actually exercise enforcement once the strip is removed. |
| `crates/ferric-core/src/scale.rs` (`select_protocol`) | if mistral gains `supports_constraint`, the loop auto-routes it to `ConstrainedJson` instead of `TextXml`. |

## Risks / unknowns
- **The ADR-020 hang might recur** when a constraint is actually passed to 0.8.15. *Mitigation:* the probe runs as a **bounded subprocess** (external `timeout`), so a hang is contained and is itself a recordable result. The capability flip is **gated on the empirical result** — we do NOT advertise `supports_constraint` until the probe proves enforcement without hang.
- **It might enforce but slowly**, or **ignore** the schema. All three (enforce / ignore / hang) are clean, recordable outcomes.

## Recommended approach
1. **Wire it** — map `Constraint::{JsonSchema,Lark,Regex}` → `mistralrs::Constraint::*` and `set_constraint` in `complete()` (a pure mapping fn, unit-tested). Keep `supports_constraint:false` provisionally (the wiring is present but unadvertised until proven).
2. **Probe it** — re-run `grammar_probe` (`trivial`, then `unified`) through the now-wired provider on Llama-3.2-1B, bounded. Record enforce / ignore / hang.
3. **Decide + document (ADR-027)** — **if it enforces without hang:** flip `supports_constraint:true` → mistral.rs becomes a real pure-Rust constrained backend (the loop auto-routes it to `ConstrainedJson`); a major win. **If it hangs/ignores:** keep the strip (guard `set_constraint` off), and ADR-027 records the definitive in-process verdict, leaving the HTTP valve as the constrained workhorse (ADR-001/025).
