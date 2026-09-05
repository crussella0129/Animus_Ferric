#!/bin/sh
# Internal source supervisor for test-lifecycle-linux.sh. Do not exec Cargo:
# this shell must remain PID 1 to reap adopted lifecycle fixture children.
set -eu
test "$$" -eq 1
test "$(id -u)" -ne 0
test "$#" -eq 6

ferric_cargo=$1
ferric_root=$2
# Restore the caller's toolchain context only after dropping all root authority.
# These are standard environment values consumed by Cargo/rustup, not scratch
# variables repurposing HOME/CARGO_HOME.
export HOME="$3" CARGO_HOME="$4" RUSTUP_HOME="$5" PATH="$6"
cd "$ferric_root"
ferric_status=0
"$ferric_cargo" test -p ferric-cli --features lifecycle-fixture \
  --test server_lifecycle_fixture --locked --offline -- --test-threads=1 \
  || ferric_status=$?
exit "$ferric_status"
