# Sprint 21 Meta

- **Sprint number:** 21
- **Start timestamp:** 2026-06-26T03:03:47Z
- **End timestamp:** 2026-06-26T03:55:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Fleet agentic capability map. Added `bench --models` (extracted `run_levels`; single path byte-identical; openai fleet sweep + a measured_level leaderboard). Ran the fleet: qwen2.5-coder:7b → 6 (Large, all L0–L6 pass); llama3.1:8b → 5 (Medium; L4/L6 fail); llama3.2:1b → none (fails even L0). Findings: (1) single-tool-call reliability ≠ agentic capability — a 1B fires single calls at 100% but can't complete a multi-turn task; (2) the code-tuned 7B beats the larger general 8B; (3) the ladder discriminates (6/5/none) so L7+ isn't urgent. Per-model measured_level persisted. ADR-030 amended.
