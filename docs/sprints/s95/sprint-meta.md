# Sprint 95 Meta

- **Sprint number:** 95
- **Start timestamp:** 2026-07-25T16:45:00Z
- **End timestamp:** 2026-07-25T18:05:00Z
- **Model:** claude-opus-5
- **Exit status:** success
- **Token count:** not observable
- **Summary:** Refresh the fleet capability map (untouched since sprints 25–26)
  and fix the two calibrator defects the run exposed.

## Outcome

**549 tests, 0 failures**; clippy 0; fmt clean.

| model | measured_level | tier |
|---|---|---|
| `qwen2.5-coder-3b` | **4** | Small |
| `qwen2.5-coder-7b` | **6** | Large |

Same family, so the difference isolates **size**. The 7B's 6 matches its
sprint-20 figure — a useful negative result: none of the guard family, truncation
fix, projector refactor or provenance gate moved the capability ceiling, which is
exactly what those ADRs claimed.

## Two defects, both the same shape

**H1** — a partial `--level` sweep silently **downgraded** a stored profile. The
7B's record went from 6/Large to 5/Medium because I ran one level to investigate
it, and `ferric query` reads that profile to size its RunPolicy. Profiles are now
written only from a full ladder.

**H2** — the first 7B sweep failed L5 and passed L6, scored 6, and looked
identical to a clean run. L5 then passed three times on repeat and a second full
sweep passed everything, so it was noise — but with one sample per level,
`max(passed)` means luck only ever inflates. Non-monotonic ladders are now
reported.

Both are *a value computed from partial information, reported as the whole
picture* — the recurring shape of this series.

## On not "fixing" H2 by changing the formula

Switching to "level before first failure" would have scored the 7B at 4, equal to
the 3B, contradicting the plain evidence that it completes L6. The problem is the
single sample and the silence, not the arithmetic — so the fix reports rather
than re-derives.

## Operational note

`cargo test --workspace` rebuilds `ferric.exe` **without**
`--features backend-openai`; a bench run started after one fails every level in
~200 ms with 0 turns. That is what the first attempt did.

## Next

Prebuilt gateway image (~15 s airlock startup), then C7/C8/B1.
