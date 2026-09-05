#!/bin/sh
# Internal source supervisor for test-lifecycle-linux.sh. Do not exec Cargo:
# this shell must remain PID 1 to reap adopted source-test children.
set -eu
test "$$" -eq 1
test "$(id -u)" -ne 0
test "$#" -eq 7

ferric_cargo=$1
ferric_root=$2
ferric_mode=$7
case "$ferric_mode" in
  lifecycle|workspace) ;;
  *)
    echo 'Unsupported Linux source-test gate mode.' >&2
    exit 2
    ;;
esac
# Restore the caller's toolchain context only after dropping all root authority.
# These are standard environment values consumed by Cargo/rustup, not scratch
# variables repurposing HOME/CARGO_HOME.
export HOME="$3" CARGO_HOME="$4" RUSTUP_HOME="$5" PATH="$6"
cd "$ferric_root"
ferric_status=0
case "$ferric_mode" in
  lifecycle)
    "$ferric_cargo" test -p ferric-cli --features lifecycle-fixture \
      --test server_lifecycle_fixture --locked --offline -- --test-threads=1 \
      || ferric_status=$?
    ;;
  workspace)
    "$ferric_cargo" test --workspace --locked --offline -- --test-threads=1 \
      || ferric_status=$?
    ;;
esac
exit "$ferric_status"
