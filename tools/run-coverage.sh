#!/usr/bin/env bash
set -e

echo "Starting Coverage Tests"
cd "$(dirname "$0")/.."

if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "cargo-tarpaulin not found. Installing..."
    cargo install cargo-tarpaulin
fi

echo "Running cargo tarpaulin..."
cargo tarpaulin --ignore-tests --out Html --fail-under 80

echo "Coverage tests passed!"
exit 0
