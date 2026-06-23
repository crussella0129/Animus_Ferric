# Agent Tasks (Persistent Backlog)

> Sprint 7 re-framed (user-confirmed): restore harness-owned constrained decoding
> on the HTTP valve; delete the PyO3 backend. The prior T-501..T-505 (Python →
> subprocess HTTP server) are SUPERSEDED and removed — they added the very Python
> backend this sprint deletes. See `sprints/s7/sprint-plans/build-plan.md`.

- [ ] T-005 (sprint 7): Delete the PyO3 backend from ferric-provider — touches: delete crates/ferric-provider/src/python.rs + python/inference.py; edit Cargo.toml + lib.rs
- [ ] T-006 (sprint 7): Remove the Python backend from the CLI + PS1 drivers — touches: crates/ferric-cli/src/{backend.rs,query.rs,toolbench_cmd.rs}, test_both_models.ps1, run_benchmarks.ps1
- [ ] T-007 (sprint 7): Rebuild the toolbench around the active protocol's parser — touches: crates/ferric-cli/src/toolbench_cmd.rs
- [ ] T-008 (sprint 7): Record ADR-021 + ADR-022; correct lying docs — touches: decisions.md, crates/ferric-provider/src/lib.rs, README.md
