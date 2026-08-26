# Sprint 83 Meta

- **Sprint number:** 83
- **Start timestamp:** 2026-07-24T20:09:35Z
- **End timestamp:** 2026-07-24T21:35:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Remediate the four demonstrated defects from the sprint-82 audit
  and clear its verified vestigial list.

## Outcome

463 → **478 tests, 0 failures**; clippy 0 warnings; fmt clean. Five commits, one
per item: A3, A1, A2, A6, vestigial cleanup.

## Notable

Three of the five fixes required a *different* change than the audit proposed,
and checking that is what this sprint was actually for:

- **A3's recommended fix was wrong.** `git read-tree HEAD` destroys the staged
  index identically to `git reset`. A private `GIT_INDEX_FILE` is the fix.
- **A2's one-line fix would never have fired.** `is_tainted` needs the needle
  inside the argument, so tainting the whole summary misses a lifted sentence.
  Granularity was the real problem.
- **A6's length filter existed for a reason** — substring scoring makes `"go"`
  match `"algorithm"`. Removing it alone would have traded one bug for another.

**A defect the audit missed, found by testing something adjacent to a known
one.** A cleanup test used an awkward session id, which exposed unsanitized ref
names, which led to: git discovery walks upward, so a non-repo workspace resolves
to the nearest ancestor repo — and on this machine `~` is a repo, so the per-turn
`git add -A` targeted the user's entire home directory. Guarded now; it took
`cargo test -p ferric-cli` from wedging past 10 minutes to 1 second.

## Deferred, with reasons

A4, A5, A7, C1, and the Dark Matter `target` contract decision. A5 and A7 touch
subsystems still unreachable from the binary — doing them properly means wiring,
not patching.
