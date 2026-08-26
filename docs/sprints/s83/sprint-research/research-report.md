# Sprint 83 — Research Report

## 1. Goal

Remediate the defects sprint 82 demonstrated, in the order its report set, and
clear the vestigial list it verified.

## 2. Primary input

`docs/verification-2026-07.md` (ADR-072). Unusually for a research phase, no new
investigation was needed to *find* the work: sprint 82 left seven defects with
file:line citations, four of them with failing tests. The research question was
narrower and more useful — **for each fix, is the obvious change the correct
one?**

That question earned its keep twice.

## 3. Findings

### The recommended fix for A3 is wrong

Sprint 82 endorsed `git read-tree HEAD`, which is what the shipped source
comment proposed. Measured in a scratch repo:

```
BEFORE staged: 'staged.txt'   AFTER 'git read-tree HEAD' staged: ''
```

It destroys the staged set identically to `git reset` — both reset the index to
HEAD. The correct approach is to not touch the real index at all, via a separate
`GIT_INDEX_FILE`. Verified to preserve the index *and* still capture untracked
files in the snapshot tree.

### The one-line fix for A2 would never fire

The audit's fix was `taint_str(&d.summary)` in place of `taint_str(&d.source)`.
Reading `sink.rs` first: `is_tainted(value)` tests `value.contains(needle)`, so a
whole-summary needle only matches a wholesale copy. The realistic attack — the
model lifting one injected sentence into a `write_file` — would sail through a
"fixed" gate. Granularity, not the value, was the substantive problem.

### A6's length filter was there for a reason

Dropping tokens of ≤2 chars broke `"Go"`. But scoring is raw substring matching,
so simply removing the filter makes `"go"` match `"algorithm"`. The filter was a
crude fix for real noise; the fix has to preserve its intent.

### A defect the audit missed, found while testing an adjacent one

A test written only to check temp-index cleanup used a session id containing `:`
and a space. It failed at `update-ref` — session ids flow unsanitized into the
git ref name. Following that thread: **git discovery walks upward**, so a
workspace that is not itself a repo resolves to the nearest ancestor repo.
Measured on this machine, where `~` is a git repo:

```
git rev-parse --show-toplevel  (from a temp dir)  ->  C:/Users/charl
```

So the per-turn `git add -A` targeted the user's entire home directory, and
`revert` would have run `git clean -fd` across it. This is more serious than the
index destruction it was found beside, and it was not in the sprint-82 report.

## 4. Conclusion

Three of the five items in scope required a different fix than the audit
proposed, and the fourth surfaced a larger bug. The pattern is the same one
sprint 82 identified from the other side: **a change that satisfies the
description of a defect is not the same as a change that removes it.** Each fix
here is anchored to the test that failed before it.

Scope for the build phase: A3, A1, A2, A6, and the vestigial list. A4, A5, A7,
C1 and the Dark Matter contract decision are deferred with reasons recorded in
ADR-073.
