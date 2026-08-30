# Sprint 116 Test Report — Invalidated

## Verdict

**No pass verdict.** This report was originally written before the required
adversarial Test Phase critique. That critique found material locked-EARS and
evidence gaps, so the prior pass is withdrawn and Sprint 116 takes the
re-architecture failure route.

See the [failure report](../failure-report.md) and
[blocking critique](critique.md). The observed green gates below are retained
as regression evidence only; they do not advance INT-0008 AC-3, AC-4, AC-6,
or AC-7 by themselves.

| Gate | Observed result and boundary |
| --- | --- |
| CLI without default features | 214/214 passed locally before merge; no clause-level mapping retained |
| CLI with all features | 220/220 passed on three consecutive pre-merge runs and once within the post-merge full-workspace all-feature run |
| Full workspace with all features | passed once post-merge with Rust sources matching `e6439b1`; local observation rather than an immutable CI artifact; a restricted attempt first hit an expected nested-Python sandbox denial |
| Feature-gated lifecycle fixture | 3/3 passed locally on Windows and native WSL Linux; absent from ordinary CI |
| GitHub CI | PR run `33294229347` and post-merge run `33320491690` passed, but default-feature tests exclude the lifecycle fixture |
| Static/compile gates | strict Clippy, formatting, diff checks, Book checks, and the scoped AArch64 feature build passed |

## Reliability history

The first pre-merge all-feature sequence exposed a helper readiness timeout. A
test-only mutex stabilized the parent unit tests, but the feature-gated E2E
retains a separate release-then-bind port and fixed-deadline risk. That
remaining risk is part of the remediation, not a closed concern.

## Evidence boundary

- The model-free E2E proves useful happy-path and stale-local/live-global
  behavior without qualifying inference or a GGUF/backend coordinate.
- Runtime evidence covers Windows and native WSL Linux x86_64; no AArch64
  runtime result exists.
- Temporary workspaces prevented operator-state mutation.
- Aggregate green suites cannot fill the missing concurrency, fault-injection,
  output-contract, and provenance links named by the finalized plan.
