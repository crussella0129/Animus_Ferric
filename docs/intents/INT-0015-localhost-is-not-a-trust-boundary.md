# INT-0015 — Localhost is not a trust boundary

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0015
- **State:** active
- **Work evidence:** [Sprint 127 build plan](../sprints/s127/sprint-plans/build-plan.md); [T-12701–T-12704 completed](../work/completed-tasks.md#t-12701-sprint-127); [T-12705–T-12709 open](../work/tasks.md#security--localhost-is-not-a-trust-boundary-int-0015)
- **Completion evidence:** none
- **Code evidence:** `crates/ferric-gate` (token, guard, layer, bind, tunnel); `crates/ferric-valve/src/{server,main}.rs` (`gated_router`, `--token-file`, `--upstream-key-file`); `crates/ferric-cli/src/{api,secure_cmd,tunnel_cmd}.rs`
- **Test evidence:** [Sprint 127 test report](../sprints/s127/sprint-tests/test-report.md)
- **Documentation evidence:** [docs/security.md](../security.md); [Sprint 127 research](../sprints/s127/sprint-research/research-report.md)

## Intent

Every Ferric network listener treats its own address as hostile territory.
Reaching `127.0.0.1` proves nothing about who is asking: any local process,
any other user, any container on the host network, and any web page in the
user's browser can reach it. That holds for cross-site simple requests, and
for DNS rebinding, where the browser also reads the response.

A request is served only when it:

- names this listener in `Host`,
- carries no browser fingerprint,
- presents this service's bearer token, and
- if it has a body, declares JSON.

These checks are compiled in and run in a fixed order. No request field and
no configuration value can relax them.

Remote use goes through a WireGuard tunnel the owner controls: keys generated
locally, no coordination server, and the Ferric host as the client's only
reachable address. The token is still required inside the tunnel.

The owner directed this on 2026-10-22, after the 2026 opencode
localhost-to-RCE advisories and the PHP2Shell pre-disclosure exploitation.
It covers both the Iron's valve (INT-0012) and the maintenance-mode
standalone API: securing an existing surface is a fix, not a new standalone
feature (INT-0010).

## Acceptance criteria

1. One crate owns the network boundary, and every Ferric HTTP listener
   (`ferric api`, `ferric-valve`) serves through it. No listener binds a
   wildcard or a public address. Loopback, or a private tunnel address
   recorded in the tunnel plan, are the only accepted binds.
2. A request whose `Host` does not name the listener is refused before
   authentication, including one that holds the valid token.
3. A request carrying `Origin` or any `Sec-Fetch-*` header is refused,
   including one that holds the valid token. No response ever carries a CORS
   header.
4. A request without the service's bearer token is refused. The token is at
   least 256 bits from the OS CSPRNG. It is stored mode 0600 outside every
   workspace, compared as a digest without early exit, never logged, and
   never forwarded to an upstream.
5. A body-bearing request that does not declare `application/json` is
   refused.
6. Refusals are distinguishable from application errors (a stable code in a
   response header and a JSON body), so a client or a scanner can tell what
   stopped a request.
7. A shipped probe replays the known attack classes against any service and
   exits non-zero if one reaches the application. The classes are: local
   process without or with a guessed token, cross-site form POST, `no-cors`
   `text/plain` fetch, DNS rebinding with and without the token, a browser
   holding the token, and a non-JSON body with the token. The probe also
   proves that a legitimate client still gets through. An offline demo shows
   the same attacks against a naive server for contrast.
8. Remote access works end to end through a self-hosted WireGuard tunnel on
   the GotaTun data plane:
   - Generated plans are mode 0600 and never overwritten.
   - The client's `AllowedIPs` is the host's single address, and a
     preshared key is set.
   - The service is reachable through the tunnel and not on the underlying
     network.
   - The probe passes across the tunnel.
9. Follow-ups are recorded, not implied:
   - the inference engine locked by `ferric server up`;
   - process sandboxing for zero-day containment;
   - tunnel parity on macOS and Windows;
   - a per-origin allow-list for a vetted web UI;
   - token rotation.

## Rationale

The gate's checks are independent so that a single failure does not open the
door. A leaked token does not help a web page, because the browser check
fires. A rebinding page that guesses the token does not get in, because the
`Host` check fires. A local process without the token gets nothing.

This is the same posture Ferric already takes toward the model: the harness
decides, the model never does. Here it is applied to the network.

## Alternatives

- **Rely on loopback binding.** This is the status quo that failed in the
  opencode advisories.
- **Rely on CORS.** CORS governs reads, not simple-request writes, and DNS
  rebinding makes the request same-origin anyway.
- **Unix domain sockets only.** Strong against browsers, but Hermes and
  other OpenAI-compatible clients speak TCP, and Windows support is uneven.
  This stays a candidate for a later local-only mode.
- **Tailscale for remote access.** Convenient, but its coordination server is
  a third party in the key-distribution path. Ferric's existing Tailscale
  serve integration stays available. WireGuard with locally generated keys is
  the default recommendation.
- **Embed GotaTun in the Ferric binary.** That would bind Ferric to a network
  daemon needing `CAP_NET_ADMIN`. Keeping the data plane a separate,
  Mullvad-maintained binary keeps Ferric unprivileged and its dependency
  surface small.

## Consequences

- Clients must present a token. Hermes sets `api_key` to the contents of
  `valve.token`; API clients send `Authorization: Bearer`.
- Browsers cannot use Ferric's HTTP services until T-12708 adds an explicit
  allow-list.
- The dependency allowlist (ADR-004) gains `x25519-dalek` and a direct
  `base64`. Both are pure Rust; `base64` was already in the tree, and
  `x25519-dalek` is the curve crate BoringTun and GotaTun themselves use.
