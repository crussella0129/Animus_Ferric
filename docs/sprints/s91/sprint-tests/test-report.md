# Sprint 91 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **534 passed / 0 failed** (529 at start, +5) |
| clippy / fmt | 0 warnings / clean |

## Live sandbox validation — the first ever

Docker 29.6.1, Linux engine. Runtimes offered by this host: `runc`, `nvidia`,
`io.containerd.runc.v2` — **no `runsc`**, which makes the fail-closed case real
rather than hypothetical.

| Property | Direct evidence |
|---|---|
| `--network none` isolates | `wget: bad address 'example.com'` |
| `Unrestricted` reaches out | `Example Domain` fetched |
| gVisor default **fails closed** | `docker: Error response from daemon: unknown or invalid runtime name: runsc` |
| capabilities really drop | `chown` fails inside the container |

The third is the one that matters. A security default that silently degraded to
`runc` would be worse than no default at all; it errors instead.

## The near-miss

The first run of this suite reported **"5 passed" in 269 s**. That was 5 *skips* —
the suite is availability-gated, `check_available()` was hanging ~60 s per call
against a half-started Docker Desktop, and the SKIP lines only appear under
`--nocapture`. I nearly reported it as validation.

The real run takes **12 s**. The file's own header warns that a green run must
not be mistaken for a validated one — the warning existed because this confusion
was foreseeable, and it still nearly landed. Timing was the tell: 269 s for five
container starts was implausible in the wrong direction.

## `check_available()` now bounded

Root cause of the above: no timeout. `docker info` **hangs** when the CLI is
present and the daemon is not, rather than failing. Now capped at 5 s with the
probe reaped. This is not just test hygiene — `Retriever::available()` sits on
the research path, so an unbounded probe would stall a real run over an optional
dependency.

## What was NOT validated

`NetworkPolicy::Proxy` — nothing constructs it, because no allowlist proxy
exists. That is the sprint's other finding: it, not Docker, is what blocks D2.
