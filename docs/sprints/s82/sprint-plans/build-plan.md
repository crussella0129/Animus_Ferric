# Finalized - DO NOT EDIT

# Sprint 82 — Build Plan

**This is an audit sprint. The deliverable is a verified report, not a code
change.** The tree ends byte-identical to `dc15da3` except for three documents.

## Tasks

- [x] T-1 — Rebuild cold after the user's `cargo clean`; record warnings/time.
- [x] T-2 — Run the three checks s81 was blocked on (`test`, `clippy`, `fmt`).
- [x] T-3 — Re-derive A1–A7 at cited `file:line`; write failing probes where possible.
- [x] T-4 — Verify B1–B8 vestigial claims; prove B3 by removing deps and compiling.
- [x] T-5 — Verify C1–C8 and D1–D3 against current source.
- [x] T-6 — Fetch Dark Matter, run its verifier, probe the contract divergence.
- [x] T-7 — Write `docs/verification-2026-07.md` (durable; `sprints/` is gitignored).
- [x] T-8 — Write ADR-072 + README sprint-log entry.
- [x] T-9 — Delete all probes; confirm clean tree.

## Explicitly out of scope

Remediation of A1–A7. Each is a behaviour change deserving its own sprint with
designed tests; s81 deferred them and that judgement stands. Ordering is set in
`docs/verification-2026-07.md` §8.

Also deliberately **not** applied: the B3 dependency removal and the B7 duplicate
deletion, though both are proven safe. The user asked which code *could* be
removed; removing it is a separate decision that is theirs to make.
