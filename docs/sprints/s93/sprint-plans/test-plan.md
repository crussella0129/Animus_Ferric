# Finalized - DO NOT EDIT

# Sprint 93 — Test Plan

## Unit (always run)

Allowlist validation, with the injection cases named explicitly: `;`, `$(…)`,
backticks, newline, quote, space, pipe, wildcard. Plus a rejected allowlist
creating no docker resources.

## Live (gated on Docker)

| Case | Expected |
|---|---|
| allowlisted host | **fetched** — also the anti-skip guard |
| non-allowlisted host | refused by the gateway |
| `unset http_proxy` | unreachable |
| `drop(airlock)` | **this** airlock's gateway is gone |
| rejected allowlist | no networks created |

Assertions about a specific airlock must name it — airlocks are unique per
instance and concurrent ones are expected.

## Gate

`cargo test --workspace` (parallel — single-threaded hides cross-test
interference), clippy 0, fmt clean, and zero leaked containers or networks.
