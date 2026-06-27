# Agent Tasks (Persistent Backlog)

> Sprint 29 (`apply_patch`) is **done** — rounded out Ring 2 (the rings-memory "room to
> grow"). `apply_patch` (`crates/ferric-tools/src/builtin/apply_patch.rs`, ring 2) applies a
> context-located unified diff to one file, atomically. Distinct from `multi_edit`: a hunk's
> context **disambiguates** which occurrence to edit (multi_edit hits only the first), plus
> diff-format familiarity. Line-based (round-trips trailing newline), single atomic write
> (failure = byte-identical). 5 tests incl. the contrast that edits the 2nd of two identical
> lines. Medium tier now offers 12 tools; Nano 6 / Small 10 unchanged. ADR-039. PR cadence clean.

Open candidates (sprint 30+):
- **Multi-file `apply_patch`** — a diff spanning several files (create/update/delete) with cross-file all-or-nothing. The clean follow-on to s29's single-file version.
- **GPU / edge run** — a CUDA llama.cpp build (or Jetson Orin Nano) to clear the s25 CPU timeouts + confirm the edge footprint; Gemma 4 might then reach L6.
- **Harder bench levels (L7+)** — best paired with a model stronger than the current 7B ceiling (which tops out at L6).
- **MCP-stdio** (ADR-012, needs an ADR-005 security call); **`--chat` plain-LLM mode** (deferred).
- **Audio on real (non-TTS) audio; video modality.**
- **A live calibration run driving `apply_patch`/Ring 2 under a real model** (qwen-7b `--max-ring 2`), confirming the new tool is drivable beyond the unit tests.
