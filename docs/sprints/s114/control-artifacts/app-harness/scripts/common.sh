#!/usr/bin/env bash
# Shared, fail-closed support for the Sprint 114 MH-RS01 harness.

set -Eeuo pipefail
IFS=$'\n\t'

S114_SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
S114_HARNESS_ROOT="$(cd -- "$S114_SCRIPT_DIR/.." && pwd -P)"
S114_REPO_ROOT="$(cd -- "$S114_SCRIPT_DIR/../../../../../.." && pwd -P)"
readonly S114_SCRIPT_DIR S114_HARNESS_ROOT S114_REPO_ROOT
readonly S114_EXPERIMENT_ROOT="$S114_REPO_ROOT/target/s114-experiment"
readonly S114_DEFAULT_STATE_ROOT="$S114_EXPERIMENT_ROOT/app-harness"
readonly S114_JOURNAL_SCHEMA="s114-command-journal-v1"
readonly S114_ZERO_HASH="0000000000000000000000000000000000000000000000000000000000000000"
readonly S114_CANDIDATE_DIAGNOSTIC_PAYLOAD_BYTES=160
readonly S114_CANDIDATE_DIAGNOSTIC_SAMPLE_BYTES=4096

s114_unhandled_error() {
    local native_exit=$1
    local source_line=$2
    trap - ERR
    printf 's114-harness: unhandled trusted infrastructure failure at line %s (native exit %s)\n' \
        "$source_line" "$native_exit" >&2
    exit 70
}

trap 's114_unhandled_error "$?" "$LINENO"' ERR

s114_die() {
    printf 's114-harness: %s\n' "$*" >&2
    exit 70
}

s114_require_command() {
    local command_name=$1
    command -v -- "$command_name" >/dev/null 2>&1 \
        || s114_die "required command is unavailable: $command_name"
}

s114_require_wsl() {
    local kernel_release
    kernel_release=$(uname -r 2>/dev/null) || s114_die "cannot identify the running kernel"
    case "$kernel_release" in
        *[Mm]icrosoft* | *WSL*) ;;
        *) s114_die "candidate execution requires WSL; native or host-shell fallback is forbidden" ;;
    esac
}

s114_validate_uint() {
    local name=$1
    local value=$2
    case "$value" in
        '' | *[!0-9]*) s114_die "$name must be an unsigned integer" ;;
    esac
    ((value > 0)) || s114_die "$name must be greater than zero"
}

s114_resolve_directory() {
    local supplied=$1
    local converted

    if [[ ! -d "$supplied" ]] && command -v wslpath >/dev/null 2>&1; then
        converted=$(wslpath -u -- "$supplied" 2>/dev/null || true)
        if [[ -n "$converted" && -d "$converted" ]]; then
            supplied=$converted
        fi
    fi
    [[ -d "$supplied" ]] || s114_die "directory does not exist: $1"
    [[ ! -L "$supplied" ]] || s114_die "directory argument may not be a symbolic link: $1"
    (cd -- "$supplied" && pwd -P)
}

s114_assert_regular_or_missing() {
    local path=$1
    local label=$2
    if [[ -e "$path" || -L "$path" ]]; then
        [[ -f "$path" && ! -L "$path" ]] \
            || s114_die "$label must be a regular, non-symlink file: $path"
        [[ $(stat -c '%h' -- "$path") == 1 ]] \
            || s114_die "$label must not have additional hard links: $path"
    fi
}

s114_assert_state_candidate_disjoint() {
    local candidate=$1
    case "$candidate/" in
        "$S114_STATE_ROOT_RESOLVED/" | "$S114_STATE_ROOT_RESOLVED"/*)
            s114_die "candidate may not be the harness state root or reside beneath it"
            ;;
    esac
    case "$S114_STATE_ROOT_RESOLVED/" in
        "$candidate/" | "$candidate"/*)
            s114_die "candidate may not contain the harness state root"
            ;;
    esac
}

s114_init_state() {
    local target_root="$S114_REPO_ROOT/target"
    local target_real
    local experiment_real
    local state_real
    local state_entry
    local state_name

    [[ -z ${S114_STATE_ROOT+x} && -z ${S114_JOURNAL+x} ]] \
        || s114_die "state and journal path overrides are forbidden"
    umask 077

    if [[ -e "$target_root" || -L "$target_root" ]]; then
        [[ -d "$target_root" && ! -L "$target_root" ]] \
            || s114_die "repository target path must be a real directory"
    else
        install -d -m 0700 -- "$target_root"
    fi
    target_real=$(cd -- "$target_root" && pwd -P)
    [[ "$target_real" == "$target_root" ]] \
        || s114_die "repository target path resolves outside its fixed location"
    if [[ -e "$S114_EXPERIMENT_ROOT" || -L "$S114_EXPERIMENT_ROOT" ]]; then
        [[ -d "$S114_EXPERIMENT_ROOT" && ! -L "$S114_EXPERIMENT_ROOT" ]] \
            || s114_die "experiment root must be a real directory"
    else
        install -d -m 0700 -- "$S114_EXPERIMENT_ROOT"
    fi
    experiment_real=$(cd -- "$S114_EXPERIMENT_ROOT" && pwd -P)
    [[ "$experiment_real" == "$S114_EXPERIMENT_ROOT" ]] \
        || s114_die "experiment root resolves outside its fixed location"
    if [[ -e "$S114_DEFAULT_STATE_ROOT" || -L "$S114_DEFAULT_STATE_ROOT" ]]; then
        [[ -d "$S114_DEFAULT_STATE_ROOT" && ! -L "$S114_DEFAULT_STATE_ROOT" ]] \
            || s114_die "harness state root must be a real directory"
    else
        install -d -m 0700 -- "$S114_DEFAULT_STATE_ROOT"
    fi
    state_real=$(cd -- "$S114_DEFAULT_STATE_ROOT" && pwd -P)
    [[ "$state_real" == "$S114_DEFAULT_STATE_ROOT" ]] \
        || s114_die "harness state root resolves outside its fixed location"
    case "$state_real/" in
        "$experiment_real"/*) ;;
        *) s114_die "state root must remain below target/s114-experiment" ;;
    esac

    S114_STATE_ROOT_RESOLVED=$state_real
    S114_JOURNAL_PATH="$state_real/command-journal.tsv"
    while IFS= read -r -d '' state_entry; do
        state_name=${state_entry##*/}
        case "$state_name" in
            runs)
                [[ -d "$state_entry" && ! -L "$state_entry" ]] \
                    || s114_die "state runs root must be a real directory"
                ;;
            run-counter | run-counter.lock | command-journal.tsv | \
                command-journal.tsv.lock | command-journal.tsv.sha256)
                s114_assert_regular_or_missing "$state_entry" "state control leaf"
                ;;
            *) s114_die "unexpected entry at harness state root: $state_entry" ;;
        esac
    done < <(find -P "$state_real" -xdev -mindepth 1 -maxdepth 1 -print0)
    if [[ ! -e "$state_real/runs" ]]; then
        install -d -m 0700 -- "$state_real/runs"
    fi
    s114_assert_regular_or_missing "$S114_JOURNAL_PATH" journal
    s114_assert_regular_or_missing "$S114_JOURNAL_PATH.lock" "journal lock"
    s114_assert_regular_or_missing "$S114_JOURNAL_PATH.sha256" "journal companion"
    exec 7>>"$S114_JOURNAL_PATH.lock"
    flock -x 7
    if [[ -e "$S114_JOURNAL_PATH" ]]; then
        [[ -s "$S114_JOURNAL_PATH" ]] \
            || s114_die "existing journal is empty"
        s114_verify_journal_structure "$S114_JOURNAL_PATH"
        S114_JOURNAL_START_SEQUENCE=$(tail -n 1 -- "$S114_JOURNAL_PATH" \
            | awk -F '\t' '{print $2}')
        S114_JOURNAL_START_HASH=$(tail -n 1 -- "$S114_JOURNAL_PATH" \
            | awk -F '\t' '{print $12}')
    elif [[ -e "$S114_JOURNAL_PATH.sha256" ]]; then
        s114_die "journal companion exists without its journal"
    else
        S114_JOURNAL_START_SEQUENCE=0
        S114_JOURNAL_START_HASH=$S114_ZERO_HASH
    fi
    flock -u 7
    exec 7>&-
    export S114_STATE_ROOT_RESOLVED S114_JOURNAL_PATH \
        S114_JOURNAL_START_SEQUENCE S114_JOURNAL_START_HASH
}

s114_sha256_file() {
    sha256sum -- "$1" | awk '{print $1}'
}

s114_assert_elf_executable() {
    local binary=$1
    local magic
    [[ -f "$binary" && ! -L "$binary" ]] \
        || s114_die "expected executable is not a regular file: $binary"
    magic=$(od -An -tx1 -N4 -- "$binary" | tr -d '[:space:]')
    [[ "$magic" == 7f454c46 ]] \
        || s114_die "expected executable is not an ELF artifact: $binary"
}

s114_b64() {
    printf '%s' "$1" | base64 -w 0
}

s114_argv_b64() {
    local encoded=''
    local separator=''
    local argument
    for argument in "$@"; do
        encoded+="$separator$(s114_b64 "$argument")"
        separator=','
    done
    printf '%s' "$encoded"
}

s114_tree_digest() {
    local root=$1
    (
        cd -- "$root"
        LC_ALL=C find -P . -mindepth 1 \( -type d -o -type f \) -print0 \
            | LC_ALL=C sort -z \
            | while IFS= read -r -d '' entry; do
                if [[ -d "$entry" ]]; then
                    printf 'D\0%s\0' "${entry#./}"
                else
                    printf 'F\0%s\0%s\0%s\0' \
                        "${entry#./}" \
                        "$(stat -c '%s' -- "$entry")" \
                        "$(s114_sha256_file "$entry")"
                fi
            done
    ) | sha256sum | awk '{print $1}'
}

s114_assert_candidate_tree() {
    local candidate=$1
    local offending
    local file_count
    local total_bytes

    [[ -f "$candidate/Cargo.toml" ]] || s114_die "candidate has no Cargo.toml: $candidate"

    offending=$(find -P "$candidate" -xdev -mindepth 1 \
        ! -type d ! -type f -print -quit)
    [[ -z "$offending" ]] \
        || s114_die "candidate contains a symlink or special filesystem object: $offending"
    offending=$(find -P "$candidate" -xdev -type f -links +1 -print -quit)
    [[ -z "$offending" ]] \
        || s114_die "candidate contains a multiply linked regular file: $offending"

    while IFS= read -r -d '' offending; do
        case "${offending#$candidate/}" in
            *$'\n'* | *$'\r'* | *$'\t'*)
                s114_die "candidate path contains a control character"
                ;;
        esac
    done < <(find -P "$candidate" -xdev -mindepth 1 -print0)

    file_count=$(find -P "$candidate" -xdev -type f -printf '.' | wc -c)
    ((file_count <= 4096)) \
        || s114_die "candidate file-count safety limit exceeded: $file_count > 4096"
    total_bytes=$(find -P "$candidate" -xdev -type f -printf '%s\n' \
        | awk '{total += $1} END {print total + 0}')
    ((total_bytes <= 134217728)) \
        || s114_die "candidate byte-size safety limit exceeded: $total_bytes > 134217728"
}

s114_allocate_run_dir() {
    local purpose=$1
    local counter_file="$S114_STATE_ROOT_RESOLVED/run-counter"
    local counter_lock="$counter_file.lock"
    local counter
    local run_dir

    [[ -d "$S114_STATE_ROOT_RESOLVED/runs" \
        && ! -L "$S114_STATE_ROOT_RESOLVED/runs" ]] \
        || s114_die "state runs root is unavailable or unsafe"
    [[ "$purpose" =~ ^[a-z0-9][a-z0-9-]*$ ]] \
        || s114_die "run purpose is not a safe identifier: $purpose"
    s114_assert_regular_or_missing "$counter_file" "run counter"
    s114_assert_regular_or_missing "$counter_lock" "run counter lock"
    exec 8>>"$counter_lock"
    flock -x 8
    if [[ -f "$counter_file" ]]; then
        counter=$(<"$counter_file")
        [[ "$counter" == 0 || "$counter" =~ ^[1-9][0-9]*$ ]] \
            || s114_die "run counter is corrupt"
        [[ $(wc -l <"$counter_file") == 1 ]] \
            || s114_die "run counter must contain exactly one record"
        ((counter < 1000000000)) || s114_die "run counter safety limit exceeded"
    else
        counter=0
    fi
    counter=$((counter + 1))
    printf '%s\n' "$counter" >"$counter_file"
    flock -u 8
    exec 8>&-

    printf -v run_dir '%s/runs/%06d-%s' "$S114_STATE_ROOT_RESOLVED" "$counter" "$purpose"
    [[ ! -e "$run_dir" && ! -L "$run_dir" ]] \
        || s114_die "refusing to reuse an existing run directory: $run_dir"
    install -d -m 0700 -- "$run_dir" "$run_dir/logs" "$run_dir/target" \
        "$run_dir/model-tests-target" "$run_dir/visible-target" \
        "$run_dir/hidden-target" "$run_dir/cli-target" \
        "$run_dir/tmp" "$run_dir/cargo-home"
    printf '%s\n' "$run_dir"
}

s114_journal_append() {
    local stage=$1
    local working_directory=$2
    local exit_code=$3
    local stdout_file=$4
    local stderr_file=$5
    shift 5

    local journal_lock="$S114_JOURNAL_PATH.lock"
    local sequence=1
    local previous_hash=$S114_ZERO_HASH
    local last_record
    local payload
    local entry_hash
    local argv_encoded
    local stdout_hash
    local stderr_hash

    argv_encoded=$(s114_argv_b64 "$@")
    stdout_hash=$(s114_sha256_file "$stdout_file")
    stderr_hash=$(s114_sha256_file "$stderr_file")

    [[ "$(dirname -- "$S114_JOURNAL_PATH")" == "$S114_STATE_ROOT_RESOLVED" ]] \
        || s114_die "journal parent escaped the fixed harness state root"
    s114_assert_regular_or_missing "$S114_JOURNAL_PATH" journal
    s114_assert_regular_or_missing "$journal_lock" "journal lock"
    s114_assert_regular_or_missing "$S114_JOURNAL_PATH.sha256" "journal companion"
    exec 9>>"$journal_lock"
    flock -x 9
    if [[ -s "$S114_JOURNAL_PATH" ]]; then
        s114_verify_journal_tail "$S114_JOURNAL_PATH"
        last_record=$(tail -n 1 -- "$S114_JOURNAL_PATH")
        sequence=$(awk -F '\t' '{print $2 + 1}' <<<"$last_record")
        previous_hash=$(awk -F '\t' '{print $12}' <<<"$last_record")
        [[ "$previous_hash" =~ ^[0-9a-f]{64}$ ]] \
            || s114_die "journal tail has an invalid entry hash"
    else
        [[ ! -e "$S114_JOURNAL_PATH" ]] \
            || s114_die "existing journal is empty"
        [[ ! -e "$S114_JOURNAL_PATH.sha256" ]] \
            || s114_die "journal companion exists without its journal"
        printf '%s\n' \
            $'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256' \
            >"$S114_JOURNAL_PATH"
    fi

    printf -v payload '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' \
        "$S114_JOURNAL_SCHEMA" \
        "$sequence" \
        "$previous_hash" \
        "$(s114_b64 "$stage")" \
        "$(s114_b64 "$working_directory")" \
        "$argv_encoded" \
        "$exit_code" \
        "$(s114_b64 "$stdout_file")" \
        "$stdout_hash" \
        "$(s114_b64 "$stderr_file")" \
        "$stderr_hash"
    entry_hash=$(printf '%s' "$payload" | sha256sum | awk '{print $1}')
    printf '%s\t%s\n' "$payload" "$entry_hash" >>"$S114_JOURNAL_PATH"
    printf '%s  %s\n' \
        "$(s114_sha256_file "$S114_JOURNAL_PATH")" \
        "$(basename -- "$S114_JOURNAL_PATH")" \
        >"$S114_JOURNAL_PATH.sha256"
    flock -u 9
    exec 9>&-
}

s114_verify_journal_companion() {
    local journal=$1
    local companion_hash
    local companion_name
    local companion_extra

    [[ -f "$journal.sha256" && ! -L "$journal.sha256" ]] \
        || s114_die "journal SHA-256 companion is unavailable"
    [[ $(stat -c '%h' -- "$journal.sha256") == 1 ]] \
        || s114_die "journal SHA-256 companion has additional hard links"
    [[ $(wc -l <"$journal.sha256") == 1 ]] \
        || s114_die "journal SHA-256 companion must contain exactly one record"
    IFS=' ' read -r companion_hash companion_name companion_extra <"$journal.sha256"
    [[ -z ${companion_extra:-} && "$companion_hash" =~ ^[0-9a-f]{64}$ ]] \
        || s114_die "journal SHA-256 companion is malformed"
    [[ "$companion_hash" == "$(s114_sha256_file "$journal")" ]] \
        || s114_die "journal SHA-256 companion does not match"
    [[ "$companion_name" == "$(basename -- "$journal")" ]] \
        || s114_die "journal SHA-256 companion names the wrong artifact"
}

s114_verify_journal_tail() {
    local journal=$1
    local header=$'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256'
    local last_record previous_record
    local schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 exit_code
    local stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra
    local previous_schema previous_sequence previous_hash
    local payload computed

    [[ -s "$journal" && ! -L "$journal" ]] \
        || s114_die "journal is absent, empty, or unsafe: $journal"
    [[ $(stat -c '%h' -- "$journal") == 1 ]] \
        || s114_die "journal has additional hard links"
    [[ $(head -n 1 -- "$journal") == "$header" ]] \
        || s114_die "journal header does not match $S114_JOURNAL_SCHEMA"
    s114_verify_journal_companion "$journal"

    last_record=$(tail -n 1 -- "$journal")
    IFS=$'\t' read -r schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 \
        exit_code stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra \
        <<<"$last_record"
    [[ -z ${extra:-} && "$schema" == "$S114_JOURNAL_SCHEMA" ]] \
        || s114_die "journal tail has an invalid schema or field count"
    [[ "$sequence" =~ ^[1-9][0-9]*$ \
        && "$recorded_previous" =~ ^[0-9a-f]{64}$ \
        && "$exit_code" =~ ^[0-9]+$ \
        && "$stdout_hash" =~ ^[0-9a-f]{64}$ \
        && "$stderr_hash" =~ ^[0-9a-f]{64}$ \
        && "$entry_hash" =~ ^[0-9a-f]{64}$ ]] \
        || s114_die "journal tail has malformed scalar fields"
    printf -v payload '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' \
        "$schema" "$sequence" "$recorded_previous" "$stage_b64" "$cwd_b64" \
        "$argv_b64" "$exit_code" "$stdout_path_b64" "$stdout_hash" \
        "$stderr_path_b64" "$stderr_hash"
    computed=$(printf '%s' "$payload" | sha256sum | awk '{print $1}')
    [[ "$computed" == "$entry_hash" ]] \
        || s114_die "journal tail entry hash does not match"

    if [[ "$sequence" == 1 ]]; then
        [[ "$recorded_previous" == "$S114_ZERO_HASH" \
            && $(wc -l <"$journal") == 2 ]] \
            || s114_die "first journal record has invalid ancestry"
    else
        previous_record=$(tail -n 2 -- "$journal" | sed -n '1p')
        IFS=$'\t' read -r previous_schema previous_sequence _ _ _ _ _ _ _ _ _ previous_hash _ \
            <<<"$previous_record"
        [[ "$previous_schema" == "$S114_JOURNAL_SCHEMA" \
            && "$previous_sequence" =~ ^[1-9][0-9]*$ \
            && "$previous_hash" =~ ^[0-9a-f]{64}$ \
            && "$sequence" == $((previous_sequence + 1)) \
            && "$recorded_previous" == "$previous_hash" ]] \
            || s114_die "journal tail does not extend its preceding record"
    fi
}

s114_verify_journal_structure() {
    local journal=$1
    local expected_sequence=1
    local previous_hash=$S114_ZERO_HASH
    local line_number=0
    local header=$'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256'
    local schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 exit_code
    local stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra
    local payload computed

    [[ -s "$journal" && ! -L "$journal" ]] \
        || s114_die "journal is absent, empty, or unsafe: $journal"
    [[ $(stat -c '%h' -- "$journal") == 1 ]] \
        || s114_die "journal has additional hard links"
    [[ $(head -n 1 -- "$journal") == "$header" ]] \
        || s114_die "journal header does not match $S114_JOURNAL_SCHEMA"
    s114_verify_journal_companion "$journal"

    while IFS=$'\t' read -r schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 \
        exit_code stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra; do
        line_number=$((line_number + 1))
        [[ -z ${extra:-} && "$schema" == "$S114_JOURNAL_SCHEMA" \
            && "$sequence" == "$expected_sequence" \
            && "$recorded_previous" == "$previous_hash" \
            && "$exit_code" =~ ^[0-9]+$ \
            && "$stdout_hash" =~ ^[0-9a-f]{64}$ \
            && "$stderr_hash" =~ ^[0-9a-f]{64}$ \
            && "$entry_hash" =~ ^[0-9a-f]{64}$ ]] \
            || s114_die "journal structure breaks at record $line_number"
        printf -v payload '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' \
            "$schema" "$sequence" "$recorded_previous" "$stage_b64" "$cwd_b64" \
            "$argv_b64" "$exit_code" "$stdout_path_b64" "$stdout_hash" \
            "$stderr_path_b64" "$stderr_hash"
        computed=$(printf '%s' "$payload" | sha256sum | awk '{print $1}')
        [[ "$computed" == "$entry_hash" ]] \
            || s114_die "journal entry hash mismatch at record $line_number"
        previous_hash=$entry_hash
        expected_sequence=$((expected_sequence + 1))
    done < <(tail -n +2 -- "$journal")
    ((line_number > 0)) || s114_die "journal contains no command records"
}

s114_verify_journal_since() {
    local journal=${1:-$S114_JOURNAL_PATH}
    local start_sequence=${2:-$S114_JOURNAL_START_SEQUENCE}
    local start_hash=${3:-$S114_JOURNAL_START_HASH}
    local expected_sequence=$((start_sequence + 1))
    local previous_hash=$start_hash
    local verified=0
    local first_line=$((start_sequence + 2))
    local schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 exit_code
    local stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra
    local stdout_path stderr_path stdout_real stderr_real payload computed

    [[ "$start_sequence" =~ ^[0-9]+$ && "$start_hash" =~ ^[0-9a-f]{64}$ ]] \
        || s114_die "journal verification checkpoint is malformed"
    [[ -s "$journal" && ! -L "$journal" ]] \
        || s114_die "journal is absent, empty, or unsafe: $journal"
    [[ $(stat -c '%h' -- "$journal") == 1 ]] \
        || s114_die "journal has additional hard links"
    [[ $(head -n 1 -- "$journal") == \
        $'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256' ]] \
        || s114_die "journal header does not match $S114_JOURNAL_SCHEMA"
    s114_verify_journal_companion "$journal"

    while IFS=$'\t' read -r schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 \
        exit_code stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra; do
        verified=$((verified + 1))
        [[ -z ${extra:-} && "$schema" == "$S114_JOURNAL_SCHEMA" \
            && "$sequence" == "$expected_sequence" \
            && "$recorded_previous" == "$previous_hash" \
            && "$exit_code" =~ ^[0-9]+$ \
            && "$stdout_hash" =~ ^[0-9a-f]{64}$ \
            && "$stderr_hash" =~ ^[0-9a-f]{64}$ \
            && "$entry_hash" =~ ^[0-9a-f]{64}$ ]] \
            || s114_die "new journal chain breaks at sequence $expected_sequence"
        stdout_path=$(printf '%s' "$stdout_path_b64" | base64 -d) \
            || s114_die "journal sequence $sequence has invalid stdout path encoding"
        stderr_path=$(printf '%s' "$stderr_path_b64" | base64 -d) \
            || s114_die "journal sequence $sequence has invalid stderr path encoding"
        [[ -f "$stdout_path" && ! -L "$stdout_path" \
            && -f "$stderr_path" && ! -L "$stderr_path" ]] \
            || s114_die "journal sequence $sequence evidence is unavailable"
        [[ $(stat -c '%h' -- "$stdout_path") == 1 \
            && $(stat -c '%h' -- "$stderr_path") == 1 ]] \
            || s114_die "journal sequence $sequence evidence has additional hard links"
        stdout_real=$(realpath -e -- "$stdout_path")
        stderr_real=$(realpath -e -- "$stderr_path")
        case "$stdout_real" in
            "$S114_STATE_ROOT_RESOLVED"/runs/*/logs/*) ;;
            *) s114_die "journal sequence $sequence stdout escaped run logs" ;;
        esac
        case "$stderr_real" in
            "$S114_STATE_ROOT_RESOLVED"/runs/*/logs/*) ;;
            *) s114_die "journal sequence $sequence stderr escaped run logs" ;;
        esac
        [[ $(s114_sha256_file "$stdout_path") == "$stdout_hash" \
            && $(s114_sha256_file "$stderr_path") == "$stderr_hash" ]] \
            || s114_die "journal sequence $sequence evidence hash mismatch"
        printf -v payload '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' \
            "$schema" "$sequence" "$recorded_previous" "$stage_b64" "$cwd_b64" \
            "$argv_b64" "$exit_code" "$stdout_path_b64" "$stdout_hash" \
            "$stderr_path_b64" "$stderr_hash"
        computed=$(printf '%s' "$payload" | sha256sum | awk '{print $1}')
        [[ "$computed" == "$entry_hash" ]] \
            || s114_die "journal entry hash mismatch at sequence $sequence"
        previous_hash=$entry_hash
        expected_sequence=$((expected_sequence + 1))
    done < <(tail -n +"$first_line" -- "$journal")
    ((verified > 0)) || s114_die "current invocation appended no journal records"
    [[ $(tail -n 1 -- "$journal" | awk -F '\t' '{print $2}') \
        == $((expected_sequence - 1)) ]] \
        || s114_die "journal gained an unverified trailing record"
}

s114_verify_journal() {
    local journal=${1:-$S114_JOURNAL_PATH}
    local expected_sequence=1
    local previous_hash=$S114_ZERO_HASH
    local line_number=0
    local schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 exit_code
    local stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra
    local payload computed
    local stdout_path stderr_path stdout_real stderr_real
    local header=$'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256'

    [[ -s "$journal" && ! -L "$journal" ]] \
        || s114_die "journal is absent, empty, or unsafe: $journal"
    [[ $(stat -c '%h' -- "$journal") == 1 ]] \
        || s114_die "journal has additional hard links"
    IFS= read -r header <"$journal" || s114_die "cannot read journal header"
    [[ "$header" == $'schema\tsequence\tprevious_sha256\tstage_b64\tcwd_b64\targv_b64\texit_code\tstdout_path_b64\tstdout_sha256\tstderr_path_b64\tstderr_sha256\tentry_sha256' ]] \
        || s114_die "journal header does not match $S114_JOURNAL_SCHEMA"

    while IFS=$'\t' read -r schema sequence recorded_previous stage_b64 cwd_b64 argv_b64 \
        exit_code stdout_path_b64 stdout_hash stderr_path_b64 stderr_hash entry_hash extra; do
        line_number=$((line_number + 1))
        [[ -z ${extra:-} ]] || s114_die "journal record $line_number has extra fields"
        [[ "$schema" == "$S114_JOURNAL_SCHEMA" ]] \
            || s114_die "journal record $line_number has the wrong schema"
        [[ "$sequence" == "$expected_sequence" ]] \
            || s114_die "journal sequence breaks at record $line_number"
        [[ "$recorded_previous" == "$previous_hash" ]] \
            || s114_die "journal hash chain breaks at record $line_number"
        [[ "$exit_code" =~ ^[0-9]+$ ]] \
            || s114_die "journal record $line_number has an invalid exit code"
        [[ "$stdout_hash" =~ ^[0-9a-f]{64}$ && "$stderr_hash" =~ ^[0-9a-f]{64}$ ]] \
            || s114_die "journal record $line_number has an invalid output hash"

        stdout_path=$(printf '%s' "$stdout_path_b64" | base64 -d) \
            || s114_die "journal record $line_number has invalid stdout path encoding"
        stderr_path=$(printf '%s' "$stderr_path_b64" | base64 -d) \
            || s114_die "journal record $line_number has invalid stderr path encoding"
        [[ -f "$stdout_path" && ! -L "$stdout_path" ]] \
            || s114_die "journal record $line_number stdout evidence is unavailable"
        [[ -f "$stderr_path" && ! -L "$stderr_path" ]] \
            || s114_die "journal record $line_number stderr evidence is unavailable"
        [[ $(stat -c '%h' -- "$stdout_path") == 1 \
            && $(stat -c '%h' -- "$stderr_path") == 1 ]] \
            || s114_die "journal record $line_number evidence has additional hard links"
        stdout_real=$(realpath -e -- "$stdout_path") \
            || s114_die "journal record $line_number stdout evidence cannot be canonicalized"
        stderr_real=$(realpath -e -- "$stderr_path") \
            || s114_die "journal record $line_number stderr evidence cannot be canonicalized"
        case "$stdout_real" in
            "$S114_STATE_ROOT_RESOLVED"/runs/*/logs/*) ;;
            *) s114_die "journal record $line_number stdout evidence escaped run logs" ;;
        esac
        case "$stderr_real" in
            "$S114_STATE_ROOT_RESOLVED"/runs/*/logs/*) ;;
            *) s114_die "journal record $line_number stderr evidence escaped run logs" ;;
        esac
        [[ $(s114_sha256_file "$stdout_path") == "$stdout_hash" ]] \
            || s114_die "journal record $line_number stdout evidence hash mismatch"
        [[ $(s114_sha256_file "$stderr_path") == "$stderr_hash" ]] \
            || s114_die "journal record $line_number stderr evidence hash mismatch"

        printf -v payload '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' \
            "$schema" "$sequence" "$recorded_previous" "$stage_b64" "$cwd_b64" \
            "$argv_b64" "$exit_code" "$stdout_path_b64" "$stdout_hash" \
            "$stderr_path_b64" "$stderr_hash"
        computed=$(printf '%s' "$payload" | sha256sum | awk '{print $1}')
        [[ "$computed" == "$entry_hash" ]] \
            || s114_die "journal entry hash mismatch at record $line_number"
        previous_hash=$entry_hash
        expected_sequence=$((expected_sequence + 1))
    done < <(tail -n +2 -- "$journal")
    ((line_number > 0)) || s114_die "journal contains no command records"

    s114_verify_journal_companion "$journal"
}

s114_run_logged() {
    local stage=$1
    local working_directory=$2
    local stdout_file=$3
    local stderr_file=$4
    shift 4
    [[ ${1:-} == '--' ]] || s114_die "internal error: logged command lacks -- separator"
    shift

    local exit_code
    local original_directory=$PWD
    install -d -m 0700 -- "$(dirname -- "$stdout_file")" "$(dirname -- "$stderr_file")"
    : >"$stdout_file"
    : >"$stderr_file"
    cd -- "$working_directory"
    if "$@" >"$stdout_file" 2>"$stderr_file"; then
        exit_code=0
    else
        exit_code=$?
    fi
    cd -- "$original_directory"
    s114_journal_append "$stage" "$working_directory" "$exit_code" \
        "$stdout_file" "$stderr_file" "$@"
    S114_LAST_COMMAND_EXIT=$exit_code
}

s114_emit_stage_result() {
    local stage=$1
    local status=$2
    local exit_code=$3
    local stdout_file=$4
    local stderr_file=$5
    local stage_record="$stdout_file.stage.json"
    [[ "$stage" =~ ^[a-z0-9_-]{1,32}$ \
        && "$status" =~ ^[a-z0-9_-]{1,32}$ \
        && "$exit_code" =~ ^[0-9]+$ ]] \
        || s114_die "stage-result scalar is malformed"
    s114_assert_regular_or_missing "$stage_record" "stage-result evidence"
    [[ ! -e "$stage_record" ]] \
        || s114_die "refusing to overwrite stage-result evidence"
    printf '{"schema":"s114-check-stage-v1","stage":"%s","status":"%s","exit_code":%s,"stdout_sha256":"%s","stderr_sha256":"%s"}\n' \
        "$stage" "$status" "$exit_code" \
        "$(s114_sha256_file "$stdout_file")" \
        "$(s114_sha256_file "$stderr_file")" \
        >"$stage_record"
}

s114_show_failure_logs() {
    local stage=$1
    local stdout_file=$2
    local stderr_file=$3
    [[ "$stage" =~ ^[a-z0-9-]{1,32}$ ]] \
        || s114_die "failure-log stage label is unsafe"
    printf '{"schema":"s114-log-reference-v1","stage_b64":"%s","stdout_bytes":%s,"stdout_sha256":"%s","stderr_bytes":%s,"stderr_sha256":"%s"}\n' \
        "$(s114_b64 "$stage")" \
        "$(stat -c '%s' -- "$stdout_file")" \
        "$(s114_sha256_file "$stdout_file")" \
        "$(stat -c '%s' -- "$stderr_file")" \
        "$(s114_sha256_file "$stderr_file")" >&2
}

# Expose a small, terminal-safe excerpt only for candidate-controlled stages
# whose diagnostics are part of the disclosed repair loop. Hidden and trusted
# control-stage callers must continue to use s114_show_failure_logs alone.
#
# Each stream samples at most 4,096 bytes from both its head and tail, selects
# the first error/panic/assertion context plus its next line (or the last two
# lines when no marker is found), and emits at most 160 payload bytes across
# three prefixed lines (two sanitized lines plus a trusted context marker).
# With at most ten candidate-failure stages and one bounded log-reference
# record per stage, stderr remains below the frozen 12,000-byte stream limit.
s114_show_candidate_diagnostics() {
    local stage=$1
    local stdout_file=$2
    local stderr_file=$3
    local stream
    local evidence_file
    local prefix

    [[ "$stage" =~ ^[a-z0-9-]{1,32}$ ]] \
        || s114_die "candidate diagnostic stage label is unsafe"

    for stream in stdout stderr; do
        if [[ "$stream" == stdout ]]; then
            evidence_file=$stdout_file
        else
            evidence_file=$stderr_file
        fi
        [[ -f "$evidence_file" && ! -L "$evidence_file" \
            && $(stat -c '%h' -- "$evidence_file") == 1 ]] \
            || s114_die "candidate diagnostic evidence is unavailable or unsafe"
        prefix="S114-UNTRUSTED $stage/$stream | "
        {
            head -c "$S114_CANDIDATE_DIAGNOSTIC_SAMPLE_BYTES" -- "$evidence_file"
            printf '\n'
            tail -c "$S114_CANDIDATE_DIAGNOSTIC_SAMPLE_BYTES" -- "$evidence_file"
        } \
            | LC_ALL=C sed -E $'s/\033\\[[0-?]*[ -\\/]*[@-~]//g; s/\033.//g' \
            | LC_ALL=C tr -cd '\011\012\040-\176' \
            | LC_ALL=C awk -v prefix="$prefix" \
                -v line_limit="$((S114_CANDIDATE_DIAGNOSTIC_PAYLOAD_BYTES / 2))" '
                function emit(line) {
                    if (emitted >= 2) return
                    print prefix substr(line, 1, line_limit)
                    emitted += 1
                }
                {
                    folded = tolower($0)
                    if (!selected && folded ~ /(error(\[[^]]*\])?:|panicked|assertion|left:|right:)/) {
                        emit($0)
                        selected = 1
                        need_context = 1
                        next
                    }
                    if (need_context && length($0) != 0) {
                        emit($0)
                        need_context = 0
                    }
                    previous = last
                    last = $0
                }
                END {
                    if (!selected) {
                        if (length(previous) != 0) emit(previous)
                        if (length(last) != 0) emit(last)
                    }
                    if (emitted == 0) print prefix "[empty stream]"
                    else print prefix "[bounded diagnostic context]"
                }
            ' >&2
    done
}

s114_prepare_runtime() {
    s114_require_wsl
    local required
    local login_home
    local rustup_command=''
    local selected_toolchain=1.96-x86_64-unknown-linux-gnu
    local tool_path
    for required in base64 bwrap find flock prlimit realpath sha256sum sort stat timeout; do
        s114_require_command "$required"
    done

    rustup_command=$(command -v rustup 2>/dev/null || true)
    if [[ -z "$rustup_command" ]]; then
        s114_require_command getent
        s114_require_command id
        login_home=$(getent passwd "$(id -u)" | awk -F ':' 'NR == 1 {print $6}')
        [[ -n "$login_home" ]] || s114_die "cannot resolve the WSL login home"
        if [[ -x "$login_home/.cargo/bin/rustup" ]]; then
            rustup_command="$login_home/.cargo/bin/rustup"
        fi
    fi
    [[ -n "$rustup_command" && -x "$rustup_command" ]] \
        || s114_die "required command is unavailable: rustup"

    S114_HOST_CARGO=$("$rustup_command" which --toolchain "$selected_toolchain" cargo 2>/dev/null) \
        || s114_die "selected Rust toolchain has no cargo component"
    S114_HOST_RUSTC=$("$rustup_command" which --toolchain "$selected_toolchain" rustc 2>/dev/null) \
        || s114_die "selected Rust toolchain has no rustc component"
    S114_HOST_RUSTDOC=$("$rustup_command" which --toolchain "$selected_toolchain" rustdoc 2>/dev/null) \
        || s114_die "selected Rust toolchain has no rustdoc component"
    S114_HOST_TOOLCHAIN_ROOT=$(cd -- "$(dirname -- "$S114_HOST_CARGO")/.." && pwd -P)
    [[ "$(basename -- "$S114_HOST_TOOLCHAIN_ROOT")" == "$selected_toolchain" ]] \
        || s114_die "rustup resolved an unexpected toolchain root"
    [[ -d "$S114_HOST_TOOLCHAIN_ROOT/lib" && ! -L "$S114_HOST_TOOLCHAIN_ROOT/lib" ]] \
        || s114_die "selected Rust toolchain lib directory is unavailable"
    for tool_path in "$S114_HOST_CARGO" "$S114_HOST_RUSTC" "$S114_HOST_RUSTDOC"; do
        [[ -f "$tool_path" && -x "$tool_path" && ! -L "$tool_path" ]] \
            || s114_die "selected Rust tool is not a real executable: $tool_path"
        case "$(realpath -- "$tool_path")" in
            "$S114_HOST_TOOLCHAIN_ROOT"/bin/*) ;;
            *) s114_die "selected Rust tool escaped its toolchain root" ;;
        esac
    done
    export S114_HOST_CARGO S114_HOST_RUSTC S114_HOST_RUSTDOC S114_HOST_TOOLCHAIN_ROOT
}

s114_verify_frozen_inputs() {
    local manifest="$S114_HARNESS_ROOT/frozen-inputs.json"
    local companion="$S114_HARNESS_ROOT/frozen-inputs.sha256"
    local freezer="$S114_HARNESS_ROOT/freeze-inputs.ps1"
    local gate_run
    local gate_stdout
    local gate_stderr
    local freezer_windows
    local gate_exit
    local manifest_hash

    for required_input in "$manifest" "$companion" "$freezer"; do
        [[ -f "$required_input" && ! -L "$required_input" ]] \
            || s114_die "frozen input artifact is missing or unsafe: $required_input"
    done

    s114_require_command wslpath
    s114_require_command pwsh.exe
    freezer_windows=$(wslpath -w -- "$freezer") \
        || s114_die "cannot convert the frozen-input verifier path for Windows"
    gate_run=$(s114_allocate_run_dir frozen-input-verify)
    gate_stdout="$gate_run/logs/frozen-input-verify.stdout"
    gate_stderr="$gate_run/logs/frozen-input-verify.stderr"
    s114_run_logged frozen-input-verify "$S114_REPO_ROOT" \
        "$gate_stdout" "$gate_stderr" -- \
        timeout --foreground --signal=TERM --kill-after=5s 30s \
        pwsh.exe -NoLogo -NoProfile -NonInteractive \
            -File "$freezer_windows" -Verify
    gate_exit=$S114_LAST_COMMAND_EXIT
    if ((gate_exit != 0)); then
        s114_show_failure_logs frozen-input-verify "$gate_stdout" "$gate_stderr"
        s114_die "frozen operator inputs did not verify"
    fi
    grep -Eq '"verified"[[:space:]]*:[[:space:]]*true' "$gate_stdout" \
        || s114_die "frozen-input verifier omitted its success attestation"
    manifest_hash=$(tr -d '\r\n' <"$companion")
    [[ "$manifest_hash" =~ ^[0-9a-f]{64}$ ]] \
        || s114_die "frozen input manifest companion is malformed"
    grep -q "$manifest_hash" "$gate_stdout" \
        || s114_die "frozen-input verifier attested a different manifest hash"
    S114_FROZEN_MANIFEST_HASH=$manifest_hash
    export S114_FROZEN_MANIFEST_HASH
}

# Logged helpers always return success after recording the wrapped command's
# status here. Callers must classify this value immediately.
declare -g S114_LAST_COMMAND_EXIT=0

# Callers may populate this as alternating host-source / sandbox-destination
# pairs before calling s114_run_sandbox_logged.
declare -ag S114_EXTRA_RO_BINDS=()

s114_run_sandbox_logged() {
    local stage=$1
    local candidate=$2
    local target_dir=$3
    local temp_dir=$4
    local cargo_home=$5
    local sandbox_working_directory=$6
    local stdout_file=$7
    local stderr_file=$8
    shift 8
    [[ ${1:-} == '--' ]] || s114_die "internal error: sandbox command lacks -- separator"
    shift

    local wall_seconds=${S114_STAGE_WALL_SECONDS:-180}
    local cpu_seconds=${S114_STAGE_CPU_SECONDS:-150}
    local address_bytes=${S114_STAGE_ADDRESS_BYTES:-8589934592}
    local file_bytes=${S114_STAGE_FILE_BYTES:-1073741824}
    local process_count=${S114_STAGE_PROCESS_COUNT:-128}
    local open_files=${S114_STAGE_OPEN_FILES:-512}
    local target_read_only=${S114_TARGET_READ_ONLY:-0}
    local launcher_attestation="$stdout_file.launcher-attestation"
    local -a launcher_exit_records=()
    local launcher_status
    local launcher_status_exit
    local status_key
    local -a bwrap_args
    local system_path
    local index
    local source_path
    local destination_path

    # prlimit bounds each process and each file. It is not an aggregate cgroup
    # quota; the separate process-count and wall bounds cap that residual risk.

    s114_validate_uint S114_STAGE_WALL_SECONDS "$wall_seconds"
    s114_validate_uint S114_STAGE_CPU_SECONDS "$cpu_seconds"
    s114_validate_uint S114_STAGE_ADDRESS_BYTES "$address_bytes"
    s114_validate_uint S114_STAGE_FILE_BYTES "$file_bytes"
    s114_validate_uint S114_STAGE_PROCESS_COUNT "$process_count"
    s114_validate_uint S114_STAGE_OPEN_FILES "$open_files"
    [[ "$target_read_only" == 0 || "$target_read_only" == 1 ]] \
        || s114_die "S114_TARGET_READ_ONLY must be 0 or 1"
    (( ${#S114_EXTRA_RO_BINDS[@]} % 2 == 0 )) \
        || s114_die "internal error: extra read-only bind list is not paired"

    for source_path in "$candidate" "$target_dir" "$temp_dir" "$cargo_home"; do
        [[ -d "$source_path" && ! -L "$source_path" ]] \
            || s114_die "sandbox mount source is not a real directory: $source_path"
    done
    s114_assert_regular_or_missing "$launcher_attestation" \
        "sandbox launcher attestation"
    [[ ! -e "$launcher_attestation" ]] \
        || s114_die "refusing to reuse a sandbox launcher attestation"

    bwrap_args=(
        --unshare-user
        --unshare-pid
        --unshare-ipc
        --unshare-uts
        --unshare-net
        --die-with-parent
        --new-session
        --json-status-fd 3
        --clearenv
        --tmpfs /
        --proc /proc
        --dev /dev
        --dir /etc
        --dir /opt
        --dir /opt/toolchain
        --dir /opt/toolchain/bin
        --dir /opt/toolchain/lib
        --dir /workspace
        --dir /target
        --dir /tmp
        --dir /homeless
    )
    for system_path in /usr /bin /sbin /lib /lib64; do
        if [[ -e "$system_path" ]]; then
            bwrap_args+=(--ro-bind "$system_path" "$system_path")
        fi
    done
    for system_path in /etc/alternatives /etc/ld.so.cache; do
        if [[ -e "$system_path" ]]; then
            bwrap_args+=(--ro-bind "$system_path" "$system_path")
        fi
    done

    bwrap_args+=(
        --ro-bind "$S114_HOST_CARGO" /opt/toolchain/bin/cargo
        --ro-bind "$S114_HOST_RUSTC" /opt/toolchain/bin/rustc
        --ro-bind "$S114_HOST_RUSTDOC" /opt/toolchain/bin/rustdoc
        --ro-bind "$S114_HOST_TOOLCHAIN_ROOT/lib" /opt/toolchain/lib
    )
    bwrap_args+=(--ro-bind "$candidate" /workspace)
    if [[ "$target_read_only" == 1 ]]; then
        bwrap_args+=(--ro-bind "$target_dir" /target)
    else
        bwrap_args+=(--bind "$target_dir" /target)
    fi
    bwrap_args+=(
        --bind "$temp_dir" /tmp
        --dir /tmp/cargo-home
        --bind "$cargo_home" /tmp/cargo-home
    )

    for ((index = 0; index < ${#S114_EXTRA_RO_BINDS[@]}; index += 2)); do
        source_path=${S114_EXTRA_RO_BINDS[index]}
        destination_path=${S114_EXTRA_RO_BINDS[index + 1]}
        [[ -e "$source_path" && ! -L "$source_path" ]] \
            || s114_die "extra bind source is missing or a symlink: $source_path"
        case "$destination_path" in
            /hidden | /candidate-build)
                [[ -d "$source_path" ]] \
                    || s114_die "the $destination_path bind source must be a directory"
                bwrap_args+=(--dir "$destination_path" --ro-bind "$source_path" "$destination_path")
                ;;
            /cases)
                [[ -d "$source_path" ]] \
                    || s114_die "the /cases bind source must be a directory"
                bwrap_args+=(--dir /cases --ro-bind "$source_path" /cases)
                ;;
            *) s114_die "extra bind destination is not allowlisted: $destination_path" ;;
        esac
    done

    bwrap_args+=(
        --remount-ro /
        --setenv HOME /homeless
        --setenv USER sandbox
        --setenv LOGNAME sandbox
        --setenv LANG C
        --setenv LC_ALL C
        --setenv TZ UTC
        --setenv PATH /opt/toolchain/bin:/usr/bin:/bin
        --setenv CARGO_HOME /tmp/cargo-home
        --setenv RUSTC /opt/toolchain/bin/rustc
        --setenv RUSTDOC /opt/toolchain/bin/rustdoc
        --setenv CARGO_NET_OFFLINE true
        --setenv CARGO_BUILD_JOBS 1
        --setenv CARGO_INCREMENTAL 0
        --setenv CARGO_TARGET_DIR /target
        --setenv RUSTFLAGS '-C codegen-units=1 -C link-arg=-Wl,--threads=1'
        --setenv RUST_TEST_THREADS 1
        --setenv TMPDIR /tmp
        --setenv RUST_BACKTRACE 0
        --setenv SOURCE_DATE_EPOCH 0
        --chdir "$sandbox_working_directory"
        --cap-drop ALL
        --
        /usr/bin/prlimit
        "--cpu=${cpu_seconds}:${cpu_seconds}"
        "--as=${address_bytes}:${address_bytes}"
        "--fsize=${file_bytes}:${file_bytes}"
        "--nproc=${process_count}:${process_count}"
        "--nofile=${open_files}:${open_files}"
        --core=0:0
        --
        "$@"
    )

    exec 3>"$launcher_attestation"
    s114_run_logged "$stage" "$S114_REPO_ROOT" "$stdout_file" "$stderr_file" -- \
        timeout --foreground --signal=TERM --kill-after=5s "${wall_seconds}s" \
        bwrap "${bwrap_args[@]}"
    exec 3>&-
    [[ -f "$launcher_attestation" && ! -L "$launcher_attestation" \
        && $(stat -c '%h' -- "$launcher_attestation") == 1 \
        && $(stat -c '%s' -- "$launcher_attestation") -le 1024 ]] \
        || s114_die "sandbox launcher failed before the candidate command started"
    launcher_status=$(head -n 1 -- "$launcher_attestation")
    [[ "$launcher_status" == '{ "child-pid": '* \
        && "$launcher_status" == *' }' \
        && $(grep -Fc '"child-pid"' "$launcher_attestation" || true) == 1 ]] \
        || s114_die "sandbox launcher omitted its unique child-start status"
    for status_key in child-pid mnt-namespace net-namespace pid-namespace; do
        [[ $(grep -Eo "\"${status_key}\": [1-9][0-9]*" \
            <<<"$launcher_status" | wc -l) == 1 ]] \
            || s114_die "sandbox launcher status omitted a positive $status_key"
    done
    mapfile -t launcher_exit_records < <(
        grep -E '^\{ "exit-code": [0-9]+ \}$' "$launcher_attestation" || true
    )
    ((${#launcher_exit_records[@]} <= 1)) \
        || s114_die "sandbox launcher emitted duplicate exit status"
    if ((${#launcher_exit_records[@]} == 1)); then
        launcher_status_exit=${launcher_exit_records[0]#*": "}
        launcher_status_exit=${launcher_status_exit% *}
        [[ "$launcher_status_exit" == "$S114_LAST_COMMAND_EXIT" ]] \
            || s114_die "sandbox launcher exit status disagrees with the trusted driver"
    elif ((S114_LAST_COMMAND_EXIT != 124 && S114_LAST_COMMAND_EXIT != 137)); then
        s114_die "sandbox setup failed before the fixed command completed"
    fi
}

s114_initialize() {
    local required
    local inherited_override
    for inherited_override in \
        S114_STAGE_WALL_SECONDS S114_STAGE_CPU_SECONDS \
        S114_STAGE_ADDRESS_BYTES S114_STAGE_FILE_BYTES \
        S114_STAGE_PROCESS_COUNT S114_STAGE_OPEN_FILES \
        S114_TARGET_READ_ONLY; do
        [[ ! -v $inherited_override ]] \
            || s114_die "inherited sandbox resource overrides are forbidden: $inherited_override"
    done
    for required in awk base64 find flock od sha256sum sort stat tr; do
        s114_require_command "$required"
    done
    s114_init_state
}
