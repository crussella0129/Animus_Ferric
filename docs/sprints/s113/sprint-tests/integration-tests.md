# Sprint 113 Integration Test Results

- **Tested code head:** `dbaada383cd58415dfc775ec2c9d7e55a28bbcd0`
- **Executed:** 2026-08-26
- **Result:** pass locally; authoritative PR CI pending
- **Intent oracle:** [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md), acceptance criteria 1–8

## Executed gates

| Command | Result | Confirmation |
| --- | --- | --- |
| `cargo test --workspace` | PASS | All workspace, integration, live sandbox/container, template-hygiene, and doc-test targets passed on the host. Four declared fixtures/network tests remained ignored. |
| `cargo test -p ferric-cli --features backend-openai` | PASS | 163 binary unit, 6 bench-mock, 62 CLI integration, and 3 template-hygiene tests; zero failures and no hanging server future. |
| `cargo clippy --all-targets -- -D warnings` | PASS | Exact default-feature CI configuration. |
| `cargo clippy -p ferric-cli --features backend-openai --all-targets -- -D warnings` | PASS | Exact feature-gated CI configuration. |
| `cargo check --workspace --target aarch64-unknown-linux-gnu` | PASS | Cross-target workspace type check. |

The first restricted-sandbox workspace attempt made
`process_alive_follows_a_real_process` fail and stalled live registration
inspection. It was interrupted after retaining the signature. The same
unmodified suite then passed outside the process-inspection sandbox, including
both tests in 2.79 seconds. This is classified as a harness-environment result,
not hidden as a product pass or counted as a code failure.

## EARS evidence

| Clause | Arrangement and SHALL assertion | Executed named evidence |
| --- | --- | --- |
| T-11305.1 | Supported Evidence dispatch uses prepare → controller → approval → commit → measured effect and shared guidance. | `full_evidence_path_reads_edits_repairs_verifies_and_completes`; `stale_commit_is_typed_and_an_admitted_call_prompts_once`; `sink_denial_happens_before_a_verification_process_starts`; `evidence_guidance_is_added_to_custom_prompts_and_legacy_is_literal` |
| T-11305.2 | Omitted resume policy inherits; explicit mismatch fails before a new trace or mutation. | `resume_target_inherits_omitted_policy_and_rejects_an_explicit_mismatch`; `resume_harness_policy_mismatch_is_clear_and_allocates_no_new_trace`; `live_evidence_resume_injects_byte_identical_stale_recovery_packet_and_verifies` |
| T-11305.3 | EvidencePlanner fails closed rather than relabeling Evidence-only execution. | `planner_rejection_writes_no_trace_or_workspace_effect`; `unavailable_policies_write_no_event_and_dispatch_no_tool`; `evidence_runs_and_planner_fails_before_trace_or_workspace_mutation`; `evidence_talk_is_allowed_but_do_escalation_fails_explicitly` |
| T-11310.1 | API planner preflight fails before bind/trace/workspace effects. | `unsupported_planner_fails_before_api_bind_or_trace` |
| T-11310.2 | Bounded API, MCP, and ICM seams preserve explicit Evidence policy. | `backend_surface_policy_propagation`; `bounded_mcp_request_preserves_explicit_evidence_policy`; `bounded_icm_stage_preserves_explicit_evidence_policy` |
| T-11310.3 | The backend-enabled target terminates and contains no stale unsupported-Evidence expectation. | Complete passing `cargo test -p ferric-cli --features backend-openai` target |

The fail-closed planner matrix also directly covers query, API, MCP, ICM,
resume, and shared structural validation. Chat's `/do` path is explicitly
tested as unavailable under Evidence, but there is no separate direct
EvidencePlanner chat-launch test; chat reaches the same shared preflight and no
planner availability is claimed.

## CI status

The repository workflow runs on pull requests and pushes to `main`, not on an
unreviewed `dev` push. Therefore authoritative CI is truthfully **pending until
the final `dev → main` pull request exists**. Loop must verify successful—not
skipped or cancelled—Ubuntu and Windows default jobs, Ubuntu backend-openai
Clippy, and the aarch64 check. GitHub tests a synthetic merge ref, so the report
must retain both the pushed `dev` SHA and the workflow SHA while confirming the
PR `headRefOid` equals the pushed head.
