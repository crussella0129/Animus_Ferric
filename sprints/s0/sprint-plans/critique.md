# Plan Critique — Sprint 0

> Critic: subagent (adversarial review per prompts/plan-critic.md), 2026-06-10.
> Primary-agent dispositions recorded inline under each concern as **Response:**.

## Concerns

### C-001: EARS criteria incompleteness for T-001
- **Where:** build-plan.md T-001 success criteria.
- **Quote:** "WHEN `cargo fmt --check` and `cargo clippy ...` run, THEN both SHALL exit 0."
- **Failure mode:** EARS-vague
- **Why it matters:** fmt and clippy were combined in one clause; a partial failure is ambiguous.
- **Suggested response:** fix-in-plan.
- **Response: FIXED-IN-PLAN.** T-001 now has three separate WHEN/THEN/SHALL clauses (build, fmt, clippy).

### C-002: measured_level override tested only in the downgrade direction
- **Where:** build-plan.md T-003 / test-plan.md T-003.
- **Failure mode:** plan-test-mismatch
- **Why it matters:** override logic could pass downgrade and fail upgrade.
- **Suggested response:** fix-in-plan.
- **Response: FIXED-IN-PLAN.** Added `measured_level_upgrade` (1B + measured L4 → SMALL-grade policy) to test-plan; T-003 notes now state the override is bidirectional.

### C-003: Hidden dependency T-008 → T-003 via max_tools
- **Where:** build-plan.md T-008 Depends-on.
- **Failure mode:** hidden-dep
- **Response: REJECT.** The critique is wrong because T-008's `Depends on:` line already explicitly lists `T-003, T-007`; the dependency is declared, not hidden.

### C-004: E2E unlock contingent on unknown GPU
- **Where:** test-plan.md End-to-End section.
- **Failure mode:** e2e-drift
- **Response: FIXED-IN-PLAN (clarification).** The user has since locked a CPU-first baseline (NVIDIA GPU present, CUDA/AMD as later options); test-plan E2E section now states the L0 smoke is CPU-only and therefore hardware-independent — the GPU unknown cannot block the s1 unlock.

### C-005: Lineage hard-won fixes not explicitly checkpointed
- **Where:** research-report.md §4 "rewrite scope" risk vs build-plan.
- **Failure mode:** missing-risk
- **Response: FIXED-IN-PLAN.** Added a "Lineage-Fix Ledger" section to build-plan.md mapping all 12 lineage fixes to either an s0 task (✓) or a named deferral (s1 loop / s1-s2 context manager / s1 config / s1 backends); deferred rows are mirrored into `agent-tasks/agent-tasks.md` during Build.

### C-006: decisions.md empty but T-012 requires 9 ADRs
- **Failure mode:** ignored-ADR
- **Response: DEFER-WITH-RATIONALE (per critic's own assessment).** Sequencing is intentional — ADR text is already drafted in T-012's notes and is written at the end of Build; no plan change needed.

### C-007: T-012 bundles repo creation + push + ADR writing
- **Failure mode:** granularity
- **Response: FIXED-IN-PLAN.** Split into T-012 (record ADRs 001–009, commit) and T-013 (create public repo, push, verify CI conclusion as a separate step). Schema tree updated.

### C-008: T-002 polymorphic-args criterion slightly overstated vs test
- **Failure mode:** plan-test-mismatch (minor)
- **Response: DEFER-WITH-RATIONALE (per critic's own assessment).** `message_roundtrip_json` covers value-correctness transitively; combination of the two tests satisfies the criterion.

### C-009: T-008 conflates capping and sorting in one EARS clause
- **Failure mode:** EARS-vague
- **Response: FIXED-IN-PLAN.** Split into two clauses: length ≤ max_tools; identical alphabetically-sorted ordering across consecutive calls.

## Confidence

`proceed-with-caveats` (critic) → all four fix-in-plan items applied, one clarification applied, two deferred per the critic's own rationale, one rejected with reason. Plans amended and ready to lock.
