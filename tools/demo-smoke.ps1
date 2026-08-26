[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$KeepWorkspace
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Ferric = Join-Path $RepoRoot "target\release\ferric.exe"
$TempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$RunRoot = Join-Path $TempBase "animus-ferric-demo-$PID"
$RunRoot = [IO.Path]::GetFullPath($RunRoot)
$script:LastFerricOutput = @()
$Passed = [Collections.Generic.List[string]]::new()

if (
    -not $RunRoot.StartsWith($TempBase, [StringComparison]::OrdinalIgnoreCase) -or
    [IO.Path]::GetFileName($RunRoot) -notlike "animus-ferric-demo-*"
) {
    throw "refusing to use unsafe demo workspace path: $RunRoot"
}

function Invoke-Ferric {
    param([Parameter(Mandatory)][string[]]$CommandArguments)

    Write-Host ""
    Write-Host ("> ferric " + ($CommandArguments -join " ")) -ForegroundColor Cyan
    $Output = & $Ferric @CommandArguments 2>&1
    $Exit = $LASTEXITCODE
    $script:LastFerricOutput = @($Output | ForEach-Object { $_.ToString() })
    foreach ($Line in $script:LastFerricOutput) {
        Write-Host $Line
    }
    if ($Exit -ne 0) {
        throw "ferric exited with code $Exit"
    }
}

function Invoke-FerricExpectedFailure {
    param(
        [Parameter(Mandatory)][string[]]$CommandArguments,
        [Parameter(Mandatory)][string]$ExpectedPattern
    )

    Write-Host ""
    Write-Host ("> ferric " + ($CommandArguments -join " ") + "  [expected failure]") -ForegroundColor Cyan
    $Output = & $Ferric @CommandArguments 2>&1
    $Exit = $LASTEXITCODE
    $script:LastFerricOutput = @($Output | ForEach-Object { $_.ToString() })
    foreach ($Line in $script:LastFerricOutput) {
        Write-Host $Line
    }
    if ($Exit -eq 0) {
        throw "ferric unexpectedly accepted a guarded operation"
    }
    $JoinedOutput = $script:LastFerricOutput -join [Environment]::NewLine
    if ($JoinedOutput -notmatch $ExpectedPattern) {
        throw "ferric failed for the wrong reason; expected output matching '$ExpectedPattern'"
    }
}

function Assert-FileText {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Expected
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "expected file was not created: $Path"
    }
    $Actual = Get-Content -Raw -LiteralPath $Path
    if ($Actual -ne $Expected) {
        throw "unexpected content in $($Path): '$Actual'"
    }
}

function Add-Pass {
    param([Parameter(Mandatory)][string]$Message)

    $Passed.Add($Message)
    Write-Host ("PASS  " + $Message) -ForegroundColor Green
}

try {
    Set-Location -LiteralPath $RepoRoot
    if (-not $SkipBuild) {
        Write-Host "> cargo build --release -p ferric-cli --features backend-openai" -ForegroundColor Cyan
        & cargo build --release -p ferric-cli --features backend-openai
        if ($LASTEXITCODE -ne 0) {
            throw "release build failed"
        }
    }
    if (-not (Test-Path -LiteralPath $Ferric -PathType Leaf)) {
        throw "release binary not found at $Ferric; rerun without -SkipBuild"
    }

    New-Item -ItemType Directory -Path $RunRoot | Out-Null

    Invoke-Ferric -CommandArguments @("--version")
    Add-Pass "fresh release binary starts"

    $QueryWorkspace = Join-Path $RunRoot "query"
    New-Item -ItemType Directory -Path $QueryWorkspace | Out-Null
    & git -C $QueryWorkspace init --quiet --initial-branch=main
    if ($LASTEXITCODE -ne 0) {
        throw "could not initialize the query demo repository"
    }
    Invoke-Ferric -CommandArguments @(
        "query",
        "--mock",
        "--workspace", $QueryWorkspace,
        "Create the deterministic demo artifact."
    )
    Assert-FileText -Path (Join-Path $QueryWorkspace "ferric-mock.txt") -Expected "mock run"
    Add-Pass "offline query completes and writes its guarded artifact"

    $TraceDir = Join-Path $QueryWorkspace ".ferric\trace"
    $Traces = @(
        Get-ChildItem -LiteralPath $TraceDir -Filter "*.jsonl" -File |
            Sort-Object LastWriteTimeUtc -Descending
    )
    if ($Traces.Count -eq 0) {
        throw "query emitted no trace in $TraceDir"
    }
    $TracePath = $Traces[0].FullName
    Invoke-Ferric -CommandArguments @("trace", "cat", $TracePath)
    if (($script:LastFerricOutput -join [Environment]::NewLine) -notmatch "session end \(task_complete\)") {
        throw "trace did not record a task_complete session"
    }
    $MockArtifact = Join-Path $QueryWorkspace "ferric-mock.txt"
    Remove-Item -LiteralPath $MockArtifact
    Invoke-Ferric -CommandArguments @("trace", "verify", $TracePath)
    if (Test-Path -LiteralPath $MockArtifact) {
        throw "trace verification replayed a recorded write"
    }
    Add-Pass "trace is readable and safely verifiable"

    Set-Content -LiteralPath (Join-Path $QueryWorkspace ".env") -Value "DEMO_SECRET=not-for-models"
    Invoke-FerricExpectedFailure -CommandArguments @(
        "query",
        "--mock",
        "--workspace", $QueryWorkspace,
        "--file", ".env",
        "Read the attached secret."
    ) -ExpectedPattern "denied_read_file"
    $TraceCountAfterDenial = @(
        Get-ChildItem -LiteralPath $TraceDir -Filter "*.jsonl" -File
    ).Count
    if ($TraceCountAfterDenial -ne $Traces.Count) {
        throw "attachment denial created a trace, so inference may have started"
    }
    Add-Pass "sensitive attachments are denied before inference"

    Invoke-Ferric -CommandArguments @("skills", "list", "--workspace", $QueryWorkspace)
    Add-Pass "skill discovery is available without authorizing execution"

    $LaunchWorkspace = Join-Path $RunRoot "launched-project"
    Invoke-Ferric -CommandArguments @(
        "launch",
        "--name", "monday-demo",
        "--path", $LaunchWorkspace,
        "--goal", "Demonstrate a deterministic Ferric project launch."
    )
    foreach ($Relative in @(
        "README.md",
        ".gitignore",
        "docs\.sprint-loop-book",
        "docs\README.md",
        "docs\intents\INT-0001-initial-project-goal.md",
        "docs\work\tasks.md",
        "docs\work\completed-tasks.md"
    )) {
        if (-not (Test-Path -LiteralPath (Join-Path $LaunchWorkspace $Relative) -PathType Leaf)) {
            throw "launch omitted $Relative"
        }
    }
    Add-Pass "fully flagged project launch is non-interactive"

    $IcmWorkspace = Join-Path $RunRoot "icm"
    Invoke-Ferric -CommandArguments @("icm", "init", $IcmWorkspace)
    Invoke-Ferric -CommandArguments @("icm", "plan", $IcmWorkspace)
    Invoke-Ferric -CommandArguments @("icm", "run", "--auto", "--mock", $IcmWorkspace)
    foreach ($Stage in @("01_research", "02_script", "03_production")) {
        Assert-FileText -Path (Join-Path $IcmWorkspace "stages\$Stage\ferric-mock.txt") -Expected "mock run"
    }
    Add-Pass "three-stage ICM pipeline completes offline"

    $CronWorkspace = Join-Path $RunRoot "cron"
    Invoke-Ferric -CommandArguments @(
        "cron",
        "--workspace", $CronWorkspace,
        "add", "monday-demo",
        "--schedule", "1h",
        "--command", "query",
        "--prompt", "Create the scheduled demo artifact.",
        "--mock"
    )
    Invoke-Ferric -CommandArguments @("cron", "--workspace", $CronWorkspace, "run", "--dry-run")
    Invoke-Ferric -CommandArguments @("cron", "--workspace", $CronWorkspace, "run")
    Assert-FileText -Path (Join-Path $CronWorkspace "ferric-mock.txt") -Expected "mock run"
    Invoke-Ferric -CommandArguments @("cron", "--workspace", $CronWorkspace, "run")
    if (($script:LastFerricOutput -join [Environment]::NewLine) -notmatch "No jobs due") {
        throw "cron state did not suppress an immediate duplicate run"
    }
    Add-Pass "scheduled mock query runs once and persists due-state"

    Write-Host ""
    Write-Host ("DEMO SMOKE PASS: {0} checks" -f $Passed.Count) -ForegroundColor Green
    Write-Host "Workspace: $RunRoot"
}
finally {
    Set-Location -LiteralPath $RepoRoot
    if (-not $KeepWorkspace -and (Test-Path -LiteralPath $RunRoot)) {
        Remove-Item -LiteralPath $RunRoot -Recurse -Force
    }
}
