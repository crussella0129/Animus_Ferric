#!/usr/bin/env bash
# Build the operator-owned static grader offline into ignored experiment state.

set -Eeuo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

s114_initialize
s114_prepare_runtime

grader_root="$S114_HARNESS_ROOT/grader"
grader_manifest="$grader_root/Cargo.toml"
grader_lock="$grader_root/Cargo.lock"

[[ -f "$grader_manifest" ]] || s114_die "grader manifest is missing: $grader_manifest"
[[ -f "$grader_lock" ]] || s114_die "grader lockfile is missing: $grader_lock"
[[ ! -L "$grader_root" ]] || s114_die "grader root may not be a symbolic link"
[[ ! -e "$grader_root/target" ]] \
    || s114_die "grader/target residue is forbidden; trusted builds must use ignored experiment state"
grader_offending=$(find -P "$grader_root" -mindepth 1 \
    ! -type d ! -type f -print -quit)
[[ -z "$grader_offending" ]] \
    || s114_die "grader source contains a symlink or special filesystem object"

build_run=$(s114_allocate_run_dir grader-build)
build_target="$build_run/target"
build_home="$build_run/home"
build_cargo_home="$build_run/cargo-home"
build_tmp="$build_run/tmp"
grader_binary="$build_target/release/mh-rs01-grader"
install -d -m 0700 -- "$build_target" "$build_home" "$build_cargo_home" "$build_tmp"
grader_before=$(s114_tree_digest "$grader_root")
build_stdout="$build_run/logs/grader-build.stdout"
build_stderr="$build_run/logs/grader-build.stderr"

s114_run_logged grader-build "$S114_REPO_ROOT" "$build_stdout" "$build_stderr" -- \
    timeout --foreground --signal=TERM --kill-after=5s 60s \
    prlimit --cpu=50:50 --as=8589934592:8589934592 \
        --fsize=1073741824:1073741824 --nproc=128:128 --nofile=512:512 \
        --core=0:0 -- \
    env -i \
        HOME="$build_home" \
        PATH="$(dirname -- "$S114_HOST_CARGO"):/usr/bin:/bin" \
        LANG=C LC_ALL=C TZ=UTC \
        TMPDIR="$build_tmp" \
        CARGO_HOME="$build_cargo_home" \
        CARGO_NET_OFFLINE=true \
        CARGO_INCREMENTAL=0 \
        RUSTC="$S114_HOST_RUSTC" \
        RUSTDOC="$S114_HOST_RUSTDOC" \
        SOURCE_DATE_EPOCH=0 \
    "$S114_HOST_CARGO" build --offline --locked --release \
        --manifest-path "$grader_manifest" \
        --target-dir "$build_target"
build_exit=$S114_LAST_COMMAND_EXIT
if ((build_exit != 0)); then
    s114_emit_stage_result grader_build fail "$build_exit" "$build_stdout" "$build_stderr"
    s114_show_failure_logs grader-build "$build_stdout" "$build_stderr"
    s114_die "trusted grader did not build; candidate execution remains blocked"
fi

grader_after=$(s114_tree_digest "$grader_root")
[[ "$grader_after" == "$grader_before" ]] \
    || s114_die "offline grader build changed its tracked source tree"
[[ -f "$grader_binary" && -x "$grader_binary" && ! -L "$grader_binary" ]] \
    || s114_die "expected grader binary was not produced: $grader_binary"
s114_assert_elf_executable "$grader_binary"

grader_hash=$(s114_sha256_file "$grader_binary")
printf '%s  %s\n' "$grader_hash" "mh-rs01-grader" \
    >"$build_run/grader-binary.sha256"
printf '{"schema":"s114-grader-build-v1","status":"pass","source_tree_sha256":"%s","binary_sha256":"%s"}\n' \
    "$grader_before" "$grader_hash" \
    >"$build_run/grader-build.json"
s114_emit_stage_result grader_build pass 0 "$build_stdout" "$build_stderr"
printf '%s\n' "$grader_binary"
