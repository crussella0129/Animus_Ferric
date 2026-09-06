# Sprint 121 integration evidence

Corrected immutable source: `a417c5d00361fd25a238346e5015fb07ed5ae7c7`.
Root executed the canonical Windows `cargo test --workspace --locked -- --test-threads=1`
with existing Python 3.12.14 selected by `FERRIC_TEST_PYTHON` and local
`CARGO_INCREMENTAL=0`. Formatting, included-fixture formatting and workspace
all-target warnings-denied Clippy passed. [Per-suite output](windows-source-a417c5d.txt)
confirms 1,303 passes/thirteen documented ignores and zero failures. The
[corrected CI matrix](ci-checkpoint-003.md) and fresh live qualification passed;
final independent review/report remain acceptance gates.
The earlier [failed CI](ci-checkpoint-001.md), [instrumented non-reproduction](ci-checkpoint-002.md)
and [fixture correction with negative control](windows-reset-correction.md)
are retained separately; the historical native failure's cause is unknown.

## Actual query / HTTP / trace / resume

`query_output_budget` passed three tests, including actual streaming and
nonstreaming HTTP `max_tokens` versus policy/trace agreement, invalid cap
admission without server contacts or trace/workspace effects, and fragmented
request framing on Windows. `cli` passed 75 tests, including
`query_output_budget_resume_guidance_roundtrip`,
`query_resume_budget_is_invocation_scoped`, and
`query_resume_changed_reserve_rejects_before_effects`. The generated command is
executed through the supported shell with hostile path quoting, explicit cap
and declared context; manual omission/new cap is fresh and changed reserve
refuses without inherited authority or clipping. All HTTP workers are finite
and joined; all CLI children return through checked source-owned cleanup.

The Build record retains the initial hidden-worker failure, actual Windows
accepted-socket correction and negative control. Those results are not
rewritten by the passing immutable-source integration run.

## Actual benchmark / sidecar / row / profile

All ten `bench_budget` integrations and seven existing `bench_mock` cases
passed. They cover preflight refusal before Python effects; unchanged omitted
and explicit-one legacy mock behavior; explicit cap/context propagation;
actual provider requests, row/summary/sidecar/trace readback; provider error,
output limit and a six-second parent timeout after an actual request; and
retention-only or row-append-only infrastructure failures. Every successful
test return includes checked child cleanup and joined HTTP fixture workers.

The actual single/fleet full-ladder failure-preservation matrix contains eight
profile-store cases and 84 scripted HTTP provider errors. It preserves absent,
valid, unrelated and malformed profile bytes and diagnostic labels. The
Ferric-as-`--python-bin` fixture deliberately passes version preflight and
rejects Python grading argv, proving fixed-argv plumbing only. Separate
complete-success synthetic tests exercise the real shared publication helper;
neither is mislabeled real-model or grader success.

Authoritative Python grading tests in the library separately ran against the
explicit existing interpreter. The scale does not alter grader, fixture,
startup, capture or cleanup deadlines. Parent timeout, provider error, output
limit and recording failure remain different observed categories, without
claiming a generic provider error was a provider deadline.

## Human front door and native gates

`budget_docs` passed one test; `human_cli` eight; `human_docs` one;
`source_execution` two; `template_hygiene` three. They preserve `cargo r` as
the first example, the four primary actions, non-TTY zero-effect success,
bounded decline/EOF/configuration outcomes, expert discovery and documentation
of diagnostic controls/evidence without new mandatory setup choices.

The original source-head CI matrix passed Linux workspace (1,305/nine
ignores), backend-free CLI on both hosts (416/no ignores each), native
lifecycle (five Windows/six Linux), backend Clippy and ARM64 compilation.
Windows workspace failed one existing first-run journey after 507 partial
passes; its later CLI integration suites did not run on that CI host.
Root's local successes do not erase that failed gate. No CI rerun occurred.
The later instrumented matrix passed unchanged suites but was not accepted as
a repair. The corrected source adds a real reset-recovery journey, a composed
fatal-refusal/cleanup journey, a five-kind fatal matrix and deterministic
absolute-deadline coverage. All four passed in the fresh full Windows run.
The corrected immutable matrix then passed all eight jobs: Windows workspace
1,303/thirteen ignores, isolated Linux 1,309/nine, backend-free 416 each, native
lifecycle five/six, backend Clippy and both ARM64 checks. All four new regressions
passed both native workspace jobs; no timing, test schedule or assertion was
weakened to obtain this result. Full job identities and confirmations are in
[corrected CI evidence](ci-checkpoint-003.md).

Existing Ubuntu WSL2 has Rust and namespace tools, but `sudo -n` requires
interactive authentication. Native WSL formatting passed; the canonical
non-root namespace runtime gate was not run locally and no sudoers/root
bypass was made. Successful isolated Linux CI is not broad ordinary-host or
macOS parity. The optional local model smoke is retained separately in
[E2E evidence](e2e-tests.md). Additional [WSL qualification](wsl-checkpoint-001.md)
passed 35 process-free core tests and core Clippy with an unchanged locked
dependency graph, without claiming full runtime or ownership parity.
