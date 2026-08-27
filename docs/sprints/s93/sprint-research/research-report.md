# Sprint 93 — Research Report

## 1. Goal

Give Ferric the airlock lifecycle ADR-082 left to the caller.

## 2. The design decisions that carry weight

**Attach order.** The gateway starts on the egress network *only*, and joins the
internal network after it reports ready. Otherwise a sandbox could reach a proxy
that is listening but has not yet loaded its filter — an open window at exactly
the moment the airlock looks up.

**Poll, never sleep.** `apk add` timing varies with the network. A fixed sleep
that is usually long enough is a flake generator; the poll also detects a gateway
that *exited* and surfaces its last log line rather than waiting out the timeout.

**RAII teardown.** A panic between `start` and an explicit `stop` would leak the
one container on the machine with egress. `Drop` is the only way to make that
impossible rather than merely unlikely.

**Fail closed.** No path returns a usable sandbox when the gateway did not come
up.

## 3. Allowlist validation is a boundary, not hygiene

Entries are written into a filter file by a shell command inside the gateway. An
entry containing `;` or `$(…)` is command injection into the container that by
construction *has* network access — the worst possible place for it. Validation
restricts to the DNS hostname charset, which removes the possibility instead of
escaping it, and runs before any docker resource exists.

## 4. What the sprint got wrong, and why it is instructive

The teardown test asserted on the shared `ferric-gateway-` prefix rather than on
its own container. It passed single-threaded and failed in the parallel suite.

The teardown code was right. The interesting part is that unique per-instance
naming was added *specifically* so tests and callers could distinguish airlocks —
and the first test written against it did not use it. Convenience (`contains`)
beat precision, and only parallelism exposed the difference.

## 5. State of D2

Unblocked. `Airlock::start` + `NetworkPolicy::Airlock` is a verified mechanism;
`WebRetriever` still is not reachable from the binary. What remains is wiring —
CLI surface, allowlist as configuration, and a per-run vs per-query decision that
matters because standing an airlock up costs ~15 s.
