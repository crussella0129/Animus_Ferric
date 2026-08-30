# Sprint 115 end-to-end tests

## Managed runtime — passed

T-11503 attempt 002 launched the T-11501-qualified Ferric binary exactly once
against Qwen3.8-27B UD-Q4_K_M at context 32,768, 24 GPU layers, 12 threads,
batch 512, seed 42, and one parallel slot. The CUDA b10516 server proved 24 of
66 layers offloaded, flash attention, Q8 key/value cache, health/model identity,
owned process/listener/runfiles, a two-turn constrained smoke, one warmup, and
three scored 256-token samples. Median decoded throughput was
3.565083339811294 tokens/s, above the frozen 2.0 threshold.

The same process was published as a `qualified_running` handoff. It later
ended outside the frozen trial before downstream consumption, so the
qualification remains valid historical evidence but not a reusable live
handoff.

## Frozen application — not started

T-11410 did not begin. No one-turn segment, 27-turn linked continuation,
candidate mutation, network-disabled grader, application score, or app-run
manifest exists. The external three-turn counter app used a different task,
engine, context, and evidence protocol and is explicitly excluded.

## Cold state — passed as a closeout predicate

The five exact disposable roots, owned process/listener, and both server
registration paths are absent. The final stale local registration was removed
only after its SHA-256 matched the retained attempt-002 handoff, the global
registration and listener were absent, and the retained process identity was
not live. No process signal was sent; models, committed evidence, and retained
quarantine remain.
