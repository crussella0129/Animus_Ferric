Finalized - DO NOT EDIT

# Sprint 113 Build Plan

## Intents

- [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) — state: active; acceptance criteria covered: 1–8.

## Schema Tree

- Evidence-bound autonomous recovery
  - Wire and tool evidence
    - T-11301: policy and trace foundation
    - T-11302: typed preparation and measured effects
  - Causal controller
    - T-11303: evidence and repair invariants
    - T-11304: durable replay and recovery
    - T-11305: controlled dispatch and product surfaces
    - T-11309: remove implicit legacy syntax-check execution
    - T-11310: close feature-gated surface verification
  - Causal experiment
    - T-11306: reproducible paired runner
    - T-11307: frozen development screen and bounded revision selection
    - T-11311: frozen paired confirmation
    - T-11312: untouched held tasks and teardown
    - T-11308: planner decision and durable closeout

## Execution Sequence

### T-11301: Establish additive harness-policy and trace wire foundations
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-core/src/harness.rs`, `crates/ferric-trace/src/event.rs`, `crates/ferric-loop/src/trace_structure.rs`, `crates/ferric-cli/src/trace_cmd.rs`
- **Depends on:** (none)
- **Acceptance criterion:** AC-1 and the compatibility portion of AC-5.
- **Success criterion (EARS):**
  - **WHEN** pre-evidence serialized policy or trace data is read, **THEN** Ferric **SHALL** default it to legacy behavior without inventing controller evidence.
  - **WHEN** an evidence trace carries controller events, **THEN** the shared structural validator **SHALL** accept only versioned, causally placed payloads and reject them under legacy policy.
- **Notes:** Landed in `8fc7c4f5469ff85f17c626369231e0f3881195c0`; the final audit must include a literal pre-evidence trace fixture plus negative version/order cases, not only an isolated policy line.

### T-11302: Prepare controlled operations and report measured effects
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-tools/src/control.rs`, `crates/ferric-tools/src/builtin/controlled_read.rs`, `crates/ferric-tools/src/builtin/controlled_file.rs`, `crates/ferric-tools/tests/controlled_navigation.rs`, `crates/ferric-tools/tests/controlled_mutations.rs`, `crates/ferric-tools/tests/controlled_structural.rs`
- **Depends on:** T-11301
- **Acceptance criterion:** AC-1 and AC-2.
- **Success criterion (EARS):**
  - **WHEN** a controlled navigation call succeeds, **THEN** the registry **SHALL** return an explicit normalized observation envelope with completeness and deterministic content identity.
  - **WHEN** a prepared mutation is stale, byte-identical, opaque, or an absent/valid-to-invalid supported syntax transition, **THEN** controlled publication **SHALL** preserve the target and return a typed refusal before approval.
  - **WHEN** an admitted structural or content operation changes workspace state, **THEN** it **SHALL** report measured path effects rather than inferred success.
- **Notes:** Landed across `c7408215d898720c5057b9b973c6032a038851e6`, `d362b6e9684677e58c899ea13c48b176ba96b4eb`, and `97844df6a03346760e03a39cacb166dcdfff5d1d`.

### T-11303: Enforce causal observation, mutation, and repair invariants
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-loop/src/controller.rs`, `crates/ferric-loop/src/controlled_dispatch.rs`, `crates/ferric-loop/src/trace_structure.rs`, `crates/ferric-loop/tests/evidence_dispatch_tests.rs`
- **Depends on:** T-11301, T-11302
- **Acceptance criterion:** AC-2 and AC-3.
- **Success criterion (EARS):**
  - **WHEN** an existing-content mutation lacks fresh complete evidence from an earlier model turn, **THEN** the controller **SHALL** reject it before any approver or commit path runs.
  - **WHEN** a named check fails, **THEN** the controller **SHALL** require a later-turn qualifying inspection before repair and **SHALL** refuse the same check at the unchanged epoch before process spawn.
  - **WHEN** a call causes one or more real workspace effects, **THEN** the controller **SHALL** advance exactly one mutation epoch and project the same invariant during trace verification.
- **Notes:** Landed primarily in `05ae8946e314c60eccb10748b8efcaa4e43b89a9` and `841bf3a0d2614d96766482c5be1f661ec4aa1c68`.

### T-11304: Preserve controller truth across replay and recovery
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-loop/src/replay.rs`, `crates/ferric-loop/src/run.rs`, `crates/ferric-loop/src/compact.rs`, `crates/ferric-loop/tests/recovery_protocol_tests.rs`, `crates/ferric-loop/tests/resume_tests.rs`
- **Depends on:** T-11301, T-11303
- **Acceptance criterion:** AC-4.
- **Success criterion (EARS):**
  - **WHEN** an evidence session resumes from a valid pause or crash prefix, **THEN** replay **SHALL** restore controller/checkpoint truth, conservatively stale inherited file evidence, and inject a byte-stable machine-derived recovery packet.
  - **WHEN** model history is compacted or a resume is resumed again, **THEN** controller truth **SHALL** remain independent of any model-authored summary.
- **Notes:** Landed in `271b6daf78bc62a406f2f424e62344dc2bf34b16` after the initial recovery work.

### T-11305: Integrate controlled dispatch and compatible product surfaces
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-loop/src/run.rs`, `crates/ferric-loop/src/controlled_dispatch.rs`, `crates/ferric-cli/src/query.rs`, `crates/ferric-cli/src/chat.rs`, `crates/ferric-cli/src/api.rs`, `crates/ferric-cli/src/mcp.rs`, `crates/ferric-cli/src/icm.rs`, `crates/ferric-cli/tests/cli.rs`
- **Depends on:** T-11302, T-11303, T-11304
- **Acceptance criterion:** AC-3 and AC-5.
- **Success criterion (EARS):**
  - **WHEN** evidence policy is selected on a supported surface, **THEN** Ferric **SHALL** use the same prepare → controller → approval → commit → measured-effect order and general policy guidance.
  - **WHEN** resume policy is omitted or explicitly mismatched, **THEN** Ferric **SHALL** respectively inherit the traced policy or fail before a new trace/workspace mutation.
  - **WHEN** `evidence_planner` is requested before its protocol exists, **THEN** every surface **SHALL** fail closed without relabeling evidence-only execution.
- **Notes:** Product plumbing began in `78d0558a6760ac6f29df2a55ab221d6cd87554b1`; live dispatch landed in `841bf3a0d2614d96766482c5be1f661ec4aa1c68`.

### T-11306: Build a reproducible paired autonomy runner
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/backend.rs`, `crates/ferric-cli/src/autonomy_cmd.rs`, `crates/ferric-bench/src/runner.rs`, `crates/ferric-bench/src/autonomy_results.rs`, `crates/ferric-bench/src/summary.rs`
- **Depends on:** T-11301, T-11305
- **Acceptance criterion:** AC-6.
- **Success criterion (EARS):**
  - **WHEN** a paired autonomy evaluation starts, **THEN** the runner **SHALL** freeze and hash distinct arm binaries, schedule adjacent deterministic counterbalanced arms on fresh workspaces, and retain collision-safe traces with exact managed-server provenance.
  - **WHEN** a coordinate is incomplete, provenance-dirty, or infrastructure-unpaired, **THEN** summary scoring **SHALL** exclude it rather than record a model loss.
- **Notes:** Managed sampling landed in `40af4711545122f69b987c2097d7a9ec3cdc16cd`; paired evaluation landed in `380923fd9f381f0b97761199514a9af927817214`.

### T-11309: Remove implicit execution from legacy syntax warnings
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-tools/src/builtin/check_syntax.rs`, `crates/ferric-tools/src/builtin/write_file.rs`, `crates/ferric-tools/tests/builtin_file_tools.rs`
- **Depends on:** T-11302
- **Acceptance criterion:** AC-2's no-implicit-execution boundary.
- **Success criterion (EARS):**
  - **WHEN** any harness policy writes model-authored Python source, **THEN** syntax validation **SHALL** parse the candidate bytes in-process without launching Python, consulting `PATH`, importing `site`, or executing workspace code.
  - **WHEN** legacy mode writes syntactically invalid Python, **THEN** it **SHALL** preserve its warning-only compatibility behavior while reporting the bounded in-process parser diagnostic.
  - **WHEN** the model writes a valid `sitecustomize.py` containing import-time side effects, **THEN** Ferric **SHALL** write only the requested source file and **SHALL NOT** create the side-effect marker.
- **Notes:** The old helper used bare `python`/`python3 -c`; CPython places the current directory on `sys.path` for `-c` and normally imports `sitecustomize`. The controlled candidate parser already supplies the non-executing replacement primitive.

### T-11310: Close feature-gated product-surface verification
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `crates/ferric-cli/src/api.rs`, `crates/ferric-cli/src/mcp.rs`, `crates/ferric-cli/src/icm.rs`, `crates/ferric-cli/tests/cli.rs`
- **Depends on:** T-11305
- **Acceptance criterion:** AC-5.
- **Success criterion (EARS):**
  - **WHEN** API launch configuration selects unavailable EvidencePlanner, **THEN** preflight **SHALL** fail before bind, trace allocation, or workspace mutation.
  - **WHEN** a bounded API, MCP, or ICM request selects supported Evidence policy, **THEN** the resulting run configuration or `PolicySelected` event **SHALL** remain Evidence rather than silently defaulting to Legacy.
  - **WHEN** the backend-openai CLI test target runs, **THEN** it **SHALL** complete without an unbounded server future and without stale unsupported-Evidence expectations.
- **Notes:** The API test authored before Evidence became live still passes `evidence` to an unsupported-policy case; it now reaches `axum::serve` and can wait indefinitely. Use bounded request/config seams for positive propagation coverage.

### T-11307: Run the frozen development screen and bounded revision selection
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `docs/sprints/s113/sprint-tests/`, `docs/sprints/s113/control-artifacts/`, and only trace-justified general revisions under `crates/ferric-loop/`, `crates/ferric-tools/`, or `crates/ferric-cli/`
- **Depends on:** T-11302, T-11303, T-11304, T-11305, T-11306, T-11309, T-11310
- **Acceptance criterion:** AC-7.
- **Success criterion (EARS):**
  - **WHEN** the exact frozen H01/H04/H08 screen runs against the pinned model and managed-server topology, **THEN** the retained evidence **SHALL** be complete, infrastructure-clean, structurally verified, mechanism-safe, and report objective/contract outcomes without post-selection.
  - **WHEN** a screen has at least one objective-and-contract completion, zero unsafe completions or mechanism violations, and no more than the control's one unnecessary clarification, **THEN** Ferric **SHALL** select that candidate; a nonzero screen that fails any safety, mechanism, or clarification gate **SHALL** be falsified instead.
  - **WHEN** the first screen remains 0/3, **THEN** Ferric **SHALL** retain that result and permit no more than two general, trace-justified revisions, each with a new binary hash and separate screen evidence; a qualifying revision **SHALL** be selected and an exhausted budget **SHALL** be falsified.
  - **WHEN** a candidate is selected or falsified, **THEN** Ferric **SHALL** record the selected hash or falsification rationale and **SHALL NOT** inspect held tasks.
- **Notes:** The live structural-tool success from `97844df6a03346760e03a39cacb166dcdfff5d1d` is supporting mechanism evidence, not a substitute for this fixed matrix. This task ends at candidate selection; it does not run confirmation or held tasks.

### T-11311: Freeze and run paired stability confirmation
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `docs/sprints/s113/sprint-tests/` and ignored run-owned `.ferric/` evaluation artifacts
- **Depends on:** T-11307
- **Acceptance criterion:** AC-7 paired confirmation.
- **Success criterion (EARS):**
  - **WHEN** T-11307 selects a candidate, **THEN** the runner **SHALL** freeze and hash that exact binary before any confirmation episode and **SHALL** reject later binary drift.
  - **WHEN** paired confirmation runs, **THEN** it **SHALL** produce all 18 adjacent counterbalanced H01/H04/H08 rows on fresh equal initial trees and score only complete, infrastructure-clean, trace-valid pairs.
  - **WHEN** confirmation is summarized, **THEN** promotion **SHALL** require a positive paired objective delta and at least one evidence task completed in two of three repeats without safety, contract, clarification, or mechanism regression.
  - **WHEN** T-11307 records falsification without a qualifying candidate, **THEN** confirmation **SHALL** be recorded as skipped, no confirmation episode **SHALL** run, and the closeout path **SHALL** remain valid.

### T-11312: Evaluate untouched held tasks and prove teardown
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `docs/sprints/s113/sprint-tests/` and ignored run-owned `.ferric/` evaluation artifacts
- **Depends on:** T-11311
- **Acceptance criterion:** AC-7 held-task generalization, trace verification, and teardown.
- **Success criterion (EARS):**
  - **WHEN** paired confirmation finishes with frozen arms, **THEN** the same hashes **SHALL** run once per arm on previously uninspected H02/H03/H05/H06/H07 and report every paired outcome without tuning.
  - **WHEN** held-task results are summarized, **THEN** promotion **SHALL** require a positive aggregate paired objective-completion delta, at least one evidence-only objective-and-contract pass, no loss of a control-passing contract, no increase in unnecessary clarifications, and zero unsafe completions or mechanism violations; otherwise the intervention **SHALL** be falsified.
  - **WHEN** T-11311 records skipped confirmation because no candidate qualified, **THEN** held evaluation **SHALL** also be recorded as skipped and the previously uninspected held tasks **SHALL** remain sealed.
  - **WHEN** all evaluation episodes finish, **THEN** every retained trace **SHALL** pass side-effect-free structural verification and the report **SHALL** bind each result to its trace and binary hash.
  - **WHEN** the managed server is shut down, **THEN** independent checks **SHALL** prove the process, listener, runfile, and matching model server are absent.

### T-11308: Decide the planner boundary and close durable records
- **Intent:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md)
- **Touches:** `docs/intents/INT-0001-evidence-bound-autonomous-recovery.md`, `docs/work/tasks.md`, `docs/work/completed-tasks.md`, `docs/sprints/s113/`, and planner code only if a separately approved design requires it
- **Depends on:** T-11312
- **Acceptance criterion:** AC-8.
- **Success criterion (EARS):**
  - **WHEN** evidence-only evaluation is complete or explicitly falsified, **THEN** `docs/sprints/s113/planner-decision.md` **SHALL** record an explicit planner orchestration design or rejection, link the measured and skipped evaluation evidence, and explain how that evidence supports the decision.
  - **WHEN** no planner protocol is implemented, **THEN** `evidence_planner` **SHALL** remain unavailable and **SHALL NOT** silently execute evidence-only behavior under a planner label.
  - **WHEN** sprint 113 closes, **THEN** intent state, completed work, test evidence, migration-safe documentation, and architectural consequences **SHALL** agree.
- **Notes:** The user controls any future planner scope; this task cannot widen it implicitly.
