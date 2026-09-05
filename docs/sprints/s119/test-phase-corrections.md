# Sprint 119 Test-phase corrections

These are corrections within locked E01/E04/E05/E07, not a replacement plan or
permission to weaken acceptance. The first committed head `712e3cc` remains a
failed acceptance attempt even though most individual gates passed.

## Windows membership during cleanup

Retained native process handles fixed the observed gap between Job accounting
and object signalling for a stable member set. A subsequent review found that
members could still create a descendant after that set was captured.

Microsoft documents `TotalProcesses` as the lifetime association count,
including terminated processes and attempted associations rejected by limits:
[basic Job accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information).
It documents Job termination as covering associated processes and nested Jobs:
[Job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
The implemented correction uses the count as a conservative snapshot fence;
a changed count cannot validate the earlier handle set. This is an inference
from the documented accounting contract, requiring a deterministic late-child
source regression and native execution before acceptance. The focused Windows
test `windows_cleanup_rejects_post_snapshot_admission` passed: it creates a real
late descendant after the inner snapshot, observes the inner owner's exit 125,
and uses an independent stable outer Job snapshot to prove exact owner, leader,
and descendant termination. Full shared tests then passed 7/7 with one source
fixture ignored. This does not claim retained identities for every previously
exited historical object; zero active scope membership and the retained live
objects' signals are the supported success boundary.

## Linux positive managed lifetime

Keeping the `server up` launcher's process-group identity unreaped protects the
numeric group anchor, but an exited launcher becomes a zombie. Its fd directory
is unreadable, so production's complete-peer ownership classifier correctly
refuses authority. Three positive lifecycle tests failed in native CI.

The source fixture needs a surviving supervisor as the retained group anchor:
it runs and reaps the real CLI launcher, publishes its actual exit status, then
stays alive under the fixture lifetime guard while the managed server runs.
Production must continue rejecting incomplete listener-owner visibility. The
implemented source topology must pass the full isolated Linux lifecycle suite without
manual reaping or changes to the production authority classifier.

## Local correction gate

After both corrections, Windows warnings-denied workspace/lifecycle-feature
clippy and fmt passed. `cargo test -p ferric-process -p ferric-bench -p
ferric-cli --locked --offline --quiet` passed **474 tests / 4 intentional source
fixture ignores** with explicit real Python grading. The separate native
Windows lifecycle suite passed **5/5 in 20.28s**. These are working-tree
correction checks; the next committed head must pass native Linux CI too.

## Deadline ordering correction

The independent E01 review found that an already-drained final observation
could be accepted before checking whether member retention consumed the cleanup
deadline. `cleanup_complete` now rejects an observation at or after the
deadline before it can certify success; native termination is still attempted
first. `windows_cleanup_deadline_precedes_success` deterministically checks
drained and non-drained states before, exactly at, and after the deadline.
Windows shared fmt/clippy and tests passed **8/8, one source fixture ignored**.
This final Windows-only correction must receive its own confirmed CI head.
