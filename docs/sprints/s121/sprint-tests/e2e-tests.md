# Sprint 121 end-to-end evidence

## Immutable source and route

Tested source head: `2856c63209865f69b3d3727f84fd92f63f9dfa51` (completed
T-12104 `688b939a1f1793f36321fdb5ae570a5b75dcf272` plus its ledger backfill).
This record is Test input, not the final accepted report; the independent
critic and authoritative CI conclusion remain required.

The composed query/HTTP/resume/benchmark/profile routes are mandatory alongside
the live smoke. Their named scenarios, actual native suite confirmations and
synthetic-versus-real boundaries are retained in [integration evidence](integration-tests.md)
and [clause coverage](coverage-ledger.md). No argv-only or mock-L0 result
substitutes for those composed routes.

## Fresh local explicit-budget live smoke — passed

At the immutable head above, root executed:

```text
cargo test -p ferric-cli --bin ferric --release live_budget_tests::real_model_explicit_budget_smoke --locked -- --ignored --exact --test-threads=1 --nocapture
```

`CARGO_INCREMENTAL=0` avoids regenerating the previously exhausted compiler
cache. `FERRIC_LIVE_MODEL` selected the existing repository
`models/qwen2.5-coder-7b-instruct-q4_k_m.gguf`; no download occurred.
`FERRIC_BUDGET_LIVE_EVIDENCE_DIR` named a fresh `live-test-001` directory.
The actual Cargo parent entered the source-defined child under the existing
process owner; no target artifact was directly invoked by the operator.

One test passed, zero failures/ignores, in **13.55 s**. [Raw report](live-test-001/live-budget-report.json)
SHA-256: `35826c7d2a967b9b05e76330d6772e4fadcea14c9a47e8917b06ecd416f7a9c3`.
It retains the complete provider-admission request/response, original trace
bytes and digest, stage journal, verified endpoint/model and compiled
Windows/x86_64 `debug_assertions=false` coordinates.

- Model: 4,683,073,536 bytes; SHA-256
  `509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c`.
- Runtime: installed llama-server `10034 (505b1ed15)`; executable SHA-256
  `05db01f06449ddf2ac178efb761e5229b9e097170fae81ee08ee3a2dbe1fd976`.
- Actual prepared path: owned loopback engine, CPU-only, zero GPU layers,
  parallel one, context 4096, declared parameters 7B, temperature zero.
- Setup 6.069 s; engine preparation 3.633 s; model hashing 2.398 s;
  request 6.740 s; parent total 13.519 s. These are observations, not a
  hardware calibration or scheduling guarantee.
- Request and actual `MainActionBudget` trace both recorded explicit 1024.
  The model returned `task_complete` with summary `Ferric budget smoke complete`.
- Setup/request watchdogs were joined without firing; the source-owned engine
  cleanup and independent parent process-scope cleanup both passed. No manual
  process repair occurred.
- Trace SHA-256 `2eb300ca17142630dee9e9f5f8f45f0f4f3e40358cc177752f2b63c5f1f9804f`
  was independently recomputed from the retained UTF-8 trace bytes and matched.
  The isolated non-Git folder produced an explicit VCS snapshot note; Git or
  application success is not part of this smoke.

The setup/request/parent/cleanup/whole-test limits remain 90/30/150/5/180 s.
The deterministic cancellation and synchronous-stall fixtures independently
exercise those ownership paths with shortened test budgets. Only Windows
nested Jobs qualify abrupt nested-engine outer containment here; opt-in Linux
live execution is an explicit pre-effect non-pass. WSL does not remove that
known T-11904 boundary.

## Retained failure and build-profile diagnosis

The first debug Build attempt failed its 90-second setup deadline before any
main request, returning after checked cleanup at 94.835 s. Its original
[live-build-001 report](live-build-001/live-budget-report.json) is unchanged.
It lacked stage timings, so its preparation/hash split remains unknown.
Inspection exposed uncancellable bulk identity hashing in the fixture. The
fixture-only correction checks the original deadline/cancellation around each
64 KiB chunk and retains stage timing/partial journals; five regressions prove
those changes. Production startup/hash/process code was not retuned.

A fixed 16 MiB identity fixture took 386 ms in debug and 13 ms in release with
the same digest. This directly demonstrates local fixture hashing overhead,
not a model-inference speed effect. Independent review accepted using Cargo's
existing release profile with the same model/runtime/features/assertions and
limits. The separate corrected [live-build-002](live-build-002/live-budget-report.json)
passed before the fresh immutable-source Test run above. Neither later result
rewrites the initial failure or establishes debug-profile live acceptance.

## Explicitly not yet this sprint's E2E

INT-0007 T-11506 → T-11410 → T-11412 still owns the frozen larger-model
medium-horizon app, no-repair boundary, independent sandboxed grader, archive
and layered Sprint Loops support verdict. Automatic speed/fit/profile reuse is
T-11507/T-12023; reasoning and independently tuned compaction is T-11508. This
tiny prepared-path completion, scripted large-action tests and synthetic
calibration matrices do not satisfy those outcomes or realize either intent.
