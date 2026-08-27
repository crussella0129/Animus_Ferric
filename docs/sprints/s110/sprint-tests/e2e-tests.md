# Sprint 110 End-to-End Tests

## Deterministic offline rehearsal

The final full `.\tools\demo-smoke.ps1` invocation rebuilt the release binary
and passed all eight checks:

1. version;
2. mock query creates the promised artifact;
3. trace cat plus side-effect-free trace verify;
4. `.env` attachment is denied;
5. skills list;
6. noninteractive launch creates the documented skeleton;
7. ICM init, plan, and three mock stages;
8. cron dry-run, real mock run, and due-state transition.

Before trace verification, the smoke removes the artifact named by the
recorded write and proves verification does not recreate it. The sensitive
attachment check requires the exact read-guard rule and proves no new trace was
created.

The Docker and live-model runners were not invoked, but their contracts were
statically repaired: Compose overrides the idle `sleep` entrypoint with
`ferric`, the PowerShell run validates the requested Python behavior and
deletion, and the live sweep validates exact original/copied content plus
deletion.

## Not exercised

- A live OpenAI-compatible model server.
- Docker Compose and the Ornstein airlock.
- Tailscale or remote filesystem transport.
- A non-loopback authenticated HTTP deployment.

These require external state unavailable on this workstation and are not
represented as passes.
