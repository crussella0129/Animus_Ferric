# Sprint 103 test plan — Finalized - DO NOT EDIT

This is a behaviour-neutral refactor, so the primary evidence is that **574
existing tests stay green** — they are the specification of the behaviour being
preserved. New tests only where the change creates a claim nothing checked.

## The gap this closes

The feature-off diagnostic is what a user meets when the binary lacks
`backend-openai` — the exact condition that produced sprint 101's false
positive. It existed in three byte-identical copies and **nothing tested any of
them**. Three copies with no test is three places to drift silently.

1. `the_backend_feature_diagnostic_names_the_feature_and_the_alternative` —
   assert `BACKEND_FEATURE_MISSING` names both the cargo feature to rebuild
   with and `--mock`. It is a user-facing string in an error path that cannot
   be reached in a normally-built binary, so a unit assertion is the only
   place it can be checked at all.

## Regression surface

- `cargo test --workspace` — 574 green, no count change expected from the
  refactor itself (+1 from the above).
- `cargo clippy --workspace --all-targets` at 0, `cargo fmt --check` clean.
- **`cargo check -p ferric-cli --no-default-features`** — the refactor moves
  `#[cfg]` boundaries, and the feature-off build is precisely what the moved
  code guards. A default-features-only run would not compile the branches being
  changed.

## Live smoke, because the touched paths are entry points

`chat`, `icm` and `mcp` all had their construction rewritten. Each gets driven
in `--mock` mode end to end; a compile is not evidence that the wiring still
reaches a provider.

## Not tested, stated plainly

The `Real` branch of the new builder against an actual model. It is the same
two calls it was before, moved; sprint 101 already exercised the live path, and
this sprint changes no request, prompt, or policy. Claiming live coverage for a
move would be the overstatement this project keeps correcting.
