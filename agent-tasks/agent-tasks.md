# Agent Tasks (Persistent Backlog)

> Sprint 13: complete **Ring 0** of the tool-rings north star — build `edit_file`
> (surgical replace) + `delete_path` (guard-scoped, recursive-gated), then re-run
> the toolbench to MEASURE the full core fires at 100%. Plan: `sprints/s13/sprint-plans/build-plan.md`.

- [ ] T-1301 (sprint 13): `edit_file` builtin (Write, first-occurrence replace) + register + tests — touches: crates/ferric-tools/src/builtin/edit_file.rs (new), builtin/mod.rs, tests/builtin_file_tools.rs
- [ ] T-1302 (sprint 13): `delete_path` builtin (Write, recursive-gated) + register + tests — touches: crates/ferric-tools/src/builtin/delete_path.rs (new), builtin/mod.rs, tests/builtin_file_tools.rs
- [ ] T-1303 (sprint 13): Docs — README builtin list + Sprint 13 timeline — touches: README.md, docs/

Next (sprint 14): formalize the rings — a `ring` field on `ToolSpec`, ring-aware
`tools_for_policy` (trim from the outer ring, fixing the alphabetical `max_tools`
cap), a config ring-cap + measured auto-promotion, ADR. See [[ferric-tool-rings]].
