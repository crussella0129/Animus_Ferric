# Sprint 123 Test Report — ferric-cli library extraction (INT-0009 AC-1 · T-12028)

## Verdict: PASS (clean)

Tested head: `d038ec6`. Local gates (dev host, Windows):

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean; plus the two extra feature shapes clean under `-D warnings`: `-p ferric-cli --features lifecycle-fixture` and `-p ferric-cli --no-default-features`.
- `cargo test --workspace --locked` — **40 suites ok, 0 failed** (ferric-cli library test binary 404/0, including the new `bin_identity` test and the relocated `routing_tests`).
- `cargo test -p ferric-cli --features lifecycle-fixture --test server_lifecycle_fixture` — **5/5**.
- `cargo build -p ferric-cli` — **zero** "found to be present in multiple build targets" warnings.

Authoritative multi-host CI (Linux + Windows, aarch64 check, lifecycle-fixture + no-default-features jobs) runs on push at the Loop phase.

## What was proven (INT-0009 AC-1)

| Acceptance outcome | Test | Result |
|---|---|---|
| Duplicate-source warning gone (T-12028) | `cargo build` warning count 0 | PASS |
| Fixture binary identity preserved (pure) | `name_is_lifecycle_fixture_matches_only_the_fixture_binary` | PASS |
| Fixture transport still activates (real) | `tests/server_lifecycle_fixture.rs` (5/5, unchanged) | PASS |
| Behavior-preserving | full workspace + `lifecycle-fixture` + `no-default-features` shapes | PASS |

The `ferric-cli` command surface now lives in a library; `ferric` and
`ferric-lifecycle-test` are thin shims over it, so no source file is claimed by
two build targets. The `CARGO_BIN_NAME` binary-identity gate — which cannot use
`env!` in a library — was threaded through a set-once `bin_identity` seam, and
the lifecycle fixture spawning the real `ferric-lifecycle-test` binary proves the
gate behavior is unchanged.

## Caveats (from `critique.md`)

- **C-001 (identity shift):** closed by evidence — the 5/5 lifecycle fixture and the pure predicate test prove the compile-time→runtime move preserved the gate exactly.
- **C-002 (refactor coverage bounds):** accepted — the code moved within one crate, every test moved with it, and `cargo test --workspace` runs all targets; the one source-structure guard was updated to the new file and re-verified.

## Intent status

INT-0009 is **active** and this increment (AC-1) is delivered; AC-2 (serving-layer
separability) and AC-4 (large-file module splits) remain active follow-on
increments, so the intent is **not** marked realized. INT-0008 **T-12028** is
closed by this work.
