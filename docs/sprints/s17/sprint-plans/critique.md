# Plan Critique — Sprint 17

> Self-critique against `prompts/plan-critic.md`.

## Concerns

### C-001: Read-back silently changes `query` behavior for existing users
- **Failure mode:** surprising-default
- **Response:** **gated + tested.** Reading defaults to `benchmarks/`, but with no file `read_profile`→`None`→behavior is byte-identical to today. The `--mock` "no-op safety" test pins this. The profile only takes effect once the user has actually run `bench`/`--calibrate-rings` (i.e., opted in by measuring). Documented in ADR-029.

### C-002: (model, protocol) key mismatch between writer and reader
- **Failure mode:** silent-miss
- **Response:** **accept, fails safe.** If `query`'s model string / protocol label doesn't match what the writer stored, `read_profile`→`None`→no worse than today (no crash, no wrong promotion). The label is derived the same way (`{protocol:?}`) and documented; a mismatch is a missed *optimization*, never a *regression*.

### C-003: `calibrated_ring` caps but `measured_level` can raise — do they fight?
- **Failure mode:** semantic-conflict
- **Response:** **they compose correctly.** `measured_level` raises the *tier* (capability ceiling); `calibrated_ring` caps `max_ring` at the *proven* ring. A model promoted to a higher tier still only gets rings it demonstrated. That's the intended "earned, not assumed" semantics — restrict-to-proven on top of measured-capability.

### C-004: Scope — touches a second crate (ferric-bench) + two CLI commands
- **Failure mode:** scope-creep
- **Response:** **bounded + cohesive.** ferric-bench already owns the store; T-1701 is ~3 small fns reusing the `write_profile` pattern. The CLI side is wiring a load into two existing flag-sites. The measured_level read-back rides the same path for free — fixing a genuine write-without-read orphan, not new surface.

## Confidence
`clean` — small additive primitives over an existing store, a safe-by-default read-back, AI-verifiable via ferric-bench units + a `--mock` before/after CLI test; the ollama E2E is confirmation, not the proof.
