Finalized - DO NOT EDIT

# Sprint 9 Test Plan — Fleet Calibration

## Unit Tests (default CI, `cfg(test)`)
- **T-901** (`toolbench_cmd.rs`): `render_leaderboard` — sorts a `Vec<BenchSummary>` by overall rate descending, contains each model name + its verdict band; combined JSONL has one block per model.
- **T-902** (`openai.rs`): `toolcall_from_content` — `{"name":"read_file","arguments":{"path":"x"}}` → `ToolCall{read_file}`; `{"tool":"read_file","args":{...}}` → same; `arguments` as a JSON *string* parses; ordinary prose → `None`. And the `complete()` parse path: empty `tool_calls` + tool-call `content` → a synthesized `tool_calls`.

## Integration Tests
- Scripted multi-`BenchSummary` (hand-built, mixed rates) → `render_leaderboard` ordering is best→worst and the combined `.jsonl` has the expected rows.

## End-to-End Tests (the deliverable — RUN it)
- **Fleet calibration run:** `ferric toolbench --backend openai --models qwen2.5-coder:7b,llama3.1:8b --protocol grammar --report fleet.md` against the running ollama → a real sorted leaderboard. Sweep 1–2 small `D:\Models` GGUFs via `--backend mistral` (TextXml) for the low end. **This produces the capability table — the sprint's headline artifact** (saved to the sprint findings).
- **mistral.rs 0.8.15 viability probe (ADR-023/024 gate):** `grammar_probe` (`trivial`, then `unified`) against the bumped dep on Llama-3.2-1B as a bounded subprocess. Returns → mistral.rs gains a constrained path (promote); hangs → TextXml-only (deprioritize). Record + update ADR-020/023 at Loop close.
- These are runnable now (ollama installed; GGUFs present) — no human setup required.

## Notes
- The leaderboard is a human-facing capability readout; it does **not** write `measured_level` (that stays `ferric bench`'s job, ADR-019). Kept distinct.
