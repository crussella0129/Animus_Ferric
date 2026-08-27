#!/usr/bin/env bash
# Prove the WSL Bubblewrap boundary before any candidate code is executed.

set -Eeuo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
# shellcheck source=common.sh
source "$SCRIPT_DIR/common.sh"

s114_initialize
s114_prepare_runtime

candidate=$(s114_resolve_directory "${1:-$PWD}")
s114_assert_state_candidate_disjoint "$candidate"
s114_assert_candidate_tree "$candidate"
[[ ! -e "$candidate/.s114-source-write-canary" ]] \
    || s114_die "reserved source-write canary already exists in candidate"

run_dir=$(s114_allocate_run_dir preflight)
before_hash=$(s114_tree_digest "$candidate")

containment_stdout="$run_dir/logs/containment.stdout"
containment_stderr="$run_dir/logs/containment.stderr"
S114_STAGE_WALL_SECONDS=15 S114_STAGE_CPU_SECONDS=10 s114_run_sandbox_logged \
    preflight-containment \
    "$candidate" \
    "$run_dir/target" \
    "$run_dir/tmp" \
    "$run_dir/cargo-home" \
    /workspace \
    "$containment_stdout" \
    "$containment_stderr" \
    -- \
    /bin/bash -c '
        set -Eeuo pipefail
        [[ ! -e /mnt/c && ! -e /root && ! -e /home ]]
        [[ ! -e /opt/cargo && ! -e /opt/rustup ]]
        mapfile -t opt_entries < <(
            /usr/bin/find /opt -mindepth 1 -maxdepth 1 -printf "%f\n" | /usr/bin/sort
        )
        [[ ${#opt_entries[@]} == 1 && ${opt_entries[0]} == toolchain ]]
        mapfile -t tool_entries < <(
            /usr/bin/find /opt/toolchain -mindepth 1 -maxdepth 1 -printf "%f\n" \
                | /usr/bin/sort
        )
        [[ ${#tool_entries[@]} == 2 ]]
        [[ ${tool_entries[0]} == bin && ${tool_entries[1]} == lib ]]
        mapfile -t tool_bins < <(
            /usr/bin/find /opt/toolchain/bin -mindepth 1 -maxdepth 1 -printf "%f\n" \
                | /usr/bin/sort
        )
        [[ ${#tool_bins[@]} == 3 ]]
        [[ ${tool_bins[0]} == cargo && ${tool_bins[1]} == rustc \
            && ${tool_bins[2]} == rustdoc ]]
        [[ -x /opt/toolchain/bin/cargo && -x /opt/toolchain/bin/rustc \
            && -x /opt/toolchain/bin/rustdoc && -d /opt/toolchain/lib ]]
        [[ $(command -v cargo) == /opt/toolchain/bin/cargo ]]
        [[ $(command -v rustc) == /opt/toolchain/bin/rustc ]]
        cargo --version | /usr/bin/grep -Eq "^cargo 1\\.96\\."
        rustc --version | /usr/bin/grep -Eq "^rustc 1\\.96\\."
        interface_count=0
        while IFS=: read -r interface_name _; do
            interface_name=${interface_name//[[:space:]]/}
            [[ "$interface_name" == lo ]] || exit 43
            interface_count=$((interface_count + 1))
        done < <(/usr/bin/tail -n +3 /proc/net/dev)
        [[ "$interface_count" == 1 ]]
        for read_only_canary in \
            /.s114-root-write \
            /etc/.s114-write \
            /opt/.s114-write \
            /homeless/.s114-write \
            /cargo-home; do
            if : > "$read_only_canary" 2>/dev/null; then
                exit 44
            fi
        done
        if : > /workspace/.s114-source-write-canary 2>/dev/null; then
            exit 41
        fi
        if /usr/bin/timeout 2s /bin/bash -c "exec 3<>/dev/tcp/198.51.100.1/9" \
            >/dev/null 2>&1; then
            exit 42
        fi
        printf "target-isolated\n" > /target/preflight-target
        printf "tmp-isolated\n" > /tmp/preflight-tmp
    '
containment_exit=$S114_LAST_COMMAND_EXIT
if ((containment_exit != 0)); then
    s114_emit_stage_result bubblewrap_boundary fail "$containment_exit" \
        "$containment_stdout" "$containment_stderr"
    s114_show_failure_logs preflight-containment "$containment_stdout" "$containment_stderr"
    s114_die "Bubblewrap containment canary failed; candidate execution remains blocked"
fi
[[ $(<"$run_dir/target/preflight-target") == 'target-isolated' ]] \
    || s114_die "isolated target write canary was not retained"
[[ $(<"$run_dir/tmp/preflight-tmp") == 'tmp-isolated' ]] \
    || s114_die "isolated temporary write canary was not retained"
[[ ! -e "$candidate/.s114-source-write-canary" ]] \
    || s114_die "read-only source canary changed the candidate"
s114_emit_stage_result bubblewrap_boundary pass 0 \
    "$containment_stdout" "$containment_stderr"

timeout_stdout="$run_dir/logs/timeout.stdout"
timeout_stderr="$run_dir/logs/timeout.stderr"
S114_STAGE_WALL_SECONDS=1 S114_STAGE_CPU_SECONDS=10 \
    s114_run_sandbox_logged \
        preflight-timeout \
        "$candidate" \
        "$run_dir/target" \
        "$run_dir/tmp" \
        "$run_dir/cargo-home" \
        /workspace \
        "$timeout_stdout" \
        "$timeout_stderr" \
        -- \
        /bin/bash -c 'sleep 30'
timeout_exit=$S114_LAST_COMMAND_EXIT
case "$timeout_exit" in
    124 | 137) ;;
    *)
        s114_emit_stage_result wall_timeout fail "$timeout_exit" \
            "$timeout_stdout" "$timeout_stderr"
        s114_show_failure_logs preflight-timeout "$timeout_stdout" "$timeout_stderr"
        s114_die "wall-time canary was not terminated by the configured bound"
        ;;
esac
s114_emit_stage_result wall_timeout pass "$timeout_exit" \
    "$timeout_stdout" "$timeout_stderr"

resource_stdout="$run_dir/logs/resource.stdout"
resource_stderr="$run_dir/logs/resource.stderr"
S114_STAGE_WALL_SECONDS=10 S114_STAGE_CPU_SECONDS=5 S114_STAGE_FILE_BYTES=1048576 \
    s114_run_sandbox_logged \
        preflight-file-limit \
        "$candidate" \
        "$run_dir/target" \
        "$run_dir/tmp" \
        "$run_dir/cargo-home" \
        /workspace \
        "$resource_stdout" \
        "$resource_stderr" \
        -- \
        /usr/bin/dd if=/dev/zero of=/target/resource-limit-canary.bin \
            bs=65536 count=32 status=none
resource_exit=$S114_LAST_COMMAND_EXIT
resource_size=0
if [[ -f "$run_dir/target/resource-limit-canary.bin" ]]; then
    resource_size=$(stat -c '%s' -- "$run_dir/target/resource-limit-canary.bin")
fi
if ((resource_exit == 0 || resource_size > 1048576)); then
    s114_emit_stage_result resource_limit fail "$resource_exit" \
        "$resource_stdout" "$resource_stderr"
    s114_show_failure_logs preflight-file-limit "$resource_stdout" "$resource_stderr"
    s114_die "prlimit file-size canary escaped its configured bound"
fi
s114_emit_stage_result resource_limit pass "$resource_exit" \
    "$resource_stdout" "$resource_stderr"

after_hash=$(s114_tree_digest "$candidate")
[[ "$after_hash" == "$before_hash" ]] \
    || s114_die "candidate tree changed during containment preflight"

printf '{"schema":"s114-preflight-v1","status":"pass","candidate_before_sha256":"%s","candidate_after_sha256":"%s","journal_sha256":"%s"}\n' \
    "$before_hash" \
    "$after_hash" \
    "$(s114_sha256_file "$S114_JOURNAL_PATH")" \
    >"$run_dir/preflight.json"
cat -- "$run_dir/preflight.json"
