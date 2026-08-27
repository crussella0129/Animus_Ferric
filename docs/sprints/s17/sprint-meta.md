# Sprint 17 Meta

- **Sprint number:** 17
- **Start timestamp:** 2026-06-25T22:10:29Z
- **End timestamp:** 2026-06-25T23:05:00Z
- **Model:** claude-opus-4-8
- **Exit status:** success
- **Token count:** (not observed)
- **Summary:** Durable promotion — closed the profile read-back loop. `model_profiles.json` was written by `ferric bench` but never read; now `toolbench --calibrate-rings --profile-dir` persists `calibrated_ring` (preserving `measured_level`) and `ferric query --profile-dir` reads the profile back, seeding `measured_level` (→ tier) + `calibrated_ring` (→ max_ring) so a proven model auto-runs at its earned capability. Operator `--max-ring` still wins; missing file = no-op. Proven end-to-end: llama3.2:1b calibrated → ring 1 persisted → query read Some(1). ADR-029.
