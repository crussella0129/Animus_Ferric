# Finalized - DO NOT EDIT

# Sprint 91 — Test Plan

Every test must **skip loudly** when no daemon is reachable — CI has none, and a
suite that fails on a missing optional dependency teaches people to ignore it.

| Property | Expected |
|---|---|
| sandbox executes at all | baseline; without it the rest is ambiguous |
| `Denied` network | egress **fails** |
| `Unrestricted` network | egress **succeeds** (proves the above isn't just a missing `wget`) |
| `--cap-drop=ALL` | `chown` fails |
| default with `runsc` absent | **errors** — never a silent fallback |

The pairing of rows 2 and 3 is the point: either alone is uninformative.

## Gate

`cargo test --workspace` > 529, clippy 0, fmt clean.
