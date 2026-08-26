# Sprint 18 Meta

- **Sprint number:** 18
- **Start timestamp:** 2026-06-25T22:55:30Z
- **End timestamp:** 2026-06-25T23:40:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Rounded out Ring 1 — added `find_files` (find by name, the companion to `search_files`' content search) and `copy_file` (the organize complement to `move_path`), making Ring 1 a coherent four-tool "find & organize" set. Pure-`std::fs`, guard-scoped, `ring: 1`; Small's `max_tools`=10 fits Ring 0 (6) + Ring 1 (4) exactly. Re-bench: both qwen2.5-coder:7b AND llama3.2:1b still calibrate `--max-ring 1` at 100% with Ring 1 now 10 tools — widening the ring cost zero reliability, even at 1B. ADR-028 amended.
