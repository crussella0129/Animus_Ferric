# Agent Tasks (Persistent Backlog)

> Sprint 28 (repeated-failure guard) is **done** — completed the loop-hardening guard
> family. `FailureGuard` (`crates/ferric-loop/src/failure.rs`) keys off tool *results*:
> consecutive turns whose dispatched tools all errored → an early stop with
> `StopReason::RepeatedFailure` (WARN_AT=2/STOP_AT=3). Catches the "different tools, all
> failing" mode the repetition (resets on signature) and no-progress (resets on name)
> guards both miss. 6 new tests incl. the integration that stops different-failing-tools
> while the other two stay silent. Honest scope (ADR-038): bounds wasted compute +
> sharpens the diagnostic; does not lift a capability ceiling. PR cadence clean.
>
> **The three guards now compose by threshold:** repetition (2 identical strikes) →
> repeated-failure (3 all-error turns) → no-progress (5 same-name turns).

Open candidates (sprint 29+):
- **GPU / edge run** — a CUDA llama.cpp build (or Jetson Orin Nano) to clear the s25 CPU timeouts + confirm the edge footprint; Gemma 4 might then reach L6.
- **Harder bench levels (L7+)** — rank above a 7B; the ladder currently tops out at L6.
- **More Ring-2 tools** — `apply_patch` (unified-diff application) beyond `multi_edit`.
- **MCP-stdio** (ADR-012, needs an ADR-005 security call); **`--chat` plain-LLM mode** (deferred).
- **Audio on real (non-TTS) audio; video modality.**
- **A bench level that exercises the guards** — e.g. a deliberately-impossible task to confirm a real model hits `repeated_failure`/`no_progress` (vs only the scripted harness).
