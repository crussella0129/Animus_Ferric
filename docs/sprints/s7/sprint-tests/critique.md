# Test Critique — Sprint 7

> Self-critique (no subagent spawned) against `prompts/test-critic.md`'s seven
> failure modes. Each concern has an inline **Response**.

## Concerns

### C-001: T-006 "`--help` no longer lists python" has no explicit test
- **Where:** `build-plan.md` T-006 EARS ("WHEN `ferric query --help` lists `--backend` values, THEN `python` SHALL NOT appear"); `integration-tests.md` Component D.
- **Failure mode:** EARS-coverage
- **Why it matters:** the clause has no asserting `test_*`.
- **Response:** **reject (compile-enforced).** `--backend`'s value set is the `BackendArg` enum, which now contains exactly `{Mistral, Openai}` — clap derives `--help` from it. A `python` value cannot appear unless the enum regrows the variant, which would fail to compile. The `cargo tree` pyo3-count-0 + the workspace build under all backend features (both verified) cover the substance; a string-match on `--help` text would be a brittle restatement of a compile-time guarantee.

### C-002: Model-free tests prove the loop handles constrained output, not that a server *enforces* it
- **Where:** `integration-tests.md` Component A+B+C; `e2e-tests.md` E2E-1.
- **Failure mode:** e2e-cop-out (boundary)
- **Why it matters:** the 122 model-free tests prove the loop builds the constrained request, parses `{tool,args}`, dispatches, and traces honestly — but a `MockProvider` returns *already-valid* JSON. They do not prove a real server actually honors `response_format` (the whole thesis). ADR-009 requires a real-GGUF run for any change touching runtime/providers/grammar.
- **Response:** **defer-with-rationale (the human-heartbeat checkpoint).** This is by design the sprint's stop point (criterion #1): the real-model acceptance is the user's visual heartbeat, and the user explicitly anticipated it. The AI-verifiable system test (`mock_query_end_to_end`) proves the plumbing end-to-end; `e2e-tests.md` specifies the exact human-launched runs (E2E-1 capability probe is the load-bearing one). ADR-009 is a *merge* gate — the real-GGUF run is required before the Loop phase opens/merges a PR, not to close the Test phase. The mistral.rs TextXml path is now compile-verified (clean clippy against mistralrs 0.8.15) and the user has `Llama-3.2-1B` locally, so the heartbeat is immediately runnable.

### C-003: aarch64 cross-check not run locally
- **Where:** test-plan Verification (`cargo check --target aarch64-unknown-linux-gnu`).
- **Failure mode:** e2e-cop-out (env)
- **Why it matters:** the ADR-004 portability gate wasn't exercised locally.
- **Response:** **defer-with-rationale (CI-gated, low-risk).** The aarch64 target isn't installed on this machine. The sprint adds **no new default-graph dependency** (Constraint is pure serde_json; the action schema/parsers are pure Rust; the HTTP/mistralrs paths stay feature-gated and out of the aarch64 default graph), so the gate is unaffected by construction. CI's `aarch64-check` job is the source of truth on push.

## Confidence
`proceed-with-caveats` — every build-plan EARS clause maps to a tight, value-asserting test (122/0 green; negative paths covered for validate/parse/no-action/truncation; no flake risk — all deterministic, MockProvider-driven). The two real-environment gates (real-model acceptance, aarch64) are deferred to the human heartbeat and CI respectively, which is exactly where the stop criterion places them.
