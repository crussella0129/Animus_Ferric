# Sprint 83 — Test Report

## Gate

| Check | Result |
|---|---|
| `cargo test --workspace` | **478 passed / 0 failed** (baseline 463, +15) |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `cargo fmt --all --check` | clean |

## New coverage, by defect

| Defect | Tests | What they pin |
|---|---|---|
| A3 | 4 (ferric-vcs) | staged index preserved; untracked still captured in the tree; ancestor repo refused *and* left unmodified; no temp index left behind |
| A1 | 4 (ferric-loop) | large output reaches the model truncated; the model is TOLD it was truncated; the trace keeps the full text; small output untouched |
| A2 | 4 (ferric-guard) | the old shape fails BOTH ways; a lifted sentence is tainted; sub-12-char fragments don't become needles; end-to-end Deny-at-Write / Allow-at-Read |
| A6 | 3 (ferric-tools) | short query matches its vault; short query does NOT match inside a longer word; longer terms still match as substrings |

Each of A1/A3/A6 had a sprint-82 probe that failed on `main`; the permanent
tests are those probes, kept.

## Two fixes that would have been wrong

Worth recording, because in both cases the obvious fix compiles and passes a
naive test:

1. **A3.** Sprint 82 recommended `git read-tree HEAD`, following the shipped
   source comment. Measured in a scratch repo, it destroys the staged set
   identically to `git reset` — both reset the index to HEAD. Only a separate
   `GIT_INDEX_FILE` works.
2. **A2.** Correcting `taint_str(&d.source)` to `taint_str(&d.summary)` is the
   one-line fix the audit named, and it would almost never fire: `is_tainted`
   needs the needle *inside* the argument, so a whole-summary needle misses the
   model lifting one injected sentence. Granularity was the real fix.

## One defect found by testing something adjacent

`snapshot_leaves_no_temp_index_behind` used a session id with a `:` and a space
— chosen only to exercise filename sanitization. It failed on `update-ref`,
which exposed that session ids also flow unsanitized into the git *ref* name.
Following that led to the ancestor-repo containment bug (git discovery walks
upward; on this machine `~` is a repo, so a non-git workspace resolved to the
entire home directory). **That bug was not in the sprint-82 audit at all** and is
more serious than the one this task set out to fix.

Empirical confirmation of its scope: adding the containment guard took
`cargo test -p ferric-cli` from wedging past 10 minutes to **1 second**.

## Deferred

A4, A5, A7, C1 and the Dark Matter `target` decision — see ADR-073 for why each
is deferred rather than rushed. A5/A7 in particular touch subsystems still
unreachable from the binary; doing them means wiring, not patching.
