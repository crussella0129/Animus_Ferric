# Sprint 91 — Research Report

## 1. Goal

Docker became available, so validate A5's airlock against a real daemon — the
work blocked since sprint 33.

## 2. Getting the daemon up was itself informative

Docker Desktop was installed but its engine was not running: the `docker-desktop`
WSL distro showed `Stopped`, and `docker info` **hung** rather than erroring. That
hang is a defect in `check_available()`, not just an environment quirk — the
function sits on `Retriever::available()`, so a half-started daemon would stall a
research run indefinitely. Now bounded at 5 s.

## 3. The result that matters

This host has no `runsc`, so the default configuration could be tested for the
property that actually makes it trustworthy: it **fails closed**. `docker` refuses
the run with `unknown or invalid runtime name: runsc` rather than falling back to
the standard runtime. A security default that degrades silently is worse than
none, and this one does not.

## 4. Docker was not D2's blocker

`WebRetriever` is still unreachable from the binary, and wiring it in is still
wrong — but for a different reason than assumed. `NetworkPolicy::Proxy` exists
and **nothing constructs it**, because the allowlist proxy was deferred back in
ADR-040/045 and never built. So the two available configurations are:

- `Denied` — a web retriever that cannot fetch anything;
- `Unrestricted` — precisely the egress sprint 84 made opt-out.

**The proxy is the whole remaining gap**, and now the only one. That reframes the
next piece of Ornstein work from "needs a containerizer" to "needs the proxy".

## 5. Method note

The first suite run reported 5 passes that were 5 skips. The lesson is not
"add --nocapture"; it is that an availability-gated suite reports two different
things through one channel, and the timing was the only honest signal (269 s for
five container starts is implausible in the wrong direction).
