# Development Screen and Candidate Verdict

## Frozen coordinate

Sprint 113 screened only H01, H04, and H08, recovery variant, one Evidence
trial each, using constrained JSON grammar against the pinned
Qwen2.5-Coder-7B-Instruct Q4_K_M artifact. Every scoreable run used context
8192, temperature 0, seed 42, one server slot, CPU-only launch, the frozen v1
corpus SHA-256
`bb0ce1ec3f12a917096690e5a286232bfa05394c3c3d22d0589cb25542446323`,
and model SHA-256
`509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c`.

Selection required at least one task to pass both objective and authoritative
contract, three complete infrastructure-clean rows, verified traces, no unsafe
or admitted mechanism violation, and no more than the control's one unnecessary
clarification. Only a 0/3 screen could consume a general, trace-justified
revision, with at most two revisions.

## Complete lineage

Screen 001 is retained as an excluded preflight: H04 reached the task-wide
deadline before a result row was persisted, so the run had only two of three
rows and was not scored. Commit `d3c7016` made a validated timed-out trace
persist as a scoreable model result, after which the fixed screen was rerun from
the beginning.

| Screen | Candidate | Binary SHA-256 | Objective + contract | Clarification | Verdict |
| --- | --- | --- | ---: | ---: | --- |
| 002 | unchanged evidence mechanism | `f0b3d39dffa5ee8baa366f484a81412aa2d5f31e16280d855a0df28127a9ffdb` | 0/3 | 1 unnecessary | retain; revision allowed |
| 003 | revision 1, commit `90bc6a2` | `0565844cb61e83683f5df08e9345730fde1ea47676badbdfadb5406c8d5b380a` | 0/3 | 0 | retain; final revision allowed |
| 004 | revision 2, commit `10448ae` | `f98c4875bc272b8c17b26e3dda1f5d414ae3e23e03514319dda06a2801708f53` | 0/3 | 0 | falsified; budget exhausted |

Revision 1 was derived only from screen-002 traces: it improved evidence
recovery after a blocked action and used cause-specific controller feedback.
Revision 2 was derived only from screen-003 traces: controller-only refusals no
longer advanced or reset the real execution-failure streak, block counts became
durable typed checkpoint state, malformed checkpoint arithmetic failed closed,
and edit/evidence guidance became more explicit. Each revision had a distinct
committed source change and release-binary hash.

## Final screen 004

- **Run:** `autonomy-1787781412661-27096-0`
- **Rows:** 3 expected, 3 observed, 3 scoreable
- **Infrastructure failures:** 0
- **Objective + contract:** 0/3
- **Clarifications:** 0 observed, 0 unnecessary
- **Results SHA-256:** `094e21fa2a43c17e40df03a96877f7bf77db95644cade24dce80f0b05310e94b`
- **Summary SHA-256:** `2f6d6fb1d6e117b335ee9f693de4f5389f86884ae97343a8f58d6c676c5d285d`

| Task | Terminal | Wall time | Objective | Contract | Trace SHA-256 |
| --- | --- | ---: | --- | --- | --- |
| H01 | `repetition_guard` | 350,072 ms | fail | fail | `bd43d9e54d32d207b85d0a7142ea50035c4a87174f0a615f67f1b8b1630db023` |
| H04 | `oscillation` | 410,013 ms | fail | fail | `75923d06aa6410841aa650afc5bdfeb51870160e158c06145d8a71a453b6a4af` |
| H08 | `oscillation` | 246,196 ms | fail | fail | `b35516212f57e7e0490be2072ef0ba5ed8f632f82bb17c7707c81588f34a65ec` |

Independent verification used the frozen, read-only screen artifact. H01 had
170 records, 15 turns, and 14 calls; H04 had 174 records, 15 turns, and 14
calls; H08 had 164 records, 14 turns, and 13 calls. All three side-effect-free
`ferric trace verify` invocations exited zero and executed no tools.

The traces record 36 allowed permission checks, seven contained workspace
effects, 12 enforced controller blocks (2 repair-inspection, 1 no-effect, and 9
repeated-check), and five blocked H01 completion attempts. No block was bypassed
and no trace inconsistency was found. H01 and H04 nevertheless mutated one
named file before inspecting every named task file, contrary to Evidence
guidance v2. Forty-four nonfatal VCS snapshot notes reported that the temporary
seed workspaces were not Git repositories; the harness still retained all
effects and classified every row as infrastructure-clean.

## Decision

No candidate reached the minimum 1/3 objective-and-contract threshold. The two
permitted trace-justified revisions are exhausted. T-11307 therefore falsifies
the Evidence candidate; there is no selected candidate hash. Paired
confirmation and held-task evaluation must not run.

The tracked structured evidence is under
[`../control-artifacts/evidence-screens/`](../control-artifacts/evidence-screens/).

