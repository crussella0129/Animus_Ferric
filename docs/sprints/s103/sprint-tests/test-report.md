# Sprint 103 test report

**574 → 575 tests, 0 failures.** clippy 0 and `cargo fmt --check` clean in
**both** feature configurations.

This is a behaviour-neutral refactor, so the 574 existing tests *are* the
specification of what had to be preserved. The one new test covers the one
claim nothing checked.

## New test (1)

`the_backend_feature_diagnostic_names_the_feature_and_the_alternative`
(`ferric-cli/src/backend.rs`) — the feature-off message must name both the
cargo feature to rebuild with and `--mock`. It is a user-facing error in a path
a normally-built binary cannot reach, which is exactly why it went unnoticed as
three byte-identical copies; a unit assertion is the only place it can be
checked at all.

## Two things a compile would not have caught

1. **`--no-default-features`.** The change moves `#[cfg]` boundaries, and the
   feature-off build is precisely what the moved code guards. Both
   `cargo check -p ferric-cli --no-default-features` and the matching clippy
   run are clean — a default-features-only run would never have compiled the
   branches being changed.
2. **Live `--mock` drives of all three rewired entry points.** Compiling proves
   the types line up, not that the wiring still reaches a provider.
   - `ferric chat --mock` → `[mock chat] I hear you (talk mode — no action taken).`
   - `ferric icm run --mock --auto` → all three stages `task_complete`,
     `ICM pipeline finished.`
   - `ferric mcp --mock` over JSON-RPC → `initialize` answered, then
     `tools/call` returned `mock run complete` with `isError: false`.

## Not claimed

The `Real` branch against an actual model. It is the same two calls it was
before, relocated; this sprint changes no request, prompt, or policy, and
sprint 101 already exercised the live path. Calling a move "live-verified"
would be the kind of overstatement this project keeps correcting.
