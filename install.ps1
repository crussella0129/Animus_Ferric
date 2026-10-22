# Install Ferric on Windows (sprint 128, INT-0016):
#
#   irm https://raw.githubusercontent.com/crussella0129/Animus_Ferric/main/install.ps1 | iex
#
# Downloads the prebuilt ferric.exe from the latest GitHub release, checks its
# SHA-256, installs it under %LOCALAPPDATA%\Programs\ferric and adds that
# folder to your user PATH. Then run `ferric` in any project folder: the first
# run offers (once) to download the engine and a model that fit this machine.
#
# Before the first release exists, it builds from source with cargo instead.
#
# Environment:
#   FERRIC_BIN_DIR           install directory
#   FERRIC_INSTALL_BASE_URL  where release assets live (default: the latest release)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'crussella0129/Animus_Ferric'
$base = if ($env:FERRIC_INSTALL_BASE_URL) { $env:FERRIC_INSTALL_BASE_URL } else { "https://github.com/$repo/releases/latest/download" }
$binDir = if ($env:FERRIC_BIN_DIR) { $env:FERRIC_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\ferric' }

# Windows on Arm runs the x64 build under emulation.
$target = 'x86_64-pc-windows-msvc'
$asset = "ferric-$target.zip"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ferric-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $tmp | Out-Null
$installed = $null

try {
    # Windows PowerShell 5.1 may not offer TLS 1.2 by default; GitHub needs it.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    # A failed download means no release yet (fall back to cargo); a failed
    # checksum is an error and stops the install.
    $downloaded = $false
    try {
        Write-Host "Downloading $asset ..."
        Invoke-WebRequest -UseBasicParsing "$base/$asset" -OutFile (Join-Path $tmp $asset)
        Invoke-WebRequest -UseBasicParsing "$base/$asset.sha256" -OutFile (Join-Path $tmp "$asset.sha256")
        $downloaded = $true
    } catch {
        Write-Host "No prebuilt release is available yet."
    }
    if ($downloaded) {
        $expected = ((Get-Content (Join-Path $tmp "$asset.sha256") -Raw).Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { throw "checksum mismatch for $asset (expected $expected, got $actual)" }
        Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
        New-Item -ItemType Directory -Force $binDir | Out-Null
        Copy-Item (Join-Path $tmp "ferric-$target\ferric.exe") (Join-Path $binDir 'ferric.exe') -Force
        $installed = Join-Path $binDir 'ferric.exe'
        Write-Host "Verified SHA-256 $actual"
    }

    if (-not $installed) {
        if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
            throw "building from source needs Rust: install it from https://rustup.rs, then run this again"
        }
        Write-Host "Building ferric from source with cargo (a few minutes) ..."
        cargo install --locked --git "https://github.com/$repo" ferric-cli --root (Join-Path $tmp 'root')
        if ($LASTEXITCODE -ne 0) { throw "cargo install failed" }
        New-Item -ItemType Directory -Force $binDir | Out-Null
        Copy-Item (Join-Path $tmp 'root\bin\ferric.exe') (Join-Path $binDir 'ferric.exe') -Force
        $installed = Join-Path $binDir 'ferric.exe'
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $binDir)) {
        [Environment]::SetEnvironmentVariable('Path', ($(if ($userPath) { "$userPath;" } else { '' }) + $binDir), 'User')
        Write-Host "Added $binDir to your user PATH (new terminals pick it up)."
    }
    if (-not (($env:Path -split ';') -contains $binDir)) { $env:Path = "$env:Path;$binDir" }

    Write-Host "Installed $(& $installed --version) at $installed"
    Write-Host ""
    Write-Host "Next: cd into a project and run  ferric"
    Write-Host "The first run offers once to download the engine and a model that fit this machine."
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
