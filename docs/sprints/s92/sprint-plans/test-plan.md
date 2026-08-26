# Finalized - DO NOT EDIT

# Sprint 92 — Test Plan

## Before writing code

Demonstrate that the current `Proxy` policy can be bypassed, and that an
`--internal` network cannot. Neither is worth assuming.

## Unit

| Case | Expected |
|---|---|
| `Airlock` argv | attaches the **isolated** network |
| `Airlock` argv | **never** `--network bridge` (the negative assertion that matters) |
| `Airlock` argv | proxy env points at the gateway |

## Live (gated on Docker)

Stand up the real topology and assert all three:

1. allowlisted host → fetched
2. non-allowlisted host → refused
3. `unset http_proxy` → **unreachable**

Row 3 is the regression. Row 1 doubles as the anti-skip guard: it cannot pass
unless Docker, the network, and the gateway are all genuinely working.

## Gate

`cargo test --workspace`, clippy 0, fmt clean, no leftover docker resources.
