# INT-0009 — Lean, decomposed harness architecture

- **State:** planned
- **Work evidence:** [Sprint 123 ferric-cli library extraction plan](../sprints/s123/sprint-plans/build-plan.md#execution-sequence); [direction & refactor plan](../plans/2026-09-06-direction-and-refactor.md)
- **Test evidence:** (none yet)

## Intent

The agent harness's structure should read the way the crate graph already does:
a clean, layered core with peripheral concerns as satellites, and no single file
or crate carrying unrelated responsibilities. Today the layering between crates
is sound, but `ferric-cli` is a ~68 KLOC binary crate that mixes command
dispatch, the human surface, inference-server lifecycle, Tailscale serving, and
process ownership in flat multi-thousand-line files (`server.rs` alone is ~18 K
lines). A repeated external review — and the project's own template goals — call
for breaking these into modules, submodules, and where warranted separate
crates, so the harness core can stand alone and stay legible.

Decomposition here is **behavior-preserving reorganization**, proven by the
existing test suite, never a rewrite. Each increment leaves the shipped behavior
byte-for-byte and is justified by a concrete readability, reuse, or separability
win — long is not automatically tangled, and a move that only churns `git blame`
without a real win is declined.

## Acceptance criteria

1. `ferric-cli` exposes a library target that owns the command surface and
   modules; its binaries (`ferric`, and the feature-gated lifecycle binary) are
   thin shims over it, so no source file is claimed by two build targets and the
   duplicate-source build warning is gone (closes T-12028). The lifecycle
   fixture's binary-identity behavior is preserved.
2. The inference-server / Tailscale / process-ownership cluster is separable
   from the agent core: the loop + tools + provider path can be built and
   exercised without compiling the serving layer. (Later increment.)
3. No decomposition increment changes shipped behavior: the full workspace suite
   passes unchanged at each step, and any behavior change is refused here and
   routed through the owning intent instead.
4. The largest files are broken along their existing responsibility clusters
   into modules/submodules, each with a single coherent concern. (Later
   increment; `server.rs` is the flagship.)

## Rationale

The crate graph is already a clean DAG (leaves → mid libs → `ferric-loop` →
`ferric-cli`); the problem is concentrated inside `ferric-cli`. Extracting a
library is the enabling first step: it is required anyway to remove the
duplicate-`[[bin]]` warning cleanly (both `ferric` and `ferric-lifecycle-test`
currently point at `src/main.rs`), and it turns the binary into a thin
dispatcher that later module- and crate-level splits can build on without
touching the entry points again.

## Alternatives

- Leave `ferric-cli` as a flat binary crate and suppress the warning: rejected —
  it neither removes the two-targets-one-file coupling nor enables the
  decomposition the review calls for.
- Jump straight to extracting a `ferric-serve` crate: rejected as the *first*
  step — the library boundary and thin binaries are the prerequisite that makes
  a later crate split mechanical rather than entangled with the entry points.

## Consequences

The binary entry points become 3-line shims; the compile-time `CARGO_BIN_NAME`
binary-identity check must be threaded from the shim into the library (the
library has no `CARGO_BIN_NAME`). All existing `crate::` paths remain valid
because the code stays in one crate. Later increments (serving-layer crate,
`server.rs` module split) attach to this boundary.

## Transition history

- 2026-09-06: created as `proposed`. Sprint 123 plans the first increment — the
  `ferric-cli` library extraction with thin binaries — which also closes the
  INT-0008 T-12028 duplicate-source warning. Later increments (serving-layer
  separability, large-file module splits) remain proposed.
- 2026-09-06: moved `proposed` → `planned` when Sprint 123's Plan locked the
  library-extraction increment (T-12301), targeting AC-1; AC-2/AC-4 remain
  proposed for later increments.
