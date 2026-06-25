# Agent Tasks (Persistent Backlog)

> Sprint 16: ring **calibration** — `ferric toolbench --calibrate-rings` sweeps a
> model ring-by-ring and reports the highest ring it reliably drives (the
> demonstrated-reliability promotion the user described). Composes existing
> `bench_model`/`verdict`/`max_ring`. Plan: `sprints/s16/sprint-plans/build-plan.md`.

- [ ] T-1601 (sprint 16): `--calibrate-rings` sweep + pure `recommend_max_ring` + per-model report — touches: crates/ferric-cli/src/toolbench_cmd.rs
- [ ] T-1602 (sprint 16): Docs — calibration workflow + run_benchmarks.ps1 step + timeline — touches: README.md, docs/testbench.md, run_benchmarks.ps1

Later: persist the recommended ring into a model profile (auto-apply, not just
recommend); more Ring-1/2 tools; MCP-stdio (ADR-012); live-media heartbeat.
