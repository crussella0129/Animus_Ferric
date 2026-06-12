# Artifact: L0–L6 Benchmark Harness Port Spec (s2)

> Source: Explore agent over Animus scripts/run_benchmark.py + tests/benchmarks/, 2026-06-12.

## The ladder (faithful to YAML specs)
| L | name | task | timeout | max iters |
|---|---|---|---|---|
| 0 | single-readonly-tool | list files (setup: hello.txt, notes.md); forbidden: write/delete/edit/bash | 60s | 5 |
| 1 | single-file-rename | hello.py → greet.py | 90s | 6 |
| 2 | multi-step-ops | rename + mkdir tests + mkdir docs | 180s | 12 |
| 3 | single-file-construction | greet.py with hello() returning 'world' (regex-verified) | 180s | 8 |
| 4 | multi-file-with-test | greet.py + tests/test_greet.py with assert | 300s | 12 |
| 5 | mini-cli | argparse CLI + test | 600s | 18 |
| 6 | full-todo-app | todo.py add/list/done + todos.json + subprocess test | 900s | 25 |

Spec fields: level, name, description, prompt, setup_files{path:content}, expectations[{path,type:file|dir|missing,content_regex?}], expected_tools_called, expected_tools_called_any_of, forbidden_tools_called, max_iterations, wall_clock_timeout_s, optional post_verify_command(+stdout_regex,timeout).

## Run protocol (port 1:1)
materialize temp workspace (write setup_files) → invoke agent CLI with workspace+model+trace flags → subprocess with timeout → parse trace → verify expectations (file/dir/missing + content_regex) → verify tools (required ∧ any_of ∧ ¬forbidden) → detect failure-admission phrases in task_complete summary → optional post_verify command in workspace → completed = !timed_out ∧ exit==0 ∧ expectations ∧ tools ∧ post_verify ∧ terminator∈{task_complete, final_text} → append row to results.jsonl → cleanup (unless --keep-workspace).

## Results row (key fields)
run_id, started_at, completed, level, level_name, variant, model, tier_observed, grammar_active_observed, iterations, plan_steps, steps_executed, tokens_in/out, wall_time_s, repetition_guard_fires, turn_terminator, tool_calls_made[{name,args}], tools_unique_called, task_complete_summary, failure_admission_phrase, expectations_ok(+detail), tools_ok(+detail incl. unnamed_tool_call_count), post_verify{...}, exit_code, timed_out, trace_event_count, stderr_excerpt.

## Animus→Ferric metric mapping
Direct: iterations→count(TurnStart); tokens→sum(TurnEnd.in/out); wall→ts_ms diff; terminator→SessionEnd.reason; repetition_guard_fires→count(RepetitionGuard.stopped); tool_calls→ToolCall events; task_complete summary→ToolCall args (extract in runner).
**Gaps (flagged):** plan_steps/steps_executed (no planner — record null); tier_observed + constraint status not in trace → **additive trace fix: emit a PolicySelected/extended SessionStart event carrying tier, protocol, and policy budgets** (readers tolerate unknowns, ADR-002).

## Port shape (recommended)
**New crate `ferric-bench`** (lib) + `ferric bench` subcommand in ferric-cli. Spec files: **TOML** in `crates/ferric-bench/specs/l0.toml..l6.toml` (serde_yaml deprecated; toml is already in-tree via oovra and standard). Results: `benchmarks/results.jsonl` (same row spirit). Runner spawns the `ferric` binary (same separation as the L0 smoke — avoids executor coupling) — release-profile binary REQUIRED (s1 lesson). Mock-driven integration tests for the harness itself.

## Calibration pipeline
highest level passed (completed ∧ expectations ∧ tools) → measured_level (0–6) → recorded in results + a `benchmarks/model_profiles.json`; next runs construct ModelProfile with measured_level → tier_for_level override (ferric-core, already implemented + tested). Result row also records tier_from_params vs tier_from_measured.

Note: forbidden-tool names in Animus specs (move_path, glob_search, edit_file, bash, delete_path) include tools Ferric doesn't have yet — port specs with Ferric's current tool names (read_file/write_file/list_dir + task_complete) and mark levels needing absent tools (L1/L2 need move/mkdir!) — **flag: L1/L2 are not runnable until Ferric grows move_path/make_dir tools; either add those two NANO file tools in s2 (small, lineage-proven) or scope calibration to L0/L3/L4 initially.**
