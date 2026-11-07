# INT-0017 — Minimal code

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0017
- **State:** active
- **Work evidence:** [Sprint 129 build plan](../sprints/s129/sprint-plans/build-plan.md); [T-12901–T-12905](../work/tasks.md#minimal-code-int-0017)
- **Completion evidence:** [T-12901–T-12905](../work/completed-tasks.md#t-12901-sprint-129) (AC-1 to AC-5); AC-6 open items are T-12906 to T-12908
- **Code evidence:** the workspace, folded from 20 crates to 14
- **Test evidence:** [Sprint 129 test report](../sprints/s129/sprint-tests/test-report.md)
- **Documentation evidence:** [Sprint 129 research report](../sprints/s129/sprint-research/research-report.md)

## Intent

Ferric should be as small as its behaviour allows. Each responsibility should
have one implementation, with no dead code, no compatibility shims nobody
needs and no test that only repeats another. It should stay professional
and readable while it shrinks.

The owner directed this on 2026-11-07. After sprints 126 to 128 the code
looked like disjointed, overlapping pieces: "Ferric core, the valve, etc."
The owner asked for a redundancy-analysis swarm over the entire codebase, a
plan to refactor it to absolute minimality, and the refactor itself.

## Acceptance criteria

1. The whole workspace is analysed by parallel read-only analysts. Every
   finding is backed by file:line evidence and a whole-workspace search, and
   each one is classified as one of: duplicate, dead code, overlapping
   module or crate, over-abstraction, redundant test, or stale scaffolding.
2. One plan separates the work into three kinds:
   - behaviour-preserving consolidations, which are carried out;
   - defects found on the way, which are fixed with regression tests;
   - retirements of user-visible surfaces, which are listed with their cost
     and are **not** carried out without the owner's approval (INT-0010:
     retiring a surface needs its own approval).
3. Each consolidation passes fmt, workspace clippy with `-D warnings`
   (default features, no default features, and the Windows target), and the
   workspace tests. The test inventory matches the sprint-start baseline
   except for removals the plan names, each with the test that still
   covers it.
4. A crate with a single consumer is folded into that consumer unless a
   boundary test or an active intent keeps it separate. ferric-iron's
   dependency boundary (INT-0011) stays intact.
5. Lines of Rust and the crate count are measured before and after and
   reported per area.
6. Whatever the sprint does not finish is recorded as tasks under this
   intent, so the plan stays the single source.

## Rationale

Every duplicate is a place where a fix lands once and the bug stays in the
copy. Two of this sprint's defects were exactly that:

- One copy of the SSE decoder handled the in-band error event and the other
  did not.
- One scaffolder refused a symlinked target and the other did not.

Fewer crates, paths and tests also make the Iron cheaper to read, review and
qualify, which is Ferric's job under INT-0010.

## Alternatives

- **Leave the code and only stop adding to it.** The duplicates already
  disagree, and they keep costing every reviewer.
- **One large rewrite.** Rejected. Behaviour and evidence have to survive,
  so the work is a sequence of verified, behaviour-preserving steps, one area
  at a time.
- **Retire the large standalone surfaces now.** Tailscale Serve,
  `server adopt` with schema-1 records, and the paired autonomy runner are
  the biggest single savings. Retiring them changes what users can do, so
  the plan presents them for the owner's decision instead.

## Consequences

- Six crates become modules:
  - `ferric_loop::vcs`;
  - in ferric-cli: `prompt`, `cron::schedule`, `skills`, `icm::workspace`
    and `launch::scaffold`.
  Their former integration tests run as unit tests.
- Code and docs that named a removed crate or re-export path now name the
  new one.
- Retirement decisions recorded by the owner become their own intents or
  amendments.
