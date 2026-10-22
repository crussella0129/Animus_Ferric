#!/bin/sh
# Install Ferric on Linux or macOS (sprint 128, INT-0016):
#
#   curl -fsSL https://raw.githubusercontent.com/crussella0129/Animus_Ferric/main/install.sh | sh
#
# Downloads the prebuilt `ferric` for this machine from the latest GitHub
# release, checks its SHA-256, and puts it in ~/.local/bin. Then run `ferric`
# in any project folder: the first run offers (once) to download the engine
# and a model that fit this machine.
#
# Before the first release exists, it builds from source with cargo instead.
#
# Environment:
#   FERRIC_BIN_DIR           install directory (default ~/.local/bin)
#   FERRIC_INSTALL_BASE_URL  where release assets live (default: the latest release)
set -eu

REPO="crussella0129/Animus_Ferric"
BASE="${FERRIC_INSTALL_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
BIN_DIR="${FERRIC_BIN_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
fail() { printf 'ferric install: %s\n' "$*" >&2; exit 1; }

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64 | Linux-amd64) target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
  Darwin-arm64 | Darwin-aarch64) target="aarch64-apple-darwin" ;;
  Darwin-x86_64) target="x86_64-apple-darwin" ;;
  *) target="" ;;
esac

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --proto '=https,http' "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    fail "needs curl or wget"
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM
installed=""

if [ -n "$target" ]; then
  asset="ferric-$target.tar.gz"
  say "Downloading $asset ..."
  if fetch "$BASE/$asset" "$tmp/$asset" 2>/dev/null && fetch "$BASE/$asset.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
    expected="$(cut -d' ' -f1 < "$tmp/$asset.sha256")"
    actual="$(sha256_of "$tmp/$asset")"
    [ "$expected" = "$actual" ] || fail "checksum mismatch for $asset (expected $expected, got $actual)"
    tar -xzf "$tmp/$asset" -C "$tmp"
    mkdir -p "$BIN_DIR"
    cp "$tmp/ferric-$target/ferric" "$BIN_DIR/ferric.new"
    chmod 755 "$BIN_DIR/ferric.new"
    mv -f "$BIN_DIR/ferric.new" "$BIN_DIR/ferric"
    installed="$BIN_DIR/ferric"
    say "Verified SHA-256 $actual"
  else
    say "No prebuilt release is available for $target yet."
  fi
fi

if [ -z "$installed" ]; then
  command -v cargo >/dev/null 2>&1 || fail "building from source needs Rust: install it from https://rustup.rs, then run this again"
  say "Building ferric from source with cargo (a few minutes) ..."
  cargo install --locked --git "https://github.com/$REPO" ferric-cli --root "$tmp/root"
  mkdir -p "$BIN_DIR"
  cp "$tmp/root/bin/ferric" "$BIN_DIR/ferric"
  installed="$BIN_DIR/ferric"
fi

say "Installed $("$installed" --version) at $installed"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    say ""
    say "$BIN_DIR is not on your PATH. Add it, for example:"
    say "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile && . ~/.profile"
    ;;
esac
say ""
say "Next: cd into a project and run  ferric"
say "The first run offers once to download the engine and a model that fit this machine."
