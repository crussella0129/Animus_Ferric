# Sprint 121 integration evidence

Immutable source: `2856c63209865f69b3d3727f84fd92f63f9dfa51`.
Root executed the canonical Windows `cargo test --workspace --locked -- --test-threads=1`
with existing Python 3.12.14 selected by `FERRIC_TEST_PYTHON` and local
`CARGO_INCREMENTAL=0`. Formatting, included-fixture formatting and workspace
all-target warnings-denied Clippy passed. [Per-suite output](windows-source-2856c63.txt)
confirms 1,299 passes/eleven documented ignores. The separate required
[CI checkpoint failed](ci-checkpoint-001.md); this is evidence, not a Test pass.

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

The independent source-head CI matrix passed Linux workspace (1,305/nine
ignores), backend-free CLI on both hosts (416/no ignores each), native
lifecycle (five Windows/six Linux), backend Clippy and ARM64 compilation.
Windows workspace failed one existing first-run journey after 507 partial
passes; its later CLI integration suites did not run on that CI host.
Root's local successes do not erase that failed gate. No CI rerun occurred.

Existing Ubuntu WSL2 has Rust and namespace tools, but `sudo -n` requires
interactive authentication. Native WSL formatting passed; the canonical
non-root namespace runtime gate was not run locally and no sudoers/root
bypass was made. Successful isolated Linux CI is not broad ordinary-host or
macOS parity. The optional local model smoke is retained separately in
[E2E evidence](e2e-tests.md).
