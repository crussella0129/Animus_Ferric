# Sprint 123 — Research Report

## Sprint Goal

Extract `ferric-cli` into a **library crate with thin binaries** — the first
increment of INT-0009 (lean, decomposed architecture) — which also **closes
T-12028** (the duplicate-source `[[bin]]` build warning observed in the human
terminal trials). Behavior-preserving: proven by the existing workspace suite,
no shipped behavior change.

## Existing Code Survey

- **The warning's real cause is deliberate and load-bearing.** `ferric-cli`
  declares two binaries over the *same* source (`crates/ferric-cli/Cargo.toml`):
  `ferric` → `src/main.rs`, and `ferric-lifecycle-test` → `src/main.rs`
  (`required-features = ["lifecycle-fixture"]`). The second exists so a second
  binary is built from identical code under a **different `CARGO_BIN_NAME`**:
  production code branches on it at `crates/ferric-cli/src/tailscale_localapi.rs:219`
  (`env!("CARGO_BIN_NAME") == "ferric-lifecycle-test"`) to enable a model-free
  fixture LocalAPI transport, and `crates/ferric-cli/tests/server_lifecycle_fixture.rs:106`
  spawns it by name via `CARGO_BIN_EXE_ferric-lifecycle-test`. So the warning
  cannot be silenced by deleting a target; the two names must both exist.
- **`main.rs` is already thin-ish and mechanical to lift.** `crates/ferric-cli/src/main.rs`
  (304 lines) holds ~28 `mod` declarations (lines 18-50), the `clap` `Cli`/`Command`
  types, `fn main` (162), `dispatch` (177), `resolve_cli` (219), and a
  `routing_tests` module. Moving the modules + `Cli`/`dispatch` into a `lib.rs`
  and leaving `fn main` a shim is a pure code move; every `crate::` path stays
  valid because it remains one crate.
- **Two lifecycle binaries already coexist.** `ferric-lifecycle-fixture`
  (`src/bin/ferric_lifecycle_fixture.rs`, a real separate file) shows the
  `src/bin/` shim pattern the new `ferric-lifecycle-test` shim will follow.
- **Tests reference the binaries only by `CARGO_BIN_EXE_*`** (`tests/cli.rs`,
  `human_cli.rs`, `server_lifecycle_fixture.rs`, …), so as long as the two bin
  *names* survive, the harness is unaffected.

## External Sources

- Cargo: a library target and one or more binary targets in the same package is
  the idiomatic shape; `env!("CARGO_BIN_NAME")` is defined **only** when
  compiling a binary, not a library — so the identity check must move to the
  binary shim and be passed into the library (e.g. `run(bin_name)`), or become a
  runtime `current_exe()` basename check. The compile-time-to-parameter route
  keeps the exact current semantics with no new failure mode.

## Risks / Unknowns / Dependencies

- **CI-critical fixture path.** The lifecycle-fixture jobs (both hosts) and the
  Tailscale fixture-transport gate are the highest-risk touch. The bin-identity
  behavior must be preserved exactly; the plan threads `CARGO_BIN_NAME` from each
  shim into the library rather than changing the detection semantics.
- **Breadth vs. depth.** The move touches every `mod` line in `main.rs` and the
  two/three bin files, but it is mechanical and behavior-preserving; the risk is
  a missed re-export or a `pub(crate)` visibility that a bin shim needs.
- **`no-default-features` and `lifecycle-fixture` build shapes** must both still
  compile (CI covers both) — the lib must be feature-clean.
- **Scope discipline (INT-0009).** This increment is *only* the library boundary
  + thin bins + the warning fix. It does **not** split `server.rs`, extract a
  serving crate, or move files into subdirectories — those are named later
  increments so this sprint stays a reviewable, behavior-preserving move.

## Recommended Approach

1. Add `crates/ferric-cli/src/lib.rs` owning the ~28 `mod` declarations, the
   `Cli`/`Command` types, `dispatch`, `resolve_cli`, and a `pub fn run(bin_name:
   &str) -> ExitCode` (today's `fn main` body, taking the binary name).
2. Reduce `src/main.rs` to a shim: `fn main() -> ExitCode { ferric_cli::run(env!("CARGO_BIN_NAME")) }`.
3. Add `src/bin/ferric_lifecycle_test.rs` with the same shim (its `CARGO_BIN_NAME`
   is `ferric-lifecycle-test`); point the `[[bin]]` at it. Declare `[lib]`.
4. Thread `bin_name` from `run` to the identity gate in `tailscale_localapi.rs`,
   replacing the library-invalid `env!("CARGO_BIN_NAME")` with the passed value;
   keep the exact `== "ferric-lifecycle-test"` semantics.
5. Verify: full workspace `fmt`/`clippy`/`test` green, plus the
   `lifecycle-fixture` and `no-default-features` build shapes, and confirm the
   duplicate-source warning is gone from a plain `cargo build`.

## Intents Reviewed

- [INT-0009 — Lean, decomposed harness architecture](../../intents/INT-0009-lean-decomposed-architecture.md)
  — **created** (`proposed`). This sprint is its first increment (the library
  extraction); AC-1 is the sprint's target, AC-2/AC-4 are named later increments.
- [INT-0008 — Unified local model workflow](../../intents/INT-0008-unified-local-model-workflow.md)
  — **selected.** T-12028 (the duplicate-source warning "observed in the real
  terminal trials") is closed as a consequence of INT-0009's AC-1; no INT-0008
  text change is required.

## Referenced Artifacts

- This report: `docs/sprints/s123/sprint-research/research-report.md`
- New intent: `docs/intents/INT-0009-lean-decomposed-architecture.md`
- Direction plan (context): `docs/plans/2026-09-06-direction-and-refactor.md`
