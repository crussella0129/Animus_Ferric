#!/usr/bin/env pwsh
# Install the `ferric` binary onto your PATH (Windows / PowerShell).
#
# `cargo install` builds in release AND copies the binary into
# %USERPROFILE%\.cargo\bin, which rustup already added to your PATH — so this is
# the one step that both builds and puts `ferric` on your PATH. It is identical
# in spirit to tools/install.sh for Linux/macOS.
#
# Re-run this after every `git pull` or source change. A plain
# `cargo build --release` refreshes target\release\ but NOT the copy on your
# PATH, so the `ferric` you invoke can silently lag the code (this is how a stale
# binary still offering a removed flag sneaks in). `--force` re-installs even when
# the version string is unchanged, which it always is here (0.1.0).
#
# Usage:
#   .\tools\install.ps1                 # default: --features backend-openai
#   .\tools\install.ps1 -Features ""    # trace/offline tooling only, no backend
param(
    [string]$Features = "backend-openai"
)
$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
$crate = Join-Path $repo "crates/ferric-cli"

$featureArgs = @()
if ($Features -and $Features.Trim() -ne "") {
    $featureArgs = @("--features", $Features)
    Write-Host "Installing ferric (--features $Features) from $repo ..." -ForegroundColor Cyan
} else {
    Write-Host "Installing ferric (no backend feature) from $repo ..." -ForegroundColor Cyan
}

cargo install --path $crate @featureArgs --force
if ($LASTEXITCODE -ne 0) { throw "cargo install failed (exit $LASTEXITCODE)" }

$bin = Join-Path $env:USERPROFILE ".cargo\bin\ferric.exe"
Write-Host "`nInstalled:" -ForegroundColor Green
& $bin --version
Write-Host "Location: $bin"

if (-not (Get-Command ferric -ErrorAction SilentlyContinue)) {
    Write-Warning "'$env:USERPROFILE\.cargo\bin' is not on your PATH. Add it (rustup normally does this) so 'ferric' resolves."
}
