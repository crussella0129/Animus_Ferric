#!/usr/bin/env bash
# Install the `ferric` binary onto your PATH (Linux / macOS).
#
# `cargo install` builds in release AND copies the binary into ~/.cargo/bin,
# which rustup already added to your PATH — so this is the one step that both
# builds and puts `ferric` on your PATH. It mirrors tools/install.ps1 for Windows.
#
# Re-run this after every `git pull` or source change. A plain
# `cargo build --release` refreshes target/release/ but NOT the copy on your
# PATH, so the `ferric` you invoke can silently lag the code (this is how a stale
# binary still offering a removed flag sneaks in). `--force` re-installs even when
# the version string is unchanged, which it always is here (0.1.0).
#
# Usage:
#   ./tools/install.sh                  # default: --features backend-openai
#   ./tools/install.sh ""               # trace/offline tooling only, no backend
set -euo pipefail

FEATURES="${1-backend-openai}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="${REPO}/crates/ferric-cli"

if [ -n "${FEATURES}" ]; then
    echo "Installing ferric (--features ${FEATURES}) from ${REPO} ..."
    cargo install --path "${CRATE}" --features "${FEATURES}" --force
else
    echo "Installing ferric (no backend feature) from ${REPO} ..."
    cargo install --path "${CRATE}" --force
fi

BIN="${CARGO_HOME:-${HOME}/.cargo}/bin/ferric"
echo
echo "Installed:"
"${BIN}" --version
echo "Location: ${BIN}"

if ! command -v ferric >/dev/null 2>&1; then
    echo "WARNING: '${CARGO_HOME:-${HOME}/.cargo}/bin' is not on your PATH. Add it (rustup normally does this) so 'ferric' resolves." >&2
fi
