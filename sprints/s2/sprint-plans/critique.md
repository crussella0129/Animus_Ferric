# Plan Critique — Sprint 2

> Critic: subagent (adversarial review per prompts/plan-critic.md), 2026-06-12.
> Primary-agent dispositions inline as **Response:**. 13 concerns: 9 fixed in plan, 2 deferred with rationale, 2 rejected with reason.

### C-001: ActionProtocol ownership / select_protocol untested — **FIXED-IN-PLAN.** T-207 notes clarify ferric-core defines, ferric-loop imports; `select_protocol_matrix` unit test added to test plan.
### C-002: StopReason::TruncatedAction not explicit — **FIXED-IN-PLAN.** T-208 notes now name the enum variant + as_str mapping.
### C-003: max_tokens plumbing site vague — **FIXED-IN-PLAN.** T-210 EARS names the existing SamplingParams.max_tokens field and the query.rs construction site (no provider type change).
### C-004: oovra rev verification untestable — **FIXED-IN-PLAN (clarified).** A git rev is immutable (branch movement cannot drift it); the acceptance test is T-209 compiling and calling the API at that rev. Noted in T-201.
### C-005: No static llguidance-safety guarantee for generated schemas — **DEFER-WITH-RATIONALE (per critic).** The generator emits only the verified construct set; the smoke-grammar run is the schema-compile acceptance gate; llguidance is the source of truth.
### C-006: Result-framing assertion implicit — **FIXED-IN-PLAN.** grammar_happy_path now explicitly asserts the `[tool_result for write_file]` user-role message in the recorded second request.
### C-007: Mock truncation injection unclear — **FIXED-IN-PLAN.** T-208 notes: MockProvider's settable `truncated` (T-204) is the test hook; the bench runner never injects truncation.
### C-008: T-211 missing dep on T-205 — **REJECT.** The critique is wrong because T-211's Depends-on line already reads "T-201, T-205".
### C-009: ADR-010 backend validation unproven — **REJECT.** The critique is wrong because `MistralRsProvider::complete()` has called `request.validate()` as its first line since s1 T-109 (documented in the module and ADR-010); the layered coverage (validate_matrix + adr010/grammar_request_shape + real-model gate) was settled in the s1 test critique (C-002 there).
### C-010: E2E circularity (gate pre-asserts the grammar's terminator fix) — **FIXED-IN-PLAN.** Both smoke variants accept {task_complete, final_text}; the SWEEP measures the per-protocol task_complete rate; Qwen-7B explicitly informational/non-blocking.
### C-011: PromptLineage type undefined — **FIXED-IN-PLAN.** Defined as plain `Vec<(String, String)>` id+version tuples in RunArgs, matching PromptComposed.composed_of.
### C-012: "No FinalText in grammar mode" under-tested — **FIXED-IN-PLAN.** `grammar_non_action_json_rejected` test added; T-207 notes state valid-but-non-action JSON is rejected, never FinalText.
### C-013: Property/branch order fragility — **FIXED-IN-PLAN.** T-206 EARS strengthened: insertion-order keys (tool first) AND deterministic branch order (registry-sorted, terminator last).

## Confidence
`proceed-with-caveats` (critic) → all dispositions applied. Ready to lock.
