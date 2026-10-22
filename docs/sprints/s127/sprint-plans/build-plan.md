# Sprint 127 — Build plan

Owner direction (2026-10-22): make Ferric a production-grade harness on the
assumption that zero-days will keep coming. Secure service over localhost
fully, and use GotaTun (preferred) rather than Tailscale for remote access,
because Tailscale's coordination server is a centralization risk. The work
must be demonstrable end to end the same day.

## Execution sequence

| Task | What | Acceptance criteria |
|---|---|---|
| T-12701 | `ferric-gate` crate: token, guard, layer, bind | INT-0015 AC-1–AC-6 |
| T-12702 | Gate `ferric-valve` (plus upstream key) and `ferric api` (plus `--tunnel`) | AC-1, AC-4 |
| T-12703 | `ferric secure probe` and `ferric secure demo` | AC-7 |
| T-12704 | `ferric tunnel init/up/status/down` on GotaTun, plus a namespace end-to-end run | AC-8 |
| T-12705–T-12709 | Follow-ups recorded in the ledger | AC-9 |

## Design decisions

- **Four checks, fixed order, nothing configurable per request.** Each check
  covers a distinct class of attacker. The Host and browser checks run
  before the token check, so a leaked token never helps a web page.
- **Refuse browsers outright.** Every Ferric client (Hermes, curl, the IDE,
  scripts) is non-browser. Modern browsers send `Sec-Fetch-*` on every
  request and `Origin` on cross-origin requests and on all POSTs, so their
  presence is a reliable fingerprint. An allow-list for a vetted web UI is a
  deliberate later step (T-12708).
- **The token lives outside workspaces.** The model's file tools are
  contained to the workspace, so the model cannot read the credential that
  guards its own API.
- **Strip the credential at the gate.** Handlers and upstreams never see it.
  The valve presents llama.cpp's own key through a default header.
- **GotaTun stays a separate binary.** Ferric never needs `CAP_NET_ADMIN`;
  only `tunnel up`/`down` do, and they are separate commands. Ferric speaks
  the standard UAPI, so kernel WireGuard and the official apps take the same
  files.
- **Dependencies (ADR-004 amendment).** `x25519-dalek` 2 (`static_secrets`)
  derives WireGuard public keys; ring cannot export a static X25519 secret.
  `base64` 0.22 encodes WireGuard keys and was already in the tree
  transitively. Both are pure Rust. `tower` (already in the tree via axum) is
  a dev-dependency of `ferric-gate`.
