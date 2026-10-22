# Sprint 127 — Critique

- **Risk: breaking existing clients.** The valve and API now need a token,
  so Hermes needs `api_key` set and API callers need `Authorization`. This is
  intended, and it is documented in the README, the valve README and
  docs/security.md. Accepted.
- **Risk: the token is readable by the user's own malware.** True of any
  bearer secret, and stated in docs/security.md. Containment is T-12706
  (OS sandbox). Accepted, with a follow-up.
- **Risk: the engine stays open on its own port.** `ferric server up` does
  not yet pass `--api-key-file`. The valve supports an upstream key, and the
  docs show the manual flag. T-12705. Accepted, with a follow-up.
- **Risk: the tunnel is Linux-only for `up`.** The plan files are standard
  WireGuard, so other platforms use the WireGuard app. T-12707.
- **Risk: browsers are refused outright.** No current client is a browser.
  T-12708 adds an explicit allow-list if a web UI appears.
- **Verdict:** proceed. Each check is independent and tested in isolation,
  and the end-to-end path is proven on real WireGuard.
