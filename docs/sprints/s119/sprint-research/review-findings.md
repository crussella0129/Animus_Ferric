# Independent research review findings

Reviewers: `process_review_research` and `lifecycle_review_research`.
Both inspected source only; neither launched executables or changed files.

Review dimensions (engineering code-review checklist): security needs changes
at the exact ownership/signal boundary; correctness needs changes for rollback
and reaping; performance needs changes for bounded pipe collection;
maintainability needs changes because duplicated implementations diverged.

1. P1: CLI containment's command-group Windows spawn can abandon a suspended
   child after a failed assignment/resume. Guard ownership immediately.
2. P1: command-group wait is not zero-active-process Job evidence. Query actual
   accounting within the shared bounded cleanup deadline.
3. P1: Linux PDEATHSIG SIGKILL can kill a nested group owner before its watcher
   runs. Do not overstate POSIX scope; test controlled cancellation/reaping and
   retain an outer Linux namespace boundary where needed.
4. P2: Copied PGID registry snapshots race normal removal/reuse. Serialize
   registry signal/absence/removal and prohibit new registration during shutdown.
5. P2: Linux grandchildren need a reaper. Merely seeing pidfd POLLIN can still
   mean zombie; scope adopted-child waits or prove POLLHUP before success.
6. P2: Exact-process regression accepts any positive poll event. Reject
   POLLNVAL/errors, handle EINTR against the deadline, and require exit events.
7. P2: server-registration helper waits before draining pipes and later
   collects unboundedly. Move it to the shared file-capture boundary while
   preserving its barrier and concurrency assertions.

Sound foundations worth retaining: file-backed bounded capture, retained
native process handles, atomic readiness publication, failure classification,
and source-defined recursive test helpers launched by Cargo's test harness.
