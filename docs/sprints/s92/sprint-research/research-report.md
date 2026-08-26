# Sprint 92 — Research Report

## 1. Goal

Build the allowlist egress proxy that sprint 91 identified as the last blocker
for Ornstein's web plane.

## 2. Why the sprint did something else first

Every sprint in this series that shipped a fix without first checking the
foundation produced a fix that did not work (A2's taint, sprint 83's endorsement
of `read-tree HEAD`). So before building the proxy, I checked whether
`NetworkPolicy::Proxy` — the thing it would plug into — constrains anything.

It does not. `http_proxy` is honoured by cooperative clients; a container runs
`unset http_proxy` and reaches the internet directly. Measured, with the proxy
pointed at a dead port, fetching example.com in full.

**Building a careful allowlist proxy behind a bypassable mechanism would have
been the most convincing version of this series' recurring mistake** — a lot of
correct-looking work enforcing nothing.

## 3. The correct foundation, verified before use

A docker network created `--internal` has no route out. Checked against both DNS
and a raw IP so the result could not be a name-resolution artefact:
`bad address` and `Network unreachable`.

## 4. The shape of the fix

`Proxy(url)` → `Airlock { network, proxy_url }`. Both fields load-bearing, and
the type now says which does what: the **network** enforces, the URL only points
cooperative clients at the gateway. The unit test's key assertion is negative —
never `--network bridge`.

## 5. What is done and what is not

**Done:** the type expresses an enforced airlock; the topology is proven live
(allowlisted fetch, non-allowlisted 403, bypass unreachable).

**Not done:** Ferric does not create the network or run the gateway. The live
test does that itself. That lifecycle — create, start, health-check, attach,
tear down, plus the allowlist as configuration — is the remaining piece before
D2, and `sandbox_live.rs::start_airlock` is the working reference for it.
