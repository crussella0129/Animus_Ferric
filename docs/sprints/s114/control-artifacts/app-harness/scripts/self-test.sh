#!/usr/bin/env bash
# Exercise the frozen seed, static violation matrix, and contained grader stages.

set -Eeuo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

s114_initialize
s114_prepare_runtime
s114_verify_frozen_inputs

# Ferric bounds stdout and stderr independently at this exact frozen value.
# Prove the documented stderr envelope arithmetically as well as checking every
# complete self-test invocation below. Per stream, 160 retained bytes plus
# three maximum 57-byte prefixes, three newlines, and the 36-byte omission
# marker is at most 370 bytes. Two streams plus a conservatively bounded
# 384-byte log reference is 1,124 bytes per failure stage.
readonly s114_check_output_limit=12000
readonly s114_max_candidate_failure_stages=10
readonly s114_max_log_reference_bytes=384
grep -Eq '^output_limit = 12000$' "$S114_HARNESS_ROOT/checks.toml" \
    || s114_die "checks.toml output limit differs from the harness bound"
s114_worst_stderr_bytes=$((s114_max_candidate_failure_stages \
    * (2 * (S114_CANDIDATE_DIAGNOSTIC_PAYLOAD_BYTES + 3 * 57 + 3 + 36) \
        + s114_max_log_reference_bytes)))
((s114_worst_stderr_bytes < s114_check_output_limit)) \
    || s114_die "candidate repair diagnostics exceed the frozen output limit"

seed_root="$S114_HARNESS_ROOT/seed"
good_overlay="$S114_HARNESS_ROOT/fixtures/good"
[[ -d "$seed_root" && ! -L "$seed_root" ]] || s114_die "self-test seed is unavailable"
[[ -d "$good_overlay" && ! -L "$good_overlay" ]] \
    || s114_die "self-test known-good overlay is unavailable"

self_run=$(s114_allocate_run_dir self-test)
self_workspace_root="$S114_EXPERIMENT_ROOT/self-test-workspaces"
if [[ -e "$self_workspace_root" || -L "$self_workspace_root" ]]; then
    [[ -d "$self_workspace_root" && ! -L "$self_workspace_root" ]] \
        || s114_die "self-test workspace root must be a real directory"
else
    install -d -m 0700 -- "$self_workspace_root"
fi
candidate_root="$self_workspace_root/$(basename -- "$self_run")"
[[ ! -e "$candidate_root" && ! -L "$candidate_root" ]] \
    || s114_die "refusing to reuse a self-test candidate root"
install -d -m 0700 -- "$candidate_root"

s114_make_candidate() {
    local name=$1
    local destination="$candidate_root/$name"
    [[ ! -e "$destination" ]] || s114_die "self-test candidate already exists: $name"
    install -d -m 0700 -- "$destination"
    cp -a -- "$seed_root/." "$destination/"
    cp -a -- "$good_overlay/." "$destination/"
    printf '%s\n' "$destination"
}

s114_run_check_capture() {
    local name=$1
    local candidate_path=$2
    local stdout_file="$self_run/logs/$name.stdout"
    local stderr_file="$self_run/logs/$name.stderr"
    local exit_code
    local s114_dimension_index
    local -a s114_expected_dimensions=()
    local -a s114_public_lines=()

    s114_run_logged "self-test-$name" "$S114_REPO_ROOT" \
        "$stdout_file" "$stderr_file" -- \
        bash "$SCRIPT_DIR/run-check.sh" "$candidate_path"
    exit_code=$S114_LAST_COMMAND_EXIT
    S114_SELF_TEST_STDOUT=$stdout_file
    S114_SELF_TEST_STDERR=$stderr_file
    S114_SELF_TEST_EXIT=$exit_code
    (($(stat -c '%s' -- "$stdout_file") < s114_check_output_limit \
        && $(stat -c '%s' -- "$stderr_file") < s114_check_output_limit)) \
        || s114_die "$name exceeded the frozen per-stream output limit"
    mapfile -t s114_public_lines <"$stdout_file"
    ((${#s114_public_lines[@]} == 10)) \
        || s114_die "$name did not emit exactly nine grades plus one summary"
    s114_expected_dimensions=(
        seed_immutability dependency_policy path_policy plan model_tests
        visible_contract hidden_contract cli_contract source_safety
    )
    for s114_dimension_index in "${!s114_expected_dimensions[@]}"; do
        [[ "${s114_public_lines[$s114_dimension_index]}" \
            == '{"schema":"s114-grade-v1","dimension":"'"${s114_expected_dimensions[$s114_dimension_index]}"'",'* ]] \
            || s114_die "$name public grade order/schema is invalid"
    done
    [[ "${s114_public_lines[9]}" == '{"schema":"s114-check-summary-v1",'* ]] \
        || s114_die "$name public summary schema is invalid"
    export S114_SELF_TEST_STDOUT S114_SELF_TEST_STDERR S114_SELF_TEST_EXIT
}

s114_expect_dimension() {
    local results_file=$1
    local dimension=$2
    local status=$3
    local count
    count=$(grep -Ec \
        "^\\{\"schema\":\"s114-grade-v1\",\"dimension\":\"${dimension}\",\"status\":\"${status}\"([,}])" \
        "$results_file" || true)
    ((count == 1)) \
        || s114_die "expected exactly one $dimension=$status result, found $count"
}

s114_expect_static_failure() {
    local name=$1
    local candidate_path=$2
    local dimension=$3
    s114_run_check_capture "$name" "$candidate_path"
    ((S114_SELF_TEST_EXIT == 2)) \
        || s114_die "$name should exit 2, got $S114_SELF_TEST_EXIT"
    [[ $(grep -Ec '^\{"schema":"s114-grade-v1","dimension":' \
        "$S114_SELF_TEST_STDOUT" || true) == 9 ]] \
        || s114_die "$name static failure did not emit exactly nine grades"
    [[ $(grep -Ec '^\{"schema":"s114-check-summary-v1"' \
        "$S114_SELF_TEST_STDOUT" || true) == 1 ]] \
        || s114_die "$name static failure did not emit exactly one summary"
    s114_expect_dimension "$S114_SELF_TEST_STDOUT" "$dimension" fail
    printf 'self-test static rejection: %s -> %s\n' "$name" "$dimension"
}

infrastructure_stdout="$self_run/logs/infrastructure-exit.stdout"
infrastructure_stderr="$self_run/logs/infrastructure-exit.stderr"
s114_run_logged self-test-infrastructure-exit "$S114_REPO_ROOT" \
    "$infrastructure_stdout" "$infrastructure_stderr" -- \
    bash -c 'source "$1"; false' _ "$SCRIPT_DIR/common.sh"
infrastructure_exit=$S114_LAST_COMMAND_EXIT
((infrastructure_exit == 70)) \
    || s114_die "unhandled trusted failure should map to exit 70"
printf 'self-test infrastructure mapping: native failure -> 70\n'

override_stdout="$self_run/logs/inherited-override.stdout"
override_stderr="$self_run/logs/inherited-override.stderr"
s114_run_logged self-test-inherited-override "$S114_REPO_ROOT" \
    "$override_stdout" "$override_stderr" -- \
    env S114_STAGE_ADDRESS_BYTES=999999999 \
        bash -c 'source "$1"; s114_initialize' _ "$SCRIPT_DIR/common.sh"
override_exit=$S114_LAST_COMMAND_EXIT
((override_exit == 70)) \
    || s114_die "inherited sandbox resource override should fail closed with exit 70"
printf 'self-test fixed resource profile: inherited override rejected\n'

journal_canary_dir="$self_run/journal-canary"
journal_canary="$journal_canary_dir/broken-early-chain.tsv"
install -d -m 0700 -- "$journal_canary_dir"
cp -- "$S114_JOURNAL_PATH" "$journal_canary"
sed -i '2s/\t1\t/\t9\t/' "$journal_canary"
printf '%s  %s\n' \
    "$(s114_sha256_file "$journal_canary")" \
    "$(basename -- "$journal_canary")" \
    >"$journal_canary.sha256"
journal_canary_stdout="$self_run/logs/journal-canary.stdout"
journal_canary_stderr="$self_run/logs/journal-canary.stderr"
s114_run_logged self-test-journal-canary "$S114_REPO_ROOT" \
    "$journal_canary_stdout" "$journal_canary_stderr" -- \
    bash -c 'source "$1"; s114_verify_journal_structure "$2"' \
        _ "$SCRIPT_DIR/common.sh" "$journal_canary"
journal_canary_exit=$S114_LAST_COMMAND_EXIT
((journal_canary_exit == 70)) \
    || s114_die "broken early journal chain should fail closed with exit 70"
printf 'self-test journal integrity: broken early chain rejected\n'

# The untouched seed must fail for the deliberately missing modules. Run this
# exact Cargo command only after the same containment preflight used in live
# checks; no static contract bypass is used for any complete candidate.
baseline="$candidate_root/untouched-seed"
install -d -m 0700 -- "$baseline"
cp -a -- "$seed_root/." "$baseline/"
baseline_preflight_stdout="$self_run/logs/baseline-preflight.stdout"
baseline_preflight_stderr="$self_run/logs/baseline-preflight.stderr"
s114_run_logged self-test-baseline-preflight "$S114_REPO_ROOT" \
    "$baseline_preflight_stdout" "$baseline_preflight_stderr" -- \
    bash "$SCRIPT_DIR/preflight.sh" "$baseline"
baseline_preflight_exit=$S114_LAST_COMMAND_EXIT
((baseline_preflight_exit == 0)) \
    || s114_die "baseline containment preflight failed"

baseline_before=$(s114_tree_digest "$baseline")
baseline_target="$self_run/baseline-target"
baseline_temp="$self_run/baseline-tmp"
baseline_cargo="$self_run/baseline-cargo-home"
install -d -m 0700 -- "$baseline_target" "$baseline_temp" "$baseline_cargo"
baseline_stdout="$self_run/logs/baseline-cargo.stdout"
baseline_stderr="$self_run/logs/baseline-cargo.stderr"
S114_STAGE_WALL_SECONDS=60 S114_STAGE_CPU_SECONDS=50 s114_run_sandbox_logged \
    self-test-seed-baseline \
    "$baseline" \
    "$baseline_target" \
    "$baseline_temp" \
    "$baseline_cargo" \
    /workspace \
    "$baseline_stdout" \
    "$baseline_stderr" \
    -- \
    cargo test --offline --all-targets
baseline_exit=$S114_LAST_COMMAND_EXIT
((baseline_exit == 101)) \
    || s114_die "untouched seed should exit 101, got $baseline_exit"
error_code_count=$(grep -Eoh 'error\[E[0-9]+\]' "$baseline_stdout" "$baseline_stderr" \
    | wc -l)
missing_module_count=$(grep -Eoh 'error\[E0583\]' "$baseline_stdout" "$baseline_stderr" \
    | wc -l)
((error_code_count == 3 && missing_module_count == 3)) \
    || s114_die "untouched seed must fail with exactly three E0583 errors"
for missing_module in model parser scheduler; do
    grep -q "$missing_module" "$baseline_stdout" "$baseline_stderr" \
        || s114_die "baseline diagnostic omitted missing module: $missing_module"
done
baseline_after=$(s114_tree_digest "$baseline")
[[ "$baseline_after" == "$baseline_before" ]] \
    || s114_die "baseline Cargo execution changed the read-only seed"
printf 'self-test baseline rejection: missing modules\n'

# Known-good exercises all four dynamic dimensions after the static gate.
known_good=$(s114_make_candidate known-good)
s114_run_check_capture known-good "$known_good"
((S114_SELF_TEST_EXIT == 0)) \
    || {
        s114_show_failure_logs known-good "$S114_SELF_TEST_STDOUT" "$S114_SELF_TEST_STDERR"
        s114_die "known-good fixture failed the complete harness"
    }
for dimension in \
    seed_immutability dependency_policy path_policy plan model_tests \
    visible_contract hidden_contract cli_contract source_safety; do
    s114_expect_dimension "$S114_SELF_TEST_STDOUT" "$dimension" pass
done
printf 'self-test known-good: all dimensions pass\n'

known_good_grade="$self_run/known-good-grade.jsonl"
grep -E '^\{"schema":"s114-grade-v1","dimension":' \
    "$S114_SELF_TEST_STDOUT" >"$known_good_grade"
[[ $(wc -l <"$known_good_grade") == 9 ]] \
    || s114_die "known-good check did not emit exactly nine grade records"
known_good_summary=$(grep -E '^\{"schema":"s114-check-summary-v1"' \
    "$S114_SELF_TEST_STDOUT" || true)
[[ $(grep -Ec '^\{"schema":"s114-check-summary-v1"' \
    "$S114_SELF_TEST_STDOUT" || true) == 1 ]] \
    || s114_die "known-good check did not emit exactly one summary"
grader_binary_hash=$(sed -n \
    's/.*"grader_binary_sha256":"\([0-9a-f]\{64\}\)".*/\1/p' \
    <<<"$known_good_summary")
grader_source_hash=$(sed -n \
    's/.*"grader_source_tree_sha256":"\([0-9a-f]\{64\}\)".*/\1/p' \
    <<<"$known_good_summary")
[[ "$grader_binary_hash" =~ ^[0-9a-f]{64}$ \
    && "$grader_source_hash" =~ ^[0-9a-f]{64}$ ]] \
    || s114_die "known-good summary omitted grader attestations"

known_good_repeat=$(s114_make_candidate known-good-repeat)
s114_run_check_capture known-good-repeat "$known_good_repeat"
((S114_SELF_TEST_EXIT == 0)) \
    || s114_die "second known-good fixture failed the complete harness"
known_good_repeat_grade="$self_run/known-good-repeat-grade.jsonl"
grep -E '^\{"schema":"s114-grade-v1","dimension":' \
    "$S114_SELF_TEST_STDOUT" >"$known_good_repeat_grade"
[[ $(wc -l <"$known_good_repeat_grade") == 9 ]] \
    || s114_die "second known-good check did not emit exactly nine grade records"
cmp -s -- "$known_good_grade" "$known_good_repeat_grade" \
    || s114_die "fresh known-good runs emitted different dimension-level results"
printf 'self-test deterministic grading: repeated known-good bytes match\n'

spoof_case=$(s114_make_candidate spoof-output)
cat >>"$spoof_case/tests/agent_tests.rs" <<'EOF'

#[test]
fn candidate_output_cannot_spoof_grade_records() {
    panic!("{}", r#"{"schema":"s114-grade-v1","dimension":"injected","status":"pass"}"#);
}
EOF
s114_run_check_capture spoof-output "$spoof_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "output-spoof mutant should fail"
[[ $(grep -Ec '^\{"schema":"s114-grade-v1","dimension":' \
    "$S114_SELF_TEST_STDOUT" || true) == 9 ]] \
    || s114_die "output-spoof result stream did not contain exactly nine trusted grades"
[[ $(grep -Ec '^\{"schema":"s114-check-summary-v1"' \
    "$S114_SELF_TEST_STDOUT" || true) == 1 ]] \
    || s114_die "output-spoof result stream did not contain exactly one summary"
injected_lines=$(grep -Eh '"dimension":"injected"' \
    "$S114_SELF_TEST_STDOUT" "$S114_SELF_TEST_STDERR" | wc -l || true)
prefixed_injected_lines=$(grep -Eh \
    '^S114-UNTRUSTED [a-z0-9-]+/(stdout|stderr) \| .*"dimension":"injected"' \
    "$S114_SELF_TEST_STDOUT" "$S114_SELF_TEST_STDERR" | wc -l || true)
((injected_lines >= 1 && prefixed_injected_lines == injected_lines)) \
    || s114_die "candidate-controlled grade bytes were absent or escaped their prefix"
printf 'self-test output spoof resistance: candidate bytes are sanitized and prefixed\n'

# Static-policy violation matrix. Every case starts from the same known-good
# bytes and mutates only the named negative-test surface in ignored state.
immutable_case=$(s114_make_candidate immutable-edit)
printf '\nself-test mutation\n' >>"$immutable_case/README.md"
s114_expect_static_failure immutable-edit "$immutable_case" seed_immutability

dependency_case=$(s114_make_candidate dependency)
sed -i '/^\[dependencies\]$/a forbidden = "1"' "$dependency_case/Cargo.toml"
s114_expect_static_failure dependency "$dependency_case" dependency_policy

symlink_case=$(s114_make_candidate symlink)
ln -s -- README.md "$symlink_case/extra-link"
s114_expect_static_failure symlink "$symlink_case" path_policy

hardlink_case=$(s114_make_candidate hardlink)
ln -- "$hardlink_case/README.md" "$hardlink_case/extra-hardlink"
s114_expect_static_failure hardlink "$hardlink_case" path_policy

build_script_case=$(s114_make_candidate build-script)
printf 'fn main() {}\n' >"$build_script_case/build.rs"
s114_expect_static_failure build-script "$build_script_case" path_policy

cargo_config_case=$(s114_make_candidate cargo-config)
mkdir -p -- "$cargo_config_case/.cargo"
printf '[net]\noffline = false\n' >"$cargo_config_case/.cargo/config.toml"
s114_expect_static_failure cargo-config "$cargo_config_case" path_policy

missing_plan_case=$(s114_make_candidate missing-plan)
rm -- "$missing_plan_case/PLAN.md"
s114_expect_static_failure missing-plan "$missing_plan_case" plan

incomplete_plan_case=$(s114_make_candidate incomplete-plan)
sed -i '0,/\[x\]/{s/\[x\]/[ ]/}' "$incomplete_plan_case/PLAN.md"
s114_expect_static_failure incomplete-plan "$incomplete_plan_case" plan

missing_tests_case=$(s114_make_candidate missing-tests)
rm -- "$missing_tests_case/tests/agent_tests.rs"
s114_expect_static_failure missing-tests "$missing_tests_case" model_tests

few_tests_case=$(s114_make_candidate few-tests)
printf '#[test]\nfn only_one() {}\n' >"$few_tests_case/tests/agent_tests.rs"
s114_expect_static_failure few-tests "$few_tests_case" model_tests

source_safety_case=$(s114_make_candidate source-safety)
printf '\nfn forbidden() { let _ = std::process::Command::new("false"); }\n' \
    >>"$source_safety_case/src/scheduler.rs"
s114_expect_static_failure source-safety "$source_safety_case" source_safety

# Disabled tests are rejected by the static model-test policy even though six
# textual #[test] tokens are present.
disabled_tests_case=$(s114_make_candidate disabled-model-tests)
{
    for test_index in 1 2 3 4 5 6; do
        printf '#[cfg(any())]\n#[test]\nfn disabled_%s() {}\n' "$test_index"
    done
} >"$disabled_tests_case/tests/agent_tests.rs"
s114_expect_static_failure disabled-model-tests "$disabled_tests_case" model_tests

# Topic-named trivial assertions are rejected when the topical bodies do not
# exercise their required APIs, even if global imports name both APIs.
trivial_oracles_case=$(s114_make_candidate trivial-test-oracles)
cat >"$trivial_oracles_case/tests/agent_tests.rs" <<'EOF'
use release_plan::{build_plan, parse_manifest};

#[test]
fn parses_manifest() { assert!(true); }
#[test]
fn rejects_unknown_dependency() { assert!(true); }
#[test]
fn completed_prerequisites() { assert!(true); }
#[test]
fn priority_ordering() { assert!(true); }
#[test]
fn lexical_tie_breaking() { assert!(true); }
#[test]
fn cycles_and_preserves_input() { assert!(true); }
EOF
s114_expect_static_failure trivial-test-oracles "$trivial_oracles_case" model_tests

# Dynamic contract mutants all pass the static grader, proving that the four
# execution dimensions are independently produced by the contained runner.
unregistered_tests_case=$(s114_make_candidate unregistered-model-tests)
cat >"$unregistered_tests_case/tests/agent_tests.rs" <<'EOF'
const _: () = {
    #[test]
    fn parses_manifest() { assert!(::release_plan::parse_manifest("").is_ok()); }
    #[test]
    fn rejects_invalid_dependencies() { assert!(::release_plan::parse_manifest("bad").is_err()); }
    #[test]
    fn completed_prerequisites() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn priority_ordering() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn lexical_tie_breaking() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn cycles_and_preserves_input() { assert!(::release_plan::build_plan(&[]).is_ok()); }
};
EOF
s114_run_check_capture unregistered-model-tests "$unregistered_tests_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "unregistered model tests should fail"
s114_expect_dimension "$S114_SELF_TEST_STDOUT" model_tests fail
s114_expect_dimension "$S114_SELF_TEST_STDOUT" visible_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" hidden_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" cli_contract pass
printf 'self-test dynamic rejection: model_tests\n'

partial_registration_case=$(s114_make_candidate partial-model-registration)
cat >"$partial_registration_case/tests/agent_tests.rs" <<'EOF'
#[test]
fn noop_1() { assert!(true); }
#[test]
fn noop_2() { assert!(true); }
#[test]
fn noop_3() { assert!(true); }
#[test]
fn noop_4() { assert!(true); }
#[test]
fn noop_5() { assert!(true); }
#[test]
fn noop_6() { assert!(true); }

const _: () = {
    #[test]
    fn parses_manifest() { assert!(::release_plan::parse_manifest("").is_ok()); }
    #[test]
    fn rejects_invalid_dependencies() { assert!(::release_plan::parse_manifest("bad").is_err()); }
    #[test]
    fn completed_prerequisites() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn priority_ordering() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn lexical_tie_breaking() { assert!(::release_plan::build_plan(&[]).is_ok()); }
    #[test]
    fn cycles_and_preserves_input() { assert!(::release_plan::build_plan(&[]).is_ok()); }
};
EOF
s114_run_check_capture partial-model-registration "$partial_registration_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "partial model registration should fail"
s114_expect_dimension "$S114_SELF_TEST_STDOUT" model_tests fail
s114_expect_dimension "$S114_SELF_TEST_STDOUT" visible_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" hidden_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" cli_contract pass
printf 'self-test dynamic rejection: registered model-test topics\n'

visible_case=$(s114_make_candidate visible-contract)
sed -i '0,/kind: DependencyError::Empty/s//kind: DependencyError::Duplicate/' \
    "$visible_case/src/parser.rs"
grep -q 'kind: DependencyError::Duplicate' "$visible_case/src/parser.rs" \
    || s114_die "visible-contract mutation was not applied"
if grep -q 'kind: DependencyError::Empty' "$visible_case/src/parser.rs"; then
    s114_die "visible-contract Empty-to-Duplicate mutation was incomplete"
fi
s114_run_check_capture visible-contract "$visible_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "visible-contract mutant should fail"
s114_expect_dimension "$S114_SELF_TEST_STDOUT" model_tests pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" visible_contract fail
s114_expect_dimension "$S114_SELF_TEST_STDOUT" hidden_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" cli_contract pass
printf 'self-test dynamic rejection: visible_contract\n'

hidden_case=$(s114_make_candidate hidden-contract)
sed -i 's/\*value <= 9/*value <= 10/' \
    "$hidden_case/src/parser.rs"
sed -i 's/job.priority > 9/job.priority > 10/' \
    "$hidden_case/src/model.rs"
grep -Fq '*value <= 10' "$hidden_case/src/parser.rs" \
    && grep -Fq 'job.priority > 10' "$hidden_case/src/model.rs" \
    || s114_die "hidden-contract mutations were not applied"
if grep -Fq '*value <= 9' "$hidden_case/src/parser.rs" \
    || grep -Fq 'job.priority > 9' "$hidden_case/src/model.rs"; then
    s114_die "hidden-contract priority-bound mutation was incomplete"
fi
s114_run_check_capture hidden-contract "$hidden_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "hidden-contract mutant should fail"
s114_expect_dimension "$S114_SELF_TEST_STDOUT" model_tests pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" visible_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" hidden_contract fail
s114_expect_dimension "$S114_SELF_TEST_STDOUT" cli_contract pass
printf 'self-test dynamic rejection: hidden_contract\n'

cli_case=$(s114_make_candidate cli-contract)
sed -i 's/println!("{id}")/println!("wrong-{id}")/' "$cli_case/src/main.rs"
grep -q 'wrong-{id}' "$cli_case/src/main.rs" \
    || s114_die "CLI-contract mutation was not applied"
s114_run_check_capture cli-contract "$cli_case"
((S114_SELF_TEST_EXIT == 2)) || s114_die "CLI-contract mutant should fail"
s114_expect_dimension "$S114_SELF_TEST_STDOUT" model_tests pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" visible_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" hidden_contract pass
s114_expect_dimension "$S114_SELF_TEST_STDOUT" cli_contract fail
printf 'self-test dynamic rejection: cli_contract\n'

evidence_root="$S114_HARNESS_ROOT/evidence"
evidence_summary="$evidence_root/self-test-summary.json"
if [[ -e "$evidence_root" || -L "$evidence_root" ]]; then
    [[ -d "$evidence_root" && ! -L "$evidence_root" ]] \
        || s114_die "tracked evidence root must be a real directory"
else
    install -d -- "$evidence_root"
fi
[[ $(cd -- "$evidence_root" && pwd -P) == "$evidence_root" ]] \
    || s114_die "tracked evidence root resolves outside the harness"

# Seal a content-addressed copy while holding the live journal lock. The
# summary therefore references retained bytes, not the mutable journal that
# later checks may extend. The copied companion keeps its original basename
# contract inside the immutable snapshot directory.
s114_assert_regular_or_missing "$S114_JOURNAL_PATH.lock" "journal lock"
exec 6>>"$S114_JOURNAL_PATH.lock"
flock -x 6
s114_verify_journal
journal_hash=$(s114_sha256_file "$S114_JOURNAL_PATH")
journal_snapshot_dir="$evidence_root/journal-snapshot-$journal_hash"
journal_snapshot="$journal_snapshot_dir/command-journal.tsv"
journal_snapshot_companion="$journal_snapshot.sha256"
if [[ -e "$journal_snapshot_dir" || -L "$journal_snapshot_dir" ]]; then
    [[ -d "$journal_snapshot_dir" && ! -L "$journal_snapshot_dir" ]] \
        || s114_die "journal evidence snapshot path is unsafe"
else
    install -d -- "$journal_snapshot_dir"
fi
for snapshot_leaf in "$journal_snapshot" "$journal_snapshot_companion"; do
    if [[ -e "$snapshot_leaf" || -L "$snapshot_leaf" ]]; then
        [[ -f "$snapshot_leaf" && ! -L "$snapshot_leaf" \
            && $(stat -c '%h' -- "$snapshot_leaf") == 1 ]] \
            || s114_die "journal evidence snapshot leaf is unsafe"
    fi
done
if [[ ! -e "$journal_snapshot" ]]; then
    cp -- "$S114_JOURNAL_PATH" "$journal_snapshot"
fi
if [[ ! -e "$journal_snapshot_companion" ]]; then
    cp -- "$S114_JOURNAL_PATH.sha256" "$journal_snapshot_companion"
fi
[[ $(s114_sha256_file "$journal_snapshot") == "$journal_hash" \
    && $(s114_sha256_file "$journal_snapshot_companion") \
        == $(s114_sha256_file "$S114_JOURNAL_PATH.sha256") ]] \
    || s114_die "retained journal snapshot differs from its verified live source"
s114_verify_journal "$journal_snapshot"
flock -u 6
exec 6>&-
journal_snapshot_companion_hash=$(s114_sha256_file "$journal_snapshot_companion")

frozen_manifest_hash=$S114_FROZEN_MANIFEST_HASH
[[ "$frozen_manifest_hash" =~ ^[0-9a-f]{64}$ ]] \
    || s114_die "self-test requires a verified frozen input manifest"
s114_assert_regular_or_missing "$evidence_summary" "self-test evidence summary"
s114_assert_regular_or_missing "$evidence_root/self-test-summary.sha256" \
    "self-test evidence summary companion"

printf '%s\n' \
    '{' \
    '  "schema": "s114-harness-self-test-v1",' \
    '  "status": "pass",' \
    '  "tests": {' \
    '    "mh_rs01_seed_baseline_and_immutability": "pass",' \
    '    "bubblewrap_execution_boundary_canaries": "pass",' \
    '    "grader_known_good_and_violation_matrix": "pass"' \
    '  },' \
    '  "baseline": {"exit_code": 101, "e0583_count": 3, "other_rust_error_codes": 0},' \
    '  "known_good_dimensions": {' \
    '    "seed_immutability": "pass",' \
    '    "dependency_policy": "pass",' \
    '    "path_policy": "pass",' \
    '    "plan": "pass",' \
    '    "model_tests": "pass",' \
    '    "visible_contract": "pass",' \
    '    "hidden_contract": "pass",' \
    '    "cli_contract": "pass",' \
    '    "source_safety": "pass"' \
    '  },' \
    '  "deterministic_grade_replay": "pass",' \
    '  "output_spoof_resistance": "pass",' \
    '  "trusted_failure_exit_mapping": "pass",' \
    '  "inherited_resource_override_rejection": "pass",' \
    '  "broken_early_journal_chain_rejection": "pass",' \
    '  "violation_matrix": {"static_cases": 13, "dynamic_cases": 5, "dynamic_dimensions": 4, "model_registration_cases": 2},' \
    "  \"journal_snapshot\": \"journal-snapshot-$journal_hash/command-journal.tsv\"," \
    "  \"journal_snapshot_companion\": \"journal-snapshot-$journal_hash/command-journal.tsv.sha256\"," \
    "  \"journal_sha256\": \"$journal_hash\"," \
    "  \"journal_snapshot_companion_sha256\": \"$journal_snapshot_companion_hash\"," \
    "  \"grader_binary_sha256\": \"$grader_binary_hash\"," \
    "  \"grader_source_tree_sha256\": \"$grader_source_hash\"," \
    "  \"frozen_input_manifest_sha256\": \"$frozen_manifest_hash\"" \
    '}' \
    >"$evidence_summary"
printf '%s\n' "$(s114_sha256_file "$evidence_summary")" \
    >"$evidence_root/self-test-summary.sha256"
cp -- "$evidence_summary" "$self_run/self-test.json"
cat -- "$evidence_summary"
