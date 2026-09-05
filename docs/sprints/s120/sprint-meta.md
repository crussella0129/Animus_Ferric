# Sprint 120 Meta

- **Sprint number:** 120
- **Book schema version:** 2
- **Start timestamp:** 2026-09-05T02:20:09Z
- **End timestamp:** (pending renewed Loop closure)
- **Model:** gpt-6-astra, ultra (user-selected; runtime identity not independently exposed)
- **Bundle version:** 0.22.0
- **Exit status:** in-progress
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Human-first local sessions with safe automatic preparation and compact expert access
- **Intents:** [INT-0008](../../intents/INT-0008-unified-local-model-workflow.md), [INT-0006](../../intents/INT-0006-truthful-policy-contract.md), [INT-0005](../../intents/INT-0005-safe-multilanguage-syntax-admission.md); [INT-0007](../../intents/INT-0007-hardware-calibrated-autonomous-development.md) reviewed dependency, not an acceptance claim.
- **Completion evidence:** Renewed Test accepted 4f4e4f0 with controlled-schedule caveat at ae18535; full exact-head CI/native/live/TTY passed; renewed Loop closure and separate post-Loop audit follow within PR 108.
- **Checkpoint:** https://github.com/crussella0129/Animus_Ferric/pull/108

## Approved resume

### Current closeout status

Renewed Test accepted source `4f4e4f04d4ee132f9df9bb422be88a5ce366915d` with
`proceed-with-caveats`: native workspace test scheduling is controlled, while
historical timeout cause and arbitrary parallel-suite robustness remain
unqualified. Both eight-job CI runs, all local canonical gates, fresh source-owned
live-model acceptance and actual Cargo terminal interaction passed. Test evidence
was committed at `ae18535`; the committed-Book router reports Loop. The critic's
post-evidence recheck preserved that verdict. Only Book files changed afterward.
Renewed Loop reconciliation retains active intents and T-12026/27 follow-ups.
PR 108 remains the sole checkpoint and must not be merged by the agent.

### Checkpoint reopening history

The final checkpoint `3b966dd` reproduced the existing PowerShell quoting
fixture's ten-second timeout in both original push and PR CI runs. Sprint 120
returns to Test within PR 108; no new sprint, branch or PR is introduced.
[Checkpoint diagnosis](sprint-tests/checkpoint-diagnosis.md) retains both
failures, the one authorized rerun and bounded local checks. The earlier close
timestamp was `2026-09-05T05:47:27Z`; its accepted-source report and extra audit
below remain historical. Fresh Test acceptance, Loop reconciliation/closure
and another extra post-Loop audit are required after diagnosis. The confidence
throttle was already adjusted once for this sprint and is not decremented again.

The same protected Sprint 114 edit was rechecked against its original SHA-256
and preserved again in exact file-specific stash
`58a0dff8d57a91aea48d234394db3ebebd94563c`. Restore this latest stash, verify
the original hash and leave it unstaged before final handoff; retain recovery
stashes. No unrelated edit enters the sprint commits.

### Prior accepted-source closure

Test accepted exact source `0ec5a0eb0f465e8220b7f2010428aed3d6f2975d` after
the independent critic closed the real E04-D guidance gap. All eight CI jobs
in run `33947290181`, native Windows gates, fresh live-model acceptance and
actual Cargo terminal interaction passed with source-owned cleanup. Accepted
Test evidence was committed at `dc9c900`; the clean-Book router reported Loop.
[Loop reconciliation](loop-review.md) retains partial active intents and open
work. Closure succeeded at `6d9e0d4`, followed by a separate clean
[extra post-Loop audit](post-loop-adversarial-review.md). The actual PR checkpoint
is recorded separately; the older phase paragraphs below preserve their
historical boundary.

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
