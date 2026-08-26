# Sprint 102 build plan — Finalized - DO NOT EDIT

**Goal:** the model-facing truncation cap gets one definition and one source,
so run, replay, and `trace verify` cannot disagree about it.

## T-10201 — put the cap in the trace, and derive it there

1. **`ferric-core`** — move `DEFAULT_TRUNCATION_LIMIT` here (both `ferric-trace`
   and `ferric-tools` depend on core; neither depends on the other).
   `ferric-tools` re-exports it so `ferric_tools::DEFAULT_TRUNCATION_LIMIT`
   keeps working at every existing call site.
2. **`ferric-trace`** — add `truncation_limit: usize` to
   `Event::PolicySelected`, with `#[serde(default = ...)]` returning
   `DEFAULT_TRUNCATION_LIMIT` so traces written before this sprint still parse
   and still project exactly as they do today.
3. **`ferric-loop/src/run.rs`** — emit the registry's cap in the event.
4. **`ferric-loop/src/projector.rs`** — the `PolicySelected` arm sets
   `self.truncation_limit`. Delete `with_truncation_limit`: one field, one
   assignment path, sourced from the event in every caller.
5. **`ferric-loop/src/replay.rs`** — no change needed beyond (4); it gets the
   cap by replaying the event it already replays.
6. **`ferric-cli/src/trace_verify.rs`** — build the registry with the traced
   cap (`Registry::with_truncation_limit`) instead of `Registry::new()`.
7. **`ferric-cli/src/trace_cmd.rs`** — include the cap in the human-readable
   `policy selected:` line (the field is now part of the record; a `cat` that
   omits it hides the thing this sprint made visible).

## T-10202 — correct the record

- `decisions.md`: amend ADR-074's A1 bullet in place — the shared-formatting
  half stands, the word "identical" did not — and write ADR-093.
- `agent-tasks/agent-tasks.md`: close E3, recording that the entry was wrong
  in **both** directions (`compact.rs` was test-only; `trace_verify.rs` was
  missing).
- `agent-tasks/completed-tasks.md`: append T-10201/T-10202.

## Out of scope, deliberately

- **No CLI flag or config key for the cap.** Making it configurable is what
  would turn this latent bug live; the sprint's job is to make the plumbing
  correct *first*. Wiring a surface is a separate decision.
- **C7** (`ferric-cli` module split) and the skills `allowed-tools` decision
  stay open — the latter explicitly wants the user's call.
