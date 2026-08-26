# Sprint 14 Meta

- **Sprint number:** 14
- **Start timestamp:** 2026-06-24T21:04:32Z
- **End timestamp:** 2026-06-24T22:30:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Formalized the tool rings — `ring` field on `ToolSpec`, `ring_for_tier` capability ceiling (honours `measured_level`), and a trim-from-outer `tools_for_policy` that fixes the alphabetical `max_tools` cap so the core is never dropped. Nano now gets exactly the 6-tool core; Small gets all 8; re-bench still 100% on both models. ADR-028.
