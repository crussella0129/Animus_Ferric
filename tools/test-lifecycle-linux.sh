#!/usr/bin/env bash
# Cargo is the sole Rust execution entry point. Only network/PID namespace
# setup needs privilege; Cargo and all test code run as the invoking identity.
set -euo pipefail

if [[ "$(uname -s)" != Linux || "$(id -u)" == 0 ]]; then
  echo 'Run this Linux lifecycle gate as a non-root user with sudo -n access.' >&2
  exit 2
fi

ferric_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ferric_root="$(cd -- "$ferric_script_dir/.." && pwd)"
cd -- "$ferric_root"

# Warm the exact target before disabling external networking. --no-run builds
# source but never extracts or launches a target artifact. The second Cargo
# invocation chooses the test target itself, with its normal runtime setup.
cargo test -p ferric-cli --features lifecycle-fixture \
  --test server_lifecycle_fixture --locked --no-run
ferric_cargo="$(command -v cargo)"
ferric_uid="$(id -u)"
ferric_gid="$(id -g)"

sudo -n unshare --pid --net --fork --mount-proc --kill-child=SIGKILL \
  /bin/sh -ceu '
    ip link set lo up
    # Keep the namespace hard-cleanup contract across credential changes.
    exec setpriv --pdeathsig keep \
      --reuid="$1" --regid="$2" --clear-groups \
      --no-new-privs --inh-caps=-all --ambient-caps=-all --bounding-set=-all \
      /bin/sh "$3" "$4" "$5" "$6" "$7" "$8" "$9"
  ' _ "$ferric_uid" "$ferric_gid" \
  "$ferric_script_dir/lifecycle-linux-reaper.sh" "$ferric_cargo" "$ferric_root" \
  "$HOME" "${CARGO_HOME:-$HOME/.cargo}" "${RUSTUP_HOME:-$HOME/.rustup}" "$PATH"
