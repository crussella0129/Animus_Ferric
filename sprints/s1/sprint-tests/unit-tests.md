# Sprint 1 Unit Tests — Results

Run 2026-06-11, local Windows (rustc 1.93.1), default features + feature-gated set: **ALL PASS** (65 default-features tests workspace-wide; 11 feature-gated provider tests run separately).

| Task | EARS clause → test | Result |
|---|---|---|
| T-101 | round-trip all new events → `jsonl_roundtrip_all_event_types` (extended to 11 event types) | pass |
| T-101 | s0 wire format keeps parsing → `s0_trace_still_parses` | pass |
| T-101 | TurnEnd carries completion → `turn_end_carries_completion` | pass |
| T-101 | render arms, no unknown fallback → `trace_cat_renders_unknown` (CLI) | pass |
| T-102 | CheckRecord on allow → `check_records_on_allow` | pass |
| T-102 | CheckRecord on deny (rule+matched) → `check_records_on_deny` | pass |
| T-102 | `.ferric` write denied → `ferric_dir_write_denied` | pass |
| T-102 | deny lists const (extended) → `denylist_is_const` | pass |
| T-102 | handler never runs on deny → `execute_blocks_on_deny` (preserved) | pass |
| T-103 | validate matrix → `validate_matrix` | pass |
| T-103 | retryability truth table → `retryability_per_variant` | pass |
| T-109 | constraint 1:1 → `constraint_maps_one_to_one` (feature-gated) | pass |
| T-109 | tools mapping → `tools_map_with_schema_reshape` (feature-gated) | pass |
| T-109 | sampling + deterministic switch → `sampling_maps_with_deterministic_switch` (feature-gated) | pass |
| T-109 | args string fallback → `args_parse_with_string_fallback` (feature-gated) | pass |
| T-109 | error classification → `error_classification` (feature-gated) | pass |
| T-109 | message mapping all roles → `message_mapping_is_panic_free_for_all_roles` (feature-gated) | pass |
| T-109 | both-set rejected before engine → first-line `request.validate()` in `complete()`; covered by `validate_matrix` + the loop's `adr010_request_shape` (no model-free way to invoke complete()) | pass (structural) |
| T-110 | trace cat preserved → `trace_cat_renders_unknown` regression | pass |
| T-110 | usage on no/unknown args → `no_args_fails_with_usage`, `unknown_args_fail_with_usage` | pass |

Notes: feature-gated tests were run locally with `--features backend-mistralrs`; CI's backend-check job compiles them under `-D warnings`.
