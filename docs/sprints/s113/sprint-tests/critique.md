# Test Critique — Sprint 113

## Concerns

### C-001: CI confirmation is PR-bound
- **Where:** `integration-tests.md` / final quality gates
- **Quote:** “CI is PR-bound and pending.”
- **Failure mode:** evidence-drift
- **Why it matters:** Local gates pass, but the report must not imply an authoritative CI success before the PR workflow runs.
- **Suggested response:** defer-with-rationale — record the tested head and passing local confirmations; identify CI as pending.
- **Response:** accepted. `integration-tests.md` separates local PASS from pending authoritative PR CI and names the required jobs, pushed head, and synthetic workflow SHA evidence.

### C-002: Original autonomy commands were not retained
- **Where:** `e2e-tests.md` / frozen development screen
- **Quote:** “Original autonomy shell commands are unavailable.”
- **Failure mode:** evidence-drift
- **Why it matters:** Exact command-level replay is weaker, although structured results, configuration, hashes, traces, and managed-server provenance preserve the semantic experiment.
- **Suggested response:** defer-with-rationale — retain the structured evidence and state the command-transcript limitation without reconstructing commands.
- **Response:** accepted. `e2e-tests.md` preserves the limitation and does not invent a transcript; all semantically material structured inputs, outputs, identities, and hashes remain linked.

### C-003: H03’s strict observer seal is qualified
- **Where:** `e2e-tests.md` / held-task audit
- **Quote:** “H03's strict observer seal is technically contaminated.”
- **Failure mode:** weak-assertion
- **Why it matters:** An absolute claim that every held prompt remained unseen would be false, despite no held outcome, trace, or tuning input influencing development.
- **Suggested response:** tighten-assertion — preserve the qualification and make no H03 generalization or promotion claim.
- **Response:** accepted. The held result is reported only as an operational no-episode/no-outcome skip with an explicit H03 observer caveat; no generalization or promotion claim is made.

### C-004: No fake-`PATH` interpreter canary
- **Where:** `unit-tests.md` / Python syntax-validation boundary
- **Quote:** “`legacy_python_warning_is_in_process_and_does_not_execute_sitecustomize`”
- **Failure mode:** negative-path
- **Why it matters:** A fake-`PATH` canary would independently detect interpreter resolution, but the in-process parser assertions and workspace side-effect canary already cover the required no-execution behavior.
- **Suggested response:** defer-with-rationale — record the redundant hardening opportunity without treating it as an acceptance gap.
- **Response:** accepted. `unit-tests.md` retains the exact limitation; the implemented RustPython path, bounded parser tests, no-temp test, and import-side-effect canary substantiate the current acceptance claim.

### C-005: Book closure remains phase-ordered
- **Where:** `e2e-tests.md` / `book_close_evidence_audit`; `sprint-meta.md`
- **Quote:** “`in-progress` with pending completion evidence.”
- **Failure mode:** evidence-drift
- **Why it matters:** The final report, intent evidence, completion ledger, and metadata must converge before closure.
- **Suggested response:** defer-with-rationale — complete the report and ledger first, then validate and close metadata through the Book helper.
- **Response:** accepted. T-11301's completion entry is reconciled; Test report and intent evidence are the next phase-ordered actions, while final sprint metadata and PR CI remain Loop evidence.

## Confidence

proceed-with-caveats
