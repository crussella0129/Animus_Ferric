# Sprint 105 test plan — Finalized - DO NOT EDIT

## The one new test that matters

**A guard against re-acquiring machine identity.** Everything else this sprint
does is a one-time cleanup; without a test, the repo drifts straight back the
next time someone pastes real `tailscale status` output into a fixture.

`no_machine_identity_in_tracked_sources` — walk the crate sources and fail on
tailnet-identity patterns (a `tail<digits>.ts.net` MagicDNS suffix, an `@`
account handle in a status sample, a `C:\Users\<name>` path). It must be
specific enough not to fire on legitimate content — this repo's prose is full
of the word "tailscale" — so it matches **shapes that can only be identity**,
not topics.

The test has to be checked in the direction that matters: confirm it **fails**
when a real-looking suffix is reintroduced. A guard that has never rejected
anything is not known to reject anything — the lesson from sprint 96's
skip-and-pass and sprint 101's false positive.

## Regression surface

- `cargo test --workspace` — 575 green. The fixture edits change values, not
  shapes, so every assertion must still hold. **If one fails, the fixture was
  asserting identity rather than structure**, and that is worth knowing.
- clippy 0, fmt clean.

## Verifying the untracking did not break the build

`git rm --cached` leaves files on disk, so a local build proves nothing about a
fresh clone. **Clone the repo to a temp directory from the local remote and
build there** — that is the only way to see what a template user gets. Check:

1. It builds and the suite passes with `sprints/`, `scratch/` and
   `benchmarks/results.jsonl` absent.
2. `ferric query --mock` runs with **no** `model_profiles.json` present —
   ADR-029 says a missing profile is a safe no-op, and this sprint makes that
   the default state for every new user, so the claim needs exercising rather
   than citing.

## Not covered

Whether the git *history* still contains the identity being removed from the
tree. It does — that is stated in the ADR as a decision for the owner, not
fixed here, and no test should imply otherwise.
