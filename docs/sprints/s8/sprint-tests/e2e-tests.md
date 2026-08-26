# Sprint 8 E2E / Acceptance Tests

**Status: possible — the real runs need a model/server = the human heartbeat.**
The launcher's pure parts + the toolbench's diagnostic logic are fully unit-
tested; what remains is exercising a real server, which only the user can start.

## AI-verifiable (green now)
- `ferric server` smoke: `status`/`down` with no server, `up --help` flags — all behave (see `integration-tests.md`).

## Real-model acceptance (NEEDS HEARTBEAT — not run)
- `e2e_server_up_toolbench`: `ferric server up --engine llama-server --model <gguf>` →
  `ferric server status` (reachable) → `ferric toolbench --backend openai --protocol grammar --report report.md`
  produces a real diagnostic report (per-tool fire rate + taxonomy + verdict, constrained vs native) →
  `ferric server down`. This is the testbench made real, and the ADR-009 real-GGUF gate for the launcher + toolbench.
- `e2e_mistralrs_0815_viability` (ADR-023 decision gate): run `grammar_probe` (`trivial`, then `unified`) against
  mistralrs **0.8.15** on `Llama-3.2-1B` as a bounded subprocess.
  - **If it returns within the bound** → the upstream ADR-020 hang is fixed; mistral.rs gains a real constrained
    path (promote it: wire `supports_constraint`, re-enable `ConstrainedJson` selection for it).
  - **If it still hangs** → mistral.rs stays the TextXml-only fallback / deprioritized (per the "functionality over
    purity" rule). Either way, record the result and update ADR-020/023.

## Unlock
Both run from one `ferric server up` now that the launcher exists. The user
provides the model/server; this is the sprint's stop checkpoint.
