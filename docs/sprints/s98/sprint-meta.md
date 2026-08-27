# Sprint 98 Meta

- **Sprint number:** 98
- **Start timestamp:** 2026-07-25T19:59:35Z
- **End timestamp:** 2026-07-25T20:35:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** B1 — and the audit entry that described it was wrong in both
  directions.

## Outcome

**553 tests, 0 failures**; clippy 0; fmt clean. ADR-089.

## B1 as filed vs B1 as found

The sprint-82 audit said: *"never constructed or matched, but serialized into
profiles and traces, so removal is a schema change with a migration, not a
deletion."*

**Not a schema change.** Checked rather than assumed:

- `model_profiles.json` stores `protocol` as a **`String`**; the on-disk value
  is `"ConstrainedJson"` — PascalCase, which the enum's `snake_case` rename
  could not have produced.
- Traces serialize **`ActionProtocol`**, the live enum.
- Nothing on disk contains `fenced_code`/`edit_format`.

`Protocol` reached no persisted artifact. Nothing to migrate.

**Bigger than two variants.** Its only use was `RunPolicy.protocol`, always
`ConstrainedJson`, read by nothing but two test assertions. `ActionProtocol` has
been the real mechanism since ADR-015/022. The whole enum and field went.

## The thing worth more than the deletion

Profiles key on `(model, protocol)` with protocol a free-form `String`, and
**six** call sites each independently wrote `format!("{protocol:?}")`. They
agreed only by all reaching for `Debug`.

Rename an `ActionProtocol` variant and `read_profile` misses — and a miss is a
documented **safe no-op** (ADR-029), so the model silently runs at its
params-derived tier. The 7B would drop Large → Small and the only symptom would
be that it got worse.

One `protocol_key()` now, plus two tests: the exact strings as a persistence
contract, and agreement with the `Debug` format it replaced — because unifying
around a *different* string would have been the orphaning it was meant to
prevent.

Two of the six turned up only on a second grep, after the first pass looked
complete. A partial unification would have been worse than none.

## Verified against the real file

`ferric query --model qwen2.5-coder-7b --profile-dir benchmarks` →
`measured_level Some(6)`, matching ADR-086.

## Next

C7 (`ferric-cli`, 19 flat modules) is the last open item from the round-2 audit.
