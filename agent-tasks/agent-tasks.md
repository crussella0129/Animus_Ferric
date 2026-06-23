# Agent Tasks (Persistent Backlog)

> Sprint 7 re-framed (user-confirmed): restore harness-owned constrained decoding
> on the HTTP valve; delete the PyO3 backend. The prior T-501..T-505 (Python →
> subprocess HTTP server) are SUPERSEDED and removed — they added the very Python
> backend this sprint deletes. See `sprints/s7/sprint-plans/build-plan.md`.

- [ ] T-002 (sprint 7): `OpenAiProvider` emits `response_format` for a JSON-Schema constraint; honest `capabilities()` — touches: crates/ferric-provider/src/openai.rs
- [ ] T-003 (sprint 7): Unified `action_schema(tools)` + `parse_json_action` in ferric-loop — touches: crates/ferric-loop/src/{grammar.rs|action.rs,lib.rs}
- [ ] T-004 (sprint 7): Protocol trichotomy `NativeTools|ConstrainedJson|TextXml` wired through the loop + `select_protocol` reads capabilities — touches: crates/ferric-core/src/scale.rs, crates/ferric-loop/src/{run.rs,protocol.rs}, crates/ferric-cli/src/query.rs, ferric-loop/tests/*
- [ ] T-005 (sprint 7): Delete the PyO3 backend from ferric-provider — touches: delete crates/ferric-provider/src/python.rs + python/inference.py; edit Cargo.toml + lib.rs
- [ ] T-006 (sprint 7): Remove the Python backend from the CLI + PS1 drivers — touches: crates/ferric-cli/src/{backend.rs,query.rs,toolbench_cmd.rs}, test_both_models.ps1, run_benchmarks.ps1
- [ ] T-007 (sprint 7): Rebuild the toolbench around the active protocol's parser — touches: crates/ferric-cli/src/toolbench_cmd.rs
- [ ] T-008 (sprint 7): Record ADR-021 + ADR-022; correct lying docs — touches: decisions.md, crates/ferric-provider/src/lib.rs, README.md
