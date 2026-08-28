# Plan Critique — Sprint 115 (Initial)

## Concerns

### C-001: Resume-command safety has no defined shell contract
- **Where:** `build-plan.md` T-11414 / E14-F; `test-plan.md` Query CLI integration
- **Quote:** “the instruction **SHALL** preserve both paths with copy/paste-safe host quoting.”
- **Failure mode:** EARS-vague
- **Why it matters:** A host OS does not identify the consuming shell; PowerShell, `cmd.exe`, Bash, and zsh have different quoting rules. The proposed test only called the output a safe host command and supplied no round-trip oracle.
- **Response:** fixed — PowerShell on Windows and POSIX `sh` on Unix are named; exact argv round-trip tests cover spaces, quotes, and metacharacters.

### C-002: Post-creation safety revalidation is under-tested
- **Where:** `build-plan.md` T-11414 / E14-D; `test-plan.md` post-create coverage
- **Quote:** “repeat canonical, reparse, type, and bidirectional disjointness checks.”
- **Failure mode:** plan-test-mismatch
- **Why it matters:** A single positive test did not prove each substituted type/link/overlap state fails before allocation.
- **Response:** fixed — a test-only substitution seam and named unit matrix cover non-directory, equality, ancestor, descendant, symlink, and Windows-reparse states.

### C-003: Supported macOS path semantics disappear from parity coverage
- **Where:** `build-plan.md` T-11414 / E14-C; `test-plan.md`; INT-0008 AC-8
- **Quote:** “platform case semantics” with a Windows-only case-alias test.
- **Failure mode:** missing-risk
- **Why it matters:** The plan partially claimed full-platform AC-8 without macOS filesystem evidence.
- **Response:** fixed — AC-8 is explicitly not claimed; the broader cross-platform workflow remains open.

### C-004: Teardown promises cleanup neither declared nor verified
- **Where:** `build-plan.md` T-11412 / E12-C; `test-plan.md` cold teardown
- **Quote:** “no ... disposable staging state.”
- **Failure mode:** hidden-dep
- **Why it matters:** Generated paths were neither enumerated in touches nor all asserted absent.
- **Response:** fixed — five exact disposable roots are named in task authority and teardown verification, while the model and retained evidence are named preservation targets.

### C-005: Destructive stale-tree preservation has no objective rule
- **Where:** `build-plan.md` T-11501 / former E15-B
- **Quote:** “inventory and preserve unique raw bytes.”
- **Failure mode:** EARS-vague
- **Why it matters:** “Unique” had no comparison set or archive/hash oracle.
- **Response:** fixed — recursive deletion was removed from the plan. Every entry is manifested and the whole exact root is moved to retained quarantine with pre/post parity.

### C-006: T-11501 combines unrelated failure and mutation boundaries
- **Where:** `build-plan.md` former T-11501
- **Quote:** release gates, stale cleanup, cold inventory, and managed runtime in one task.
- **Failure mode:** granularity
- **Why it matters:** Independent failure classifications and safe stop points were collapsed.
- **Response:** fixed — T-11501 release, T-11502 harness/sandbox, and T-11503 runtime/handoff are ordered tasks.

### C-007: INT-0008 AC-2 is narrowed into a different criterion
- **Where:** plan traceability versus INT-0008 AC-2
- **Quote:** “AC-2 (one truthful resume command).”
- **Failure mode:** intent-drift
- **Why it matters:** A low-level resume flag is not the high-level run/checkpoint workflow.
- **Response:** fixed — AC-2 remains open and T-11414 is labeled enabling evidence only.

### C-008: Book provenance is incomplete before plan lock
- **Where:** `sprint-meta.md`; INT-0007 Work evidence
- **Quote:** placeholder Summary/Intents and only the Sprint 114 plan link.
- **Failure mode:** intent-drift
- **Why it matters:** The current plan was absent from the authoritative Book graph.
- **Response:** fixed — Sprint 115 metadata names the summary/intents and INT-0007 links the continuation plan.

## Confidence
block
