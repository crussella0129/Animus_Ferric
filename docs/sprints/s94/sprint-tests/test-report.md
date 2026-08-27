# Sprint 94 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **545 passed / 0 failed** |
| clippy / fmt | 0 warnings / clean |
| leaked containers / networks | 0 / 0 |

## Unit — `url_host` (3 tests)

The allowlist is derived from the URLs, so this parsing *is* the boundary.

| Test | Pins |
|---|---|
| `url_host_reads_ordinary_urls` | scheme, path, query, fragment, port all handled |
| `url_host_ignores_userinfo` | `http://example.com@evil.test/` → **`evil.test`** |
| `url_host_rejects_what_it_cannot_allowlist` | non-http schemes, no host, injection-shaped hosts |

The middle one is the sharp case: keying on the decoration rather than the real
host would allowlist `example.com` while opening `evil.test`.

## Live — qwen2.5-coder-3B

| Case | Result |
|---|---|
| `--research-url http://example.com` | airlock opens for 1 host; URL fetched, quarantined, summarised correctly (22.8 s incl. ~15 s airlock) |
| same run then attempts `write_file` | **denied** — `sink policy: contaminated run; no approver available`; file not created |
| `--research-url "http://evil.test;wget"` | refused **before** the airlock opens: `host "evil.test;wget" only letters, digits, '.' and '-' are allowed` |

The second row is the one that shows the chain composes: fetching untrusted
content marks the run contaminated (ADR-080), and the provenance gate then does
its job without knowing anything about the web plane.

The third shows ordering: validation runs before any docker resource exists, so a
bad URL cannot leave an airlock behind.

## A note on the dead-code warning

`url_host` is only called from the backend-gated path, so the default build
flagged it unused. Allowed rather than gated, because its tests are security
tests and should run in **every** build — including the one without a backend.
