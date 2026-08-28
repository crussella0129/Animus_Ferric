[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$anchorPath = Join-Path $artifactDir 'raw-source-anchor.json'
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$resultPath = Join-Path $artifactDir 'runtime-self-test.json'
$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'
$lf = [string][char]10

function Read-JsonStringDates {
    param([Parameter(Mandatory = $true)][string]$Path)

    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Write-JsonStringDates {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )

    Write-JsonLf -Path $Path -Value $Value
}

function Write-JsonLineRows {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][object[]]$Rows
    )

    $text = (@($Rows | ForEach-Object {
        $_ | ConvertTo-Json -Depth 100 -Compress
    }) -join $lf) + $lf
    Write-Utf8Lf -Path $Path -Text $text
}

function Update-CaseManifest {
    param([Parameter(Mandatory = $true)][string]$Path)

    Write-HashManifest -Root $Path -OutputPath (Join-Path $Path 'files.sha256')
}

function Invoke-RecoveryValidator {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$DeferLiveModelHash,
        [switch]$UnfrozenRecoverySource,
        [switch]$RecoveryPublicationStage
    )

    $arguments = @('-AttemptPath', $Path)
    if ($DeferLiveModelHash) {
        $arguments += '-DeferLiveModelHashToFreeze'
    }
    if ($UnfrozenRecoverySource) {
        $arguments += '-UnfrozenRecoverySource'
    }
    if ($RecoveryPublicationStage) {
        $arguments += '-RecoveryPublicationStage'
    }
    $process = Invoke-PowerShellFileBounded -ScriptPath $validatorPath -Arguments $arguments -TimeoutMilliseconds 600000
    $report = try {
        $process.stdout | ConvertFrom-Json -DateKind String
    }
    catch {
        $null
    }
    [ordered]@{
        exit_code = $process.exit_code
        parseable = $null -ne $report
        report = $report
        stderr = [string]$process.stderr
    }
}

function Add-RejectionResult {
    param(
        [Parameter(Mandatory = $true)]$Results,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$DeferLiveModelHash,
        [switch]$UnfrozenRecoverySource,
        [switch]$RecoveryPublicationStage,
        [string]$ExpectedErrorPattern
    )

    $validation = Invoke-RecoveryValidator -Path $Path -DeferLiveModelHash:$DeferLiveModelHash -UnfrozenRecoverySource:$UnfrozenRecoverySource -RecoveryPublicationStage:$RecoveryPublicationStage
    $errors = if ($validation.parseable) {
        @($validation.report.errors)
    }
    else {
        @()
    }
    $patternPassed = [string]::IsNullOrWhiteSpace($ExpectedErrorPattern) -or
        @($errors | Where-Object { [string]$_ -match $ExpectedErrorPattern }).Count -gt 0 -or
        [string]$validation.stderr -match $ExpectedErrorPattern
    $Results.Add([ordered]@{
        name = $Name
        passed = ($validation.exit_code -ne 0 -and $validation.parseable -and
            -not [bool]$validation.report.passed -and $patternPassed)
        evidence = [ordered]@{
            exit_code = $validation.exit_code
            parseable = $validation.parseable
            errors = $errors
            stderr = $validation.stderr
            expected_error_pattern = $ExpectedErrorPattern
        }
    })
}

function Get-ManifestPayloadIdentities {
    param([Parameter(Mandatory = $true)][string]$Root)

    $manifestPath = Join-Path $Root 'files.sha256'
    $identities = [System.Collections.Generic.List[object]]::new()
    foreach ($line in Get-Content -LiteralPath $manifestPath) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if ($line -notmatch '^([0-9a-f]{64})  (.+)$') {
            throw "manifest line is malformed: $line"
        }
        $relative = [string]$Matches[2]
        $path = Resolve-SafeRelativePath -Root $Root -RelativePath $relative
        $item = Get-Item -LiteralPath $path
        $identities.Add([ordered]@{
            path = $relative
            bytes = [UInt64]$item.Length
            sha256 = Get-Sha256Lower -Path $path
        })
    }
    @($identities | Sort-Object { $_.path })
}

function Test-ExactCopyIdentity {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $sourceManifestPath = Join-Path $Source 'files.sha256'
    $destinationManifestPath = Join-Path $Destination 'files.sha256'
    $sourceCheck = $null
    $destinationCheck = $null
    $manifestBytesEqual = $false
    $payloadEqual = $false
    try {
        $sourceCheck = Test-HashManifest -Root $Source -ManifestPath $sourceManifestPath -RejectUnlistedFiles
        $destinationCheck = Test-HashManifest -Root $Destination -ManifestPath $destinationManifestPath -RejectUnlistedFiles
        $sourceManifestBytes = [System.IO.File]::ReadAllBytes($sourceManifestPath)
        $destinationManifestBytes = [System.IO.File]::ReadAllBytes($destinationManifestPath)
        $manifestBytesEqual = [System.Collections.StructuralComparisons]::StructuralEqualityComparer.Equals(
            $sourceManifestBytes,
            $destinationManifestBytes
        )
        $sourcePayload = @(Get-ManifestPayloadIdentities -Root $Source)
        $destinationPayload = @(Get-ManifestPayloadIdentities -Root $Destination)
        $payloadEqual = Test-JsonEquivalent -Left $sourcePayload -Right $destinationPayload
    }
    catch {
        $errors.Add($_.Exception.Message)
    }
    if ($null -eq $sourceCheck -or -not $sourceCheck.passed) {
        $errors.Add('source exact-tree manifest failed')
    }
    if ($null -eq $destinationCheck -or -not $destinationCheck.passed) {
        $errors.Add('destination exact-tree manifest failed')
    }
    if (-not $manifestBytesEqual) {
        $errors.Add('source and destination manifest bytes differ')
    }
    if (-not $payloadEqual) {
        $errors.Add('source and destination payload identities differ')
    }
    [ordered]@{
        passed = $errors.Count -eq 0
        source_manifest = $sourceCheck
        destination_manifest = $destinationCheck
        manifest_bytes_equal = $manifestBytesEqual
        payload_identities_equal = $payloadEqual
        errors = @($errors)
    }
}

function Test-PublicationDestinationGuard {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    if (-not (Test-Path -LiteralPath $Destination)) {
        return [ordered]@{
            passed = $true
            publication_allowed = $true
            destination_exists = $false
            exact_copy = $null
            reason = 'destination_absent'
        }
    }
    $identity = Test-ExactCopyIdentity -Source $Source -Destination $Destination
    [ordered]@{
        passed = $true
        publication_allowed = $false
        destination_exists = $true
        exact_copy = $identity
        reason = if ($identity.passed) {
            'destination_already_exists'
        }
        else {
            'destination_exists_with_different_tree'
        }
    }
}

function Test-RecoveryPublicationStagePath {
    param([Parameter(Mandatory = $true)][string]$Candidate)

    $errors = [System.Collections.Generic.List[string]]::new()
    $stageParent = Resolve-SafeRelativePath -Root $repoRoot -RelativePath 'target/s114-experiment/recovery-stage'
    $stageParentFull = [System.IO.Path]::GetFullPath($stageParent).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $candidateFull = [System.IO.Path]::GetFullPath($Candidate).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $prefix = $stageParentFull + [System.IO.Path]::DirectorySeparatorChar
    if (-not $candidateFull.StartsWith(
            $prefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        $errors.Add('stage path is outside the recovery-stage root')
    }
    $relative = [System.IO.Path]::GetRelativePath($stageParentFull, $candidateFull)
    $segments = @($relative -split '[\\/]')
    if ($segments.Count -ne 2) {
        $errors.Add('stage path is not owner/coordinate depth')
    }
    else {
        if ($segments[0] -cnotmatch '^[0-9a-f]{32}$') {
            $errors.Add('stage owner is not a lowercase GUID-N token')
        }
        if ($segments[1] -cne [string]$plan.operation.coordinate) {
            $errors.Add('stage leaf is not the exact recovery coordinate')
        }
    }
    foreach ($path in @(
            $stageParentFull,
            (Split-Path -Parent $candidateFull),
            $candidateFull
        )) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
                $errors.Add("stage path component is a reparse point: $path")
            }
        }
    }
    [ordered]@{
        passed = $errors.Count -eq 0
        stage_parent = $stageParentFull
        candidate = $candidateFull
        relative = $relative.Replace('\', '/')
        errors = @($errors)
    }
}

function Convert-InstantToOffsetText {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][int]$OffsetHours
    )

    $normalized = ConvertTo-UtcIso8601 -Value $Value
    $instant = [DateTimeOffset]::ParseExact(
        $normalized,
        'o',
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind
    )
    $instant.ToOffset([TimeSpan]::FromHours($OffsetHours)).ToString('o')
}

function Add-InstantSecondsToOffsetText {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][double]$Seconds,
        [Parameter(Mandatory = $true)][int]$OffsetHours
    )

    $normalized = ConvertTo-UtcIso8601 -Value $Value
    $instant = [DateTimeOffset]::ParseExact(
        $normalized,
        'o',
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind
    )
    $instant.AddSeconds($Seconds).ToOffset(
        [TimeSpan]::FromHours($OffsetHours)
    ).ToString('o')
}

if (Test-Path -LiteralPath $resultPath) {
    throw 'runtime-self-test.json already exists and will not be overwritten'
}
if ((Test-Path -LiteralPath $controlManifestPath) -or
    (Test-Path -LiteralPath $controlDigestPath)) {
    throw 'epoch-4 recovery self-test must run before immutable controls exist'
}

$plan = Read-JsonStringDates -Path $planPath
if (-not (Test-RecoveryPlanIdentity -Plan $plan)) {
    throw 'runtime plan is not the epoch-4 T-11409 recovery protocol'
}
$sourceAnchor = Read-JsonStringDates -Path $anchorPath
$sourceRoot = Resolve-SafeRelativePath -Root $repoRoot -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-SafeRelativePath -Root $repoRoot -RelativePath ([string]$plan.operation.destination_relative_path)
$sourceManifestPath = Join-Path $sourceRoot 'files.sha256'
$sourceAttemptPath = Join-Path $sourceRoot 'attempt.json'
$sourceAttestationPath = Join-Path $sourceRoot 'attestation.json'

$sourceManifestCheck = Test-HashManifest -Root $sourceRoot -ManifestPath $sourceManifestPath -RejectUnlistedFiles
if (-not $sourceManifestCheck.passed) {
    throw "raw e03 source manifest is invalid: $($sourceManifestCheck.errors -join '; ')"
}

$sourcePayloadIdentities = @(Get-ManifestPayloadIdentities -Root $sourceRoot)
$sourcePayloadBytes = [UInt64]0
foreach ($identity in $sourcePayloadIdentities) {
    $sourcePayloadBytes += [UInt64]$identity.bytes
}
$sourceManifestItem = Get-Item -LiteralPath $sourceManifestPath
$sourceAttemptItem = Get-Item -LiteralPath $sourceAttemptPath
$sourceAttestationItem = Get-Item -LiteralPath $sourceAttestationPath
$sourceAnchorFiles = @($sourceAnchor.files | Sort-Object { $_.path })
$sourceAnchorsPassed = (
    $sourceAnchor.schema -ceq 'animus-ferric-runtime-raw-source-anchor-v1' -and
    $sourceAnchor.task -ceq 'T-11409' -and
    [int]$sourceAnchor.execution_epoch -eq [int]$plan.execution_epoch -and
    [int]$sourceAnchor.publication_epoch -eq [int]$plan.publication_epoch -and
    $sourceAnchor.operation_id -ceq [string]$plan.operation.id -and
    $sourceAnchor.source_relative_path -ceq [string]$plan.operation.source_raw_relative_path -and
    $sourceAnchor.destination_relative_path -ceq [string]$plan.operation.destination_relative_path -and
    $sourceAnchor.manifest.path -ceq 'files.sha256' -and
    [UInt64]$sourceAnchor.manifest.bytes -eq [UInt64]$sourceManifestItem.Length -and
    $sourceAnchor.manifest.sha256 -ceq (Get-Sha256Lower -Path $sourceManifestPath) -and
    [int]$sourceAnchor.manifest.entry_count -eq [int]$sourceManifestCheck.entries -and
    [UInt64]$sourceAnchor.manifest.payload_bytes -eq $sourcePayloadBytes -and
    $sourceAnchor.selected.attempt.path -ceq 'attempt.json' -and
    [UInt64]$sourceAnchor.selected.attempt.bytes -eq [UInt64]$sourceAttemptItem.Length -and
    $sourceAnchor.selected.attempt.sha256 -ceq (Get-Sha256Lower -Path $sourceAttemptPath) -and
    $sourceAnchor.selected.attestation.path -ceq 'attestation.json' -and
    [UInt64]$sourceAnchor.selected.attestation.bytes -eq [UInt64]$sourceAttestationItem.Length -and
    $sourceAnchor.selected.attestation.sha256 -ceq (Get-Sha256Lower -Path $sourceAttestationPath) -and
    [UInt64]$plan.operation.manifest.bytes -eq [UInt64]$sourceManifestItem.Length -and
    $plan.operation.manifest.sha256 -ceq [string]$sourceAnchor.manifest.sha256 -and
    [UInt64]$plan.operation.attempt.bytes -eq [UInt64]$sourceAttemptItem.Length -and
    $plan.operation.attempt.sha256 -ceq (Get-Sha256Lower -Path $sourceAttemptPath) -and
    [UInt64]$plan.operation.attestation.bytes -eq [UInt64]$sourceAttestationItem.Length -and
    $plan.operation.attestation.sha256 -ceq (Get-Sha256Lower -Path $sourceAttestationPath) -and
    [int]$plan.operation.exact_manifest_entries -eq [int]$sourceManifestCheck.entries -and
    (Test-JsonEquivalent -Left $sourceAnchorFiles -Right $sourcePayloadIdentities)
)
if (-not $sourceAnchorsPassed) {
    throw 'raw e03 source anchors do not match the recovery plan'
}

$staticControlNames = @(Get-EpochFourStaticControlNames)
if ($staticControlNames.Count -ne 12) {
    throw "epoch-4 static control set must contain exactly 12 paths, observed $($staticControlNames.Count)"
}
$staticControlNameSet = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
$staticControls = [System.Collections.Generic.List[object]]::new()
foreach ($name in $staticControlNames) {
    if (-not $staticControlNameSet.Add([string]$name)) {
        throw "duplicate epoch-4 static control name: $name"
    }
    $path = Resolve-SafeRelativePath -Root $artifactDir -RelativePath ([string]$name)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "epoch-4 static control is absent: $name"
    }
    $item = Get-Item -LiteralPath $path
    $staticControls.Add([ordered]@{
        path = [string]$name
        bytes = [UInt64]$item.Length
        sha256 = Get-Sha256Lower -Path $path
    })
}

$modelPath = Resolve-SafeRelativePath -Root $repoRoot -RelativePath ([string]$plan.model.relative_path)
$modelItem = Get-Item -LiteralPath $modelPath
$liveQ4Sha256 = Get-Sha256Lower -Path $modelPath
$liveQ4Identity = [ordered]@{
    path = [string]$plan.model.relative_path
    bytes = [UInt64]$modelItem.Length
    sha256 = $liveQ4Sha256
    passed = ([UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes -and
        $liveQ4Sha256 -ceq [string]$plan.model.sha256)
}

$stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffffffZ')
$testRoot = Join-Path $repoRoot "target/s114-experiment/runtime-epoch-4-selftest/$PID-$stamp"
$validParent = Join-Path $testRoot 'valid'
$validPath = Join-Path $validParent ([string]$plan.operation.coordinate)
[System.IO.Directory]::CreateDirectory($validParent) | Out-Null
Copy-Item -LiteralPath $sourceRoot -Destination $validPath -Recurse

function Copy-Case {
    param([Parameter(Mandatory = $true)][string]$Name)

    $parent = Join-Path $testRoot $Name
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
    $destination = Join-Path $parent ([string]$plan.operation.coordinate)
    Copy-Item -LiteralPath $validPath -Destination $destination -Recurse
    $destination
}

$results = [System.Collections.Generic.List[object]]::new()
$results.Add([ordered]@{
    name = 'source_anchors_match_plan'
    passed = $sourceAnchorsPassed
    evidence = [ordered]@{
        manifest = $sourceManifestCheck
        manifest_sha256 = Get-Sha256Lower -Path $sourceManifestPath
        manifest_bytes = [UInt64]$sourceManifestItem.Length
        payload_bytes = $sourcePayloadBytes
        payload_files = $sourcePayloadIdentities.Count
    }
})
$results.Add([ordered]@{
    name = 'destination_is_absent_before_publication'
    passed = -not (Test-Path -LiteralPath $destinationRoot)
    evidence = [ordered]@{
        destination_relative_path = [string]$plan.operation.destination_relative_path
        exists = Test-Path -LiteralPath $destinationRoot
    }
})
$results.Add([ordered]@{
    name = 'live_q4_identity_matches_plan'
    passed = [bool]$liveQ4Identity.passed
    evidence = $liveQ4Identity
})

$copyIdentity = Test-ExactCopyIdentity -Source $sourceRoot -Destination $validPath
$results.Add([ordered]@{
    name = 'exact_copy_identity_helper_accepts_unchanged_tree'
    passed = [bool]$copyIdentity.passed
    evidence = $copyIdentity
})

$attemptRewritePath = Copy-Case -Name 'self-consistent-attempt-rewrite'
$attemptRewriteFile = Join-Path $attemptRewritePath 'attempt.json'
$attemptRewrite = Read-JsonStringDates -Path $attemptRewriteFile
$attemptRewrite.started_at_utc = Convert-InstantToOffsetText -Value $attemptRewrite.started_at_utc -OffsetHours -4
Write-JsonStringDates -Path $attemptRewriteFile -Value $attemptRewrite
Update-CaseManifest -Path $attemptRewritePath
$attemptRewriteIdentity = Test-ExactCopyIdentity -Source $sourceRoot -Destination $attemptRewritePath
$results.Add([ordered]@{
    name = 'self_consistent_attempt_rewrite_breaks_exact_source_anchor'
    passed = (-not $attemptRewriteIdentity.passed -and
        $attemptRewriteIdentity.source_manifest.passed -and
        $attemptRewriteIdentity.destination_manifest.passed -and
        -not $attemptRewriteIdentity.manifest_bytes_equal -and
        -not $attemptRewriteIdentity.payload_identities_equal)
    evidence = $attemptRewriteIdentity
})

$attestationRewritePath = Copy-Case -Name 'self-consistent-attestation-rewrite'
$attestationRewriteAttemptFile = Join-Path $attestationRewritePath 'attempt.json'
$attestationRewriteFile = Join-Path $attestationRewritePath 'attestation.json'
$attestationRewriteAttempt = Read-JsonStringDates -Path $attestationRewriteAttemptFile
$attestationRewrite = Read-JsonStringDates -Path $attestationRewriteFile
$equivalentAttestationInstant = Convert-InstantToOffsetText -Value $attestationRewrite.captured_at_utc -OffsetHours -4
$attestationRewrite.captured_at_utc = $equivalentAttestationInstant
$attestationRewriteAttempt.attestation.captured_at_utc = $equivalentAttestationInstant
Write-JsonStringDates -Path $attestationRewriteFile -Value $attestationRewrite
Write-JsonStringDates -Path $attestationRewriteAttemptFile -Value $attestationRewriteAttempt
Update-CaseManifest -Path $attestationRewritePath
$attestationRewriteIdentity = Test-ExactCopyIdentity -Source $sourceRoot -Destination $attestationRewritePath
$results.Add([ordered]@{
    name = 'self_consistent_attestation_rewrite_breaks_exact_source_anchor'
    passed = (-not $attestationRewriteIdentity.passed -and
        $attestationRewriteIdentity.source_manifest.passed -and
        $attestationRewriteIdentity.destination_manifest.passed -and
        -not $attestationRewriteIdentity.manifest_bytes_equal -and
        -not $attestationRewriteIdentity.payload_identities_equal)
    evidence = $attestationRewriteIdentity
})

$stageParent = Resolve-SafeRelativePath -Root $repoRoot -RelativePath 'target/s114-experiment/recovery-stage'
[System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
$validStageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($validStageOwner) | Out-Null
$validStagePath = Join-Path $validStageOwner ([string]$plan.operation.coordinate)
Copy-Item -LiteralPath $validPath -Destination $validStagePath -Recurse
$validStagePolicy = Test-RecoveryPublicationStagePath -Candidate $validStagePath
$validStageIdentity = Test-ExactCopyIdentity -Source $sourceRoot -Destination $validStagePath
$results.Add([ordered]@{
    name = 'publication_stage_policy_accepts_exact_owned_stage'
    passed = ($validStagePolicy.passed -and $validStageIdentity.passed)
    evidence = [ordered]@{
        path_policy = $validStagePolicy
        exact_tree = $validStageIdentity
        verifier_integration_deferred_until_frozen_controls = $true
    }
})

$arbitraryStagePolicy = Test-RecoveryPublicationStagePath -Candidate (
    Join-Path $testRoot "arbitrary-stage/$($plan.operation.coordinate)"
)
$wrongLeafOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
$wrongLeafPolicy = Test-RecoveryPublicationStagePath -Candidate (
    Join-Path $wrongLeafOwner 'wrong-coordinate'
)
$traversalStagePolicy = Test-RecoveryPublicationStagePath -Candidate (
    Join-Path $stageParent "../escape/$($plan.operation.coordinate)"
)
$results.Add([ordered]@{
    name = 'publication_stage_policy_rejects_arbitrary_wrong_leaf_and_traversal'
    passed = (-not $arbitraryStagePolicy.passed -and
        -not $wrongLeafPolicy.passed -and
        -not $traversalStagePolicy.passed)
    evidence = [ordered]@{
        arbitrary = $arbitraryStagePolicy
        wrong_leaf = $wrongLeafPolicy
        traversal = $traversalStagePolicy
    }
})

$reparseTarget = Join-Path $testRoot 'reparse-target'
[System.IO.Directory]::CreateDirectory($reparseTarget) | Out-Null
$reparseOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
$reparseFeasible = $false
$reparseRejected = $true
$reparseError = $null
try {
    [void](New-Item -ItemType SymbolicLink -Path $reparseOwner -Target $reparseTarget -ErrorAction Stop)
    $reparseFeasible = $true
    $reparsePolicy = Test-RecoveryPublicationStagePath -Candidate (
        Join-Path $reparseOwner ([string]$plan.operation.coordinate)
    )
    $reparseRejected = -not $reparsePolicy.passed
}
catch {
    $reparseError = $_.Exception.Message
}
$results.Add([ordered]@{
    name = 'publication_stage_policy_rejects_reparse_owner_when_feasible'
    passed = (-not $reparseFeasible -or $reparseRejected)
    evidence = [ordered]@{
        feasible = $reparseFeasible
        rejected = $reparseRejected
        setup_error = $reparseError
    }
})

$validValidation = Invoke-RecoveryValidator -Path $validPath -DeferLiveModelHash
$results.Add([ordered]@{
    name = 'valid_unchanged_raw_copy_passes'
    passed = ($validValidation.exit_code -eq 0 -and $validValidation.parseable -and
        [bool]$validValidation.report.passed -and
        $validValidation.report.schema -ceq 'animus-ferric-runtime-recovery-verification-v4' -and
        [int]$validValidation.report.execution_epoch -eq 3 -and
        [int]$validValidation.report.publication_epoch -eq 4 -and
        $validValidation.report.source_attempt_schema -ceq [string]$plan.operation.source_attempt_schema -and
        $validValidation.report.timestamp_protocol -ceq [string]$plan.timestamp_protocol)
    evidence = [ordered]@{
        exit_code = $validValidation.exit_code
        parseable = $validValidation.parseable
        report = $validValidation.report
        stderr = $validValidation.stderr
    }
})

$outsideDeferral = Invoke-RecoveryValidator -Path $sourceRoot -DeferLiveModelHash
$results.Add([ordered]@{
    name = 'model_hash_deferral_rejected_outside_selftest'
    passed = ($outsideDeferral.exit_code -ne 0 -and $outsideDeferral.parseable -and
        -not [bool]$outsideDeferral.report.passed -and
        @($outsideDeferral.report.errors | Where-Object {
            [string]$_ -match 'deferr|self-test'
        }).Count -gt 0)
    evidence = [ordered]@{
        exit_code = $outsideDeferral.exit_code
        errors = if ($outsideDeferral.parseable) { @($outsideDeferral.report.errors) } else { @() }
        stderr = $outsideDeferral.stderr
    }
})

$stageDeferralConflict = Invoke-RecoveryValidator -Path $validPath -DeferLiveModelHash -RecoveryPublicationStage
$results.Add([ordered]@{
    name = 'publication_stage_mode_is_exclusive_with_model_hash_deferral'
    passed = ($stageDeferralConflict.exit_code -ne 0 -and
        $stageDeferralConflict.parseable -and
        -not [bool]$stageDeferralConflict.report.passed -and
        @($stageDeferralConflict.report.errors | Where-Object {
            [string]$_ -match 'exclusive|mutually|cannot.*combine'
        }).Count -gt 0)
    evidence = [ordered]@{
        exit_code = $stageDeferralConflict.exit_code
        errors = if ($stageDeferralConflict.parseable) {
            @($stageDeferralConflict.report.errors)
        }
        else { @() }
        stderr = $stageDeferralConflict.stderr
    }
})

$stageSourceConflict = Invoke-RecoveryValidator -Path $sourceRoot -UnfrozenRecoverySource -RecoveryPublicationStage
$results.Add([ordered]@{
    name = 'publication_stage_mode_is_exclusive_with_unfrozen_source'
    passed = ($stageSourceConflict.exit_code -ne 0 -and
        $stageSourceConflict.parseable -and
        -not [bool]$stageSourceConflict.report.passed -and
        @($stageSourceConflict.report.errors | Where-Object {
            [string]$_ -match 'exclusive|mutually|cannot.*combine'
        }).Count -gt 0)
    evidence = [ordered]@{
        exit_code = $stageSourceConflict.exit_code
        errors = if ($stageSourceConflict.parseable) {
            @($stageSourceConflict.report.errors)
        }
        else { @() }
        stderr = $stageSourceConflict.stderr
    }
})

$verifierText = Get-Content -Raw -LiteralPath $validatorPath
$parseTokens = $null
$parseErrors = $null
$verifierAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $validatorPath,
    [ref]$parseTokens,
    [ref]$parseErrors
)
$deferralAssignments = @($verifierAst.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -ceq 'deferLiveModelHash'
}, $true))
$deferralGuardText = if ($deferralAssignments.Count -eq 1) {
    [string]$deferralAssignments[0].Extent.Text
}
else {
    ''
}
$controlsAbsentGuardPassed = (
    @($parseErrors).Count -eq 0 -and
    $deferralAssignments.Count -eq 1 -and
    $deferralGuardText -cmatch '\$controlsAbsent' -and
    $deferralGuardText -cmatch '\$DeferLiveModelHashToFreeze' -and
    $verifierText -match 'control-inputs\.json' -and
    $verifierText -match 'control-inputs\.sha256'
)
$results.Add([ordered]@{
    name = 'model_hash_deferral_requires_absent_controls'
    passed = $controlsAbsentGuardPassed
    evidence = [ordered]@{
        method = 'parsed_source_guard_without_mutating_control_paths'
        parse_errors = @($parseErrors | ForEach-Object { $_.Message })
        deferral_assignment = $deferralGuardText
        controls_absent_guard_present = $controlsAbsentGuardPassed
    }
})

$exactAnchorGuardPassed = (
    $verifierText -cmatch '\$recoveryAnchorCheck' -and
    $verifierText -cmatch '\$recoveryPlan\.operation\.manifest' -and
    $verifierText -cmatch '\$recoveryPlan\.operation\.attempt' -and
    $verifierText -cmatch '\$recoveryPlan\.operation\.attestation' -and
    $verifierText -cmatch '\$rawSourceAnchor\.files' -and
    $verifierText -cmatch 'if \(-not \$isSelfTestAttempt\)'
)
$results.Add([ordered]@{
    name = 'verifier_enforces_exact_anchors_outside_fixture_mode'
    passed = $exactAnchorGuardPassed
    evidence = [ordered]@{
        method = 'parsed_frozen_verifier_source'
        exact_anchor_guard_present = $exactAnchorGuardPassed
    }
})

$publicationStageGuardPassed = (
    $verifierText -cmatch '\$RecoveryPublicationStage' -and
    $verifierText -cmatch '\$isPublicationStageAttempt' -and
    $verifierText -match 'target/s114-experiment/recovery-stage' -and
    $verifierText -match '\^\[0-9a-f\]\{32\}\$' -and
    $verifierText -match 'publication-stage path does not have the exact owned stage shape'
)
$results.Add([ordered]@{
    name = 'verifier_publication_stage_mode_is_exactly_path_scoped'
    passed = $publicationStageGuardPassed
    evidence = [ordered]@{
        method = 'parsed_frozen_verifier_source'
        stage_path_guard_present = $publicationStageGuardPassed
    }
})

$dateJson = '{"z":"2026-08-27T20:36:42.9337429Z","plus":"2026-08-27T20:36:42.9337429+00:00","minus":"2026-08-27T16:36:42.9337429-04:00"}'
$dateObject = $dateJson | ConvertFrom-Json -DateKind String
$dateNormalizations = @(
    ConvertTo-UtcIso8601 -Value $dateObject.z
    ConvertTo-UtcIso8601 -Value $dateObject.plus
    ConvertTo-UtcIso8601 -Value $dateObject.minus
)
$results.Add([ordered]@{
    name = 'json_datekind_string_preserves_equivalent_offsets'
    passed = ($dateObject.z -ceq '2026-08-27T20:36:42.9337429Z' -and
        $dateObject.plus -ceq '2026-08-27T20:36:42.9337429+00:00' -and
        $dateObject.minus -ceq '2026-08-27T16:36:42.9337429-04:00' -and
        @($dateNormalizations | Select-Object -Unique).Count -eq 1)
    evidence = [ordered]@{
        preserved = $dateObject
        normalized = $dateNormalizations
    }
})

$acceptedInstants = @(
    '2026-08-27T20:36:42Z',
    '2026-08-27T20:36:42.9337429+00:00',
    '2026-08-27T16:36:42.9337429-04:00'
)
$rejectedInstants = @(
    '2026-08-27T20:36:42.9337429',
    '08/27/2026 20:36:42',
    '2026-08-27 20:36:42Z',
    'not-an-instant'
)
$acceptedResults = @($acceptedInstants | ForEach-Object {
    try { ConvertTo-UtcIso8601 -Value $_ } catch { $null }
})
$rejectedResults = @($rejectedInstants | ForEach-Object {
    $rejected = $false
    try { [void](ConvertTo-UtcIso8601 -Value $_) } catch { $rejected = $true }
    [ordered]@{ value = $_; rejected = $rejected }
})
$results.Add([ordered]@{
    name = 'strict_instant_parser_accepts_offsets_and_rejects_ambiguous_text'
    passed = (@($acceptedResults | Where-Object { $null -eq $_ }).Count -eq 0 -and
        @($rejectedResults | Where-Object { -not $_.rejected }).Count -eq 0)
    evidence = [ordered]@{
        accepted = $acceptedResults
        rejected = $rejectedResults
    }
})

$mixedPath = Copy-Case -Name 'mixed-format-chronology'
$mixedAttemptPath = Join-Path $mixedPath 'attempt.json'
$mixedPreflightPath = Join-Path $mixedPath 'preflight.json'
$mixedAttestationPath = Join-Path $mixedPath 'attestation.json'
$mixedAttempt = Read-JsonStringDates -Path $mixedAttemptPath
$mixedPreflight = Read-JsonStringDates -Path $mixedPreflightPath
$mixedAttestation = Read-JsonStringDates -Path $mixedAttestationPath
$mixedAttempt.started_at_utc = Convert-InstantToOffsetText -Value $mixedAttempt.started_at_utc -OffsetHours -4
$mixedAttempt.completed_at_utc = Convert-InstantToOffsetText -Value $mixedAttempt.completed_at_utc -OffsetHours 0
$mixedPreflight.captured_at_utc = Convert-InstantToOffsetText -Value $mixedPreflight.captured_at_utc -OffsetHours 0
$mixedAttestation.captured_at_utc = Convert-InstantToOffsetText -Value $mixedAttestation.captured_at_utc -OffsetHours -4
$mixedAttempt.attestation.captured_at_utc = [string]$mixedAttestation.captured_at_utc
foreach ($container in @($mixedAttestation.process, $mixedAttempt.attestation.process)) {
    $container.creation_date_utc = Convert-InstantToOffsetText -Value $container.creation_date_utc -OffsetHours -4
    $container.creation_binding.creation_date_utc = [string]$container.creation_date_utc
    $container.creation_binding.preflight_captured_utc = [string]$mixedPreflight.captured_at_utc
    $container.creation_binding.attestation_captured_utc = [string]$mixedAttestation.captured_at_utc
}
Write-JsonStringDates -Path $mixedAttemptPath -Value $mixedAttempt
Write-JsonStringDates -Path $mixedPreflightPath -Value $mixedPreflight
Write-JsonStringDates -Path $mixedAttestationPath -Value $mixedAttestation
Update-CaseManifest -Path $mixedPath
$mixedValidation = Invoke-RecoveryValidator -Path $mixedPath -DeferLiveModelHash
$results.Add([ordered]@{
    name = 'mixed_offset_valid_chronology_passes'
    passed = ($mixedValidation.exit_code -eq 0 -and $mixedValidation.parseable -and
        [bool]$mixedValidation.report.passed)
    evidence = [ordered]@{
        exit_code = $mixedValidation.exit_code
        report = $mixedValidation.report
        stderr = $mixedValidation.stderr
    }
})

$attemptReversePath = Copy-Case -Name 'attempt-reversal'
$attemptReverse = Read-JsonStringDates -Path (Join-Path $attemptReversePath 'attempt.json')
$attemptReverse.started_at_utc = Convert-InstantToOffsetText -Value $attemptReverse.completed_at_utc -OffsetHours 0
$attemptReverse.completed_at_utc = Convert-InstantToOffsetText -Value $attemptReverse.attestation.captured_at_utc -OffsetHours 0
Write-JsonStringDates -Path (Join-Path $attemptReversePath 'attempt.json') -Value $attemptReverse
Update-CaseManifest -Path $attemptReversePath
Add-RejectionResult -Results $results -Name 'attempt_timestamp_reversal_rejected' -Path $attemptReversePath -DeferLiveModelHash -ExpectedErrorPattern 'attempt|chronolog|timestamp'

$preflightReversePath = Copy-Case -Name 'preflight-reversal'
$preflightReverse = Read-JsonStringDates -Path (Join-Path $preflightReversePath 'preflight.json')
$preflightReverseAttempt = Read-JsonStringDates -Path (Join-Path $preflightReversePath 'attempt.json')
$preflightReverse.captured_at_utc = Add-InstantSecondsToOffsetText -Value $preflightReverseAttempt.attestation.captured_at_utc -Seconds 1 -OffsetHours 0
Write-JsonStringDates -Path (Join-Path $preflightReversePath 'preflight.json') -Value $preflightReverse
Update-CaseManifest -Path $preflightReversePath
Add-RejectionResult -Results $results -Name 'preflight_attestation_reversal_rejected' -Path $preflightReversePath -DeferLiveModelHash -ExpectedErrorPattern 'preflight|chronolog|timestamp|creation'

$attestationReversePath = Copy-Case -Name 'attestation-reversal'
$attestationReverseAttemptPath = Join-Path $attestationReversePath 'attempt.json'
$attestationReverseEvidencePath = Join-Path $attestationReversePath 'attestation.json'
$attestationReversePreflight = Read-JsonStringDates -Path (Join-Path $attestationReversePath 'preflight.json')
$attestationReverseAttempt = Read-JsonStringDates -Path $attestationReverseAttemptPath
$attestationReverseEvidence = Read-JsonStringDates -Path $attestationReverseEvidencePath
$reversedAttestationInstant = Convert-InstantToOffsetText -Value $attestationReverseAttempt.started_at_utc -OffsetHours 0
$attestationReverseAttempt.attestation.captured_at_utc = $reversedAttestationInstant
$attestationReverseEvidence.captured_at_utc = $reversedAttestationInstant
Write-JsonStringDates -Path $attestationReverseAttemptPath -Value $attestationReverseAttempt
Write-JsonStringDates -Path $attestationReverseEvidencePath -Value $attestationReverseEvidence
Update-CaseManifest -Path $attestationReversePath
Add-RejectionResult -Results $results -Name 'attestation_preflight_reversal_rejected' -Path $attestationReversePath -DeferLiveModelHash -ExpectedErrorPattern 'attestation|preflight|chronolog|timestamp|creation'

$processRecordPath = Copy-Case -Name 'process-record-timestamp'
$processRecordFile = Join-Path $processRecordPath 'launch-process.json'
$processRecord = Read-JsonStringDates -Path $processRecordFile
$processRecord.started_at_utc = '2026-08-27T20:36:43'
Write-JsonStringDates -Path $processRecordFile -Value $processRecord
Update-CaseManifest -Path $processRecordPath
Add-RejectionResult -Results $results -Name 'process_record_timestamp_tamper_rejected' -Path $processRecordPath -DeferLiveModelHash -ExpectedErrorPattern 'process|timing|timestamp|launch'

$journalPath = Copy-Case -Name 'journal-timestamp'
$journalFile = Join-Path $journalPath 'command-journal.jsonl'
$journalRows = @(Get-Content -LiteralPath $journalFile | Where-Object {
    -not [string]::IsNullOrWhiteSpace($_)
} | ForEach-Object { $_ | ConvertFrom-Json -DateKind String })
$journalRows[0].at_utc = '08/27/2026 20:36:42'
Write-JsonLineRows -Path $journalFile -Rows $journalRows
Update-CaseManifest -Path $journalPath
Add-RejectionResult -Results $results -Name 'journal_timestamp_tamper_rejected' -Path $journalPath -DeferLiveModelHash -ExpectedErrorPattern 'journal|chronolog|timestamp'

$exchangePath = Copy-Case -Name 'exchange-timestamp'
$exchangeAttemptPath = Join-Path $exchangePath 'attempt.json'
$exchangeAttestationPath = Join-Path $exchangePath 'attestation.json'
$exchangeAttempt = Read-JsonStringDates -Path $exchangeAttemptPath
$exchangeAttestation = Read-JsonStringDates -Path $exchangeAttestationPath
$exchangeAttempt.attestation.endpoints.health.started_at_utc = '2026-08-27T20:37:15'
$exchangeAttestation.endpoints.health.started_at_utc = '2026-08-27T20:37:15'
Write-JsonStringDates -Path $exchangeAttemptPath -Value $exchangeAttempt
Write-JsonStringDates -Path $exchangeAttestationPath -Value $exchangeAttestation
Update-CaseManifest -Path $exchangePath
Add-RejectionResult -Results $results -Name 'exchange_timestamp_tamper_rejected' -Path $exchangePath -DeferLiveModelHash -ExpectedErrorPattern 'exchange|timestamp|health|attestation'

$memoryPath = Copy-Case -Name 'memory-timestamp'
$memoryFile = Join-Path $memoryPath 'memory-before-launch.json'
$memoryPreflightFile = Join-Path $memoryPath 'preflight.json'
$memoryEvidence = Read-JsonStringDates -Path $memoryFile
$memoryPreflight = Read-JsonStringDates -Path $memoryPreflightFile
$memoryEvidence.captured_at_utc = 'not-an-instant'
$memoryPreflight.memory.captured_at_utc = 'not-an-instant'
Write-JsonStringDates -Path $memoryFile -Value $memoryEvidence
Write-JsonStringDates -Path $memoryPreflightFile -Value $memoryPreflight
Update-CaseManifest -Path $memoryPath
Add-RejectionResult -Results $results -Name 'memory_timestamp_tamper_rejected' -Path $memoryPath -DeferLiveModelHash -ExpectedErrorPattern 'memory|timestamp|captur'

$missingManifestPath = Copy-Case -Name 'manifest-missing'
[System.IO.File]::Delete((Join-Path $missingManifestPath 'files.sha256'))
Add-RejectionResult -Results $results -Name 'manifest_missing_rejected' -Path $missingManifestPath -DeferLiveModelHash -ExpectedErrorPattern 'manifest|files.sha256'

$extraFilePath = Copy-Case -Name 'manifest-extra-file'
Write-Utf8Lf -Path (Join-Path $extraFilePath 'unlisted-evidence.txt') -Text "unlisted$lf"
Add-RejectionResult -Results $results -Name 'manifest_extra_file_rejected' -Path $extraFilePath -DeferLiveModelHash -ExpectedErrorPattern 'unlisted|manifest'

$traversalPath = Copy-Case -Name 'manifest-path-traversal'
$traversalManifest = Join-Path $traversalPath 'files.sha256'
$traversalLines = @(Get-Content -LiteralPath $traversalManifest)
$traversalLines[0] = "$('0' * 64)  ../outside-evidence.json"
Write-Utf8Lf -Path $traversalManifest -Text (($traversalLines -join $lf) + $lf)
Add-RejectionResult -Results $results -Name 'manifest_path_traversal_rejected' -Path $traversalPath -DeferLiveModelHash -ExpectedErrorPattern 'unsafe|manifest|path'

$caseCollisionPath = Copy-Case -Name 'manifest-case-collision'
$caseCollisionManifest = Join-Path $caseCollisionPath 'files.sha256'
$caseCollisionLines = @(Get-Content -LiteralPath $caseCollisionManifest)
if ($caseCollisionLines[0] -notmatch '^([0-9a-f]{64})  (.+)$') {
    throw 'case-collision fixture could not parse first manifest row'
}
$caseCollisionLines += "$($Matches[1])  $(([string]$Matches[2]).ToUpperInvariant())"
Write-Utf8Lf -Path $caseCollisionManifest -Text (($caseCollisionLines -join $lf) + $lf)
Add-RejectionResult -Results $results -Name 'manifest_case_collision_rejected' -Path $caseCollisionPath -DeferLiveModelHash -ExpectedErrorPattern 'duplicate|manifest'

$contentTamperPath = Copy-Case -Name 'manifest-content-tamper'
$contentTamperFile = Join-Path $contentTamperPath 'down-1.stdout.log'
[System.IO.File]::AppendAllText(
    $contentTamperFile,
    "tamper$lf",
    [System.Text.UTF8Encoding]::new($false)
)
Add-RejectionResult -Results $results -Name 'manifest_content_tamper_rejected' -Path $contentTamperPath -DeferLiveModelHash -ExpectedErrorPattern 'hash mismatch|manifest'

$arbitraryParent = Join-Path $repoRoot "target/s114-experiment/runtime-epoch-4-arbitrary/$PID-$stamp"
[System.IO.Directory]::CreateDirectory($arbitraryParent) | Out-Null
$arbitraryPath = Join-Path $arbitraryParent ([string]$plan.operation.coordinate)
Copy-Item -LiteralPath $validPath -Destination $arbitraryPath -Recurse
Add-RejectionResult -Results $results -Name 'arbitrary_v3_source_path_not_authorized' -Path $arbitraryPath -UnfrozenRecoverySource -ExpectedErrorPattern 'source|raw|authoriz|exact'

$guardDestinationParent = Join-Path $testRoot 'publication-guard'
[System.IO.Directory]::CreateDirectory($guardDestinationParent) | Out-Null
$guardDestination = Join-Path $guardDestinationParent ([string]$plan.operation.coordinate)
Copy-Item -LiteralPath $validPath -Destination $guardDestination -Recurse
[System.IO.File]::AppendAllText(
    (Join-Path $guardDestination 'attempt.json'),
    $lf,
    [System.Text.UTF8Encoding]::new($false)
)
Update-CaseManifest -Path $guardDestination
$differingGuard = Test-PublicationDestinationGuard -Source $sourceRoot -Destination $guardDestination
$results.Add([ordered]@{
    name = 'existing_differing_destination_publication_guard_fails_closed'
    passed = ($differingGuard.passed -and -not $differingGuard.publication_allowed -and
        $differingGuard.destination_exists -and
        -not $differingGuard.exact_copy.passed -and
        $differingGuard.exact_copy.destination_manifest.passed -and
        $differingGuard.reason -ceq 'destination_exists_with_different_tree')
    evidence = $differingGuard
})

$nameSet = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::Ordinal
)
$duplicateNames = [System.Collections.Generic.List[string]]::new()
foreach ($result in $results) {
    if ([string]::IsNullOrWhiteSpace([string]$result.name) -or
        -not $nameSet.Add([string]$result.name)) {
        $duplicateNames.Add([string]$result.name)
    }
}
$allPassed = (
    $duplicateNames.Count -eq 0 -and
    $results.Count -gt 0 -and
    @($results | Where-Object { -not [bool]$_.passed }).Count -eq 0
)
$report = [ordered]@{
    schema = 'animus-ferric-runtime-recovery-self-test-v4'
    task = 'T-11409'
    publication_epoch = 4
    execution_epoch = 3
    source_attempt_schema = [string]$plan.operation.source_attempt_schema
    timestamp_protocol = [string]$plan.timestamp_protocol
    generated_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    passed = $allPassed
    test_count = $results.Count
    duplicate_test_names = @($duplicateNames)
    static_controls = @($staticControls)
    static_input_identities = @($staticControls)
    live_q4_identity = $liveQ4Identity
    source_manifest = [ordered]@{
        relative_path = [string]$plan.operation.manifest.relative_path
        bytes = [UInt64]$sourceManifestItem.Length
        sha256 = Get-Sha256Lower -Path $sourceManifestPath
        entries = [int]$sourceManifestCheck.entries
        passed = [bool]$sourceManifestCheck.passed
    }
    results = @($results)
}
Write-JsonLf -Path $resultPath -Value $report
$report | ConvertTo-Json -Depth 100
if (-not $allPassed) {
    exit 1
}
