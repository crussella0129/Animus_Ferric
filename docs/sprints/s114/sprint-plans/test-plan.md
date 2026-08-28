Finalized - DO NOT EDIT

# Sprint 114 Test Plan

## Intent Traceability

| Intent | Acceptance criterion | Build task / EARS clause | Verification |
| --- | --- | --- | --- |
| [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) | AC-1 exact ignored model, source, license, and hash | T-11407 / E07-A | `model_download_and_sha256_attestation` |
| INT-0007 | AC-1 fail-closed acquisition | T-11407 / E07-B | `model_acquisition_failure_is_not_verified` |
| INT-0007 | AC-1 ignored storage with tracked provenance | T-11407 / E07-C | `model_is_ignored_evidence_is_tracked` |
| INT-0007 | AC-1 conditional exact Q3 fallback | T-11407 / E07-D | `q3_fallback_download_is_gated_and_attested` |
| INT-0007 | AC-3 frozen prompt/seed/check/grader | T-11408 / E08-A | `mh_rs01_seed_baseline_and_immutability` |
| INT-0007 | AC-4 sandboxed model-authored execution | T-11408 / E08-B | `bubblewrap_execution_boundary_canaries` |
| INT-0007 | AC-3 independent deterministic grader | T-11408 / E08-C | `grader_known_good_and_violation_matrix` |
| INT-0007 | AC-2 managed-server identity and allocation | T-11409 / E09-A | `managed_server_coordinate_attestation` |
| INT-0007 | AC-2 real constrained Ferric compatibility | T-11409 / E09-B | `qwen38_grammar_nonce_smoke` |
| INT-0007 | AC-2/AC-6 truthful context fallback | T-11409 / E09-C | `runtime_failure_classification_and_context_fallback` |
| INT-0007 | AC-2/AC-6 precommitted quant selection | T-11409 / E09-D | `qwen38_quant_viability_selection` |
| INT-0007 | AC-3/AC-4 sealed no-repair invocation | T-11410 / E10-A | `mh_rs01_invocation_and_mutation_audit` |
| INT-0007 | AC-3 one linked continuation | T-11410 / E10-B | `mh_rs01_resume_lineage` |
| INT-0007 | AC-3/AC-6 seven-dimension grading and attribution | T-11410 / E10-C | `mh_rs01_final_contract_grade` |
| INT-0007 | AC-3 bounded fallback simulation | T-11410 / E10-D | `mh_rs01_existing_model_fallback_label` |
| INT-0007 | AC-5 pinned installation and discovery | T-11411 / E11-A | `sprint_loop_install_discovery` |
| INT-0007 | AC-5 controlled authorization difference | T-11411 / E11-B | `sprint_loop_no_config_injection_control` |
| INT-0007 | AC-5 native versus assisted resource access | T-11411 / E11-C | `sprint_loop_resource_resolution_split` |
| INT-0007 | AC-5 helper absence and assisted Book attempt | T-11411 / E11-D | `sprint_loop_helper_absence_and_typed_book_attempt` |
| INT-0007 | AC-5/AC-6 re-entry, Git, and remote boundary | T-11411 / E11-E | `sprint_loop_layered_capability_verdict` |
| INT-0007 | AC-5 non-Qwen behavioral fallback | T-11411 / E11-F | `sprint_loop_existing_model_control` |
| INT-0007 | AC-7 concise README with durable history | T-11413 / E13-A through E13-C | `readme_history_cleanup_and_link_audit` |
| INT-0007 | AC-3/AC-5 provenance completeness | T-11412 / E12-A | `evidence_manifest_completeness` |
| INT-0007 | AC-2/AC-3 trace and teardown integrity | T-11412 / E12-B | `trace_archive_and_teardown_audit` |
| INT-0007 | AC-6 non-inflated outcome semantics | T-11412 / E12-C | `truthfulness_and_book_state_audit` |

The terminal and proposed dependency intents are constraints, not sprint-
advanced outcomes. Their acceptance criteria are not claimed as covered or
implemented by these tests.

## Unit Tests

### T-11407 model acquisition tests

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- `model_download_and_sha256_attestation` (E07-A): the final primary file exists only after the complete third-party repository, full revision, pinned URL, filename, exact byte size, and full converter-published SHA-256 tuple is verified.
- `model_acquisition_failure_is_not_verified` (E07-B): simulated wrong hash/short transfer cannot produce a verified record or launch input.
- `model_is_ignored_evidence_is_tracked` (E07-C): `git check-ignore` accepts the GGUF, `git status` contains only small provenance files, and one record asserts conversion publisher, official upstream, Apache-2.0 license, full revision/URL, filename, bytes, and SHA-256 together.
- `q3_fallback_download_is_gated_and_attested` (E07-D): Q3 is absent unless the retained Q4 viability result authorizes it; if present, its complete pinned source/identity/size/hash tuple passes the same publication gate and no 2-bit artifact exists.

### T-11408 harness self-tests

- **Intent:** INT-0007
- `mh_rs01_seed_baseline_and_immutability` (E08-A): seed manifest matches, untouched seed fails for missing modules, and model containment cannot reach operator inputs.
- `bubblewrap_execution_boundary_canaries` (E08-B): source write and outbound-network canaries fail, unrelated host/user paths are hidden, isolated target/temp writes succeed, and timeout/resource caps terminate a deliberate overrun.
- `grader_known_good_and_violation_matrix` (E08-C): known-good fixture passes; immutable edit, dependency, symlink, extra path, missing plan/test, visible/hidden semantic, CLI, and safety fixtures each fail their named dimension.

### T-11412 evidence-schema tests

- **Intent:** INT-0007
- `evidence_manifest_completeness` (E12-A): every required identity/output field is present and every retained artifact has a resolvable hash.
- `truthfulness_and_book_state_audit` (E12-C): result labels and Book transitions agree with the structured grades and skill matrix.

### T-11413 README cleanup tests

- **Intent:** INT-0007
- `readme_history_cleanup_and_link_audit` (E13-A through E13-C): a whole-file scan finds no numeric sprint-result narrative or `docs/sprints/sN/` result link, current policy wording and four canonical Book links remain, every retained local README link resolves, and all historical sprint evidence files are unchanged.

## Integration Tests

### Managed model and Ferric integration

- **Intents:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- `managed_server_coordinate_attestation` (E09-A): every launched Q4/Q3 and `32768`/`16384` retry records process/listener ownership, health, served identity, exact requested and effective offload/context/cache/reasoning/timeout, RAM/VRAM, versions, and startup log; the ultimately selected coordinate has the complete consistent tuple.
- `qwen38_grammar_nonce_smoke` (E09-B): the real model reads the exact nonce and terminates through Ferric's grammar protocol; trace verification succeeds and no bytes mutate. One warm-up plus exactly three identical 256-token timed requests run without replacements; any error, timeout, or sub-128-token result makes the coordinate non-viable, otherwise the retained median is reproducible from all three counters/timings.
- `runtime_failure_classification_and_context_fallback` (E09-C): every context-`32768` failure is preserved and classified; only memory pressure permits one declared context-`16384` retry.
- `qwen38_quant_viability_selection` (E09-D): any Q4 functional, timed-request, minimum-length, or median failure authorizes at most one fixed-32-layer Q3 coordinate; Q3 is selected only when its smoke and all three samples are valid with median at least 2.0 decoded tokens/s. If neither passes, the exact result is `no_viable_qwen38_coordinate` with no undeclared model.

### Ferric app-session integration

- **Intents:** INT-0007
- `mh_rs01_invocation_and_mutation_audit` (E10-A): command/provenance and initial manifest are sealed, Ultra/Legacy/grammar/check/Ring-1/one-turn settings match, active tools exclude host shell, human task control, and Git mutation, and executable candidate code is reached only by the fixed sandboxed check.
- `mh_rs01_resume_lineage` (E10-B): the first segment ends specifically at `max_turns` after one turn on the failing seed, then exactly one source/child trace link, 27-turn resumed cap, and unchanged prompt/workspace/policy are observed; any other early terminal state fails persistence.

### Ferric skill integration

- **Intents:** INT-0007
- `sprint_loop_install_discovery` (E11-A): pinned unmodified adapter hashes match and the result is either exact list discovery or an exact parse/name/layout failure; a negative result gates behavioral arms without blocking model-independent conclusions.
- `sprint_loop_no_config_injection_control` (E11-B): identical `--no-config` arms differ only by explicit authorization; captured CLI diagnostic, exact `SessionPrompt.system` section bytes/hash, and the marker corroborate absent/present injection, while `PromptComposed` is excluded as proof.
- `sprint_loop_resource_resolution_split` (E11-C): the no-hint authorized arm records native linked-resource/router resolution independently from the later operator-materialized readable-resource arm; the copied resource hash equals its pinned-tree source and assistance is explicit.
- `sprint_loop_helper_absence_and_typed_book_attempt` (E11-D): a capture stub retains the actual provider request/constrained schema for Evidence/Ring-1 and Legacy/Ring-1, exact helper requests cannot execute when absent, and the later Legacy/Ring-1 materialized-resource arm uses only typed tools.
- `sprint_loop_existing_model_control` (E11-F): only `no_viable_qwen38_coordinate` permits the rehashed, freshly smoked existing 7B control; missing/mismatched/failed-smoke state becomes `fallback_control_unavailable` with no substitute, while model-independent conclusions continue.

## End-to-End Tests

- **Status:** possible
- `mh_rs01_final_contract_grade` (E10-C): from sealed seed through one resume or the six-hour cap, the final candidate receives binary results for planning, build, tests, iteration, persistence, safety, trace, and mutation reconciliation; every diff hash maps to a Ferric effect and command journal entry or no-repair/safety fail, while infrastructure remains separate.
- `mh_rs01_existing_model_fallback_label` (E10-D): the existing 7B runs only after `no_viable_qwen38_coordinate`, only after hash reverification, and is unambiguously reported as a fallback simulation rather than Qwen3.8 performance.
- `sprint_loop_layered_capability_verdict` (E11-E): the same pinned tree runs external `check-book.sh` and router from the disposable root; results score discovery, authorization, native/materialized resource access, helper exposure, typed-tool Book advancement, operator validation, re-entry, and `git_write` registered/offered/attempted/succeeded separately under Evidence/Ring-2 and Legacy/Ring-2, with no remote mutation.
- `trace_archive_and_teardown_audit` (E12-B): verify retained traces without tools, rehash archives, stop the managed server, and independently prove listener/process/runfile cleanup.

The app E2E runs model-authored Rust only after `bubblewrap_execution_boundary_canaries` passes. If the sandbox cannot be recreated, E2E is blocked as infrastructure rather than downgraded to unsandboxed execution.
