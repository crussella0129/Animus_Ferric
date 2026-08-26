# Sprint 110 Walkthrough — Monday demo readiness

## What changed

The demo path now fails closed at its important boundaries:

- trace verification observes and validates; it never replays tools;
- attachments cannot escape or bypass workspace read policy and are bounded;
- model-visible tools cannot launch a host shell or manage host processes;
- incomplete stop reasons are failures on every surface;
- mock API, unauthenticated binding, and launch behavior match their CLI
  contract;
- E2E runners preserve failures and assert observable artifacts.

## Monday path

From the repository root:

```powershell
.\tools\demo-smoke.ps1
```

The script builds `target/release/ferric.exe` with `backend-openai` and then
runs eight offline checks in a disposable workspace. A successful result ends
with all eight checks passed. It does not depend on PATH, Docker, a running
model server, network access, or Tailscale.

For manual follow-up, keep the same PowerShell session and use the
`docs/demo-guide.md` setup, which binds `ferric` to that exact release binary
instead of an older installed copy.

For a quicker repeat after the release binary is already built:

```powershell
.\tools\demo-smoke.ps1 -SkipBuild
```

## Live-demo boundary

Treat a live model/server, Docker airlock, and Tailscale demonstration as
optional additions, not prerequisites. They were unavailable during this
sprint and are not represented as validated. The deterministic offline flow is
the rehearsed fallback if any external service is unhealthy on Monday.

## Verification

Workspace tests, backend-feature tests, both strict Clippy configurations,
formatting, the aarch64 check, release build, Bash static analysis, whitespace
validation, and the eight-check smoke run all passed locally. Details are in
`sprint-tests/test-report.md`.
