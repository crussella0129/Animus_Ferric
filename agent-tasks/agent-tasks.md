# Agent Tasks (Persistent Backlog)

> Sprint 27 (no-progress guard) is **done** — closed ADR-031's second failure mode
> ("semantic flailing"): a `ProgressGuard` tracking the same-tool-**name** streak
> (arg-insensitive) warns then stops with a precise `StopReason::NoProgress`, the
> complement to the repetition guard (which hashes name + args and misses
> different-args flails). Honest scope: bounds wasted compute + sharpens the bench
> diagnostic; does not lift a capability ceiling (ADR-037). 6 new tests incl. the
> defining contrast (ProgressGuard Stops where RepetitionGuard Proceeds). PR cadence clean.

Open candidates (sprint 28+):
- **GPU / edge run** — a CUDA llama.cpp build (or Jetson Orin Nano) to clear the s25 CPU timeouts + confirm the edge footprint; Gemma 4 might then reach L6.
- **Harder bench levels (L7+)** — rank above a 7B; the ladder currently tops out at L6.
- **More Ring-2 tools** — `apply_patch` (unified-diff application) beyond `multi_edit`.
- **MCP-stdio** (ADR-012, needs an ADR-005 security call); **`--chat` plain-LLM mode** (deferred).
- **Audio on real (non-TTS) audio; video modality.**
- **Repeated-failure guard** — stop when the last K tool results are all errors (a *different* mode from flailing, noted in ADR-037 research).
