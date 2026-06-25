# Agent Tasks (Persistent Backlog)

> Sprint 16 (ring **calibration**) is **done** — `ferric toolbench --calibrate-rings`
> sweeps a model ring-by-ring and reports the highest ring it reliably drives (the
> recommended `--max-ring`). Proven vs ollama: qwen2.5-coder:7b AND llama3.2:1b both
> calibrate to `--max-ring 1` at 100%. T-1601/1602 committed. The rings loop is closed:
> defined (s14) → controllable (s15) → measured/earned (s16). [[ferric-tool-rings]]

Open candidates (sprint 17+):
- **Persist the calibrated ring** into a model profile so it auto-applies on `query` (calibration currently *recommends*; this would make the promotion durable).
- More **Ring-1/2 tools** (find/organize, plan/diff) — gives calibration more rings to sweep.
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
