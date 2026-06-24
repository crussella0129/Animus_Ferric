# Agent Tasks (Persistent Backlog)

> Sprint 12: add a workspace `search_files` tool — the content-search primitive a
> small coding agent needs to locate code before reading/editing. Guard-scoped,
> dependency-free, mirrors `list_dir`, gated at `Nano`. Plan: `sprints/s12/sprint-plans/build-plan.md`.

- [ ] T-1201 (sprint 12): `SearchFiles` builtin (substring, capped, deterministic, guard-scoped) + register + tests — touches: crates/ferric-tools/src/builtin/search_files.rs (new), builtin/mod.rs, tests/builtin_file_tools.rs
- [ ] T-1202 (sprint 12): Document `search_files` + README timeline — touches: README.md, docs/

Larger follow-on candidate: MCP-stdio integration (ADR-012). Live-media E2E
heartbeat remains human-gated (needs a multimodal server).
