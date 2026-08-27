# Sprint 94 — Research Report

## 1. Goal

Wire `WebRetriever` into the binary — D2, open since the sprint-82 audit.

## 2. The contract mismatch that shaped the CLI

`research_all(retrievers, provider, query)` hands **one** query to every plane.
But `LocalFsRetriever` wants keywords and `WebRetriever::retrieve` rejects
anything that is not an `http(s)://` URL.

So `--research "some topic"` cannot drive the web plane, and overloading it would
have been the quiet kind of wrong — a flag that silently means two things. Hence
a separate `--research-url`, repeatable.

## 3. Deriving the allowlist from the URLs

The alternative was an allowlist in configuration. Deriving it is better on two
counts: there is no second source of truth to drift, and the permission granted
is exactly the permission requested — you may reach the hosts you named, nothing
else.

It also moves the security question into one small function. `url_host` must:

- strip **userinfo**, or `http://example.com@evil.test/` allowlists the wrong
  host while looking benign;
- drop the port, which is not an allowlist concept;
- reject anything `validate_host` would refuse, **before** the airlock opens, so
  a bad URL cannot leave docker resources behind.

## 4. The gVisor default

The default requires gVisor and fails closed (ADR-074), but no machine here has
`runsc`. Requiring it silently would make the feature dead on arrival — and a
control that everyone disables reflexively is worse than a calibrated one, which
is the E2 lesson.

So it stays the default with `--allow-standard-runtime` as a *named* opt-out, and
the flag's own documentation states the thing a user needs to know: **network
isolation does not depend on it.** The airlock enforces egress either way; gVisor
is defence in depth against container escape.

## 5. Result

All three Ornstein planes are now reachable from the binary. Tailnet-FS remains
unexercised live (no sshd on any tailnet host) — unchanged, and still recorded.
