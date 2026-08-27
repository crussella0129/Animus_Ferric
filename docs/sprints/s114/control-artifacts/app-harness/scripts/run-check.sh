#!/usr/bin/env bash
# Fixed MH-RS01 check: static gate, visible tests, hidden tests, and CLI contract.
# Timeout plus kill-grace budgets total 771 seconds before small process
# teardown overhead, leaving 129 seconds below the operator-authored 900-second
# whole-check cap.

set -Eeuo pipefail
IFS=$'\n\t'
export LANG=C
export LC_ALL=C

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

s114_initialize
s114_prepare_runtime
s114_verify_frozen_inputs

candidate=$(s114_resolve_directory "${1:-$PWD}")
s114_assert_state_candidate_disjoint "$candidate"
seed_root="$S114_HARNESS_ROOT/seed"

[[ -d "$seed_root" && ! -L "$seed_root" ]] || s114_die "frozen seed is missing"

driver_dir=$(s114_allocate_run_dir check-driver)
build_stdout="$driver_dir/logs/build-grader.stdout"
build_stderr="$driver_dir/logs/build-grader.stderr"
s114_run_logged build-grader-driver "$S114_REPO_ROOT" \
    "$build_stdout" "$build_stderr" -- \
    bash "$SCRIPT_DIR/build-grader.sh"
build_exit=$S114_LAST_COMMAND_EXIT
if ((build_exit != 0)); then
    s114_emit_stage_result grader_build fail "$build_exit" "$build_stdout" "$build_stderr"
    s114_show_failure_logs grader-build "$build_stdout" "$build_stderr"
    s114_die "grader build failed; no candidate code was executed"
fi
grader_binary=$(tail -n 1 -- "$build_stdout")
[[ -x "$grader_binary" && ! -L "$grader_binary" ]] \
    || s114_die "attested grader binary is unavailable after build"
grader_binary=$(realpath -- "$grader_binary")
case "$grader_binary" in
    "$S114_STATE_ROOT_RESOLVED"/runs/*/target/release/mh-rs01-grader) ;;
    *) s114_die "grader builder returned a path outside its fresh run target" ;;
esac
s114_assert_elf_executable "$grader_binary"
grader_binary_hash=$(s114_sha256_file "$grader_binary")
grader_source_hash=$(s114_tree_digest "$S114_HARNESS_ROOT/grader")
grader_build_run=$(dirname -- "$(dirname -- "$(dirname -- "$grader_binary")")")
grader_attestation="$grader_build_run/grader-build.json"
[[ -f "$grader_attestation" && ! -L "$grader_attestation" ]] \
    || s114_die "fresh grader build attestation is unavailable"
grep -Fq "\"source_tree_sha256\":\"$grader_source_hash\"" "$grader_attestation" \
    || s114_die "grader source hash does not match its fresh-build attestation"
grep -Fq "\"binary_sha256\":\"$grader_binary_hash\"" "$grader_attestation" \
    || s114_die "grader binary hash does not match its fresh-build attestation"

run_dir=$(s114_allocate_run_dir check)
stage_root="$run_dir/stages"
for stage_name in \
    model-tests-compile model-tests-list model-tests-run \
    visible-contract-compile visible-contract-run all-targets \
    hidden-compile hidden-run cli-build; do
    install -d -m 0700 -- \
        "$stage_root/$stage_name/target" \
        "$stage_root/$stage_name/tmp" \
        "$stage_root/$stage_name/cargo-home"
done
static_stdout="$run_dir/logs/static.stdout"
static_stderr="$run_dir/logs/static.stderr"
static_results="$run_dir/static-results.jsonl"

s114_run_logged grader-static "$S114_REPO_ROOT" \
    "$static_stdout" "$static_stderr" -- \
    timeout --foreground --signal=TERM --kill-after=5s 45s \
    prlimit --cpu=30:30 --as=2147483648:2147483648 \
        --fsize=67108864:67108864 --nproc=32:32 --nofile=256:256 --core=0:0 -- \
    "$grader_binary" \
        --candidate "$candidate" \
        --seed "$seed_root" \
        --results "$static_results"
static_exit=$S114_LAST_COMMAND_EXIT

case "$static_exit" in
    0 | 2) ;;
    *)
        s114_emit_stage_result static_grader infrastructure_error "$static_exit" \
            "$static_stdout" "$static_stderr"
        s114_show_failure_logs static-grader "$static_stdout" "$static_stderr"
        s114_die "static grader failed as infrastructure; no candidate code was executed"
        ;;
esac
[[ -f "$static_results" ]] || s114_die "static grader did not create its requested result file"
[[ $(s114_sha256_file "$static_results") == $(s114_sha256_file "$static_stdout") ]] \
    || s114_die "static grader stdout and --results bytes differ"

s114_grader_dimension_line() {
    local dimension=$1
    local -a matches=()
    mapfile -t matches < <(grep -E \
        "\"dimension\"[[:space:]]*:[[:space:]]*\"${dimension}\"" \
        "$static_results" || true)
    ((${#matches[@]} == 1)) \
        || s114_die "static grader must emit exactly one $dimension result"
    printf '%s\n' "${matches[0]}"
}

static_seed=$(s114_grader_dimension_line seed_immutability)
static_dependencies=$(s114_grader_dimension_line dependency_policy)
static_paths=$(s114_grader_dimension_line path_policy)
static_plan=$(s114_grader_dimension_line plan)
static_model_tests=$(s114_grader_dimension_line model_tests)
static_source_safety=$(s114_grader_dimension_line source_safety)

final_results="$run_dir/results.jsonl"
if ((static_exit == 2)); then
    {
        printf '%s\n' "$static_seed" "$static_dependencies" "$static_paths" \
            "$static_plan" "$static_model_tests"
        printf '%s\n' \
            '{"schema":"s114-grade-v1","dimension":"visible_contract","status":"not_run","reason":"static_gate_failed"}' \
            '{"schema":"s114-grade-v1","dimension":"hidden_contract","status":"not_run","reason":"static_gate_failed"}' \
            '{"schema":"s114-grade-v1","dimension":"cli_contract","status":"not_run","reason":"static_gate_failed"}'
        printf '%s\n' "$static_source_safety"
    } >"$final_results"
    static_seal_stdout="$run_dir/logs/static-result-seal.stdout"
    static_seal_stderr="$run_dir/logs/static-result-seal.stderr"
    s114_run_logged static-result-seal "$S114_REPO_ROOT" \
        "$static_seal_stdout" "$static_seal_stderr" -- \
        sha256sum -- "$final_results"
    ((S114_LAST_COMMAND_EXIT == 0)) || s114_die "static result sealing failed"
    s114_verify_journal_since
    cat -- "$final_results"
    s114_emit_stage_result static_grader candidate_failure "$static_exit" \
        "$static_stdout" "$static_stderr"
    printf '{"schema":"s114-check-summary-v1","results_sha256":"%s","grader_binary_sha256":"%s","grader_source_tree_sha256":"%s","frozen_input_manifest_sha256":"%s"}\n' \
        "$(s114_sha256_file "$final_results")" \
        "$grader_binary_hash" \
        "$grader_source_hash" \
        "$S114_FROZEN_MANIFEST_HASH"
    exit 2
fi
s114_emit_stage_result static_grader pass 0 "$static_stdout" "$static_stderr"

# The static-only grader may inspect a rejected candidate without executing it.
# A passing static gate is followed by the independent filesystem checks and
# Bubblewrap canaries immediately before any candidate compilation or binary.
s114_assert_candidate_tree "$candidate"
before_hash=$(s114_tree_digest "$candidate")
preflight_stdout="$driver_dir/logs/preflight.stdout"
preflight_stderr="$driver_dir/logs/preflight.stderr"
s114_run_logged preflight-driver "$S114_REPO_ROOT" \
    "$preflight_stdout" "$preflight_stderr" -- \
    bash "$SCRIPT_DIR/preflight.sh" "$candidate"
preflight_exit=$S114_LAST_COMMAND_EXIT
if ((preflight_exit != 0)); then
    s114_emit_stage_result containment_preflight fail "$preflight_exit" \
        "$preflight_stdout" "$preflight_stderr"
    s114_show_failure_logs containment-preflight "$preflight_stdout" "$preflight_stderr"
    s114_die "containment preflight failed; no candidate code was executed"
fi
s114_emit_stage_result containment_preflight pass 0 \
    "$preflight_stdout" "$preflight_stderr"

hidden_root=''
for hidden_candidate in \
    "$S114_HARNESS_ROOT/grader/hidden-tests" \
    "$S114_HARNESS_ROOT/grader/hidden"; do
    if [[ -f "$hidden_candidate/Cargo.toml" ]]; then
        hidden_root=$hidden_candidate
        break
    fi
done
[[ -n "$hidden_root" && ! -L "$hidden_root" ]] \
    || s114_die "operator-owned hidden test crate is unavailable"
hidden_offending=$(find -P "$hidden_root" -mindepth 1 \
    ! -type d ! -type f -print -quit)
[[ -z "$hidden_offending" ]] \
    || s114_die "hidden test crate contains a symlink or special filesystem object"
hidden_expected_tests=6
hidden_declared_tests=$(grep -Ec '^[[:space:]]*#\[test\][[:space:]]*$' \
    "$hidden_root/src/lib.rs" || true)
((hidden_declared_tests == hidden_expected_tests)) \
    || s114_die "hidden source and frozen expected-test count disagree"

model_tests_stdout="$run_dir/logs/model-tests-run.stdout"
model_tests_stderr="$run_dir/logs/model-tests-run.stderr"
model_tests_compile_stdout="$run_dir/logs/model-tests-compile.stdout"
model_tests_compile_stderr="$run_dir/logs/model-tests-compile.stderr"
model_tests_list_stdout="$run_dir/logs/model-tests-list.stdout"
model_tests_list_stderr="$run_dir/logs/model-tests-list.stderr"
model_test_binary_before=$S114_ZERO_HASH
S114_EXTRA_RO_BINDS=()
S114_STAGE_WALL_SECONDS=60 S114_STAGE_CPU_SECONDS=50 s114_run_sandbox_logged \
    model-tests-compile \
    "$candidate" \
    "$stage_root/model-tests-compile/target" \
    "$stage_root/model-tests-compile/tmp" \
    "$stage_root/model-tests-compile/cargo-home" \
    /workspace \
    "$model_tests_compile_stdout" \
    "$model_tests_compile_stderr" \
    -- \
    cargo test --offline --test agent_tests --no-run
model_tests_compile_exit=$S114_LAST_COMMAND_EXIT
if ((model_tests_compile_exit == 0)); then
    mapfile -t model_test_binaries < <(find \
        "$stage_root/model-tests-compile/target/debug/deps" \
        -maxdepth 1 -type f -name 'agent_tests-*' ! -name '*.*' -print)
    ((${#model_test_binaries[@]} == 1)) \
        || s114_die "model-test compile must produce exactly one test executable"
    model_test_binary_name=$(basename -- "${model_test_binaries[0]}")
    s114_assert_elf_executable "${model_test_binaries[0]}"
    model_test_binary_before=$(s114_sha256_file "${model_test_binaries[0]}")
    S114_STAGE_WALL_SECONDS=10 S114_STAGE_CPU_SECONDS=5 \
        S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
        S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
        S114_TARGET_READ_ONLY=1 \
        s114_run_sandbox_logged \
        model-tests-list \
        "$candidate" \
        "$stage_root/model-tests-compile/target" \
        "$stage_root/model-tests-list/tmp" \
        "$stage_root/model-tests-list/cargo-home" \
        /workspace \
        "$model_tests_list_stdout" \
        "$model_tests_list_stderr" \
        -- \
        "/target/debug/deps/$model_test_binary_name" --list --format terse
    model_tests_list_exit=$S114_LAST_COMMAND_EXIT
    model_test_binary_after_list=$(s114_sha256_file "${model_test_binaries[0]}")
    [[ "$model_test_binary_after_list" == "$model_test_binary_before" ]] \
        || s114_die "read-only model-test executable changed during test listing"
    S114_STAGE_WALL_SECONDS=30 S114_STAGE_CPU_SECONDS=20 \
        S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
        S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
        S114_TARGET_READ_ONLY=1 \
        s114_run_sandbox_logged \
        model-tests-run \
        "$candidate" \
        "$stage_root/model-tests-compile/target" \
        "$stage_root/model-tests-run/tmp" \
        "$stage_root/model-tests-run/cargo-home" \
        /workspace \
        "$model_tests_stdout" \
        "$model_tests_stderr" \
        -- \
        "/target/debug/deps/$model_test_binary_name" --test-threads=1
    model_tests_exit=$S114_LAST_COMMAND_EXIT
    model_test_binary_after=$(s114_sha256_file "${model_test_binaries[0]}")
    [[ "$model_test_binary_after" == "$model_test_binary_before" ]] \
        || s114_die "read-only model-test executable changed during execution"
else
    model_tests_list_exit=$model_tests_compile_exit
    model_tests_exit=$model_tests_compile_exit
    cp -- "$model_tests_compile_stdout" "$model_tests_list_stdout"
    cp -- "$model_tests_compile_stderr" "$model_tests_list_stderr"
    cp -- "$model_tests_compile_stdout" "$model_tests_stdout"
    cp -- "$model_tests_compile_stderr" "$model_tests_stderr"
fi
# The selected Rust 1.96 libtest emits only `<name>: test` records for terse
# listing. Requiring every nonempty line to match is stricter than depending on
# a historical optional footer.
mapfile -t registered_model_test_names < <(sed -n 's/: test$//p' \
    "$model_tests_list_stdout")
registered_model_tests=${#registered_model_test_names[@]}
model_names_distinct=1
declare -A s114_registered_model_names=()
model_topic_parsing=0
model_topic_invalid_dependencies=0
model_topic_completed=0
model_topic_priority=0
model_topic_lexical=0
model_topic_cycles=0
model_topic_preservation=0
for registered_name in "${registered_model_test_names[@]}"; do
    if [[ -z "$registered_name" || -n ${s114_registered_model_names[$registered_name]+present} ]]; then
        model_names_distinct=0
    else
        s114_registered_model_names[$registered_name]=1
    fi
    # The disclosed topic-name contract is ASCII-case-insensitive. Preserve the
    # raw libtest name for distinctness, and fold only the predicate input.
    registered_name_folded=${registered_name,,}
    if [[ "$registered_name_folded" == *pars* \
        || ("$registered_name_folded" == *manifest* \
            && ("$registered_name_folded" == *valid* \
                || "$registered_name_folded" == *accept*)) ]]; then
        model_topic_parsing=1
    fi
    if [[ ("$registered_name_folded" == *depend* \
            && "$registered_name_folded" =~ (invalid|reject|unknown|duplicate|self|empty|error)) \
        || ("$registered_name_folded" =~ (unknown|duplicate|self|empty) \
            && "$registered_name_folded" =~ (reject|invalid)) ]]; then
        model_topic_invalid_dependencies=1
    fi
    if [[ "$registered_name_folded" == *completed* \
        || ("$registered_name_folded" == *done* \
            && "$registered_name_folded" =~ (prereq|depend|job|omit|satisf|unlock)) ]]; then
        model_topic_completed=1
    fi
    if [[ "$registered_name_folded" == *priority* \
        || ("$registered_name_folded" == *highest* \
            && "$registered_name_folded" == *ready*) ]]; then
        model_topic_priority=1
    fi
    if [[ "$registered_name_folded" == *lexical* \
        || "$registered_name_folded" == *tie* \
        || "$registered_name_folded" == *alphabet* ]]; then
        model_topic_lexical=1
    fi
    if [[ "$registered_name_folded" == *cycle* \
        || "$registered_name_folded" == *deadlock* ]]; then
        model_topic_cycles=1
    fi
    if [[ ("$registered_name_folded" == *preserv* \
            && "$registered_name_folded" =~ (input|job|manifest)) \
        || ("$registered_name_folded" == *not* \
            && "$registered_name_folded" == *mutat*) ]]; then
        model_topic_preservation=1
    fi
done
model_topic_coverage=0
if ((model_topic_parsing == 1 \
    && model_topic_invalid_dependencies == 1 \
    && model_topic_completed == 1 \
    && model_topic_priority == 1 \
    && model_topic_lexical == 1 \
    && model_topic_cycles == 1 \
    && model_topic_preservation == 1)); then
    model_topic_coverage=1
fi
model_list_nonempty_lines=$(grep -Ec '.' "$model_tests_list_stdout" || true)
model_list_contract=0
if ((model_tests_list_exit == 0 \
    && registered_model_tests >= 6 \
    && model_list_nonempty_lines == registered_model_tests \
    && model_names_distinct == 1 \
    && model_topic_coverage == 1)) \
    && [[ ! -s "$model_tests_list_stderr" ]]; then
    model_list_contract=1
fi
mapfile -t model_test_result_lines < <(grep -E '^test result:' \
    "$model_tests_stdout" || true)
mapfile -t model_test_summaries < <(grep -E \
    '^test result: ok\. [0-9]+ passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in .+s$' \
    "$model_tests_stdout" || true)
executed_model_tests=0
if ((${#model_test_summaries[@]} == 1)); then
    executed_model_tests=$(awk '{print $4}' <<<"${model_test_summaries[0]}")
fi
if ((model_tests_exit == 0 \
    && model_list_contract == 1 \
    && ${#model_test_result_lines[@]} == 1 \
    && ${#model_test_summaries[@]} == 1 \
    && executed_model_tests == registered_model_tests)); then
    model_tests_status=pass
    model_dimension_exit=0
else
    model_tests_status=fail
    if ((model_tests_compile_exit != 0)); then
        model_dimension_exit=$model_tests_compile_exit
        model_diagnostic_stage=model-tests-compile
        model_diagnostic_stdout=$model_tests_compile_stdout
        model_diagnostic_stderr=$model_tests_compile_stderr
    elif ((model_tests_list_exit != 0)); then
        model_dimension_exit=$model_tests_list_exit
        model_diagnostic_stage=model-tests-list
        model_diagnostic_stdout=$model_tests_list_stdout
        model_diagnostic_stderr=$model_tests_list_stderr
    elif ((model_list_contract != 1)); then
        model_dimension_exit=2
        model_diagnostic_stage=model-tests-list
        model_diagnostic_stdout=$model_tests_list_stdout
        model_diagnostic_stderr=$model_tests_list_stderr
    elif ((model_tests_exit != 0)); then
        model_dimension_exit=$model_tests_exit
        model_diagnostic_stage=model-tests-run
        model_diagnostic_stdout=$model_tests_stdout
        model_diagnostic_stderr=$model_tests_stderr
    else
        model_dimension_exit=2
        model_diagnostic_stage=model-tests-run
        model_diagnostic_stdout=$model_tests_stdout
        model_diagnostic_stderr=$model_tests_stderr
    fi
    s114_show_failure_logs "$model_diagnostic_stage" \
        "$model_diagnostic_stdout" "$model_diagnostic_stderr"
    s114_show_candidate_diagnostics "$model_diagnostic_stage" \
        "$model_diagnostic_stdout" "$model_diagnostic_stderr"
fi
model_tests_evidence="$run_dir/model-tests-evidence.json"
printf '{"schema":"s114-dynamic-evidence-v1","dimension":"model_tests","compile_exit_code":%s,"list_exit_code":%s,"test_exit_code":%s,"registered":%s,"executed":%s,"topic_coverage":%s,"binary_sha256":"%s","compile_stdout_sha256":"%s","compile_stderr_sha256":"%s","list_stdout_sha256":"%s","list_stderr_sha256":"%s","test_stdout_sha256":"%s","test_stderr_sha256":"%s"}\n' \
    "$model_tests_compile_exit" "$model_tests_list_exit" "$model_tests_exit" \
    "$registered_model_tests" "$executed_model_tests" "$model_topic_coverage" \
    "$model_test_binary_before" \
    "$(s114_sha256_file "$model_tests_compile_stdout")" \
    "$(s114_sha256_file "$model_tests_compile_stderr")" \
    "$(s114_sha256_file "$model_tests_list_stdout")" \
    "$(s114_sha256_file "$model_tests_list_stderr")" \
    "$(s114_sha256_file "$model_tests_stdout")" \
    "$(s114_sha256_file "$model_tests_stderr")" \
    >"$model_tests_evidence"
aggregate_stderr="$run_dir/logs/aggregate.stderr"
: >"$aggregate_stderr"
s114_emit_stage_result model_tests "$model_tests_status" "$model_dimension_exit" \
    "$model_tests_evidence" "$aggregate_stderr"

visible_stdout="$run_dir/logs/visible.stdout"
visible_stderr="$run_dir/logs/visible.stderr"
visible_compile_stdout="$run_dir/logs/visible-compile.stdout"
visible_compile_stderr="$run_dir/logs/visible-compile.stderr"
candidate_rlib_before=$S114_ZERO_HASH
candidate_rlib_path=''
S114_EXTRA_RO_BINDS=()
S114_STAGE_WALL_SECONDS=60 S114_STAGE_CPU_SECONDS=50 s114_run_sandbox_logged \
    visible-contract-compile \
    "$candidate" \
    "$stage_root/visible-contract-compile/target" \
    "$stage_root/visible-contract-compile/tmp" \
    "$stage_root/visible-contract-compile/cargo-home" \
    /workspace \
    "$visible_compile_stdout" \
    "$visible_compile_stderr" \
    -- \
    cargo test --offline --test contract --no-run
visible_compile_exit=$S114_LAST_COMMAND_EXIT
if ((visible_compile_exit == 0)); then
    mapfile -t visible_binaries < <(find \
        "$stage_root/visible-contract-compile/target/debug/deps" \
        -maxdepth 1 -type f -name 'contract-*' ! -name '*.*' -print)
    ((${#visible_binaries[@]} == 1)) \
        || s114_die "visible-contract compile must produce exactly one test executable"
    visible_binary_name=$(basename -- "${visible_binaries[0]}")
    s114_assert_elf_executable "${visible_binaries[0]}"
    visible_binary_before=$(s114_sha256_file "${visible_binaries[0]}")
    mapfile -t candidate_rlibs < <(find \
        "$stage_root/visible-contract-compile/target/debug/deps" \
        -maxdepth 1 -type f -name 'librelease_plan-*.rlib' -print)
    ((${#candidate_rlibs[@]} == 1)) \
        || s114_die "visible compile must produce exactly one candidate rlib"
    candidate_rlib_path=${candidate_rlibs[0]}
    [[ ! -L "$candidate_rlib_path" && $(stat -c '%h' -- "$candidate_rlib_path") == 1 ]] \
        || s114_die "candidate rlib is a symlink or has additional hard links"
    candidate_rlib_before=$(s114_sha256_file "$candidate_rlib_path")
    S114_STAGE_WALL_SECONDS=30 S114_STAGE_CPU_SECONDS=20 \
        S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
        S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
        S114_TARGET_READ_ONLY=1 \
        s114_run_sandbox_logged \
        visible-contract \
        "$candidate" \
        "$stage_root/visible-contract-compile/target" \
        "$stage_root/visible-contract-run/tmp" \
        "$stage_root/visible-contract-run/cargo-home" \
        /workspace \
        "$visible_stdout" \
        "$visible_stderr" \
        -- \
        "/target/debug/deps/$visible_binary_name" --test-threads=1
    visible_contract_exit=$S114_LAST_COMMAND_EXIT
    visible_binary_after=$(s114_sha256_file "${visible_binaries[0]}")
    [[ "$visible_binary_after" == "$visible_binary_before" ]] \
        || s114_die "read-only visible-contract executable changed during execution"
else
    visible_contract_exit=$visible_compile_exit
    cp -- "$visible_compile_stdout" "$visible_stdout"
    cp -- "$visible_compile_stderr" "$visible_stderr"
fi

all_targets_stdout="$run_dir/logs/all-targets.stdout"
all_targets_stderr="$run_dir/logs/all-targets.stderr"
S114_STAGE_WALL_SECONDS=90 S114_STAGE_CPU_SECONDS=75 \
    S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
    S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
    s114_run_sandbox_logged \
    all-targets \
    "$candidate" \
    "$stage_root/all-targets/target" \
    "$stage_root/all-targets/tmp" \
    "$stage_root/all-targets/cargo-home" \
    /workspace \
    "$all_targets_stdout" \
    "$all_targets_stderr" \
    -- \
    cargo test --offline --all-targets -- --test-threads=1
all_targets_exit=$S114_LAST_COMMAND_EXIT

if ((visible_contract_exit == 0 && all_targets_exit == 0)); then
    visible_status=pass
    visible_exit=0
else
    visible_status=fail
    if ((visible_contract_exit != 0)); then
        visible_exit=$visible_contract_exit
    else
        visible_exit=$all_targets_exit
    fi
    if ((visible_contract_exit != 0)); then
        if ((visible_compile_exit != 0)); then
            s114_show_failure_logs visible-contract-compile \
                "$visible_compile_stdout" "$visible_compile_stderr"
            s114_show_candidate_diagnostics visible-contract-compile \
                "$visible_compile_stdout" "$visible_compile_stderr"
        else
            s114_show_failure_logs visible-contract "$visible_stdout" "$visible_stderr"
            s114_show_candidate_diagnostics visible-contract \
                "$visible_stdout" "$visible_stderr"
        fi
    fi
    if ((all_targets_exit != 0)); then
        s114_show_failure_logs all-targets "$all_targets_stdout" "$all_targets_stderr"
        s114_show_candidate_diagnostics all-targets \
            "$all_targets_stdout" "$all_targets_stderr"
    fi
fi
visible_evidence="$run_dir/visible-contract-evidence.json"
printf '{"schema":"s114-dynamic-evidence-v1","dimension":"visible_contract","contract_compile_exit_code":%s,"contract_test_exit_code":%s,"all_targets_exit_code":%s,"contract_binary_sha256":"%s","contract_compile_stdout_sha256":"%s","contract_compile_stderr_sha256":"%s","contract_test_stdout_sha256":"%s","contract_test_stderr_sha256":"%s","all_targets_stdout_sha256":"%s","all_targets_stderr_sha256":"%s"}\n' \
    "$visible_compile_exit" "$visible_contract_exit" "$all_targets_exit" \
    "${visible_binary_before:-$S114_ZERO_HASH}" \
    "$(s114_sha256_file "$visible_compile_stdout")" \
    "$(s114_sha256_file "$visible_compile_stderr")" \
    "$(s114_sha256_file "$visible_stdout")" \
    "$(s114_sha256_file "$visible_stderr")" \
    "$(s114_sha256_file "$all_targets_stdout")" \
    "$(s114_sha256_file "$all_targets_stderr")" \
    >"$visible_evidence"
s114_emit_stage_result visible_contract "$visible_status" "$visible_exit" \
    "$visible_evidence" "$aggregate_stderr"

hidden_stdout="$run_dir/logs/hidden.stdout"
hidden_stderr="$run_dir/logs/hidden.stderr"
hidden_compile_stdout="$run_dir/logs/hidden-compile.stdout"
hidden_compile_stderr="$run_dir/logs/hidden-compile.stderr"
hidden_binary_before=$S114_ZERO_HASH
hidden_binary_path="$stage_root/hidden-compile/target/mh-rs01-hidden-tests"
if ((visible_compile_exit == 0)); then
    candidate_build_dir=$(dirname -- "$candidate_rlib_path")
    candidate_rlib_name=$(basename -- "$candidate_rlib_path")
    [[ $(s114_sha256_file "$candidate_rlib_path") == "$candidate_rlib_before" ]] \
        || s114_die "candidate rlib changed before hidden compilation"
    S114_EXTRA_RO_BINDS=(
        "$hidden_root" /hidden
        "$candidate_build_dir" /candidate-build
    )
    S114_STAGE_WALL_SECONDS=60 S114_STAGE_CPU_SECONDS=50 \
        s114_run_sandbox_logged \
        hidden-contract-compile \
        "$candidate" \
        "$stage_root/hidden-compile/target" \
        "$stage_root/hidden-compile/tmp" \
        "$stage_root/hidden-compile/cargo-home" \
        /hidden \
        "$hidden_compile_stdout" \
        "$hidden_compile_stderr" \
        -- \
        rustc --test /hidden/src/lib.rs \
            --crate-name mh_rs01_hidden_tests \
            --edition=2024 \
            --extern "release_plan=/candidate-build/$candidate_rlib_name" \
            -L dependency=/candidate-build \
            -o /target/mh-rs01-hidden-tests
    hidden_compile_exit=$S114_LAST_COMMAND_EXIT
    S114_EXTRA_RO_BINDS=()
    [[ $(s114_sha256_file "$candidate_rlib_path") == "$candidate_rlib_before" ]] \
        || s114_die "read-only candidate rlib changed during hidden compilation"
else
    hidden_compile_exit=2
    : >"$hidden_compile_stdout"
    printf 'visible candidate rlib prerequisite was unavailable\n' >"$hidden_compile_stderr"
fi
if ((hidden_compile_exit == 0)); then
    s114_assert_elf_executable "$hidden_binary_path"
    hidden_binary_before=$(s114_sha256_file "$hidden_binary_path")
    S114_STAGE_WALL_SECONDS=30 S114_STAGE_CPU_SECONDS=20 \
        S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
        S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
        S114_TARGET_READ_ONLY=1 \
        s114_run_sandbox_logged \
        hidden-contract \
        "$candidate" \
        "$stage_root/hidden-compile/target" \
        "$stage_root/hidden-run/tmp" \
        "$stage_root/hidden-run/cargo-home" \
        /workspace \
        "$hidden_stdout" \
        "$hidden_stderr" \
        -- \
        /target/mh-rs01-hidden-tests --test-threads=1
    hidden_exit=$S114_LAST_COMMAND_EXIT
    hidden_binary_after=$(s114_sha256_file "$hidden_binary_path")
    [[ "$hidden_binary_after" == "$hidden_binary_before" ]] \
        || s114_die "read-only hidden-test executable changed during execution"
    [[ $(s114_sha256_file "$candidate_rlib_path") == "$candidate_rlib_before" ]] \
        || s114_die "candidate rlib changed after hidden execution"
else
    hidden_exit=$hidden_compile_exit
    cp -- "$hidden_compile_stdout" "$hidden_stdout"
    cp -- "$hidden_compile_stderr" "$hidden_stderr"
fi
mapfile -t hidden_result_summaries < <(grep -E '^test result:' \
    "$hidden_stdout" || true)
mapfile -t hidden_expected_summaries < <(grep -E \
    "^test result: ok\\. ${hidden_expected_tests} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in .+s$" \
    "$hidden_stdout" || true)
if ((hidden_exit == 0 \
    && (${#hidden_result_summaries[@]} != 1 \
        || ${#hidden_expected_summaries[@]} != 1))); then
    hidden_exit=2
fi
if ((hidden_exit == 0)); then
    hidden_status=pass
else
    hidden_status=fail
fi
hidden_evidence="$run_dir/hidden-contract-evidence.json"
printf '{"schema":"s114-dynamic-evidence-v1","dimension":"hidden_contract","expected_tests":%s,"result_summary_count":%s,"compile_exit_code":%s,"test_exit_code":%s,"candidate_rlib_sha256":"%s","binary_sha256":"%s","compile_stdout_sha256":"%s","compile_stderr_sha256":"%s","test_stdout_sha256":"%s","test_stderr_sha256":"%s"}\n' \
    "$hidden_expected_tests" "${#hidden_result_summaries[@]}" \
    "$hidden_compile_exit" "$hidden_exit" "$candidate_rlib_before" "$hidden_binary_before" \
    "$(s114_sha256_file "$hidden_compile_stdout")" \
    "$(s114_sha256_file "$hidden_compile_stderr")" \
    "$(s114_sha256_file "$hidden_stdout")" \
    "$(s114_sha256_file "$hidden_stderr")" \
    >"$hidden_evidence"
# Hidden diagnostics and their hashes remain in operator-only journaled evidence.
# Ferric receives only the stable aggregate dimension record emitted below.

cli_build_stdout="$run_dir/logs/cli-build.stdout"
cli_build_stderr="$run_dir/logs/cli-build.stderr"
S114_STAGE_WALL_SECONDS=60 S114_STAGE_CPU_SECONDS=50 s114_run_sandbox_logged \
    cli-build \
    "$candidate" \
    "$stage_root/cli-build/target" \
    "$stage_root/cli-build/tmp" \
    "$stage_root/cli-build/cargo-home" \
    /workspace \
    "$cli_build_stdout" \
    "$cli_build_stderr" \
    -- \
    cargo build --offline --bin release_plan
cli_build_exit=$S114_LAST_COMMAND_EXIT

cli_stdout="$run_dir/logs/cli.stdout"
cli_stderr="$run_dir/logs/cli.stderr"
cli_evidence="$run_dir/cli-contract-evidence.jsonl"
: >"$cli_stdout"
: >"$cli_stderr"
printf '{"schema":"s114-cli-evidence-v1","stage":"build","exit_code":%s,"stdout_sha256":"%s","stderr_sha256":"%s"}\n' \
    "$cli_build_exit" \
    "$(s114_sha256_file "$cli_build_stdout")" \
    "$(s114_sha256_file "$cli_build_stderr")" \
    >"$cli_evidence"
if ((cli_build_exit == 0)); then
    cli_binary="$stage_root/cli-build/target/debug/release_plan"
    [[ -f "$cli_binary" && -x "$cli_binary" && ! -L "$cli_binary" ]] \
        || s114_die "CLI build did not produce the expected executable"
    s114_assert_elf_executable "$cli_binary"
    cli_binary_hash=$(s114_sha256_file "$cli_binary")
    cli_cases="$run_dir/cli-cases"
    install -d -m 0700 -- "$cli_cases"
    printf '%s\n' \
        'completed | 9 | done |' \
        'alpha | 7 | pending | completed' \
        'beta | 7 | pending | completed' \
        'gamma | 4 | pending | alpha,beta' \
        >"$cli_cases/success.txt"
    printf '%s\n' 'completed | 9 | done |' >"$cli_cases/empty-plan.txt"
    printf '%s\n' 'alpha | 4 | pending | absent' >"$cli_cases/unknown.txt"
    printf '%s\n' \
        'alpha | 4 | pending | beta' \
        'beta | 7 | pending | alpha' \
        >"$cli_cases/cycle.txt"
    # Consumed by the sourced sandbox helper from the nested CLI-case function.
    # shellcheck disable=SC2034
    S114_EXTRA_RO_BINDS=("$cli_cases" /cases)

    s114_run_cli_case() {
        local case_name=$1
        shift
        local case_stdout="$run_dir/logs/cli-$case_name.stdout"
        local case_stderr="$run_dir/logs/cli-$case_name.stderr"
        local case_state="$stage_root/cli-$case_name"
        local case_exit
        install -d -m 0700 -- "$case_state/tmp" "$case_state/cargo-home"
        S114_STAGE_WALL_SECONDS=10 S114_STAGE_CPU_SECONDS=5 \
            S114_STAGE_ADDRESS_BYTES=2147483648 S114_STAGE_PROCESS_COUNT=16 \
            S114_STAGE_FILE_BYTES=67108864 S114_STAGE_OPEN_FILES=128 \
            S114_TARGET_READ_ONLY=1 s114_run_sandbox_logged \
            "cli-$case_name" \
            "$candidate" \
            "$stage_root/cli-build/target" \
            "$case_state/tmp" \
            "$case_state/cargo-home" \
            /workspace \
            "$case_stdout" \
            "$case_stderr" \
            -- \
            /target/debug/release_plan "$@"
        case_exit=$S114_LAST_COMMAND_EXIT
        S114_CLI_CASE_STDOUT=$case_stdout
        S114_CLI_CASE_STDERR=$case_stderr
        S114_CLI_CASE_EXIT=$case_exit
        printf '{"schema":"s114-cli-evidence-v1","stage":"%s","exit_code":%s,"stdout_sha256":"%s","stderr_sha256":"%s"}\n' \
            "$case_name" "$case_exit" \
            "$(s114_sha256_file "$case_stdout")" \
            "$(s114_sha256_file "$case_stderr")" \
            >>"$cli_evidence"
        [[ $(s114_sha256_file "$cli_binary") == "$cli_binary_hash" ]] \
            || s114_die "read-only CLI executable changed during $case_name"
    }

    cli_pass=1
    s114_run_cli_case success /cases/success.txt
    success_hash=$(printf 'alpha\nbeta\ngamma\n' | sha256sum | awk '{print $1}')
    if ((S114_CLI_CASE_EXIT != 0)) \
        || [[ $(s114_sha256_file "$S114_CLI_CASE_STDOUT") != "$success_hash" ]] \
        || [[ -s "$S114_CLI_CASE_STDERR" ]]; then
        cli_pass=0
        s114_show_failure_logs cli-success \
            "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
        s114_show_candidate_diagnostics cli-success \
            "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
    fi

    s114_run_cli_case empty-plan /cases/empty-plan.txt
    if ((S114_CLI_CASE_EXIT != 0)) \
        || [[ -s "$S114_CLI_CASE_STDOUT" || -s "$S114_CLI_CASE_STDERR" ]]; then
        cli_pass=0
        s114_show_failure_logs cli-empty-plan \
            "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
        s114_show_candidate_diagnostics cli-empty-plan \
            "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
    fi

    for invalid_case in unknown cycle no-args two-args missing-file; do
        case "$invalid_case" in
            unknown) cli_arguments=(/cases/unknown.txt) ;;
            cycle) cli_arguments=(/cases/cycle.txt) ;;
            no-args) cli_arguments=() ;;
            two-args) cli_arguments=(/cases/success.txt /cases/unknown.txt) ;;
            missing-file) cli_arguments=(/cases/does-not-exist.txt) ;;
        esac
        s114_run_cli_case "$invalid_case" "${cli_arguments[@]}"
        if ((S114_CLI_CASE_EXIT == 0)) \
            || [[ -s "$S114_CLI_CASE_STDOUT" ]] \
            || [[ $(head -c 6 -- "$S114_CLI_CASE_STDERR") != 'error:' ]]; then
            cli_pass=0
            s114_show_failure_logs "cli-$invalid_case" \
                "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
            s114_show_candidate_diagnostics "cli-$invalid_case" \
                "$S114_CLI_CASE_STDOUT" "$S114_CLI_CASE_STDERR"
        fi
    done
    if ((cli_pass == 1)); then
        cli_exit=0
        printf 'all seven read-only CLI cases passed\n' >"$cli_stdout"
    else
        cli_exit=2
        printf 'one or more read-only CLI cases failed\n' >"$cli_stderr"
    fi
else
    cli_exit=$cli_build_exit
    cp -- "$cli_build_stdout" "$cli_stdout"
    cp -- "$cli_build_stderr" "$cli_stderr"
    s114_show_failure_logs cli-build "$cli_build_stdout" "$cli_build_stderr"
    s114_show_candidate_diagnostics cli-build \
        "$cli_build_stdout" "$cli_build_stderr"
fi
if ((cli_exit == 0)); then
    cli_status=pass
else
    cli_status=fail
fi
s114_emit_stage_result cli_contract "$cli_status" "$cli_exit" \
    "$cli_evidence" "$aggregate_stderr"

source_status=pass
source_reason='static_source_policy_passed_and_candidate_tree_unchanged'
offending=$(find -P "$candidate" -xdev -mindepth 1 ! -type d ! -type f -print -quit)
if [[ -n "$offending" ]]; then
    source_status=fail
    source_reason='candidate_tree_gained_symlink_or_special_object_during_check'
    after_hash=$S114_ZERO_HASH
else
    after_hash=$(s114_tree_digest "$candidate")
    if [[ "$after_hash" != "$before_hash" ]]; then
        source_status=fail
        source_reason='candidate_tree_changed_during_check'
    fi
fi

{
    printf '%s\n' "$static_seed" "$static_dependencies" "$static_paths" \
        "$static_plan"
    printf '{"schema":"s114-grade-v1","dimension":"model_tests","status":"%s","registered":%s,"executed":%s,"topic_coverage":%s,"compile_exit_code":%s,"list_exit_code":%s,"test_exit_code":%s}\n' \
        "$model_tests_status" "$registered_model_tests" "$executed_model_tests" \
        "$model_topic_coverage" "$model_tests_compile_exit" \
        "$model_tests_list_exit" "$model_tests_exit"
    printf '{"schema":"s114-grade-v1","dimension":"visible_contract","status":"%s","contract_exit_code":%s,"all_targets_exit_code":%s}\n' \
        "$visible_status" "$visible_contract_exit" "$all_targets_exit"
    printf '{"schema":"s114-grade-v1","dimension":"hidden_contract","status":"%s"}\n' \
        "$hidden_status"
    printf '{"schema":"s114-grade-v1","dimension":"cli_contract","status":"%s","build_exit_code":%s,"contract_exit_code":%s}\n' \
        "$cli_status" "$cli_build_exit" "$cli_exit"
    printf '{"schema":"s114-grade-v1","dimension":"source_safety","status":"%s","reason":"%s","before_sha256":"%s","after_sha256":"%s"}\n' \
        "$source_status" "$source_reason" "$before_hash" "$after_hash"
} >"$final_results"

seal_stdout="$run_dir/logs/result-seal.stdout"
seal_stderr="$run_dir/logs/result-seal.stderr"
s114_run_logged result-seal "$S114_REPO_ROOT" "$seal_stdout" "$seal_stderr" -- \
    sha256sum -- "$final_results"
((S114_LAST_COMMAND_EXIT == 0)) || s114_die "result sealing failed"
s114_verify_journal_since
cat -- "$final_results"
printf '{"schema":"s114-check-summary-v1","results_sha256":"%s","grader_binary_sha256":"%s","grader_source_tree_sha256":"%s","frozen_input_manifest_sha256":"%s"}\n' \
    "$(s114_sha256_file "$final_results")" \
    "$grader_binary_hash" \
    "$grader_source_hash" \
    "$S114_FROZEN_MANIFEST_HASH"

if [[ "$model_tests_status" == pass \
    && "$visible_status" == pass \
    && "$hidden_status" == pass \
    && "$cli_status" == pass \
    && "$source_status" == pass ]]; then
    exit 0
fi
exit 2
