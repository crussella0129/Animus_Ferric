# Sprint 121 Meta

- **Sprint number:** 121
- **Book schema version:** 2
- **Start timestamp:** 2026-09-05T13:07:50Z
- **End timestamp:** 2026-09-06T02:03:54Z
- **Model:** unknown
- **Bundle version:** 0.22.0
- **Exit status:** success
- **Token count:** (filled at Loop Phase if observable)
- **Summary:** Add explicit attributable expert output/benchmark budgets without changing normal launch or promoting diagnostic calibration.
- **Intents:** INT-0007 AC-11/12 partial; INT-0008 AC-6/11/12 preservation.
- **Completion evidence:** Accepted Test/critique at 322fe20; corrected source a417c5d, eight-job CI 34004554100 and fresh 15.07s live pass; Loop reconciliation 889010a. Fresh extra audit and human-approve checkpoint follow.

## Intake and protected state

PR 108 was owner-merged at `ffab58de35c7dd341ae35f43bc06fb5794b52c59`.
The read-only remote intake found no open PRs; `dev` was fast-forwarded to
that baseline before this sprint's writes. The installed 0.22.0 substrate
check passed. No new branch, worktree, push or PR has occurred.

The sole unrelated Sprint 114 edit was preserved in path-specific stash
`2428203eee25ad36bf684a09bc4b2c151ef71765` for clean Book gates. Its original
SHA-256 is `8ecf94878e7ad745aea28a9365af58ee111c80b26d21a15a0f434edb2beb75db`.
Restore the exact edit and verify that hash before handoff, leave it unstaged,
and retain the recovery stash. Do not absorb it into this sprint.

Research selects the T-11505 explicit budget-control prerequisite under active
INT-0007/8. Model acquisition, automatic speed/fit calibration and the frozen
application trial remain separate work. No implementation or runtime
acceptance is claimed by source inspection.

Research ended at 13:27:17 UTC after 19 minutes 27 seconds. The installed
budget helper reported `files=31 sources=3` and its expected cap-exceeded exit;
the report contains the required cross-cutting Budget Override. Source,
dependency, tooling and CI paths remain unchanged against the merged baseline.

## Research exit and Plan checkpoint

Research was committed at `13c7ccb`; the clean-Book helper passed and the router
reported `plan`. The skill's Claude-specific Plan Mode tools are unavailable
on this host (tool discovery returned no matching APIs). No invocation or
approval is fabricated: implementation remains unchanged while the owner
reviews [scratch Build/Test plans](sprint-plans/plan-proposal.md).

Canonical build/test plans remain empty and unlocked. Explicit owner approval
must precede canonical plan writing, the required Plan critic and atomic
finalization. A preliminary proposal critique, if obtained, does not replace
those gates. Build, Test, Loop, the extra post-Loop audit and the sprint PR have
not occurred. No new sprint or remote checkpoint is authorized by this pause.

The independent [initial preliminary critique](sprint-plans/proposal-critique-initial.md)
returned `block` for trace/sidecar collision preservation, live-fixture
deadline composition and generated-resume context. The scratch proposal was
revised to name no-clobber pair tests, a source-supervised fixture with reserved
cleanup budgets, and cap/context round-trip plus changed-policy refusal.
Re-review is required; the initial verdict is retained, not overwritten.

The subsequent [independent preliminary re-review](sprint-plans/proposal-critique.md)
returned `clean` after inspecting those actual revisions. This accepts the
scratch proposal for owner review only. Canonical plans remain empty and
unlocked, confidence remains 0.2, and no Build/Test/Loop completion is implied.
The installed skill's explicit plan-approval gate pauses this turn. After
approval, use these reviewed scratch boundaries to write canonical plans,
attach Work evidence to active intents without no-op transitions, obtain the
canonical critic verdict, commit the Book and run the finalization helper.

## Plan-approval handoff

The reviewed proposal was committed at `b3412baf98232993d7ee0cee1e6e21bc570399e1`.
At that checkpoint `check-tracked.sh` reported a fully committed Book and
`current-phase.sh` reported `plan`. Source, dependencies, CI and tooling are
unchanged from the owner-merged baseline. No new runtime tests or model loads
were performed; no sprint push or PR was created.

The exact protected stash `2428203eee25ad36bf684a09bc4b2c151ef71765` was then
applied successfully and its original SHA-256 verified. The Sprint 114 JSON is
again the sole unrelated unstaged modification; recovery stashes remain.
Do not apply this stash again. Before a future clean-Book gate, preserve that
current edit explicitly rather than staging it into Sprint 121. The source-
unchanged proposal is awaiting owner approval; the sprint remains in-progress.

## Owner approval and canonical Plan

The owner explicitly approved the revised proposal with “Approved, continue”.
The canonical Build and Test plans now carry that approved scope unchanged;
the required independent canonical critique and helper lock precede all source
work. The unavailable Claude-specific Plan Mode APIs remain unavailable; no
invocation is fabricated and source remained unchanged through approval.

The restored unrelated Sprint 114 JSON was hash-checked again and preserved
in new path-specific stash `32e0fdfcb7c9e9fff78d5cd0b70e649e758e56d6`.
Restore this exact stash at handoff, verify the original SHA-256 above, leave
the edit unstaged, and retain the recovery stash. Do not reapply the older
already-restored stash. No source acceptance or remote checkpoint is implied.

The independent canonical Plan critic reviewed the actual canonical plans and
returned `clean`. Its exact verdict is retained in `sprint-plans/critique.md`;
the preliminary proposal review was not substituted for this gate.

## Build progress and verification interruptions

The helper finalized both plans and the router reported `build`; ordered
T-12101 through T-12104 were queued at `b72ae0f`. Both linked intents remain
active without a no-op transition. T-12101 source and finite fixtures are in
progress; no task is yet accepted or committed as complete.

The first all-target compile exposed an exhaustive trace-verifier match that
needed the additive main-request event; the mechanical handling was added.
The subsequent core/trace/loop test build failed before execution because the
drive had zero free bytes (`rustc-LLVM: no space on device`, Windows linker
PDB write errors). The separately started CLI fixture build also failed while
writing its incremental query cache. Neither command is a test pass.

After both Cargo commands exited, the exact ordinary, untracked
`target/debug/incremental` cache was checked for reparse points and removed.
Its measured size was 22,046,318,514 bytes; free space afterward was
19,474,415,616 bytes. This is regenerable compiler cache, not model, source,
retained trace or experiment evidence. Further local gates disable Cargo
incremental caching only; production policy, test assertions and CI schedule
are unchanged. A source compiler-cache failure is not a lifecycle cleanup
failure or an accepted run.

T-12101 subsequently passed its corrected coherent gates and was committed at
`8be0e9f` (`ddf0aed` ledger backfill). Its initial failures and verified Windows
HTTP-fixture correction remain in [Build verification](sprint-tests/build-verification.md).
T-12102 is in progress: the first integrated compile and fifteen source-owned
CLI tests passed, while library fault matrices and task-boundary gates remain
outstanding. T-12103/04, formal Test, Loop, the extra independent review and
the sole PR are still ahead. No confidence update or model verdict is claimed.

The owner additionally noted that WSL is installed on this computer. After
the current Windows qualification round, inspect the existing WSL distribution
and toolchain for local Linux verification. Availability alone is not a test
pass, and no distribution installation or ownership-boundary expansion is
inferred. The existing Unix nested-process-group limitation remains explicit.

T-12103 passed its coherent gates and was committed at `9e0f9c8`, with ledger
backfill `17691cf`. T-12104 has begun. Its first expert-help/human/source
documentation gate passed twelve tests (one budget-doc, eight human CLI, one
human-doc and two source-execution ratchets). The primary surface remains four
actions, with no new budget choices in normal launch or `run`.

Live fixture design review exposed the already documented Unix nested-group
boundary: forcibly stopping a test-body process cannot prove teardown of an
engine launched in a separate process group by that body. Windows nested Jobs
can own that scope. The live smoke is therefore Windows-qualified; an opt-in
Linux live attempt must report an explicit non-pass before resource effects.
This is not a model or Linux-live parity verdict. Deterministic Linux normal
setup/request cancellation fixtures still prove their own cleanup; outer
synchronous-stall fixtures deliberately run before engine creation or with a
pure provider and prove only their actual child scope. Independent read-only
review found this consistent with the locked local-live requirement and
existing T-11904 deferral. WSL does not remove that limitation by its presence.

The initial Windows composed workspace gate passed 1,294 tests with eleven
documented ignores; workspace formatting, included-fixture formatting and
warnings-denied Clippy passed. The separate first live smoke failed its setup
deadline before a request, with checked engine/process cleanup; raw evidence
and the attribution gap are retained in Build verification. This is not a
completed T-12104 or formal Test pass.

After that Windows round, read-only WSL inspection found an existing Ubuntu
WSL2 distribution, non-root user, Rust/Cargo 1.96.1, Python and the required
namespace tools. Passwordless `sudo -n` was unavailable (interactive
authentication required), so the canonical isolated Linux workspace/lifecycle
runner is not locally ready. No sudoers change, root test bypass, distribution
or toolchain installation was made. The unchanged Linux CI gate remains
required; local availability is not substituted for its result.

T-12104 completed its Build boundary after a fixture-only cancellation/timing
correction and an independently reviewed Cargo release-profile qualification.
All native lifecycle and focused hash/cancellation tests passed. The separate
live-build-002 smoke passed in 13.76 s with actual explicit-1024 provenance and
checked cleanup; failed live-build-001 remains unchanged. The optimized
fixture hash comparison does not imply faster external inference or reconstruct
the first attempt's missing stage timings. Full details and report/source
digests are in Build verification. Formal immutable-source Test/CI, independent
critic, Loop, extra fresh audit and the sole PR remain outstanding. Confidence
is still 0.2, and both intents remain active partial work.

## Formal Test checkpoint interruption

T-12104 committed at `688b939`, with backfill/immutable source head
`2856c63209865f69b3d3727f84fd92f63f9dfa51`. Clean Book routing entered Test;
that exact head was pushed and confirmed on `origin/dev`. No PR exists.
Root's fresh Windows workspace passed 1,299 tests/eleven documented ignores,
and the independently retained `live-test-001` smoke passed in 13.55 s.

However, initial exact-head CI run `34002834811` failed its Windows workspace
job in the existing first-run decision-budget journey. The other seven jobs
passed, including Linux's 1,305-test workspace. [Checkpoint evidence](sprint-tests/ci-checkpoint-001.md)
retains the exact failure and counts. The original native inspection error
was discarded; no cause or successful final matrix can be inferred. Test-only
diagnostics and one fixed bounded local series are the next diagnostic checks,
not a blind CI rerun or production native-ownership redesign. Formal Test,
Loop and the single PR remain blocked on qualification; confidence is unchanged.

The instrumented `4eded51` checkpoint passed all eight CI jobs, but this was
retained as non-reproduction, not a repair. Independent Test critique returned
`block` and found a separate concrete connection-reset defect in the human
source fixture. Its narrow correction now has four new regressions and an
executed failing mutation control with checked cleanup; production native
ownership and every existing deadline remain unchanged. Historical CI cause
remains unknown. Corrected immutable-source qualification and renewed critic
must precede the Test report and Loop. Existing Ubuntu WSL additionally passed
35 process-free core tests and core Clippy; local namespace runtime gates remain
unavailable without sudo authorization. Full details are retained under
`sprint-tests/`, including the original failure and diagnostic checkpoints.

Corrected immutable source `a417c5d00361fd25a238346e5015fb07ed5ae7c7` passed
root's 1,303-test Windows workspace, all eight canonical CI jobs and a fresh
15.07-second owned-model smoke. Independent Test re-review resolved C-001/002,
verified source/report/trace digests and accepted `proceed-with-caveats` for
the still-unknown historical CI cause. The accepted Test report is linked from
both active intents. This ends Test only: confidence, Loop reconciliation,
extra fresh audit and the sole PR checkpoint remain subsequent obligations.
