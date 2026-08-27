# Sprint 102 research — E3: one truncation cap across run / replay / verify

## The logged claim

Backlog E3 (from the sprint-84 audit, recorded in ADR-074's correction at
`decisions.md:1341`):

> `run.rs:560` seeds the projector from `Registry::truncation_limit()`;
> `replay.rs:63` and `compact.rs:224` use `TraceProjector::new()` (default).
> Latent until someone configures a non-default cap. Also correct ADR-074's
> claim that run and replay are "identical by construction".

## What is actually true

Every construction of a `TraceProjector` in the workspace (3 total):

| site | cap | verdict |
|---|---|---|
| `ferric-loop/src/run.rs:603` | `args.registry.truncation_limit()` | correct |
| `ferric-loop/src/replay.rs:63` | `DEFAULT_TRUNCATION_LIMIT` | **divergence** |
| `ferric-loop/src/compact.rs:224` | `DEFAULT_TRUNCATION_LIMIT` | **not a divergence — inside `#[cfg(test)]` (line 148), a test helper** |

So the entry over-counted. `maybe_compact` takes `&TraceProjector` by
reference and inherits whatever `run()` built, so the compaction path was
never a second source.

**And it under-counted.** A third production site, not in the entry at all:

- `ferric-cli/src/trace_verify.rs:134` — `ferric trace verify` rebuilds the
  run from the trace to re-execute it against a `MockProvider`, and builds
  `Registry::new()` — the default cap. It faithfully restores `tier`,
  `protocol`, `max_turns`, `max_tools`, and both token budgets from
  `PolicySelected`, and then silently substitutes a default for the one knob
  the event does not carry. A trace produced under a non-default cap would
  re-verify against a different context window and report a mismatch that is
  an artifact of the verifier, not of the run.

## Root cause

Not three independent oversights. **The cap is not in the trace.**

`PolicySelected` carries six run knobs; `truncation_limit` is not one of
them, so nothing downstream of the trace file *can* know it. `run()` is
correct only because it has the live `Registry` in hand — the one context
where the trace isn't the source. Both other paths have only the trace, so
both had to guess, and both guessed the default.

That also settles what "correct the ADR-074 wording" means. The claim was:

> the **projector** now applies it, which keeps run and replay identical by
> construction rather than by parallel maintenance

The message-*formatting* logic genuinely is shared — that half stands. What
was over-claimed is the word *identical*: a shared function fed a different
parameter from a different source is not identical by construction. The fix
restores the property rather than only weakening the sentence.

## Blast radius

- `DEFAULT_TRUNCATION_LIMIT` lives in `ferric-tools`, but a serde default on
  the event needs the constant inside `ferric-trace`, and `ferric-trace` must
  not depend on `ferric-tools`. Both crates already depend on `ferric-core`.
  Moving the constant to `ferric-core` and re-exporting it from `ferric-tools`
  keeps every existing `ferric_tools::DEFAULT_TRUNCATION_LIMIT` path
  compiling.
- `Event::PolicySelected` is constructed or matched in 8 files. Adding a field
  needs `#[serde(default)]` so pre-existing traces on disk still parse — and
  the default is exactly today's behaviour, so old traces are unaffected.
- `TraceProjector::with_truncation_limit` becomes redundant once the projector
  derives the cap from the event: keeping both leaves two ways to set one
  field, which is the antipattern this sprint is about.

## Latency of the bug

Real but unreached: `with_truncation_limit` has no non-default caller outside
tests, and no CLI flag or config key sets it. Nothing a user can currently do
triggers any of the three. This is a correctness-of-the-record fix and the
removal of a trap, not a live-defect fix — and the report says so rather than
inflating it.
