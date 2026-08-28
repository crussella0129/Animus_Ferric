[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$targetBoundary = Join-Path $repoRoot 'target'
$testRoot = Join-Path $repoRoot 'target\s114-probe-verifier-tests'
$expectedRoot = [System.IO.Path]::GetFullPath($testRoot).TrimEnd('\')
if (-not $expectedRoot.StartsWith(
        [System.IO.Path]::GetFullPath($repoRoot).TrimEnd('\') + '\target\',
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw 'verifier test root escapes the repository target directory'
}

function Assert-SafeTestRoot([string]$ExistingRoot) {
    $fullBoundary = [System.IO.Path]::GetFullPath($targetBoundary).TrimEnd('\')
    $fullRoot = [System.IO.Path]::GetFullPath($ExistingRoot).TrimEnd('\')
    if (-not $fullRoot.StartsWith(
            $fullBoundary + '\',
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'verifier test root escapes its deletion boundary'
    }
    $cursor = Get-Item -Force -LiteralPath $fullRoot
    while ($null -ne $cursor) {
        if ($cursor.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "verifier test path must not contain a reparse ancestor: $($cursor.FullName)"
        }
        if ([string]::Equals(
                $cursor.FullName.TrimEnd('\'),
                $fullBoundary,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            break
        }
        $parentPath = Split-Path -Parent $cursor.FullName
        if ([string]::IsNullOrWhiteSpace($parentPath)) {
            throw 'verifier test deletion boundary was not reached'
        }
        $cursor = Get-Item -Force -LiteralPath $parentPath
    }
    foreach ($item in @(
        Get-Item -Force -LiteralPath $fullRoot
        Get-ChildItem -Force -Recurse -LiteralPath $fullRoot
    )) {
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "verifier test tree must not contain a reparse point: $($item.FullName)"
        }
    }
}

function Write-Utf8([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, $utf8NoBom)
}

function Write-Json([string]$Path, [object]$Value) {
    Write-Utf8 $Path (($Value | ConvertTo-Json -Depth 20) + "`n")
}

function Rewrite-SelfManifest([string]$CaseRoot) {
    $evidenceRoot = Join-Path $CaseRoot 'evidence'
    $lines = @(
        Get-ChildItem -File -LiteralPath $evidenceRoot |
            Where-Object Name -ne 'files.sha256' |
            Sort-Object Name |
            ForEach-Object {
                $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
                "$hash  $($_.Name)"
            }
    )
    Write-Utf8 (Join-Path $evidenceRoot 'files.sha256') (($lines -join "`n") + "`n")
}

function New-Case([string]$Name) {
    $caseRoot = Join-Path $testRoot $Name
    New-Item -ItemType Directory -Path $caseRoot | Out-Null
    Copy-Item -Recurse -Force -LiteralPath (Join-Path $PSScriptRoot 'evidence') -Destination $caseRoot
    Copy-Item -Force -LiteralPath (Join-Path $PSScriptRoot 'capability-report.md') -Destination $caseRoot
    $caseRoot
}

function Assert-Rejected([string]$Name, [scriptblock]$Mutation, [string]$ExpectedText) {
    $caseRoot = New-Case $Name
    & $Mutation $caseRoot
    $output = @(& pwsh -NoProfile -File (Join-Path $PSScriptRoot 'verify-probe.ps1') `
        -ArtifactRoot $caseRoot 2>&1 | ForEach-Object { $_.ToString() }) -join "`n"
    if ($LASTEXITCODE -eq 0) {
        throw "negative verifier case unexpectedly passed: $Name"
    }
    if (-not $output.Contains($ExpectedText)) {
        throw "negative verifier case '$Name' missed expected diagnostic '$ExpectedText': $output"
    }
    [pscustomobject]@{
        case = $Name
        rejected = $true
        diagnostic = $ExpectedText
    }
}

if (-not (Test-Path -LiteralPath $targetBoundary -PathType Container) -or
    (Get-Item -Force -LiteralPath $targetBoundary).Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    )) {
    throw 'repository target directory is missing or is a reparse point'
}
if (Test-Path -LiteralPath $testRoot) {
    Assert-SafeTestRoot $testRoot
    $resolved = (Resolve-Path -LiteralPath $testRoot).Path.TrimEnd('\')
    if (-not [string]::Equals(
            $resolved,
            $expectedRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'existing verifier test root resolved unexpectedly'
    }
    Remove-Item -Recurse -Force -LiteralPath $resolved
}
New-Item -ItemType Directory -Path $testRoot | Out-Null
Assert-SafeTestRoot $testRoot

try {
    $results = @(
        Assert-Rejected 'nested-extra' {
            param($caseRoot)
            $nested = Join-Path $caseRoot 'evidence\nested'
            New-Item -ItemType Directory -Path $nested | Out-Null
            Write-Utf8 (Join-Path $nested 'extra.txt') "unexpected`n"
        } 'evidence payloads must be flat files'

        Assert-Rejected 'raw-output-rewrite' {
            param($caseRoot)
            Write-Utf8 (Join-Path $caseRoot 'evidence\source-remote.stdout.txt') `
                "https://example.invalid/changed.git`n"
            Rewrite-SelfManifest $caseRoot
        } 'source_remote_raw'

        Assert-Rejected 'empty-command-list' {
            param($caseRoot)
            $path = Join-Path $caseRoot 'evidence\capability-verdict.json'
            $verdict = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
            $verdict.commands = @()
            Write-Json $path $verdict
            Rewrite-SelfManifest $caseRoot
        } 'command_set_exact'

        Assert-Rejected 'duplicate-command' {
            param($caseRoot)
            $path = Join-Path $caseRoot 'evidence\capability-verdict.json'
            $verdict = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
            $verdict.commands = @($verdict.commands) + @($verdict.commands[0])
            Write-Json $path $verdict
            Rewrite-SelfManifest $caseRoot
        } 'duplicate command record'

        Assert-Rejected 'altered-gate' {
            param($caseRoot)
            $path = Join-Path $caseRoot 'evidence\capability-verdict.json'
            $verdict = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
            $verdict.capability_layers.authorized = 'yes'
            Write-Json $path $verdict
            Rewrite-SelfManifest $caseRoot
        } 'behavioral_layers_gated'

        Assert-Rejected 'traversal-manifest-path' {
            param($caseRoot)
            $path = Join-Path $caseRoot 'evidence\source-manifest.json'
            $manifest = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
            $manifest.files[0].path = '../LICENSE'
            Write-Json $path $manifest
            Rewrite-SelfManifest $caseRoot
        } 'manifest path is not a normalized relative path'

        Assert-Rejected 'wrong-workspace-identity' {
            param($caseRoot)
            Write-Utf8 (Join-Path $caseRoot 'evidence\workspace-commit.stdout.txt') `
                "0000000000000000000000000000000000000000`n"
            Rewrite-SelfManifest $caseRoot
        } 'workspace_commit_raw'
    )

    [pscustomobject]@{
        schema = 'animus-ferric-s114-sprint-loop-verifier-tests-v1'
        passed = $true
        negative_cases = $results.Count
        results = $results
    } | ConvertTo-Json -Depth 5
}
finally {
    if (Test-Path -LiteralPath $testRoot) {
        Assert-SafeTestRoot $testRoot
        $resolved = (Resolve-Path -LiteralPath $testRoot).Path.TrimEnd('\')
        if ([string]::Equals(
                $resolved,
                $expectedRoot,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            Remove-Item -Recurse -Force -LiteralPath $resolved
        }
    }
}
