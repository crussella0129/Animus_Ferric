# Finalized - DO NOT EDIT

# Sprint 92 — Build Plan

- [x] T-1 — Check whether `NetworkPolicy::Proxy` enforces anything (it does not).
- [x] T-2 — Verify `--internal` networks genuinely isolate, DNS and raw IP.
- [x] T-3 — Prove the full airlock topology by hand before coding it.
- [x] T-4 — Replace `Proxy(url)` with `Airlock { network, proxy_url }`; argv tests.
- [x] T-5 — Live test standing up network + gateway + allowlist, asserting the bypass is closed.
- [x] T-6 — ADR-082, README, agent-tasks, sprint artifacts.

## Out of scope

The airlock **lifecycle** (Ferric creating the network and running the gateway)
and D2 itself. Named explicitly rather than implied — see the research report.
