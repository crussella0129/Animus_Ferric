# Sprint 113 Test Report

- **Tested source head:** `dbaada383cd58415dfc775ec2c9d7e55a28bbcd0`
- **Test date:** 2026-08-26
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Test verdict:** proceed-with-caveats
- **Intent outcome:** abandoned; the implementation is structurally verified, but the frozen performance hypothesis is falsified

## Outcome

Sprint 113's local Test phase passes its explicit no-candidate route. The Rust
implementation, product surfaces, compatibility boundary, runner mechanics,
Book-v2 Launch scaffold, and retained traces pass their named verification.
The intervention did **not** pass the real-model improvement gate: the unchanged
Evidence screen and both permitted general revisions each produced 0/3
objective-and-contract completions. That terminal result is preserved rather
than converted into a feature-success claim. Confirmation and held evaluation
were correctly skipped; EvidencePlanner remains unavailable and fail-closed.

## Acceptance-criterion result

| Criterion | Result | Evidence |
| --- | --- | --- |
| AC-1 typed evidence and Legacy compatibility | PASS | Literal pre-Evidence fixture, Legacy/controller rejection, supported-version and causal-order tests in [unit results](unit-tests.md). |
| AC-2 safe observations/publication and no implicit syntax execution | PASS with redundant-hardening caveat | Navigation, CAS/no-effect/opaque/syntax matrices, measured structural effects, in-process RustPython and `sitecustomize` side-effect tests. No separate fake-`PATH` canary exists. |
| AC-3 causal controller/check barriers | PASS | Prior-turn evidence, callback-before-denial, repair inspection, unchanged-check no-spawn, one-epoch multi-path integration. |
| AC-4 durable replay/recovery/compaction | PASS | Pause/crash, clarification, resume-of-resume, stale coverage, canonical packet, live resume, and compaction-independence tests. |
| AC-5 compatible surfaces and fail-closed planner | PASS | Query/resume/chat boundary plus bounded API/MCP/ICM propagation; shared planner rejection precedes bind/trace/provider/workspace effects. |
| AC-6 paired-runner attribution | PASS model-free | Frozen/copy/hash/freshness/schedule/collision tests and strict dirty/unpaired exclusion. No paired model episode was allowed after falsification. |
| AC-7 frozen Qwen gate and bounded revisions | FALSIFIED as designed; closeout route PASS with caveat | Screens 002–004 are complete, infrastructure-clean 0/3 results with distinct hashes; all 15 retained traces verify. H03's prompt-line observer seal is qualified, and no held outcome or tuning input was used. |
| AC-8 planner decision and coherent close | PASS for Test; final metadata/CI phase-ordered to Loop | Explicit evidence-based planner rejection, reconciled completion ledger, this report/critique, and fail-closed surface tests. Sprint metadata and PR-bound CI are final Loop evidence. |

The intent remains `abandoned`, not `realized`: passing structural tests cannot
override the failed material-improvement criterion.

## Local quality gates

| Gate | Result | Notes |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | Final source formatting. |
| Affected packages (`ferric-core`, `ferric-trace`, `ferric-tools`, `ferric-loop`, `ferric-bench`, `animus-launch`) | PASS | 32 core, 32 trace, 172 tools, 131 loop-library, 79 bench, and 22 Launch tests passed; declared fixtures ignored only. |
| `cargo test --workspace` | PASS | Full host run; all workspace/integration/live sandbox and container/doc targets passed. |
| `cargo test -p ferric-cli --features backend-openai` | PASS | 163 binary unit, 6 bench-mock, 62 CLI, 3 hygiene; no hanging future. |
| `cargo clippy --all-targets -- -D warnings` | PASS | Exact default-feature CI configuration. |
| `cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings` | PASS | Exact backend-enabled configuration. |
| `cargo check --workspace --target aarch64-unknown-linux-gnu` | PASS | Cross-target workspace check. |
| Book-v2 validator | PASS | `valid v2 Book (1 intent chapters)`. |
| `tools/demo-smoke.ps1` | PASS | Eight offline product checks, including deterministic Book-v2 Launch. |
| `git diff --check` | PASS | No whitespace errors. |

A restricted-sandbox workspace attempt failed/stalled only the Windows
process-inspection tests. The unchanged host-permission rerun passed those tests
and the full workspace; [integration results](integration-tests.md) retains the
harness classification instead of hiding the first signature.

## Real-model and lifecycle evidence

- [Development screen](development-screen.md): final run
  `autonomy-1787781412661-27096-0`, 3/3 scoreable, 0 infrastructure errors,
  0/3 objective plus contract, no clarification, revision budget exhausted.
- [Confirmation skip](confirmation-skip.md): no candidate hash, paired row,
  pair ID, confirmation workspace, or confirmation trace exists.
- [Held/teardown audit](held-and-teardown.md): no held episode/outcome/trace;
  H03 prompt-line caveat disclosed; evaluated process/listener/runfiles/health
  absent after shutdown.
- Fresh Test audit: all 15 frozen-control/screen traces verified read-only with
  stable before/after SHA-256 and “No tools were executed”; all 14 persisted
  rows bind to their trace, binary, model, and corpus identities.
- Fresh cold-state recheck: evaluated PID 48468 absent, port 8080 has no
  listener, local/global runfiles absent, and `server status` reports no server.

## Critic dispositions and remaining caveats

The final [Test critique](critique.md) returned `proceed-with-caveats`.

1. Authoritative CI is pending because `.github/workflows/ci.yml` is
   pull-request/main-push triggered; no `dev`-push success is invented. Loop
   must bind the pushed `dev` head to the PR and verify every required job.
2. The original autonomy shell command lines were not retained verbatim.
   Structured semantic inputs, outputs, server/binary/model/corpus identities,
   hashes, results, and traces are retained; no command transcript is
   reconstructed after the fact.
3. A migration search surfaced one H03 prompt line after candidate
   falsification. No held outcome or trace informed development, but the report
   makes no absolute observer-seal, held-generalization, or promotion claim.
4. Python no-execution evidence is strong but lacks a redundant fake-`PATH`
   canary. The RustPython-only code path, bounded/parser/no-temp tests, and
   import-side-effect marker prove the current SHALL response.
5. Raw byte-bound evidence retains machine paths under the immutable
   provenance exception; live template sources and new human-authored prose
   use documentation values.

## CI handoff

Loop must push `dev`, verify `git log origin/main..dev` contains only Sprint
113, open the sole `dev → main` PR, confirm its `headRefOid` equals the pushed
head, and require successful Ubuntu/Windows default jobs, Ubuntu
backend-openai Clippy, and aarch64 check. GitHub's workflow SHA is expected to
be a synthetic merge ref and must be recorded separately. The user owns the
merge; this sprint must not merge its own PR.
