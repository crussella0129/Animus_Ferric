# Sprint 84 — Research Report

## 1. Goal

Clear everything sprint 83 deferred: A4, A5, A7, C1–C5, and the Dark Matter
contract divergence.

## 2. Method

Same discipline as s83: for each item, check whether the audit's *proposed
remedy* is the correct one before applying it, and write the test that proves the
defect first.

## 3. Findings

### A4 is bigger than `manage_task`

The audit scoped A4 to `manage_task`. Writing its test found the identical
`block_in_place(|| Handle::current().block_on(..))` pair in **`shell_exec`** — a
Ring-0 tool reachable from far more paths. Before changing it, checked the blast
radius: every in-process tool path builds `Runtime::new()` (multi-thread), and
`cron` only *looks* current-thread because it spawns jobs as subprocesses. So the
fix converts panics to errors without changing anything that previously worked.

### A defect in no report: colliding task ids

Two tests in `background_tasks.rs` began flaking against each other. The first
reading was "cross-test interference through the process-global registry" —
plausible, and wrong. The real cause: `format!("task-{millis}")` is not unique,
so two tasks started in the same millisecond get the same id, and the registry is
keyed by id — the second **silently evicts the first**, losing its `Child`
handle. The task becomes unlistable, uninspectable, unkillable.

**Writing the flake off as test noise was the mistake.** The flake was the bug
reporting itself.

### A5 cannot be tested live, so test what matters

Docker is absent on this machine, so the container path cannot run. Rather than
ship a security default untested, the argv construction — which is the
security-relevant part, since it decides what flags actually reach `docker` — is
split into a pure `docker_args()` and tested directly.

### The Dark Matter question is two questions, not one

The *call* shape (DM requires `target`, Ferric required `query`) is an outright
incompatibility: a call from DM's own docs is rejected. That is a bug with one
correct answer, and it is fixed.

The *return* shape (DM's `{chunks,truncated}` envelope vs Ferric's markdown) is
not. Flipping it changes what every small model sees and would invalidate
ADR-071's measured 97.5% prompt reduction. That is a decision wanting a
measurement, so it is left open and stated rather than resolved quietly.

### DM's verifier needed testing more than writing

The new schema check lied twice before it worked, and only a negative control
caught it:
- grepping `"required": ["query"]` false-positives on the legitimate `anyOf` branch;
- grepping the whole file false-negatives, because a *test* mentions the field name.

A check nobody has tried to break is not evidence.

## 4. Conclusion

Same shape as s82/s83: the defects were invisible to a green suite, and the two
found this sprint were both found by testing something *adjacent* to a known bug
rather than the bug itself.
