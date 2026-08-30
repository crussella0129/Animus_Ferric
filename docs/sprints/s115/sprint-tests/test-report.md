# Sprint 115 test report

## Verdict

**Aborted with partial results.** Ferric's changed query surface, qualified release, frozen
harness, sandbox, and exact CUDA Qwen runtime all passed. The qualified server
ended externally before the frozen MH-RS01 application trial began. No
medium-horizon application or new Sprint Loops capability result is claimed.

| Task | Result | Evidence boundary |
| --- | --- | --- |
| T-11414 | passed | Safe external query trace root and exact resume commands; Rust and CLI suites passed. |
| T-11501 | passed | Backend-enabled release bound to source and 20 passing qualification gates. |
| T-11502 | passed | Lossless quarantine, frozen harness, standalone Git, and network-disabled sandbox requalified. |
| T-11503 | partial; closed on abort | E17-A/B and publication of an immutable attempt-002 handoff passed. The handoff later expired unused; E17-C downstream consumption did not occur and is represented by T-11506. |
| T-11410 | not started | No frozen app invocation, continuation, candidate, traces, grader, or repair attribution exists. |
| T-11412 | incomplete | E12-A/B had no application evidence to archive or audit. The E12-C cold-state predicate and E12-D truthful Book disposition were completed independently. |

## Key measured result

The exact Qwen3.8-27B UD-Q4_K_M model ran through the managed CUDA b10516
server at context 32,768 with 24 of 66 layers offloaded, flash attention, and
Q8 key/value cache. It passed the constrained smoke and produced a median
3.565083339811294 decoded tokens/s across three scored samples.

## Excluded evidence

The operator-supplied field report describes a working small counter app on a
different CPU-only b10034 server at context 8,192. It is promising but lacks
the frozen MH-RS01 task, forced continuation, sandboxed grader, no-repair
attribution, effect reconciliation, and retained manifest. It informs the
ordered product backlog only.

## Closeout checks

- Runtime static control: 31 checks passed; both attempt snapshots unchanged.
- Attempt-002 offline runtime verification: 64 files passed with the exact
  predecessor control binding.
- Attempt-002 offline handoff verification: passed with canonical UTC identity.
- All five exact disposable roots: absent.
- Owned process and port 8080 listener: absent.
- Local/global server registrations: absent after exact-hash stale-local-only
  cleanup; no process signal sent.
- Unrelated Sprint 114 acquisition evidence: preserved unstaged and unchanged
  by this sprint closeout.

## Carry-forward

T-11504 lifecycle identity safety and T-11505 bounded calibration controls are
prerequisites to a freshly qualified release/runtime handoff (T-11506). Only
then may T-11410 and T-11412 resume. T-11507 through T-11509 retain the wider
runtime discovery, reasoning/compaction, and compact human-command-surface work.
