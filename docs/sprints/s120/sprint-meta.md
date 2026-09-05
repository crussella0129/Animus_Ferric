# Sprint 120 Meta

- **Sprint number:** 120
- **Book schema version:** 2
- **Start timestamp:** 2026-09-05T02:20:09Z
- **End timestamp:** 2026-09-05T05:47:27Z
- **Model:** gpt-6-astra, ultra (user-selected; runtime identity not independently exposed)
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Human-first local sessions with safe automatic preparation and compact expert access
- **Intents:** [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md), [INT-0006](../../intents/INT-0006-truthful-policy-contract.md), [INT-0005](../../intents/INT-0005-safe-multilanguage-syntax-admission.md); [INT-0007](../../intents/INT-0007-hardware-calibrated-autonomous-development.md) reviewed dependency, not an acceptance claim.
- **Completion evidence:** Accepted Test at 0ec5a0e: all eight CI jobs, named clauses, fresh Cargo live/TTY and checked cleanup; Loop reconciled, extra post-Loop audit required before the sole PR.

## Approved resume

### Current closeout status

Test accepted exact source `0ec5a0eb0f465e8220b7f2010428aed3d6f2975d` after
the independent critic closed the real E04-D guidance gap. All eight CI jobs
in run `33947290181`, native Windows gates, fresh live-model acceptance and
actual Cargo terminal interaction passed with source-owned cleanup. Accepted
Test evidence was committed at `dc9c900`; the clean-Book router reported Loop.
[Loop reconciliation](loop-review.md) retains partial active intents and open
work. Closure, the extra post-Loop audit and actual PR checkpoint are recorded
separately; the older phase paragraphs below preserve their historical boundary.

### Build exit

All six approved tasks now have implementation and focused/native verification
evidence. The final coherent Windows workspace passed 1,245 tests with seven
intentional ignores; the separately required lifecycle suite passed 5/5.
Formatting and warnings-denied workspace/lifecycle clippy passed. Failed fixture
attempts and their bounded-reader/writer corrections remain in unit evidence.
The implementation-head CI push and exact-head live/terminal runs are next;
Test acceptance, Loop, extra post-Loop audit and the sole PR are still pending.

### Build implementation dependencies

T-12003's workspace concurrency contract requires a persistent root
`.ferric-startup.lock`, protected by the model write denylist and ignored by Git.
Putting the only lock inside replaceable `.ferric` could admit two starts after
a directory replacement. Retained directory capabilities protect preference
and trace publication; human traces use the existing canonical `.ferric/trace`
directory, not a second plural spelling. These are implementation dependencies
of locked E03, not new user authority or additional workflow ledgers.

Independent Build review found and closed stale-single-model selection, missing
resource copy, implicit model-directory rebinding and late version-probe
success. T-12004 additionally requires a retained-file JsonlSink constructor,
direct nonredirecting prepared-endpoint generation, and a `conservative` tier
provenance label distinct from an operator override. These preserve the approved
truthfulness/ownership contract; the locked plans are unchanged. Aggregate Test,
live acceptance, Loop and the extra post-Loop audit remain outstanding.

Canonical critique is clean. `finalize-plan.sh` locked both plans atomically
after commit `6bcb15f`; the router reported Build. Tasks are queued in dependency
order. No acceptance criterion was weakened during proposal promotion.

The owner reviewed the proposal and explicitly approved proceeding. The canonical
Build/Test plans are promoted from that reviewed proposal, with only schema
headings, intent links and dependency ordering normalized. The source-freeze
boundary remains in force until independent canonical critique and atomic lock.
The previously unavailable Claude Plan Mode tools are not claimed to have run.

For this approved Build continuation, the same sole unrelated Sprint 114 edit
was hash-verified and preserved in file-specific stash
`47361e44d78506f1975a8bcca89746eb597f765c`. Restore this latest stash and verify
the original SHA-256 at handoff; older stash entries remain recovery copies.

## Intake and preservation

PR 107 was verified merged at `2026-09-05T02:10:13Z`, merge commit
`17fc166bc8143ef85f3f3859f6a156902e0a68dd`. Main and dev were synchronized to
that baseline before initializing Sprint 120. The baseline also includes the
owner-merged RustPython dependency PR 106; its compile failure is R01, not a
Sprint 120 source regression. No feature branch or worktree was created.

The unchanged user edit at
`docs/sprints/s114/control-artifacts/model/acquisition-tests.json` is preserved
in file-specific stash `5821f481e133a45c0a41a77fbc7c575df62f2ce3`.
Original SHA-256:
`8ecf94878e7ad745aea28a9365af58ee111c80b26d21a15a0f434edb2beb75db`.
Restore that exact edit and verify the hash before handoff; never stage it into
this sprint. Retain the stash as recovery evidence.

The requested 0.21.0 skill path is absent; installed 0.22.0 is used and was
disclosed. Its Claude-specific EnterPlanMode/ExitPlanMode APIs are unavailable
on this Codex host. Source remains unchanged during Research and proposal
review; no plan approval or Build completion is inferred from tool absence.

## Research exit and Plan checkpoint

Research/intent records were committed at `9368bff`. The installed budget helper
reported `files=51 sources=5` (file cap exceeded); the report's explicit Budget
Override justifies the owner-requested cross-crate review. The clean Book check
passed and the router reported `plan`. No source implementation was changed.

[Plan scratch](sprint-plans/plan-proposal.md) contains the proposed Build/Test
plans and exact affected acceptance boundaries. Its
[independent preliminary critique](sprint-plans/proposal-critique.md) is clean
after correcting an overbroad cancellation acceptance implication. The proposed
provider task selects R04/R16 from the initial review follow-up list; other
unselected findings remain durable backlog. Canonical build/test plans remain
empty and unlocked; this preliminary review does not substitute for their
required post-approval critic and lock.

The installed skill's explicit plan-approval gate pauses this turn. Approval
has not been inferred. Research is complete, Plan is awaiting owner approval,
and Build, Test, Loop, the extra post-Loop audit and the sprint PR have not
occurred. Sprint status remains in-progress. The user's one-sprint/one-PR rule
is preserved: no second sprint or premature PR is created.

## Plan-approval handoff

Reviewed proposal commit: `d5125cd`. At that checkpoint the Book was fully
committed, the router reported Plan, and a diff of implementation/dependency/
tool/CI paths against the merged baseline was empty. No source regression fix,
model load, application test, or completed sprint is claimed.

The preserved Sprint 114 edit was then restored from the exact stash above and
SHA-256 verified against the original value. It is intentionally the sole
user-owned working-tree modification after this metadata checkpoint is saved.
The stash remains available for recovery. A subsequent clean-Book gate must
preserve this edit explicitly again rather than absorb or overwrite it.
These research/proposal commits are local on dev; no Sprint 120 push or PR has
been performed at this approval boundary.
