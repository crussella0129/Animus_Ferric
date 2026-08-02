param (
    [string]$Model,
    [switch]$Mock
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$TempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$Workspace = [System.IO.Path]::GetFullPath(
    (Join-Path $TempBase "animus-ferric-e2e-$PID")
)
$LogFile = Join-Path $RepoRoot "e2e_test_log.txt"
$Prompt = if ($Mock) {
    # The built-in provider is intentionally fixed: one write_file followed by
    # task_complete. A mock test that expects an arbitrary generated project is
    # guaranteed to be a false negative (or, previously, a false green).
    "create the deterministic mock artifact and finish"
}
else {
    "Create a folder called 'project_root', then create two folders within it: 'src' and 'temp'. Inside 'src', create a file called 'math.py' that contains a function to add two numbers and print the result. Finally, delete the 'temp' folder and report task_complete."
}

if (
    -not $Workspace.StartsWith($TempBase, [StringComparison]::OrdinalIgnoreCase) -or
    [System.IO.Path]::GetFileName($Workspace) -notlike "animus-ferric-e2e-*"
) {
    throw "refusing to use unsafe E2E workspace path: $Workspace"
}

# 1. Clean up old test runs
if (Test-Path -LiteralPath $Workspace) {
    Write-Host "Cleaning up old workspace..." -ForegroundColor Yellow
    Remove-Item -LiteralPath $Workspace -Recurse -Force
}
New-Item -ItemType Directory -Path $Workspace -Force | Out-Null
Write-Host "Created fresh workspace: $Workspace" -ForegroundColor Green

# 2. Build the command
$Command = "cargo"
$ArgsList = @(
    "run", "--manifest-path", (Join-Path $RepoRoot "Cargo.toml"),
    "--release", "-p", "ferric-cli", "--features", "backend-openai",
    "--", "query", $Prompt, "--workspace", $Workspace
)

if ($Mock) {
    Write-Host "Running in MOCK mode (no real model)..." -ForegroundColor Cyan
    $ArgsList += "--mock"
} else {
    if (-not $Model) {
        Write-Host "ERROR: You must provide -Model when not using -Mock." -ForegroundColor Red
        Write-Host "Example: .\tools\e2e_test.ps1 -Model `"gpt-4o`"" -ForegroundColor Red
        exit 1
    }
    Write-Host "Running with REAL model: $Model" -ForegroundColor Cyan
    $ArgsList += "--model"
    $ArgsList += $Model
}

Write-Host "Starting E2E Test. Output is being written to $LogFile..." -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green

# 3. Execute and preserve the native exit status across Tee-Object.
$ErrorActionPreference = "Continue"
& $Command @ArgsList 2>&1 | Tee-Object -FilePath $LogFile
$QueryExit = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($QueryExit -ne 0) {
    throw "ferric query failed (exit $QueryExit); see $LogFile"
}

Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Query completed; validating artifacts and trace..." -ForegroundColor Green

# 4. Assert the artifact promised by the selected path.
if ($Mock) {
    $Artifact = Join-Path $Workspace "ferric-mock.txt"
    if (-not (Test-Path -LiteralPath $Artifact)) {
        throw "mock query exited zero but did not create $Artifact"
    }
    if ((Get-Content -Raw -LiteralPath $Artifact).Trim() -ne "mock run") {
        throw "mock artifact content is incorrect: $Artifact"
    }
}
else {
    $Artifact = Join-Path $Workspace "project_root\src\math.py"
    if (-not (Test-Path -LiteralPath $Artifact)) {
        throw "live query exited zero but did not create $Artifact"
    }
    $MathSource = Get-Content -Raw -LiteralPath $Artifact
    if ($MathSource -notmatch '(?m)^\s*def\s+[A-Za-z_][A-Za-z0-9_]*\s*\([^)]*,[^)]*\)\s*(?:->\s*[^:]+)?\s*:') {
        throw "math.py does not define a two-argument function: $Artifact"
    }
    if ($MathSource -notmatch '\+') {
        throw "math.py does not add two values: $Artifact"
    }
    if ($MathSource -notmatch '(?m)^\s*print\s*\(') {
        throw "math.py does not print a result: $Artifact"
    }

    $DeletedTemp = Join-Path $Workspace "project_root\temp"
    if (Test-Path -LiteralPath $DeletedTemp) {
        throw "live query did not delete the requested temp directory: $DeletedTemp"
    }
}

# 5. A run is not successful without a complete, task_complete trace.
$TraceDir = Join-Path $Workspace ".ferric\trace"
if (-not (Test-Path -LiteralPath $TraceDir)) {
    throw "query exited zero but wrote no trace directory: $TraceDir"
}
$LatestTrace = Get-ChildItem -LiteralPath $TraceDir -Filter "*.jsonl" |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $LatestTrace) {
    throw "query exited zero but wrote no JSONL trace under $TraceDir"
}
$TraceText = Get-Content -Raw -LiteralPath $LatestTrace.FullName
if (-not $TraceText.Contains('"type":"session_end","reason":"task_complete"')) {
    throw "trace does not end in task_complete: $($LatestTrace.FullName)"
}

# 6. Exercise the side-effect-free structural verifier against the trace we just
# produced. Cargo is incremental here; this does not rebuild the workspace.
$VerifyArgs = @(
    "run", "--manifest-path", (Join-Path $RepoRoot "Cargo.toml"),
    "--release", "-p", "ferric-cli", "--features", "backend-openai",
    "--", "trace", "verify", $LatestTrace.FullName
)
& $Command @VerifyArgs
if ($LASTEXITCODE -ne 0) {
    throw "trace verification failed (exit $LASTEXITCODE)"
}

Write-Host ""
Write-Host "E2E PASS" -ForegroundColor Green
Write-Host "Workspace: $Workspace"
Write-Host "Artifact:  $Artifact"
Write-Host "Trace:     $($LatestTrace.FullName)"
