[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$anchorPath = Join-Path $artifactDir 'raw-source-anchor.json'
$selfTestPath = Join-Path $artifactDir 'runtime-self-test.json'
$controlPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'
$verifierPath = Join-Path $artifactDir 'verify-runtime.ps1'

function Assert-RecoveryCondition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Read-RecoveryJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Resolve-RecoveryPath {
    param([Parameter(Mandatory = $true)][string]$RelativePath)
    Assert-RecoveryCondition `
        (-not [System.IO.Path]::IsPathRooted($RelativePath)) `
        "recovery path must be repository-relative: $RelativePath"
    $root = [System.IO.Path]::GetFullPath($repoRoot)
    $resolved = [System.IO.Path]::GetFullPath((Join-Path $root $RelativePath))
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-RecoveryCondition `
        ($resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) `
        "recovery path escapes the repository: $RelativePath"
    $resolved
}

function Assert-FileAnchor {
    param([Parameter(Mandatory = $true)]$Anchor)
    $path = Resolve-RecoveryPath -RelativePath ([string]$Anchor.relative_path)
    Assert-RecoveryCondition `
        (Test-Path -LiteralPath $path -PathType Leaf) `
        "anchored file is absent: $($Anchor.relative_path)"
    $item = Get-Item -LiteralPath $path -Force
    Assert-RecoveryCondition (-not $item.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) "anchored file is a reparse point: $($Anchor.relative_path)"
    Assert-RecoveryCondition `
        ([UInt64]$item.Length -eq [UInt64]$Anchor.bytes) `
        "anchored byte count differs: $($Anchor.relative_path)"
    Assert-RecoveryCondition `
        ((Get-Sha256Lower -Path $path) -ceq [string]$Anchor.sha256) `
        "anchored SHA-256 differs: $($Anchor.relative_path)"
    $path
}

function Test-ExactRecoveryTree {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)]$Anchor
    )
    $resolvedRoot = [System.IO.Path]::GetFullPath($Root)
    Assert-RecoveryCondition `
        (Test-Path -LiteralPath $resolvedRoot -PathType Container) `
        "recovery source tree is absent: $resolvedRoot"
    $rootItem = Get-Item -LiteralPath $resolvedRoot -Force
    Assert-RecoveryCondition (-not $rootItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) 'recovery source root is a reparse point'
    $reparseEntries = @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -Force |
            Where-Object {
                $_.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            }
    )
    Assert-RecoveryCondition `
        ($reparseEntries.Count -eq 0) `
        'recovery source tree contains a reparse point'

    $manifestPath = Join-Path $resolvedRoot ([string]$Anchor.manifest.path)
    Assert-RecoveryCondition `
        (Test-Path -LiteralPath $manifestPath -PathType Leaf) `
        'raw source manifest is absent'
    $manifestItem = Get-Item -LiteralPath $manifestPath
    Assert-RecoveryCondition `
        ([UInt64]$manifestItem.Length -eq [UInt64]$Anchor.manifest.bytes) `
        'raw source manifest byte count differs'
    Assert-RecoveryCondition `
        ((Get-Sha256Lower -Path $manifestPath) -ceq
            [string]$Anchor.manifest.sha256) `
        'raw source manifest SHA-256 differs'

    $expected = [System.Collections.Generic.Dictionary[string, object]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    foreach ($entry in @($Anchor.files)) {
        $relative = ([string]$entry.path).Replace('\', '/')
        Assert-RecoveryCondition `
            (-not [System.IO.Path]::IsPathRooted($relative) -and
                $relative -notmatch '(^|/)\.\.(/|$)' -and
                $relative -notmatch '(^|/)\.(/|$)') `
            "unsafe raw source entry: $relative"
        Assert-RecoveryCondition `
            (-not $expected.ContainsKey($relative)) `
            "duplicate raw source entry: $relative"
        $expected.Add($relative, $entry)
    }
    Assert-RecoveryCondition `
        ($expected.Count -eq [int]$Anchor.manifest.entry_count) `
        'raw source anchor entry count differs'

    $actualFiles = @(
        Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse -Force |
            Where-Object { $_.FullName -cne $manifestPath }
    )
    Assert-RecoveryCondition `
        ($actualFiles.Count -eq $expected.Count) `
        'raw source exact-tree file count differs'
    $actualPayloadBytes = [UInt64]0
    foreach ($file in $actualFiles) {
        $relative = [System.IO.Path]::GetRelativePath(
            $resolvedRoot,
            $file.FullName
        ).Replace('\', '/')
        Assert-RecoveryCondition `
            ($expected.ContainsKey($relative)) `
            "raw source contains an unlisted file: $relative"
        $entry = $expected[$relative]
        Assert-RecoveryCondition `
            ([UInt64]$file.Length -eq [UInt64]$entry.bytes) `
            "raw source byte count differs: $relative"
        Assert-RecoveryCondition `
            ((Get-Sha256Lower -Path $file.FullName) -ceq
                [string]$entry.sha256) `
            "raw source SHA-256 differs: $relative"
        $actualPayloadBytes += [UInt64]$file.Length
    }
    Assert-RecoveryCondition `
        ($actualPayloadBytes -eq [UInt64]$Anchor.manifest.payload_bytes) `
        'raw source payload byte total differs'

    $manifestRows = @(
        Get-Content -LiteralPath $manifestPath | ForEach-Object {
            Assert-RecoveryCondition `
                ($_ -cmatch '^([0-9a-f]{64})  (.+)$') `
                'raw source manifest contains a malformed row'
            [pscustomobject]@{
                sha256 = $Matches[1]
                path = $Matches[2].Replace('\', '/')
            }
        }
    )
    Assert-RecoveryCondition `
        ($manifestRows.Count -eq $expected.Count) `
        'raw source manifest row count differs'
    $manifestSeen = [System.Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    foreach ($row in $manifestRows) {
        Assert-RecoveryCondition `
            ($manifestSeen.Add([string]$row.path)) `
            "raw source manifest contains a duplicate: $($row.path)"
        Assert-RecoveryCondition `
            ($expected.ContainsKey([string]$row.path)) `
            "raw source manifest contains an unknown path: $($row.path)"
        Assert-RecoveryCondition `
            ([string]$row.sha256 -ceq
                [string]$expected[[string]$row.path].sha256) `
            "raw source manifest hash differs from anchor: $($row.path)"
    }

    [ordered]@{
        passed = $true
        manifest_sha256 = Get-Sha256Lower -Path $manifestPath
        entries = $expected.Count
        payload_bytes = $actualPayloadBytes
    }
}

function Get-TextSha256 {
    param([Parameter(Mandatory = $true)][string]$Text)
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($Text)
    $hash = [System.Security.Cryptography.SHA256]::HashData($bytes)
    [Convert]::ToHexString($hash).ToLowerInvariant()
}

Assert-RecoveryCondition `
    (-not (Test-Path -LiteralPath $controlPath)) `
    'epoch-4 controls already exist and will not be overwritten'
Assert-RecoveryCondition `
    (-not (Test-Path -LiteralPath $digestPath)) `
    'epoch-4 control digest already exists and will not be overwritten'

$plan = Read-RecoveryJson -Path $planPath
$anchor = Read-RecoveryJson -Path $anchorPath
$selfTest = Read-RecoveryJson -Path $selfTestPath
$expectedHead = 'a1306e5191591600551ef7c2c8676f061e8d554f'
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-RecoveryCondition ($LASTEXITCODE -eq 0) 'could not resolve repository HEAD'
Assert-RecoveryCondition ($head -ceq $expectedHead) 'repository HEAD differs from epoch-4 baseline'
Assert-RecoveryCondition `
    ([string]$plan.schema -ceq 'animus-ferric-runtime-recovery-plan-v4' -and
        [string]$plan.task -ceq 'T-11409' -and
        [int]$plan.publication_epoch -eq 4 -and
        [int]$plan.execution_epoch -eq 3 -and
        [string]$plan.timestamp_protocol -ceq
            'powershell-json-datekind-string-rfc3339-v1' -and
        [string]$plan.repository_commit_before_epoch_4_controls -ceq $head -and
        [string]$plan.operation.id -ceq
            'r04-publish-e03-01-q4-32768' -and
        [string]$plan.operation.coordinate -ceq 'e03-01-q4-32768' -and
        [string]$plan.operation.source_attempt_schema -ceq
            'animus-ferric-runtime-attempt-v3' -and
        [int]$plan.operation.exact_manifest_entries -eq 49) `
    'runtime recovery plan identity differs'
Assert-RecoveryCondition `
    ([string]$anchor.schema -ceq
        'animus-ferric-runtime-raw-source-anchor-v1' -and
        [string]$anchor.operation_id -ceq [string]$plan.operation.id -and
        [int]$anchor.execution_epoch -eq 3 -and
        [int]$anchor.publication_epoch -eq 4 -and
        [string]$anchor.source_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$anchor.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$anchor.manifest.entry_count -eq 49 -and
        [string]$anchor.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [string]$anchor.selected.attempt.sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$anchor.selected.attestation.sha256 -ceq
            [string]$plan.operation.attestation.sha256) `
    'raw source anchor identity differs from the plan'

foreach ($epochThreeAnchor in @(
        $plan.epoch_3.control_manifest,
        $plan.epoch_3.control_digest,
        $plan.epoch_3.runtime_plan,
        $plan.epoch_3.runtime_self_test
    )) {
    [void](Assert-FileAnchor -Anchor $epochThreeAnchor)
}
$epochThreeDigestPath = Resolve-RecoveryPath `
    -RelativePath ([string]$plan.epoch_3.control_digest.relative_path)
$epochThreeDigestLine = (Get-Content -Raw -LiteralPath $epochThreeDigestPath).
    TrimEnd("`r", "`n")
Assert-RecoveryCondition `
    ($epochThreeDigestLine -ceq
        [string]$plan.epoch_3.control_manifest_digest_line) `
    'epoch-3 control digest line differs'
$epochThreeControls = Read-RecoveryJson -Path (
    Resolve-RecoveryPath `
        -RelativePath ([string]$plan.epoch_3.control_manifest.relative_path)
)
Assert-RecoveryCondition `
    ([string]$epochThreeControls.schema -ceq
        'animus-ferric-runtime-control-inputs-v3' -and
        [string]$epochThreeControls.task -ceq 'T-11409' -and
        [int]$epochThreeControls.control_epoch -eq 3 -and
        [bool]$epochThreeControls.recovery_anchors.passed -and
        [bool]$epochThreeControls.measurement_continuity.passed -and
        [string]$epochThreeControls.repository.head_at_freeze -ceq $head) `
    'epoch-3 frozen controls are not valid recovery inputs'
$epochThreeRoot = Resolve-RecoveryPath `
    -RelativePath ([string]$plan.source_artifact_relative_path)
foreach ($entry in @($epochThreeControls.controls)) {
    $entryPath = [System.IO.Path]::GetFullPath((Join-Path $epochThreeRoot `
        ([string]$entry.path)))
    $entryPrefix = "$epochThreeRoot$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-RecoveryCondition `
        ($entryPath.StartsWith($entryPrefix,
                [StringComparison]::OrdinalIgnoreCase)) `
        "epoch-3 frozen control path escapes its root: $($entry.path)"
    Assert-RecoveryCondition `
        (Test-Path -LiteralPath $entryPath -PathType Leaf) `
        "epoch-3 frozen control is absent: $($entry.path)"
    Assert-RecoveryCondition `
        ([UInt64](Get-Item -LiteralPath $entryPath).Length -eq
            [UInt64]$entry.bytes -and
            (Get-Sha256Lower -Path $entryPath) -ceq [string]$entry.sha256) `
        "epoch-3 frozen control differs: $($entry.path)"
}

$staticNames = @(Get-EpochFourStaticControlNames)
Assert-RecoveryCondition ($staticNames.Count -eq 12) `
    'epoch-4 static control name set must contain exactly 12 files'
Assert-RecoveryCondition `
    (@($staticNames | Select-Object -Unique).Count -eq 12) `
    'epoch-4 static control name set contains a duplicate'
Assert-RecoveryCondition `
    ([string]$selfTest.schema -ceq
        'animus-ferric-runtime-recovery-self-test-v4' -and
        [string]$selfTest.task -ceq 'T-11409' -and
        [int]$selfTest.publication_epoch -eq 4 -and
        [int]$selfTest.execution_epoch -eq 3 -and
        [string]$selfTest.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [bool]$selfTest.passed) `
    'epoch-4 runtime self-test is not green'
$selfTestStatic = @($selfTest.static_controls)
Assert-RecoveryCondition ($selfTestStatic.Count -eq 12) `
    'runtime self-test static control count differs'
$frozenStaticControls = @()
foreach ($name in $staticNames) {
    $matches = @($selfTestStatic | Where-Object {
            [string]$_.path -ceq [string]$name
        })
    Assert-RecoveryCondition ($matches.Count -eq 1) `
        "runtime self-test has no unique static identity for $name"
    $path = Join-Path $artifactDir $name
    Assert-RecoveryCondition (Test-Path -LiteralPath $path -PathType Leaf) `
        "epoch-4 static control is absent: $name"
    $item = Get-Item -LiteralPath $path
    $hash = Get-Sha256Lower -Path $path
    Assert-RecoveryCondition `
        ([UInt64]$matches[0].bytes -eq [UInt64]$item.Length -and
            [string]$matches[0].sha256 -ceq $hash) `
        "epoch-4 static control differs from the green self-test: $name"
    $frozenStaticControls += [ordered]@{
        path = [string]$name
        bytes = [UInt64]$item.Length
        sha256 = $hash
    }
}
if ($selfTest.PSObject.Properties.Name -contains 'results') {
    $results = @($selfTest.results)
    Assert-RecoveryCondition ($results.Count -gt 0) 'runtime self-test has no results'
    Assert-RecoveryCondition `
        (@($results | Where-Object { -not [bool]$_.passed }).Count -eq 0) `
        'runtime self-test contains a failed result'
    Assert-RecoveryCondition `
        (@($results.name | Select-Object -Unique).Count -eq $results.Count) `
        'runtime self-test result names are not unique'
}
Assert-RecoveryCondition `
    ([bool]$selfTest.live_q4_identity.passed -and
        [UInt64]$selfTest.live_q4_identity.bytes -eq
            [UInt64]$plan.model.bytes -and
        [string]$selfTest.live_q4_identity.sha256 -ceq
            [string]$plan.model.sha256) `
    'runtime self-test Q4 identity differs'

$sourceRoot = Resolve-RecoveryPath `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-RecoveryPath `
    -RelativePath ([string]$plan.operation.destination_relative_path)
Assert-RecoveryCondition (-not (Test-Path -LiteralPath $destinationRoot)) `
    'recovery destination already exists at freeze'
$treeCheck = Test-ExactRecoveryTree -Root $sourceRoot -Anchor $anchor
$attempt = Read-RecoveryJson -Path (Join-Path $sourceRoot 'attempt.json')
$terminal = $plan.operation.expected_terminal
Assert-RecoveryCondition `
    ([string]$attempt.schema -ceq [string]$plan.operation.source_attempt_schema -and
        [int]$attempt.control_epoch -eq 3 -and
        [string]$attempt.task -ceq 'T-11409' -and
        [string]$attempt.coordinate -ceq [string]$plan.operation.coordinate -and
        [string]$attempt.quant -ceq [string]$terminal.quant -and
        [int]$attempt.context -eq [int]$terminal.context -and
        [string]$attempt.verdict -ceq [string]$terminal.verdict -and
        [bool]$attempt.evidence_complete -eq [bool]$terminal.evidence_complete -and
        [bool]$attempt.startup.healthy -eq [bool]$terminal.startup_healthy -and
        [bool]$attempt.attestation.passed -eq [bool]$terminal.attestation_passed -and
        [bool]$attempt.smoke.passed -eq [bool]$terminal.smoke_passed -and
        [bool]$attempt.throughput.passed -eq [bool]$terminal.throughput_passed -and
        [bool]$attempt.teardown.passed -eq [bool]$terminal.teardown_passed -and
        [double]$attempt.throughput.median_decoded_tokens_per_second -eq
            [double]$terminal.median_decoded_tokens_per_second) `
    'raw source does not retain the frozen viable terminal facts'

$sourceVerificationProcess = Invoke-PowerShellFileBounded `
    -ScriptPath $verifierPath `
    -Arguments @(
        '-AttemptPath',
        $sourceRoot,
        '-UnfrozenRecoverySource'
    )
$sourceVerification = try {
    $sourceVerificationProcess.stdout | ConvertFrom-Json -DateKind String
}
catch { $null }
Assert-RecoveryCondition `
    ($sourceVerificationProcess.exit_code -eq 0 -and
        $null -ne $sourceVerification -and
        [string]$sourceVerification.schema -ceq
            'animus-ferric-runtime-recovery-verification-v4' -and
        [bool]$sourceVerification.passed -and
        [string]$sourceVerification.operation_id -ceq
            [string]$plan.operation.id -and
        [int]$sourceVerification.execution_epoch -eq 3 -and
        [int]$sourceVerification.publication_epoch -eq 4 -and
        [string]$sourceVerification.source_attempt_schema -ceq
            [string]$plan.operation.source_attempt_schema -and
        [string]$sourceVerification.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [int]$sourceVerification.control_epoch -eq 3 -and
        [string]$sourceVerification.attestation_protocol -ceq
            [string]$epochThreeControls.attestation_protocol -and
        [string]$sourceVerification.process_command_protocol -ceq
            [string]$epochThreeControls.process_command_protocol -and
        [string]$sourceVerification.coordinate -ceq
            [string]$plan.operation.coordinate -and
        [string]$sourceVerification.verdict -ceq
            [string]$plan.operation.expected_terminal.verdict -and
        [string]$sourceVerification.control_anchor_mode -ceq
            'unfrozen_raw_recovery' -and
        [bool]$sourceVerification.live_model_identity.checked -and
        [string]$sourceVerification.live_model_identity.mode -ceq
            'checked_in_verifier' -and
        [string]$sourceVerification.live_model_identity.sha256 -ceq
            [string]$plan.model.sha256 -and
        [bool]$sourceVerification.manifest.passed -and
        [int]$sourceVerification.manifest.entries -eq
            [int]$plan.operation.exact_manifest_entries -and
        [bool]$sourceVerification.recovery_anchor.applicable -and
        [bool]$sourceVerification.recovery_anchor.passed -and
        [int]$sourceVerification.recovery_anchor.observed_entries -eq
            [int]$plan.operation.exact_manifest_entries) `
    "corrected source verification failed: $($sourceVerificationProcess.stderr)"

$localRunfile = Join-Path $repoRoot '.ferric/server.json'
$globalRunfile = Join-Path $env:APPDATA 'ferric/server.json'
$listeners = @(Get-NetTCPConnection -State Listen -LocalPort 8080 `
        -ErrorAction SilentlyContinue)
$llamaProcesses = @(Get-CimInstance Win32_Process `
        -Filter "Name = 'llama-server.exe'" -ErrorAction Stop)
$coldState = [ordered]@{
    local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
    global_runfile_absent = -not (Test-Path -LiteralPath $globalRunfile)
    listener_absent = ($listeners.Count -eq 0)
    llama_server_process_absent = ($llamaProcesses.Count -eq 0)
    memory = Get-MemorySnapshot
}
Assert-RecoveryCondition `
    ($coldState.local_runfile_absent -and
        $coldState.global_runfile_absent -and
        $coldState.listener_absent -and
        $coldState.llama_server_process_absent) `
    'Ferric/llama-server state is not cold at recovery freeze'

$modelPath = Resolve-RecoveryPath -RelativePath ([string]$plan.model.relative_path)
Assert-RecoveryCondition (Test-Path -LiteralPath $modelPath -PathType Leaf) `
    'frozen Q4 model is absent'
$modelItem = Get-Item -LiteralPath $modelPath
$modelHash = Get-Sha256Lower -Path $modelPath
Assert-RecoveryCondition `
    ([UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes -and
        $modelHash -ceq [string]$plan.model.sha256) `
    'independent freeze-time Q4 identity differs'

$controlManifest = [ordered]@{
    schema = 'animus-ferric-runtime-recovery-control-inputs-v4'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$plan.timestamp_protocol
    frozen_at_utc = (Get-Date).ToUniversalTime().ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    raw_source_anchor_sha256 = Get-Sha256Lower -Path $anchorPath
    repository = [ordered]@{
        head_at_freeze = $head
        epoch_4_pre_control_base = $expectedHead
    }
    epoch_3 = [ordered]@{
        control_manifest = $plan.epoch_3.control_manifest
        control_digest = $plan.epoch_3.control_digest
        runtime_plan = $plan.epoch_3.runtime_plan
        runtime_self_test = $plan.epoch_3.runtime_self_test
        control_manifest_digest_line =
            [string]$plan.epoch_3.control_manifest_digest_line
        transitive_controls_checked = @($epochThreeControls.controls).Count
        passed = $true
    }
    static_controls = @($frozenStaticControls)
    runtime_self_test = [ordered]@{
        relative_path = 'docs/sprints/s114/control-artifacts/runtime/epoch-4/runtime-self-test.json'
        bytes = [UInt64](Get-Item -LiteralPath $selfTestPath).Length
        sha256 = Get-Sha256Lower -Path $selfTestPath
        passed = $true
    }
    raw_source = [ordered]@{
        relative_path = [string]$plan.operation.source_raw_relative_path
        manifest_sha256 = [string]$treeCheck.manifest_sha256
        entries = [int]$treeCheck.entries
        payload_bytes = [UInt64]$treeCheck.payload_bytes
        terminal_facts_passed = $true
    }
    source_verification = [ordered]@{
        schema = [string]$sourceVerification.schema
        operation_id = [string]$sourceVerification.operation_id
        passed = [bool]$sourceVerification.passed
        report_sha256 = Get-TextSha256 `
            -Text ([string]$sourceVerificationProcess.stdout)
        hash_deferral_used = $false
    }
    model = [ordered]@{
        relative_path = [string]$plan.model.relative_path
        bytes = [UInt64]$modelItem.Length
        sha256 = $modelHash
        independently_rehashed = $true
        passed = $true
    }
    cold_state = $coldState
    destination = [ordered]@{
        relative_path = [string]$plan.operation.destination_relative_path
        absent_at_freeze = $true
    }
}

$stageParent = Resolve-RecoveryPath `
    -RelativePath 'target/s114-experiment/recovery-control-stage'
[System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
$stageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
$stageControl = Join-Path $stageOwner 'control-inputs.json'
$stageDigest = Join-Path $stageOwner 'control-inputs.sha256'
$controlPublished = $false
try {
    Write-JsonLf -Path $stageControl -Value $controlManifest -Depth 32
    $controlHash = Get-Sha256Lower -Path $stageControl
    Write-Utf8Lf -Path $stageDigest `
        -Text "$controlHash  control-inputs.json`n"
    [System.IO.File]::Move($stageControl, $controlPath, $false)
    try {
        [System.IO.File]::Move($stageDigest, $digestPath, $false)
        $controlPublished = $true
    }
    catch {
        if ((Test-Path -LiteralPath $controlPath -PathType Leaf) -and
            (Get-Sha256Lower -Path $controlPath) -ceq $controlHash) {
            [System.IO.File]::Delete($controlPath)
        }
        throw
    }
}
finally {
    if (Test-Path -LiteralPath $stageOwner -PathType Container) {
        [System.IO.Directory]::Delete($stageOwner, $true)
    }
}
Assert-RecoveryCondition $controlPublished 'epoch-4 controls were not published'

[ordered]@{
    control_manifest = 'control-inputs.json'
    control_manifest_sha256 = Get-Sha256Lower -Path $controlPath
    control_digest = 'control-inputs.sha256'
    operation_id = [string]$plan.operation.id
    source_entries = [int]$treeCheck.entries
    source_verification_passed = [bool]$sourceVerification.passed
    independently_rehashed_q4 = $true
    passed = $true
} | ConvertTo-Json -Depth 8
