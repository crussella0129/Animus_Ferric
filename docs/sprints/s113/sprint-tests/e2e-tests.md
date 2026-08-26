# Sprint 113 End-to-End Test Results

- **Tested code head:** `dbaada383cd58415dfc775ec2c9d7e55a28bbcd0`
- **Executed/audited:** 2026-08-26
- **Result:** the planned no-candidate route passes; the performance hypothesis is falsified
- **Intent oracle:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md), acceptance criteria 1–8

## Real-model development screen

`real_model_evidence_screen` and
`screen_revision_budget_and_provenance_audit` are manual, artifact-bound test
labels—not Rust functions. The immutable H01/H04/H08 Evidence screen used the
pinned Qwen artifact SHA-256
`509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c`,
corpus SHA-256
`bb0ce1ec3f12a917096690e5a286232bfa05394c3c3d22d0589cb25542446323`,
context 8192, temperature 0, seed 42, one CPU server slot, and constrained
JSON. Screen 001 is retained but excluded as incomplete. Screens 002–004 are
each 3/3 complete and infrastructure-clean, each scored 0/3 objective plus
contract, and bind distinct unchanged/revision-1/revision-2 binaries.

The final screen is run `autonomy-1787781412661-27096-0`, candidate SHA-256
`f98c4875bc272b8c17b26e3dda1f5d414ae3e23e03514319dda06a2801708f53`,
results SHA-256
`094e21fa2a43c17e40df03a96877f7bf77db95644cade24dce80f0b05310e94b`,
and summary SHA-256
`2f6d6fb1d6e117b335ee9f693de4f5389f86884ae97343a8f58d6c676c5d285d`.
Its H01/H04/H08 trace hashes are respectively
`bd43d9e54d32d207b85d0a7142ea50035c4a87174f0a615f67f1b8b1630db023`,
`75923d06aa6410841aa650afc5bdfeb51870160e158c06145d8a71a453b6a4af`,
and
`b35516212f57e7e0490be2072ef0ba5ed8f632f82bb17c7707c81588f34a65ec`.
No screen qualified, both permitted revisions are exhausted, and no selected
candidate hash exists. This is a passing falsification verdict, not a passing
model-performance result.

## Confirmation and held routes

| Planned label | Result | Evidence and assertion |
| --- | --- | --- |
| `paired_qwen_confirmation` | conditionally skipped | No candidate satisfied the antecedent; running 18 rows would violate the locked gate. |
| `frozen_confirmation_binary_audit` | conditionally skipped | No candidate hash existed to freeze. Prospective freeze/drift tests passed in the CLI target. |
| `falsified_candidate_skip_audit` | manual PASS | [confirmation-skip.md](confirmation-skip.md) and the archive contain no paired row, pair ID, confirmation workspace, or confirmation trace. |
| `held_task_comparison` | conditionally skipped | Confirmation was skipped, so no H02/H03/H05/H06/H07 episode ran. |
| `held_promotion_verdict_audit` | manual PASS with caveat | No held row, workspace, outcome, or trace exists. A migration search surfaced one H03 prompt line, so the strict observer seal is technically contaminated; no candidate change or held-outcome claim used it. |

The paired-runner, schedule, freshness, eligibility, and promotion-math code is
covered model-free. The non-triggered confirmation/held branches are not
misreported as executed model episodes.

## Retained trace and provenance audit

`retained_trace_verification_audit` is a fresh manual execution at the tested
head. All 15 retained real-model traces—three frozen-control traces and twelve
screen traces—passed `ferric trace verify`; every invocation reported that no
tools were executed. SHA-256 was identical before and after each invocation.
All 14 persisted result rows bind to the archived trace bytes, child-binary
identity, model hash, and corpus hash; row counts agree with their summaries.
Screen 001's orphan H04 trace also verifies (232 records, 20 turns, 20 calls,
interrupted) despite correctly having no persisted result row.

The original shell-level autonomy command lines were not retained verbatim.
The structured artifacts retain the semantically material runner, binary,
model, corpus, policy, protocol, task, sampling, server, trace, timing, and
workspace identities, but this remains a reproducibility caveat rather than an
invented command transcript.

Raw JSON/JSONL evidence contains machine paths because those exact bytes and
their digests are the frozen provenance. Human-authored reports use template
values. Rewriting raw files would invalidate the trace/result hashes; they are
therefore retained under the immutable sprint-evidence exception documented in
`AGENTS.md` and `template_hygiene.rs`.

## Lifecycle, closeout, and product smoke

| Planned label | Result | Evidence and assertion |
| --- | --- | --- |
| `managed_server_teardown` | manual PASS | [held-and-teardown.md](held-and-teardown.md) records the evaluated PID, listener, local/global runfiles, matching model process, and health endpoint absent after shutdown. |
| `planner_decision_evidence_audit` | manual PASS | [planner-decision.md](../planner-decision.md) links the measured 0/3 screen, confirmation/held skips, and causal rejection; EvidencePlanner remains fail-closed. |
| `book_close_evidence_audit` | manual PASS through Test; Loop pending | T-11301's commit evidence, final critique/report, and intent Test links agree. Closing sprint metadata and PR-bound CI are phase-ordered Loop evidence. |

A fresh post-Test read-only check also found evaluated PID 48468 absent, no
listener on port 8080, neither local nor global runfile present, and `ferric
server status` returning the expected nonzero “no server registered” result.

The supplemental offline `tools/demo-smoke.ps1` rebuilt the backend-enabled
release and passed all eight checks: version, guarded mock query artifact,
side-effect-free trace inspection, pre-inference secret denial, skills, Book-v2
Launch scaffold, three-stage ICM, and one-shot persisted cron execution. This
is end-to-end product/Launch evidence; it is not substituted for the real-model
result above.
