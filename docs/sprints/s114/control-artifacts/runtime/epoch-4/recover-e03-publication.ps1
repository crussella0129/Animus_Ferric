[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$anchorPath = Join-Path $artifactDir 'raw-source-anchor.json'
$controlPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'
$selfTestPath = Join-Path $artifactDir 'runtime-self-test.json'
$verifierPath = Join-Path $artifactDir 'verify-runtime.ps1'
$publicationPath = Join-Path $artifactDir 'recovery-publication.json'

function Assert-PublicationCondition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Read-PublicationJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Resolve-PublicationPath {
    param([Parameter(Mandatory = $true)][string]$RelativePath)
    Assert-PublicationCondition `
        (-not [System.IO.Path]::IsPathRooted($RelativePath)) `
        "publication path must be repository-relative: $RelativePath"
    $root = [System.IO.Path]::GetFullPath($repoRoot)
    $resolved = [System.IO.Path]::GetFullPath((Join-Path $root $RelativePath))
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-PublicationCondition `
        ($resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) `
        "publication path escapes the repository: $RelativePath"
    $resolved
}

function Assert-PublicationFileAnchor {
    param([Parameter(Mandatory = $true)]$Anchor)
    $path = Resolve-PublicationPath -RelativePath ([string]$Anchor.relative_path)
    Assert-PublicationCondition `
        (Test-Path -LiteralPath $path -PathType Leaf) `
        "anchored file is absent: $($Anchor.relative_path)"
    $item = Get-Item -LiteralPath $path -Force
    Assert-PublicationCondition (-not $item.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) "anchored file is a reparse point: $($Anchor.relative_path)"
    Assert-PublicationCondition `
        ([UInt64]$item.Length -eq [UInt64]$Anchor.bytes -and
            (Get-Sha256Lower -Path $path) -ceq [string]$Anchor.sha256) `
        "anchored file differs: $($Anchor.relative_path)"
    $path
}

function Test-PublicationExactTree {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)]$Anchor
    )
    $resolvedRoot = [System.IO.Path]::GetFullPath($Root)
    Assert-PublicationCondition `
        (Test-Path -LiteralPath $resolvedRoot -PathType Container) `
        "publication tree is absent: $resolvedRoot"
    $rootItem = Get-Item -LiteralPath $resolvedRoot -Force
    Assert-PublicationCondition (-not $rootItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) 'publication tree root is a reparse point'
    $reparseEntries = @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -Force |
            Where-Object {
                $_.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            }
    )
    Assert-PublicationCondition ($reparseEntries.Count -eq 0) `
        'publication tree contains a reparse point'
    $manifestPath = Join-Path $resolvedRoot ([string]$Anchor.manifest.path)
    Assert-PublicationCondition `
        (Test-Path -LiteralPath $manifestPath -PathType Leaf) `
        'publication manifest is absent'
    Assert-PublicationCondition `
        ([UInt64](Get-Item -LiteralPath $manifestPath).Length -eq
            [UInt64]$Anchor.manifest.bytes -and
            (Get-Sha256Lower -Path $manifestPath) -ceq
                [string]$Anchor.manifest.sha256) `
        'publication manifest differs from its frozen anchor'
    $manifestCheck = Test-HashManifest -Root $resolvedRoot `
        -ManifestPath $manifestPath -RejectUnlistedFiles
    Assert-PublicationCondition ([bool]$manifestCheck.passed) `
        "publication manifest verification failed: $(@($manifestCheck.errors) -join '; ')"
    $expected = @($Anchor.files)
    Assert-PublicationCondition `
        ($expected.Count -eq [int]$Anchor.manifest.entry_count) `
        'publication anchor entry count differs'
    $actualFiles = @(
        Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse -Force |
            Where-Object { $_.FullName -cne $manifestPath }
    )
    Assert-PublicationCondition ($actualFiles.Count -eq $expected.Count) `
        'publication exact-tree file count differs'
    $actualPayloadBytes = [UInt64]0
    foreach ($entry in $expected) {
        $relative = ([string]$entry.path).Replace('/',
            [System.IO.Path]::DirectorySeparatorChar)
        Assert-PublicationCondition `
            (-not [System.IO.Path]::IsPathRooted($relative) -and
                $relative -notmatch '(^|[\\/])\.\.([\\/]|$)') `
            "unsafe anchored publication path: $($entry.path)"
        $path = [System.IO.Path]::GetFullPath((Join-Path $resolvedRoot $relative))
        $prefix = "$resolvedRoot$([System.IO.Path]::DirectorySeparatorChar)"
        Assert-PublicationCondition `
            ($path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) `
            "anchored publication path escapes its root: $($entry.path)"
        Assert-PublicationCondition `
            (Test-Path -LiteralPath $path -PathType Leaf) `
            "anchored publication file is absent: $($entry.path)"
        $item = Get-Item -LiteralPath $path
        Assert-PublicationCondition `
            ([UInt64]$item.Length -eq [UInt64]$entry.bytes -and
                (Get-Sha256Lower -Path $path) -ceq [string]$entry.sha256) `
            "anchored publication file differs: $($entry.path)"
        $actualPayloadBytes += [UInt64]$item.Length
    }
    Assert-PublicationCondition `
        ($actualPayloadBytes -eq [UInt64]$Anchor.manifest.payload_bytes) `
        'publication payload byte total differs'
    [ordered]@{
        passed = $true
        manifest_sha256 = Get-Sha256Lower -Path $manifestPath
        entries = $expected.Count
        payload_bytes = $actualPayloadBytes
    }
}

function Invoke-CorrectedPublicationVerification {
    param(
        [Parameter(Mandatory = $true)][string]$AttemptPath,
        [switch]$RecoveryPublicationStage
    )
    $arguments = @('-AttemptPath', $AttemptPath)
    if ($RecoveryPublicationStage) {
        $arguments += '-RecoveryPublicationStage'
    }
    $expectedAnchorMode = if ($RecoveryPublicationStage) {
        'epoch_4_frozen_publication_stage'
    }
    else {
        'epoch_4_frozen_recovery'
    }
    $process = Invoke-PowerShellFileBounded -ScriptPath $verifierPath `
        -Arguments $arguments
    $report = try {
        $process.stdout | ConvertFrom-Json -DateKind String
    }
    catch { $null }
    Assert-PublicationCondition `
        ($process.exit_code -eq 0 -and
            $null -ne $report -and
            [string]$report.schema -ceq
                'animus-ferric-runtime-recovery-verification-v4' -and
            [bool]$report.passed -and
            [string]$report.operation_id -ceq
                [string]$plan.operation.id -and
            [int]$report.execution_epoch -eq 3 -and
            [int]$report.publication_epoch -eq 4 -and
            [string]$report.source_attempt_schema -ceq
                [string]$plan.operation.source_attempt_schema -and
            [string]$report.timestamp_protocol -ceq
                [string]$plan.timestamp_protocol -and
            [int]$report.control_epoch -eq 3 -and
            [string]$report.attestation_protocol -ceq
                [string]$plan.template_attestation.protocol -and
            [string]$report.process_command_protocol -ceq
                [string]$plan.process_command_attestation.protocol -and
            [string]$report.coordinate -ceq
                [string]$plan.operation.coordinate -and
            [string]$report.verdict -ceq
                [string]$plan.operation.expected_terminal.verdict -and
            [string]$report.control_anchor_mode -ceq $expectedAnchorMode -and
            [bool]$report.live_model_identity.checked -and
            [string]$report.live_model_identity.mode -ceq
                'checked_in_verifier' -and
            [string]$report.live_model_identity.sha256 -ceq
                [string]$plan.model.sha256 -and
            [bool]$report.manifest.passed -and
            [int]$report.manifest.entries -eq
                [int]$plan.operation.exact_manifest_entries -and
            [bool]$report.recovery_anchor.applicable -and
            [bool]$report.recovery_anchor.passed -and
            [int]$report.recovery_anchor.observed_entries -eq
                [int]$plan.operation.exact_manifest_entries) `
        "corrected publication verification failed: $($process.stderr)"
    $report
}

$plan = Read-PublicationJson -Path $planPath
$anchor = Read-PublicationJson -Path $anchorPath
Assert-PublicationCondition `
    ([string]$plan.schema -ceq 'animus-ferric-runtime-recovery-plan-v4' -and
        [string]$plan.task -ceq 'T-11409' -and
        [int]$plan.execution_epoch -eq 3 -and
        [int]$plan.publication_epoch -eq 4 -and
        [string]$plan.timestamp_protocol -ceq
            'powershell-json-datekind-string-rfc3339-v1' -and
        [string]$plan.operation.id -ceq
            'r04-publish-e03-01-q4-32768') `
    'runtime recovery plan identity differs'
Assert-PublicationCondition `
    ([string]$anchor.operation_id -ceq [string]$plan.operation.id -and
        [string]$anchor.source_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$anchor.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$anchor.manifest.entry_count -eq 49) `
    'raw source anchor identity differs'
Assert-PublicationCondition `
    (Test-Path -LiteralPath $controlPath -PathType Leaf) `
    'epoch-4 frozen control manifest is absent'
Assert-PublicationCondition `
    (Test-Path -LiteralPath $digestPath -PathType Leaf) `
    'epoch-4 frozen control digest is absent'
$controlHash = Get-Sha256Lower -Path $controlPath
$digestLine = (Get-Content -Raw -LiteralPath $digestPath).TrimEnd("`r", "`n")
Assert-PublicationCondition `
    ($digestLine -ceq "$controlHash  control-inputs.json") `
    'epoch-4 frozen control digest differs'
$controls = Read-PublicationJson -Path $controlPath
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-PublicationCondition ($LASTEXITCODE -eq 0) 'could not resolve repository HEAD'
Assert-PublicationCondition `
    ([string]$controls.schema -ceq
        'animus-ferric-runtime-recovery-control-inputs-v4' -and
        [string]$controls.task -ceq 'T-11409' -and
        [string]$controls.operation_id -ceq [string]$plan.operation.id -and
        [int]$controls.execution_epoch -eq 3 -and
        [int]$controls.publication_epoch -eq 4 -and
        [string]$controls.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [string]$controls.repository.head_at_freeze -ceq $head -and
        [string]$controls.repository.epoch_4_pre_control_base -ceq
            [string]$plan.repository_commit_before_epoch_4_controls -and
        [string]$controls.runtime_plan_sha256 -ceq
            (Get-Sha256Lower -Path $planPath) -and
        [string]$controls.raw_source_anchor_sha256 -ceq
            (Get-Sha256Lower -Path $anchorPath) -and
        [bool]$controls.epoch_3.passed -and
        [bool]$controls.runtime_self_test.passed -and
        [bool]$controls.source_verification.passed -and
        -not [bool]$controls.source_verification.hash_deferral_used -and
        [bool]$controls.model.passed -and
        [bool]$controls.model.independently_rehashed -and
        [bool]$controls.destination.absent_at_freeze) `
    'epoch-4 frozen control manifest is invalid'

$staticNames = @(Get-EpochFourStaticControlNames)
$frozenStatic = @($controls.static_controls)
Assert-PublicationCondition `
    ($staticNames.Count -eq 12 -and $frozenStatic.Count -eq 12) `
    'epoch-4 frozen static control count differs'
foreach ($name in $staticNames) {
    $matches = @($frozenStatic | Where-Object {
            [string]$_.path -ceq [string]$name
        })
    Assert-PublicationCondition ($matches.Count -eq 1) `
        "epoch-4 frozen static identity is absent or duplicate: $name"
    $path = Join-Path $artifactDir $name
    Assert-PublicationCondition `
        (Test-Path -LiteralPath $path -PathType Leaf) `
        "epoch-4 frozen static control is absent: $name"
    Assert-PublicationCondition `
        ([UInt64](Get-Item -LiteralPath $path).Length -eq
            [UInt64]$matches[0].bytes -and
            (Get-Sha256Lower -Path $path) -ceq
                [string]$matches[0].sha256) `
        "epoch-4 frozen static control differs: $name"
}
Assert-PublicationCondition `
    ([UInt64](Get-Item -LiteralPath $selfTestPath).Length -eq
        [UInt64]$controls.runtime_self_test.bytes -and
        (Get-Sha256Lower -Path $selfTestPath) -ceq
            [string]$controls.runtime_self_test.sha256) `
    'epoch-4 runtime self-test differs from frozen controls'
foreach ($epochThreeAnchor in @(
        $plan.epoch_3.control_manifest,
        $plan.epoch_3.control_digest,
        $plan.epoch_3.runtime_plan,
        $plan.epoch_3.runtime_self_test
    )) {
    [void](Assert-PublicationFileAnchor -Anchor $epochThreeAnchor)
}

$modelPath = Resolve-PublicationPath -RelativePath ([string]$plan.model.relative_path)
Assert-PublicationCondition `
    (Test-Path -LiteralPath $modelPath -PathType Leaf) `
    'Q4 model is absent at publication'
Assert-PublicationCondition `
    ([UInt64](Get-Item -LiteralPath $modelPath).Length -eq
        [UInt64]$controls.model.bytes -and
        (Get-Sha256Lower -Path $modelPath) -ceq
            [string]$controls.model.sha256) `
    'Q4 model differs from frozen publication controls'

$sourceRoot = Resolve-PublicationPath `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-PublicationPath `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$sourceCheck = Test-PublicationExactTree -Root $sourceRoot -Anchor $anchor
Assert-PublicationCondition `
    ([string]$sourceCheck.manifest_sha256 -ceq
        [string]$controls.raw_source.manifest_sha256 -and
        [int]$sourceCheck.entries -eq [int]$controls.raw_source.entries) `
    'raw source differs from frozen publication controls'

if (Test-Path -LiteralPath $publicationPath -PathType Leaf) {
    $existing = Read-PublicationJson -Path $publicationPath
    Assert-PublicationCondition `
        ([string]$existing.schema -ceq
            'animus-ferric-runtime-recovery-publication-v4' -and
            [string]$existing.task -ceq 'T-11409' -and
            [string]$existing.operation_id -ceq [string]$plan.operation.id -and
            [int]$existing.execution_epoch -eq 3 -and
            [int]$existing.publication_epoch -eq 4 -and
            [string]$existing.timestamp_protocol -ceq
                [string]$plan.timestamp_protocol -and
            [string]$existing.control_manifest_sha256 -ceq $controlHash -and
            [string]$existing.source.relative_path -ceq
                [string]$plan.operation.source_raw_relative_path -and
            [string]$existing.source.manifest_sha256 -ceq
                [string]$sourceCheck.manifest_sha256 -and
            [int]$existing.source.entries -eq [int]$sourceCheck.entries -and
            [string]$existing.destination.relative_path -ceq
                [string]$plan.operation.destination_relative_path -and
            [bool]$existing.stage_verification.passed -and
            [bool]$existing.published_verification.passed -and
            [bool]$existing.passed) `
        'existing recovery publication envelope differs and will not be overwritten'
    $destinationCheck = Test-PublicationExactTree `
        -Root $destinationRoot -Anchor $anchor
    Assert-PublicationCondition `
        ([string]$existing.destination.manifest_sha256 -ceq
            [string]$destinationCheck.manifest_sha256 -and
            [int]$existing.destination.entries -eq
                [int]$destinationCheck.entries) `
        'existing recovery publication destination differs'
    [void](Invoke-CorrectedPublicationVerification -AttemptPath $destinationRoot)
    $existing | ConvertTo-Json -Depth 64
    exit 0
}

$resumedExistingDestination = Test-Path -LiteralPath $destinationRoot
$stageVerification = $null
if ($resumedExistingDestination) {
    $destinationCheck = Test-PublicationExactTree `
        -Root $destinationRoot -Anchor $anchor
    $stageVerification = Invoke-CorrectedPublicationVerification `
        -AttemptPath $destinationRoot
}
else {
    $destinationParent = Split-Path -Parent $destinationRoot
    Assert-PublicationCondition `
        (Test-Path -LiteralPath $destinationParent -PathType Container) `
        'recovery destination parent is absent'
    Assert-PublicationCondition (-not (Get-Item -LiteralPath $destinationParent `
            -Force).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) 'recovery destination parent is a reparse point'
    $stageParent = Resolve-PublicationPath `
        -RelativePath 'target/s114-experiment/recovery-stage'
    [System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
    Assert-PublicationCondition (-not (Get-Item -LiteralPath $stageParent `
            -Force).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) 'recovery stage parent is a reparse point'
    $stageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
    $stageRoot = Join-Path $stageOwner ([string]$plan.operation.coordinate)
    [System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
    $published = $false
    try {
        Assert-PublicationCondition (-not (Get-Item -LiteralPath $stageOwner `
                -Force).Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) 'owned recovery stage is a reparse point'
        Copy-Item -LiteralPath $sourceRoot -Destination $stageRoot -Recurse
        [void](Test-PublicationExactTree -Root $stageRoot -Anchor $anchor)
        $stageVerification = Invoke-CorrectedPublicationVerification `
            -AttemptPath $stageRoot -RecoveryPublicationStage
        Assert-PublicationCondition (-not (Test-Path -LiteralPath $destinationRoot)) `
            'recovery destination appeared before atomic publication'
        [System.IO.Directory]::Move($stageRoot, $destinationRoot)
        $published = $true
    }
    finally {
        if (Test-Path -LiteralPath $stageOwner -PathType Container) {
            $resolvedStageParent = [System.IO.Path]::GetFullPath($stageParent)
            $resolvedStageOwner = [System.IO.Path]::GetFullPath($stageOwner)
            $stagePrefix = "$resolvedStageParent$([System.IO.Path]::DirectorySeparatorChar)"
            if ($resolvedStageOwner.StartsWith(
                    $stagePrefix,
                    [StringComparison]::OrdinalIgnoreCase
                )) {
                [System.IO.Directory]::Delete($resolvedStageOwner, $true)
            }
        }
    }
    Assert-PublicationCondition $published 'recovery attempt was not published'
    $destinationCheck = Test-PublicationExactTree `
        -Root $destinationRoot -Anchor $anchor
}

$publishedVerification = Invoke-CorrectedPublicationVerification `
    -AttemptPath $destinationRoot
$publication = [ordered]@{
    schema = 'animus-ferric-runtime-recovery-publication-v4'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$plan.timestamp_protocol
    published_at_utc = (Get-Date).ToUniversalTime().ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    control_manifest_sha256 = $controlHash
    source = [ordered]@{
        relative_path = [string]$plan.operation.source_raw_relative_path
        manifest_sha256 = [string]$sourceCheck.manifest_sha256
        entries = [int]$sourceCheck.entries
    }
    destination = [ordered]@{
        relative_path = [string]$plan.operation.destination_relative_path
        manifest_sha256 = [string]$destinationCheck.manifest_sha256
        entries = [int]$destinationCheck.entries
    }
    stage_verification = $stageVerification
    published_verification = $publishedVerification
    resumed_existing_destination = [bool]$resumedExistingDestination
    passed = $true
}

$publicationTemp = Join-Path $artifactDir (
    "recovery-publication.$([guid]::NewGuid().ToString('N')).tmp"
)
try {
    Write-JsonLf -Path $publicationTemp -Value $publication -Depth 64
    Assert-PublicationCondition (-not (Test-Path -LiteralPath $publicationPath)) `
        'recovery publication envelope appeared and will not be overwritten'
    [System.IO.File]::Move($publicationTemp, $publicationPath, $false)
}
finally {
    if (Test-Path -LiteralPath $publicationTemp -PathType Leaf) {
        [System.IO.File]::Delete($publicationTemp)
    }
}

$publication | ConvertTo-Json -Depth 64
