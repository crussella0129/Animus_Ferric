# Sprint 84 Meta

- **Sprint number:** 84
- **Start timestamp:** 2026-07-24T21:06:59Z
- **End timestamp:** 2026-07-24T22:40:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Clear everything sprint 83 deferred — A4, A5, A7, C1–C5 and the
  Dark Matter contract divergence.

## Outcome

487 → **503 tests, 0 failures**; clippy 0 warnings; fmt clean; Dark Matter
verifier PASS 62 / FAIL 0 / SKIP 0. Five commits in Ferric, one in
Animus_Dark_Matter.

The sprint-82 remediation backlog is now fully closed, with one item deliberately
left open and stated (below).

## Two defects that were in no report

Both found by testing something *adjacent* to a known bug:

- **`shell_exec` had A4's panic pair too** — the same
  `block_in_place` + `Handle::current()` combination, in a Ring-0 tool reachable
  from far more paths than `manage_task`.
- **Background-task ids collided.** `task-{millis}` is not unique; two tasks
  started in the same millisecond got the same id and the registry, keyed by id,
  silently evicted the first — losing its `Child` handle. It surfaced as a test
  flake that I initially wrote off as harmless shared-global interference. That
  write-off was the mistake.

## Left open, deliberately

DM SPEC §6.2 specifies a `{chunks:[{uri,text,score}], truncated}` return envelope
where Ferric returns markdown. Flipping it changes what every small model sees
and would invalidate ADR-071's measured 97.5% prompt reduction, so it wants an
A/B behind it rather than a unilateral change. The *call* shape — the part that
was an outright incompatibility — is fixed.

## Also open (organisational, no behaviour)

C7 (`ferric-cli`'s 19 flat modules), C8 (scattered test-runner scripts), B1
(`Protocol`'s dead variants — a trace/profile schema change, not a deletion).
