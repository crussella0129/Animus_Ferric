# Agent Tasks (Persistent Backlog)

> Sprint 17 (durable promotion) is **done** — closed the profile read-back loop.
> `model_profiles.json` was written by `ferric bench` but never read; now
> `toolbench --calibrate-rings --profile-dir` persists `calibrated_ring` and
> `ferric query --profile-dir` reads the profile back, auto-applying `measured_level`
> (tier) + `calibrated_ring` (max_ring). Proven end-to-end (llama3.2:1b: write ring 1
> → query reads Some(1)). T-1701/1702 committed, ADR-029. [[ferric-tool-rings]]

Open candidates (sprint 18+):
- **Grow the Ring-1/2 tool sets** (find/organize, plan/diff) — gives calibration more rings to sweep and the profile more to carry.
- **MCP-stdio** (ADR-012) — needs the ADR-005 external-exec security call (the user's).
- **Live-media heartbeat** — human-gated on a multimodal server.
