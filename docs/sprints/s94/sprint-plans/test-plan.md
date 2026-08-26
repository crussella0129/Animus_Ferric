# Finalized - DO NOT EDIT

# Sprint 94 — Test Plan

## Unit — the allowlist boundary

| Case | Expected |
|---|---|
| ordinary URLs | host extracted, port/path/query ignored |
| `http://example.com@evil.test/` | **`evil.test`** — userinfo must not decide the allowlist |
| non-http scheme, no host, injection-shaped host | refused |

## Live

| Case | Expected |
|---|---|
| `--research-url` a reachable host | airlock opens, content quarantined into the prompt |
| a mutation in that same run | **denied** — the run is contaminated |
| a URL whose host cannot be allowlisted | refused **before** the airlock opens |

Row 2 is the one proving the chain composes end to end; row 3 proves ordering.

## Gate

`cargo test --workspace` > 542, clippy 0, fmt clean, no leaked docker resources.
