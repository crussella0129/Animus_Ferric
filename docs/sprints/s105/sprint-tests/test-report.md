# Sprint 105 test report

**575 → 577 tests, 0 failures.** clippy 0, `cargo fmt --check` clean.

## The existing suite is the evidence for the fixture edits

Every fixture kept its shape and changed only its values, so all 575 prior
assertions had to keep passing — and they did. **That is the finding, not just
a pass:** if any had failed, it would have been asserting identity rather than
structure.

## New tests (2), in `crates/ferric-cli/tests/template_hygiene.rs`

| test | what it pins |
|---|---|
| `tracked_sources_carry_no_machine_identity` | walks `crates/`, `docs/`, `tools/`, `docker/`, `README.md` for shapes that can only be identity |
| `each_rule_rejects_identity_and_accepts_the_documentation_value` | each rule shown rejecting a real value **and** accepting the documentation one |

### The guard's self-test found a hole in the guard

The first version matched `\Users\name` in single-backslash form only. But the
likeliest place for a leaked path is **inside a Rust string literal**, where
`C:\Users\alice` is written `"C:\\Users\\alice"` and reaches the matcher
doubled — so it would have missed precisely the case it exists for. Backslashes
are collapsed first, and both forms are now asserted.

### The scan was shown failing

A planted line (`DNSName box.tail944782.ts.net at 192.168.86.27`) in
`ferric-research/src/lib.rs` made `tracked_sources_carry_no_machine_identity`
fail, naming **both** matching rules and the file:line. Reverted immediately. A
guard that has never rejected anything is not known to reject anything —
sprint 96's skip-and-pass and sprint 101's false positive are the precedent.

## Fresh-clone verification, because `git rm --cached` leaves files on disk

A local build proves nothing about what a template user receives. Cloned the
branch to a temp directory and worked from there:

1. **What ships:** `sprints/` absent, `scratch/` absent, `benchmarks/` contains
   `model_profiles.example.json` only. Correct.
2. **`cargo test --workspace` in the clone: 577 passed, 0 failed.** Nothing that
   was untracked was load-bearing.
3. **`ferric query --mock` with no `model_profiles.json`** — completed
   (`task_complete`, 2 turns, trace written). ADR-029's "a profile miss is a
   safe no-op" is now *exercised* rather than cited, which matters because this
   sprint makes it the default state for every new user.

### The behavioural change, measured

Same command, same model name, two trees:

| tree | `model_profiles.json` | tier selected |
|---|---|---|
| fresh clone | absent | **Nano** (from the params prior) |
| this machine | present, `measured_level: 6` | **Large** |

So before this sprint a template user running `--model qwen2.5-coder-7b`
inherited `Large` from a measurement taken here, at a quantization the record
never pinned. They now get the conservative params-derived default until they
run `ferric bench` themselves.

## Not covered

Whether git *history* still contains the identity removed from the tree. **It
does** — that is recorded in ADR-096 as a decision for the owner, not fixed
here, and no test should imply otherwise.
