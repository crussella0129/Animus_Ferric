# Sprint 0 Unit Tests — Results

All EARS clauses from build-plan.md map to named tests. Run 2026-06-10, local Windows (rustc 1.93.1): **ALL PASS** (also gated in CI on windows+ubuntu).

| Task | EARS clause → test | Result |
|---|---|---|
| T-001 | build/fmt/clippy gates (`cargo build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`) | pass |
| T-002 | Message round-trip → `message_roundtrip_json` | pass |
| T-002 | polymorphic args → `toolcall_args_polymorphic` | pass |
| T-003 | determinism → `policy_for_is_deterministic` | pass |
| T-003 | NANO boundaries 0.5/3.9/4.0/13.1 → `nano_tier_boundaries` | pass |
| T-003 | NANO policy shape → `nano_policy_shape` | pass |
| T-003 | measured-level downgrade → `measured_level_overrides_params` | pass |
| T-003 | measured-level upgrade → `measured_level_upgrade` | pass |
| T-003 | (extra) budget vs ctx → `prompt_budget_respects_small_context` | pass |
| T-004 | durability + round-trip → `jsonl_roundtrip_all_event_types` | pass |
| T-004 | unknown tolerance → `reader_tolerates_unknown_event` | pass |
| T-004 | monotonic seq → `seq_monotonic_per_session` | pass |
| T-004 | full untruncated output → `tool_result_full_output` | pass |
| T-005 | script order + exhaustion → `mock_replays_script_in_order` | pass |
| T-005 | constraint plumbing → `constraint_recorded_by_mock` | pass |
| T-005 | dyn-compatibility → `provider_is_dyn_compatible` | pass |
| T-006 | `..` escape → `rejects_dotdot_escape` | pass |
| T-006 | absolute outside → `rejects_absolute_outside` | pass |
| T-006 | prefix collision → `rejects_prefix_collision` | pass |
| T-006 | symlink escape → `rejects_symlink_escape` (`#[cfg(unix)]`, runs on ubuntu CI leg) | pass (CI) |
| T-006 | valid nested path → `accepts_nested_valid_path` | pass |
| T-006 | (extra) in-workspace `..` → `dotdot_within_workspace_is_allowed` | pass |
| T-007 | sensitive paths denied → `denies_sensitive_paths` | pass |
| T-007 | plain read allowed → `allows_plain_read` | pass |
| T-007 | (extra) ordinary write allowed → `allows_ordinary_write` | pass |
| T-007 | const deny lists → `denylist_is_const` | pass |
| T-008 | deny blocks handler → `execute_blocks_on_deny` | pass |
| T-008 | truncation preserves full → `output_truncation_preserves_full` | pass |
| T-008 | sorted + capped → `tools_for_policy_sorted_and_capped` | pass |
| T-008 | (extra) unknown tool → `unknown_tool_outcome` | pass |
| T-009 | write→read round-trip → `write_then_read_roundtrip` | pass |
| T-009 | outside-workspace refusal → `tools_refuse_outside_workspace` | pass |
| T-009 | deterministic listing → `list_dir_deterministic_order` | pass |
| T-009 | (extra) missing file is error → `read_missing_file_is_error_not_panic` | pass |
| T-010 | version flag → `version_flag` | pass |
| T-010 | unknown events render → `trace_cat_renders_unknown` | pass |
| T-010 | (extra) usage on no args → `no_args_fails_with_usage` | pass |
| T-011/T-013 | CI matrix green — run 27301488990 conclusion=success | pass |
| T-012 | decisions.md contains ADR-001..009 dated entries | pass (inspection) |

Notes: `rejects_symlink_escape` is unix-only by design (Windows symlink creation needs privileges); the ubuntu CI leg covers it.
