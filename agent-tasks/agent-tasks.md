# Agent Tasks (Persistent Backlog)

> Sprint 8: the self-diagnostic testbench (diagnostic `ferric toolbench` + `ferric server`
> launcher). See `sprints/s8/sprint-plans/build-plan.md`. Multimodal "any file" input is
> the planned sprint 9 (design locked in ADR-023).

- [ ] T-803 (sprint 8): `Engine` abstraction (llama-server default, Ollama) — pure command/URL — touches: crates/ferric-cli/src/server.rs, main.rs
- [ ] T-804 (sprint 8): `ferric server` subcommand + lifecycle + runfile — touches: crates/ferric-cli/src/{main.rs,server.rs}
- [ ] T-805 (sprint 8): `query`/`toolbench` auto-discover the server runfile — touches: crates/ferric-cli/src/{backend.rs,query.rs,toolbench_cmd.rs,server.rs}
- [ ] T-806 (sprint 8): Docs — README first-run/testbench + PS1 drivers wrap the launcher — touches: README.md, docs/testbench.md, run_benchmarks.ps1, test_both_models.ps1
