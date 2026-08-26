# Finalized - DO NOT EDIT

# Sprint 86 — Test Plan

## Live (the point of the sprint)

Server lifecycle `doctor`/`up`/`status`/`down`; a real agentic task through the
constrained loop; trace inspection for what actually reached the model.

## Unit (for the tailscale fix)

FQDN read from `Self.DNSName`; a pin that the real payload has **no** top-level
`DNSName` (the regression); malformed/absent/blank input yields `None`, never
`https:///v1`; `serve --bg <port>` matches the documented form.

## Explicitly not run

`tailscale serve` — outward-facing, persistent, the user's call.

## Gate

`cargo test --workspace` > 503, clippy 0, fmt clean.
