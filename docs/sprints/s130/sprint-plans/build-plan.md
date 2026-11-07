# Sprint 130 Build Plan — Retire superseded surfaces

**Goal (owner, 2026-11-07):** carry out every retirement the owner approved
from the sprint 129 plan, §E (T-12906, INT-0018). Separately, analyse the
T-12907 options: inspecting only the owner's processes, versus giving
Ferric its own user account.

**Rules:**

- Each retirement removes its code, tests and documentation.
- Everything else keeps its behaviour, error text and wire formats, as in
  sprint 129.
- Every step passes the sprint 129 gates: fmt; clippy in every build,
  including Windows; the full suite; a test-inventory diff.
- CI is watched after **every** push.

| Step | Retirement | INT-0018 | Areas | Est. |
|---|---|---|---|---|
| R-1 | Tailscale Serve publication | AC-1 | server/, tailscale_*.rs, bin_identity, lifecycle-test binary, lifecycle fixture, docs | −11.5K |
| R-2 | Schema-1 runfiles and `ferric server adopt` | AC-2 | server/, server_registration.rs, server_resolution.rs, lifecycle fixture, docs | −2.6K |
| R-3 | Paired autonomy A/B runner | AC-3 | autonomy_cmd.rs, ferric-bench autonomy_results.rs | −2.5K |
| R-4 | Legacy `Tool::run` path and `ControlCapability` | AC-4 | ferric-tools, ferric-loop | −0.5K |
| R-5 | Backend-free build | AC-5 | ferric-cli, ferric-provider, CI, docs | −0.3K |
| R-6 | `HarnessPolicy::EvidencePlanner` | AC-6 | core, loop, trace, cli | −0.1K |
| R-7 | Replay of traces from before sprint 113 | AC-7 | ferric-loop replay/projector, ferric-trace | −0.15K |
| R-8 | Tailnet research retriever | AC-8 | ferric-research | −0.3K |
| R-9 | `tools/install.*` | AC-9 | tools/, docs/getting-started.md | −0.1K |

**Order:**

1. R-8 and R-9 (small, done by hand).
2. R-1 with R-2 (same files).
3. R-3.
4. R-4, R-6 and R-7 (the loop and tools core).
5. R-5.
6. Records and the PR.
