# Agent Tasks (Persistent Backlog)

> Sprint 18 (round out Ring 1) is **done** — added `find_files` (find by name) +
> `copy_file` (organize complement to move_path), making Ring 1 a coherent four-tool
> "find & organize" set (search_files, find_files, move_path, copy_file). Re-bench:
> both qwen2.5-coder:7b AND llama3.2:1b still calibrate `--max-ring 1` at 100% with
> Ring 1 now 10 tools — widening the ring cost zero reliability. T-1801/1802/1803
> committed, ADR-028 amended. [[ferric-tool-rings]]

Open candidates (sprint 19+):
- **Ring-2 tools** (plan/diff) — the next ring out; gives calibration a third ring to sweep and a model a reason to earn Medium tier.
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
