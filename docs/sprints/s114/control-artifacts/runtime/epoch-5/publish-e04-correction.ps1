[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$runtimeDir = Split-Path -Parent $artifactDir
$epochFourDir = Join-Path $runtimeDir 'epoch-4'
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$controlPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'
$selfTestPath = Join-Path $artifactDir 'publication-self-test.json'
$epochFourControlBootstrapPath = Join-Path $epochFourDir 'control-inputs.json'
$epochFourDigestBootstrapPath = Join-Path $epochFourDir 'control-inputs.sha256'
$epochFourCommonPath = Join-Path $epochFourDir 'runtime-common.ps1'
$epochFiveCommonPath = Join-Path $artifactDir 'publication-common.ps1'
$publisherPath = $PSCommandPath

function Get-BootstrapSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    $stream = [System.IO.File]::OpenRead($Path)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        [Convert]::ToHexString($algorithm.ComputeHash($stream)).ToLowerInvariant()
    }
    finally {
        $algorithm.Dispose()
        $stream.Dispose()
    }
}

function Assert-BootstrapLeaf {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "bootstrap file is absent: $Path"
    }
    if ((Get-Item -LiteralPath $Path -Force).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) {
        throw "bootstrap file is a reparse point: $Path"
    }
}

foreach ($bootstrapLeaf in @(
        $planPath,
        $controlPath,
        $digestPath,
        $epochFourControlBootstrapPath,
        $epochFourDigestBootstrapPath,
        $epochFourCommonPath,
        $epochFiveCommonPath,
        $publisherPath
    )) {
    Assert-BootstrapLeaf -Path $bootstrapLeaf
}
$bootstrapControlSha256 = Get-BootstrapSha256 -Path $controlPath
$bootstrapDigestLine = (Get-Content -Raw -LiteralPath $digestPath).TrimEnd(
    "`r", "`n"
)
if ($bootstrapDigestLine -cne
    "$bootstrapControlSha256  control-inputs.json") {
    throw 'epoch-5 bootstrap control digest differs'
}
$bootstrapControls = Get-Content -Raw -LiteralPath $controlPath |
    ConvertFrom-Json -DateKind String
$bootstrapPlan = Get-Content -Raw -LiteralPath $planPath |
    ConvertFrom-Json -DateKind String
if ([string]$bootstrapControls.schema -cne
        'animus-ferric-runtime-publication-correction-control-inputs-v5' -or
    [string]$bootstrapControls.task -cne 'T-11409' -or
    -not [bool]$bootstrapControls.passed -or
    [string]$bootstrapControls.runtime_plan_sha256 -cne
        (Get-BootstrapSha256 -Path $planPath) -or
    [string]$bootstrapPlan.schema -cne
        'animus-ferric-runtime-publication-correction-plan-v5' -or
    [string]$bootstrapPlan.task -cne 'T-11409' -or
    [string]$bootstrapPlan.operation.id -cne
        'r05-publish-e03-01-q4-32768-after-e04-wrapper-failure') {
    throw 'epoch-5 bootstrap plan/control identity differs'
}
$bootstrapStatic = @($bootstrapControls.static_controls)
if ($bootstrapStatic.Count -ne 8) {
    throw 'epoch-5 bootstrap static control count differs'
}
foreach ($bootstrapExpectation in @(
        [pscustomobject]@{
            name = 'publication-common.ps1'; path = $epochFiveCommonPath
        },
        [pscustomobject]@{
            name = 'publish-e04-correction.ps1'; path = $publisherPath
        }
    )) {
    $matches = @($bootstrapStatic | Where-Object {
            [string]$_.path -ceq [string]$bootstrapExpectation.name
        })
    if ($matches.Count -ne 1 -or
        [UInt64](Get-Item -LiteralPath $bootstrapExpectation.path).Length -ne
            [UInt64]$matches[0].bytes -or
        (Get-BootstrapSha256 -Path $bootstrapExpectation.path) -cne
            [string]$matches[0].sha256) {
        throw "epoch-5 bootstrap static differs: $($bootstrapExpectation.name)"
    }
}
if ([UInt64](Get-Item -LiteralPath $epochFourControlBootstrapPath).Length -ne
        [UInt64]$bootstrapPlan.epoch_4.control_manifest.bytes -or
    (Get-BootstrapSha256 -Path $epochFourControlBootstrapPath) -cne
        [string]$bootstrapPlan.epoch_4.control_manifest.sha256 -or
    [UInt64](Get-Item -LiteralPath $epochFourDigestBootstrapPath).Length -ne
        [UInt64]$bootstrapPlan.epoch_4.control_digest.bytes -or
    (Get-BootstrapSha256 -Path $epochFourDigestBootstrapPath) -cne
        [string]$bootstrapPlan.epoch_4.control_digest.sha256) {
    throw 'epoch-4 bootstrap control anchor differs'
}
$bootstrapEpochFourDigest = (Get-Content -Raw `
        -LiteralPath $epochFourDigestBootstrapPath).TrimEnd("`r", "`n")
if ($bootstrapEpochFourDigest -cne
    "$((Get-BootstrapSha256 -Path $epochFourControlBootstrapPath))  control-inputs.json") {
    throw 'epoch-4 bootstrap control digest differs'
}
$bootstrapEpochFourControls = Get-Content -Raw `
    -LiteralPath $epochFourControlBootstrapPath | ConvertFrom-Json -DateKind String
$bootstrapEpochFourStatic = @($bootstrapEpochFourControls.static_controls)
$bootstrapCommonEntries = @($bootstrapEpochFourStatic | Where-Object {
        [string]$_.path -ceq 'runtime-common.ps1'
    })
if ([string]$bootstrapEpochFourControls.schema -cne
        'animus-ferric-runtime-recovery-control-inputs-v4' -or
    -not [bool]$bootstrapEpochFourControls.epoch_3.passed -or
    $bootstrapEpochFourStatic.Count -ne 12 -or
    $bootstrapCommonEntries.Count -ne 1 -or
    [UInt64](Get-Item -LiteralPath $epochFourCommonPath).Length -ne
        [UInt64]$bootstrapCommonEntries[0].bytes -or
    (Get-BootstrapSha256 -Path $epochFourCommonPath) -cne
        [string]$bootstrapCommonEntries[0].sha256) {
    throw 'epoch-4 runtime-common bootstrap identity differs'
}

. (Join-Path $epochFourDir 'runtime-common.ps1')
. (Join-Path $artifactDir 'publication-common.ps1')

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir

function Assert-PublicationCorrection {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Read-PublicationCorrectionJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Test-ExactPropertySequence {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string[]]$Expected
    )
    $null -ne $Value -and
        (@($Value.PSObject.Properties.Name) -join "`n") -ceq
            ($Expected -join "`n")
}

function Test-StrictEpochFiveUtc {
    param([AllowNull()][Parameter(Mandatory = $true)]$Value)
    if ($Value -isnot [string] -or
        [string]$Value -cnotmatch
            '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$') {
        return $false
    }
    $instant = [DateTimeOffset]::MinValue
    [DateTimeOffset]::TryParseExact(
        [string]$Value,
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'",
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::AssumeUniversal,
        [ref]$instant
    )
}

function Assert-AnchorPassed {
    param(
        [Parameter(Mandatory = $true)]$Anchor,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $check = Test-EpochFiveFileAnchor -RepositoryRoot $repoRoot `
        -Anchor $Anchor -Label $Label
    Assert-PublicationCorrection ([bool]$check.passed) `
        "$Label differs: $(@($check.errors) -join '; ')"
    $check
}

function Assert-NonReparseDirectoryChain {
    param([Parameter(Mandatory = $true)][string]$Path)

    $root = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resolved = [System.IO.Path]::GetFullPath($Path)
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-PublicationCorrection `
        ($resolved.StartsWith($prefix,
                [System.StringComparison]::OrdinalIgnoreCase)) `
        'directory-chain target is outside the repository'
    $relative = [System.IO.Path]::GetRelativePath($root, $resolved)
    $cursor = $root
    foreach ($segment in $relative.Split(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.StringSplitOptions]::RemoveEmptyEntries
        )) {
        $cursor = Join-Path $cursor $segment
        Assert-PublicationCorrection `
            (Test-Path -LiteralPath $cursor -PathType Container) `
            "directory-chain component is absent: $cursor"
        Assert-PublicationCorrection `
            (-not (Get-Item -LiteralPath $cursor -Force).Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                )) `
            "directory-chain component is a reparse point: $cursor"
    }
}

function Remove-OwnedEpochFiveStage {
    param(
        [Parameter(Mandatory = $true)][string]$StageRoot,
        [Parameter(Mandatory = $true)][string]$Coordinate
    )

    $policy = Test-EpochFiveStagePathPolicy -RepositoryRoot $repoRoot `
        -StageRoot $StageRoot -Coordinate $Coordinate
    Assert-PublicationCorrection ([bool]$policy.passed) `
        "refusing to clean an unowned stage: $(@($policy.errors) -join '; ')"
    $ownerPath = [string]$policy.owner_path
    if (-not (Test-Path -LiteralPath $ownerPath)) { return }
    Assert-PublicationCorrection `
        (Test-Path -LiteralPath $ownerPath -PathType Container) `
        'owned stage path is not a directory'
    Assert-PublicationCorrection `
        (-not (Get-Item -LiteralPath $ownerPath -Force).Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) `
        'owned stage directory became a reparse point'
    $reparseEntries = @(Get-ChildItem -LiteralPath $ownerPath -Recurse -Force |
            Where-Object {
                $_.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            })
    Assert-PublicationCorrection ($reparseEntries.Count -eq 0) `
        'owned stage contains a reparse point and will not be recursively deleted'
    [System.IO.Directory]::Delete($ownerPath, $true)
}

function Test-LegacyRecoveryEnvelope {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Envelope,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)]$SourcePlan,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][string]$DestinationPath
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $propertiesPassed = Test-ExactPropertySequence -Value $Envelope -Expected @(
        'schema',
        'task',
        'operation_id',
        'execution_epoch',
        'publication_epoch',
        'timestamp_protocol',
        'published_at_utc',
        'control_manifest_sha256',
        'source',
        'destination',
        'stage_verification',
        'published_verification',
        'resumed_existing_destination',
        'passed'
    )
    if (-not $propertiesPassed) {
        $errors.Add('legacy envelope does not have the exact 14-field contract')
    }
    try {
        if ([string]$Envelope.schema -cne
                'animus-ferric-runtime-recovery-publication-v4' -or
            [string]$Envelope.task -cne 'T-11409' -or
            [string]$Envelope.operation_id -cne
                [string]$EpochFourPlan.operation.id -or
            [int]$Envelope.execution_epoch -ne 3 -or
            [int]$Envelope.publication_epoch -ne 4 -or
            [string]$Envelope.timestamp_protocol -cne
                [string]$EpochFourPlan.timestamp_protocol -or
            -not (Test-StrictEpochFiveUtc -Value $Envelope.published_at_utc) -or
            [string]$Envelope.control_manifest_sha256 -cne
                $EpochFourControlSha256 -or
            $Envelope.resumed_existing_destination -isnot [bool] -or
            -not [bool]$Envelope.passed) {
            $errors.Add('legacy envelope identity differs')
        }
        if (-not (Test-ExactPropertySequence -Value $Envelope.source `
                -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
            [string]$Envelope.source.relative_path -cne
                [string]$EpochFourPlan.operation.source_raw_relative_path -or
            [string]$Envelope.source.manifest_sha256 -cne
                [string]$SourceCheck.manifest_sha256 -or
            [int]$Envelope.source.entries -ne [int]$SourceCheck.entries) {
            $errors.Add('legacy source binding differs')
        }
        if (-not (Test-ExactPropertySequence -Value $Envelope.destination `
                -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
            [string]$Envelope.destination.relative_path -cne
                [string]$EpochFourPlan.operation.destination_relative_path -or
            [string]$Envelope.destination.manifest_sha256 -cne
                [string]$DestinationCheck.manifest_sha256 -or
            [int]$Envelope.destination.entries -ne
                [int]$DestinationCheck.entries) {
            $errors.Add('legacy destination binding differs')
        }
        $stageMode = if ([bool]$Envelope.resumed_existing_destination) {
            'epoch_4_frozen_recovery'
        }
        else { 'epoch_4_frozen_publication_stage' }
        if (-not [bool]$Envelope.resumed_existing_destination) {
            $stagePolicy = Test-EpochFiveStagePathPolicy `
                -RepositoryRoot $repoRoot `
                -StageRoot ([string]$Envelope.stage_verification.attempt_path) `
                -Coordinate ([string]$EpochFourPlan.operation.coordinate)
            if (-not [bool]$stagePolicy.passed) {
                $errors.Add('legacy stage report path is not an owned-stage shape')
            }
        }
        $stageCheck = Test-EpochFourVerificationReport `
            -Report $Envelope.stage_verification `
            -RecoveryPlan $EpochFourPlan -SourcePlan $SourcePlan `
            -ExpectedAttemptPath ([string]$Envelope.stage_verification.attempt_path) `
            -ExpectedAnchorMode $stageMode
        foreach ($message in @($stageCheck.errors)) {
            $errors.Add("legacy stage verification: $message")
        }
        $publishedCheck = Test-EpochFourVerificationReport `
            -Report $Envelope.published_verification `
            -RecoveryPlan $EpochFourPlan -SourcePlan $SourcePlan `
            -ExpectedAttemptPath $DestinationPath `
            -ExpectedAnchorMode 'epoch_4_frozen_recovery'
        foreach ($message in @($publishedCheck.errors)) {
            $errors.Add("legacy published verification: $message")
        }
    }
    catch {
        $errors.Add("legacy envelope is malformed: $($_.Exception.Message)")
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function Test-CorrectionEvidence {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)][string]$EpochFiveControlSha256,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)][string]$LegacyEnvelopeSha256,
        [Parameter(Mandatory = $true)][UInt64]$LegacyEnvelopeBytes,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][bool]$ExpectedResumedExistingDestination
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-ExactPropertySequence -Value $Evidence -Expected @(
            'schema',
            'task',
            'operation_id',
            'failed_operation_id',
            'execution_epoch',
            'failed_publication_epoch',
            'correction_epoch',
            'timestamp_protocol',
            'corrected_at_utc',
            'control_manifest_sha256',
            'failed_epoch_control_manifest_sha256',
            'legacy_envelope',
            'source',
            'destination',
            'resumed_existing_destination',
            'legacy_envelope_validation',
            'passed'
        ))) {
        $errors.Add('correction evidence does not have the exact v5 field contract')
    }
    try {
        if ([string]$Evidence.schema -cne
                'animus-ferric-runtime-publication-correction-v5' -or
            [string]$Evidence.task -cne 'T-11409' -or
            [string]$Evidence.operation_id -cne [string]$plan.operation.id -or
            [string]$Evidence.failed_operation_id -cne
                [string]$epochFourPlan.operation.id -or
            [int]$Evidence.execution_epoch -ne 3 -or
            [int]$Evidence.failed_publication_epoch -ne 4 -or
            [int]$Evidence.correction_epoch -ne 5 -or
            [string]$Evidence.timestamp_protocol -cne
                [string]$plan.timestamp_protocol -or
            -not (Test-StrictEpochFiveUtc -Value $Evidence.corrected_at_utc) -or
            [string]$Evidence.control_manifest_sha256 -cne
                $EpochFiveControlSha256 -or
            [string]$Evidence.failed_epoch_control_manifest_sha256 -cne
                $EpochFourControlSha256 -or
            $Evidence.resumed_existing_destination -isnot [bool] -or
            [bool]$Evidence.resumed_existing_destination -ne
                $ExpectedResumedExistingDestination -or
            -not (Test-ExactPropertySequence `
                -Value $Evidence.legacy_envelope_validation `
                -Expected @('contract', 'passed')) -or
            -not [bool]$Evidence.legacy_envelope_validation.passed -or
            [string]$Evidence.legacy_envelope_validation.contract -cne
                'animus-ferric-runtime-recovery-publication-v4' -or
            -not [bool]$Evidence.passed) {
            $errors.Add('correction evidence identity differs')
        }
        if (-not (Test-ExactPropertySequence -Value $Evidence.legacy_envelope `
                -Expected @('relative_path', 'bytes', 'sha256')) -or
            [string]$Evidence.legacy_envelope.relative_path -cne
                [string]$plan.operation.legacy_envelope_relative_path -or
            [UInt64]$Evidence.legacy_envelope.bytes -ne $LegacyEnvelopeBytes -or
            [string]$Evidence.legacy_envelope.sha256 -cne
                $LegacyEnvelopeSha256) {
            $errors.Add('correction evidence legacy-envelope binding differs')
        }
        foreach ($binding in @(
                [pscustomobject]@{
                    name = 'source'; value = $Evidence.source
                    path = [string]$plan.operation.source_raw_relative_path
                    check = $SourceCheck
                },
                [pscustomobject]@{
                    name = 'destination'; value = $Evidence.destination
                    path = [string]$plan.operation.destination_relative_path
                    check = $DestinationCheck
                }
            )) {
            if (-not (Test-ExactPropertySequence -Value $binding.value `
                    -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
                [string]$binding.value.relative_path -cne $binding.path -or
                [string]$binding.value.manifest_sha256 -cne
                    [string]$binding.check.manifest_sha256 -or
                [int]$binding.value.entries -ne [int]$binding.check.entries) {
                $errors.Add("correction evidence $($binding.name) binding differs")
            }
        }
    }
    catch {
        $errors.Add("correction evidence is malformed: $($_.Exception.Message)")
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

$plan = Read-PublicationCorrectionJson -Path $planPath
Assert-PublicationCorrection (Test-EpochFivePlanIdentity -Plan $plan) `
    'epoch-5 publication correction plan identity differs'

$epochFourPlanCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_4.runtime_plan -Label 'epoch-4 recovery plan'
$rawAnchorCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_4.raw_source_anchor -Label 'epoch-4 raw-source anchor'
$epochFourControlCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_4.control_manifest -Label 'epoch-4 control manifest'
$epochFourDigestCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_4.control_digest -Label 'epoch-4 control digest'
[void](Assert-AnchorPassed -Anchor $plan.epoch_4.runtime_self_test `
        -Label 'epoch-4 runtime self-test')
$verifierCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_4.verifier -Label 'epoch-4 verifier'
[void](Assert-AnchorPassed -Anchor $plan.epoch_4.frozen_failed_publisher `
        -Label 'epoch-4 frozen failed publisher')
$epochThreeControlCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_3.control_manifest -Label 'epoch-3 control manifest'
$epochThreeDigestCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_3.control_digest -Label 'epoch-3 control digest'
$sourcePlanCheck = Assert-AnchorPassed `
    -Anchor $plan.epoch_3.runtime_plan -Label 'epoch-3 source execution plan'
[void](Assert-AnchorPassed -Anchor $plan.epoch_3.runtime_self_test `
        -Label 'epoch-3 runtime self-test')

$epochFourPlanPath = [string]$epochFourPlanCheck.resolved_path
$rawAnchorPath = [string]$rawAnchorCheck.resolved_path
$epochFourControlPath = [string]$epochFourControlCheck.resolved_path
$epochFourDigestPath = [string]$epochFourDigestCheck.resolved_path
$verifierPath = [string]$verifierCheck.resolved_path
$sourcePlanPath = [string]$sourcePlanCheck.resolved_path
$epochFourPlan = Read-PublicationCorrectionJson -Path $epochFourPlanPath
$rawAnchor = Read-PublicationCorrectionJson -Path $rawAnchorPath
$sourcePlan = Read-PublicationCorrectionJson -Path $sourcePlanPath
Assert-PublicationCorrection (Test-RecoveryPlanIdentity -Plan $epochFourPlan) `
    'anchored epoch-4 recovery plan identity differs'
Assert-PublicationCorrection (Test-RuntimePlanIdentity -Plan $sourcePlan) `
    'anchored epoch-3 source plan identity differs'
Assert-PublicationCorrection `
    ([string]$epochFourPlan.operation.id -ceq
            [string]$plan.operation.failed_operation_id -and
        [string]$epochFourPlan.operation.coordinate -ceq
            [string]$plan.operation.coordinate -and
        [string]$epochFourPlan.operation.source_raw_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$epochFourPlan.operation.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [string]$epochFourPlan.operation.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256) `
    'epoch-5 operation does not exactly bind the failed epoch-4 operation'

$epochFourControlSha256 = Get-Sha256Lower -Path $epochFourControlPath
$epochFourDigestLine = (Get-Content -Raw -LiteralPath $epochFourDigestPath).TrimEnd(
    "`r", "`n"
)
Assert-PublicationCorrection `
    ($epochFourDigestLine -ceq
        [string]$plan.epoch_4.control_manifest_digest_line) `
    'epoch-4 frozen control digest line differs'
$epochThreeDigestLine = (Get-Content -Raw `
        -LiteralPath ([string]$epochThreeDigestCheck.resolved_path)).TrimEnd(
    "`r", "`n"
)
Assert-PublicationCorrection `
    ($epochThreeDigestLine -ceq
            [string]$plan.epoch_3.control_manifest_digest_line -and
        (Get-Sha256Lower -Path ([string]$epochThreeControlCheck.resolved_path)) `
            -ceq [string]$plan.epoch_3.control_manifest.sha256) `
    'epoch-3 frozen control digest line differs'

Assert-PublicationCorrection `
    (Test-Path -LiteralPath $controlPath -PathType Leaf) `
    'epoch-5 frozen control manifest is absent'
Assert-PublicationCorrection `
    (Test-Path -LiteralPath $digestPath -PathType Leaf) `
    'epoch-5 frozen control digest is absent'
$controlSha256 = Get-Sha256Lower -Path $controlPath
$digestLine = (Get-Content -Raw -LiteralPath $digestPath).TrimEnd("`r", "`n")
Assert-PublicationCorrection `
    ($digestLine -ceq "$controlSha256  control-inputs.json") `
    'epoch-5 frozen control digest differs'
$controls = Read-PublicationCorrectionJson -Path $controlPath
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-PublicationCorrection ($LASTEXITCODE -eq 0) `
    'could not resolve repository HEAD'
Assert-PublicationCorrection `
    ([string]$controls.schema -ceq
            'animus-ferric-runtime-publication-correction-control-inputs-v5' -and
        [string]$controls.task -ceq 'T-11409' -and
        [string]$controls.operation_id -ceq [string]$plan.operation.id -and
        [string]$controls.failed_operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [int]$controls.execution_epoch -eq 3 -and
        [int]$controls.failed_publication_epoch -eq 4 -and
        [int]$controls.correction_epoch -eq 5 -and
        [string]$controls.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [string]$controls.repository.head_at_freeze -ceq $head -and
        [string]$controls.repository.epoch_5_pre_control_base -ceq
            [string]$plan.repository_commit_before_epoch_5_controls -and
        [string]$controls.runtime_plan_sha256 -ceq
            (Get-Sha256Lower -Path $planPath) -and
        [string]$controls.incident_sha256 -ceq
            (Get-Sha256Lower -Path $incidentPath) -and
        [bool]$controls.publication_self_test.passed -and
        [bool]$controls.epoch_4.passed -and
        [string]$controls.epoch_4.runtime_plan_sha256 -ceq
            [string]$plan.epoch_4.runtime_plan.sha256 -and
        [string]$controls.epoch_4.raw_source_anchor_sha256 -ceq
            [string]$plan.epoch_4.raw_source_anchor.sha256 -and
        [string]$controls.epoch_4.control_manifest_sha256 -ceq
            [string]$plan.epoch_4.control_manifest.sha256 -and
        [string]$controls.epoch_4.control_digest_sha256 -ceq
            [string]$plan.epoch_4.control_digest.sha256 -and
        [string]$controls.epoch_4.verifier_sha256 -ceq
            [string]$plan.epoch_4.verifier.sha256 -and
        [string]$controls.epoch_4.frozen_failed_publisher_sha256 -ceq
            [string]$plan.epoch_4.frozen_failed_publisher.sha256 -and
        [string]$controls.epoch_4.control_manifest_digest_line -ceq
            [string]$plan.epoch_4.control_manifest_digest_line -and
        [bool]$controls.raw_source.passed -and
        [string]$controls.raw_source.relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$controls.raw_source.attempt_sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$controls.raw_source.attestation_sha256 -ceq
            [string]$plan.operation.attestation.sha256 -and
        [bool]$controls.raw_source.terminal_facts_passed -and
        [bool]$controls.model.passed -and
        [bool]$controls.model.independently_rehashed -and
        [string]$controls.model.relative_path -ceq
            [string]$plan.model.relative_path -and
        [string]$controls.model.sha256 -ceq [string]$plan.model.sha256 -and
        [bool]$controls.cold_state.passed -and
        [bool]$controls.publication_preconditions.passed -and
        [string]$controls.publication_preconditions.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [bool]$controls.publication_preconditions.destination_absent_at_freeze -and
        [string]$controls.publication_preconditions.legacy_envelope_relative_path -ceq
            [string]$plan.operation.legacy_envelope_relative_path -and
        [bool]$controls.publication_preconditions.legacy_envelope_absent_at_freeze -and
        [string]$controls.publication_preconditions.correction_evidence_relative_path -ceq
            [string]$plan.operation.correction_evidence_relative_path -and
        [bool]$controls.publication_preconditions.correction_evidence_absent_at_freeze -and
        [bool]$controls.passed) `
    'epoch-5 frozen control manifest identity differs'

$staticNames = @(Get-EpochFiveStaticControlNames)
$frozenStatic = @($controls.static_controls)
Assert-PublicationCorrection `
    ($staticNames.Count -eq 8 -and $frozenStatic.Count -eq 8) `
    'epoch-5 frozen static control count differs'
for ($index = 0; $index -lt $staticNames.Count; $index++) {
    $name = [string]$staticNames[$index]
    $entry = $frozenStatic[$index]
    $path = Join-Path $artifactDir $name
    Assert-PublicationCorrection `
        ([string]$entry.path -ceq $name -and
            (Test-Path -LiteralPath $path -PathType Leaf) -and
            [UInt64](Get-Item -LiteralPath $path).Length -eq
                [UInt64]$entry.bytes -and
            (Get-Sha256Lower -Path $path) -ceq [string]$entry.sha256) `
        "epoch-5 frozen static control differs: $name"
}
Assert-PublicationCorrection `
    ([string]$controls.publication_self_test.relative_path -ceq
            'docs/sprints/s114/control-artifacts/runtime/epoch-5/publication-self-test.json' -and
        (Test-Path -LiteralPath $selfTestPath -PathType Leaf) -and
        [UInt64](Get-Item -LiteralPath $selfTestPath).Length -eq
            [UInt64]$controls.publication_self_test.bytes -and
        (Get-Sha256Lower -Path $selfTestPath) -ceq
            [string]$controls.publication_self_test.sha256) `
    'epoch-5 publication self-test differs from frozen controls'
$dependencySet = Test-EpochFourFrozenDependencySet `
    -RepositoryRoot $repoRoot -EpochFivePlan $plan -ExpectedHead $head
Assert-PublicationCorrection `
    ([bool]$dependencySet.passed -and
        [int]$dependencySet.static_controls_checked -eq
            [int]$controls.epoch_4.static_controls_checked -and
        [int]$dependencySet.transitive_epoch_3_controls_checked -eq
            [int]$controls.epoch_4.transitive_epoch_3_controls_checked -and
        [int]$dependencySet.static_controls_checked -eq 12 -and
        [int]$dependencySet.transitive_epoch_3_controls_checked -eq 20) `
    "frozen epoch-4/epoch-3 dependency set differs: $(@($dependencySet.errors) -join '; ')"

$sourceRoot = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$legacyEnvelopePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.legacy_envelope_relative_path)
$correctionEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.correction_evidence_relative_path)
$sourceCheck = Test-EpochFiveExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
Assert-PublicationCorrection ([bool]$sourceCheck.passed) `
    "raw source differs: $(@($sourceCheck.errors) -join '; ')"
Assert-PublicationCorrection `
    ([string]$sourceCheck.manifest_sha256 -ceq
            [string]$controls.raw_source.manifest_sha256 -and
        [int]$sourceCheck.entries -eq [int]$controls.raw_source.entries -and
        [UInt64]$sourceCheck.payload_bytes -eq
            [UInt64]$controls.raw_source.payload_bytes) `
    'raw source differs from epoch-5 frozen controls'

$modelPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.model.relative_path)
Assert-PublicationCorrection `
    ((Test-Path -LiteralPath $modelPath -PathType Leaf) -and
        -not (Get-Item -LiteralPath $modelPath -Force).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and
        [UInt64](Get-Item -LiteralPath $modelPath).Length -eq
            [UInt64]$controls.model.bytes -and
        [string]$controls.model.sha256 -ceq [string]$plan.model.sha256) `
    'Q4 model presence or frozen identity differs'

$legacyExists = Test-Path -LiteralPath $legacyEnvelopePath
$correctionExists = Test-Path -LiteralPath $correctionEvidencePath
foreach ($evidencePath in @(
        [pscustomobject]@{
            name = 'legacy envelope'; path = $legacyEnvelopePath
            exists = $legacyExists
        },
        [pscustomobject]@{
            name = 'correction evidence'; path = $correctionEvidencePath
            exists = $correctionExists
        }
    )) {
    if ([bool]$evidencePath.exists) {
        Assert-PublicationCorrection `
            (Test-Path -LiteralPath $evidencePath.path -PathType Leaf) `
            "$($evidencePath.name) path exists but is not a file"
        Assert-PublicationCorrection `
            (-not (Get-Item -LiteralPath $evidencePath.path -Force).Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                )) `
            "$($evidencePath.name) is a reparse point"
    }
}
$legacyExact = $false
$correctionExact = $false
$legacyEnvelope = $null
$correction = $null

$destinationCheck = $null
if (Test-Path -LiteralPath $destinationRoot) {
    Assert-PublicationCorrection `
        (Test-Path -LiteralPath $destinationRoot -PathType Container) `
        'recovery destination exists but is not a directory'
    $destinationCheck = Test-EpochFiveExactTree -Root $destinationRoot `
        -ManifestAnchor $rawAnchor `
        -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
    Assert-PublicationCorrection ([bool]$destinationCheck.passed) `
        "existing destination differs: $(@($destinationCheck.errors) -join '; ')"
}
elseif ($legacyExists -or $correctionExists) {
    throw 'publication evidence exists without the exact destination'
}

if ($legacyExists) {
    $legacyEnvelope = Read-PublicationCorrectionJson -Path $legacyEnvelopePath
    $legacyCheck = Test-LegacyRecoveryEnvelope -Envelope $legacyEnvelope `
        -EpochFourPlan $epochFourPlan -SourcePlan $sourcePlan `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -DestinationPath $destinationRoot
    Assert-PublicationCorrection ([bool]$legacyCheck.passed) `
        "existing legacy envelope differs: $(@($legacyCheck.errors) -join '; ')"
    $legacyExact = $true
    $legacyItem = Get-Item -LiteralPath $legacyEnvelopePath
    $legacySha256 = Get-Sha256Lower -Path $legacyEnvelopePath
    if ($correctionExists) {
        $correction = Read-PublicationCorrectionJson -Path $correctionEvidencePath
        $correctionCheck = Test-CorrectionEvidence -Evidence $correction `
            -EpochFiveControlSha256 $controlSha256 `
            -EpochFourControlSha256 $epochFourControlSha256 `
            -LegacyEnvelopeSha256 $legacySha256 `
            -LegacyEnvelopeBytes ([UInt64]$legacyItem.Length) `
            -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
            -ExpectedResumedExistingDestination `
                ([bool]$legacyEnvelope.resumed_existing_destination)
        Assert-PublicationCorrection ([bool]$correctionCheck.passed) `
            "existing correction evidence differs: $(@($correctionCheck.errors) -join '; ')"
        $correctionExact = $true
    }
}
$publicationState = Test-EpochFivePublicationState `
    -DestinationExists ($null -ne $destinationCheck) `
    -DestinationExact ($null -ne $destinationCheck -and
        [bool]$destinationCheck.passed) `
    -LegacyEnvelopeExists $legacyExists -LegacyEnvelopeExact $legacyExact `
    -CorrectionEvidenceExists $correctionExists `
    -CorrectionEvidenceExact $correctionExact
Assert-PublicationCorrection ([bool]$publicationState.passed) `
    "publication state is invalid: $(@($publicationState.errors) -join '; ')"
if ([string]$publicationState.action -ceq 'already_complete') {
    $correction | ConvertTo-Json -Depth 64
    exit 0
}
if ([string]$publicationState.action -ceq 'complete_correction_evidence') {
    [void](Invoke-EpochFourVerification -VerifierPath $verifierPath `
            -AttemptPath $destinationRoot -RecoveryPlan $epochFourPlan `
            -SourcePlan $sourcePlan)
}

if (-not $legacyExists) {
    $resumedExistingDestination = $null -ne $destinationCheck
    if ($resumedExistingDestination) {
        $stageVerification = Invoke-EpochFourVerification `
            -VerifierPath $verifierPath -AttemptPath $destinationRoot `
            -RecoveryPlan $epochFourPlan -SourcePlan $sourcePlan
        $publishedVerification = $stageVerification
    }
    else {
        $destinationParent = Split-Path -Parent $destinationRoot
        Assert-NonReparseDirectoryChain -Path $destinationParent
        $stageParent = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $repoRoot `
            -RelativePath 'target/s114-experiment/recovery-stage'
        [System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
        Assert-NonReparseDirectoryChain -Path $stageParent
        $ownerName = [guid]::NewGuid().ToString('N')
        $stageOwner = Join-Path $stageParent $ownerName
        $stageRoot = Join-Path $stageOwner ([string]$plan.operation.coordinate)
        $stagePolicy = Test-EpochFiveStagePathPolicy -RepositoryRoot $repoRoot `
            -StageRoot $stageRoot `
            -Coordinate ([string]$plan.operation.coordinate)
        Assert-PublicationCorrection ([bool]$stagePolicy.passed) `
            "generated stage path violates policy: $(@($stagePolicy.errors) -join '; ')"
        Assert-PublicationCorrection `
            (-not (Test-Path -LiteralPath $stageOwner)) `
            'generated stage owner already exists'
        [System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
        $moved = $false
        try {
            Assert-NonReparseDirectoryChain -Path $stageOwner
            Copy-Item -LiteralPath $sourceRoot -Destination $stageRoot -Recurse
            $stageCheck = Test-EpochFiveExactTree -Root $stageRoot `
                -ManifestAnchor $rawAnchor `
                -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
            Assert-PublicationCorrection ([bool]$stageCheck.passed) `
                "staged exact copy differs: $(@($stageCheck.errors) -join '; ')"
            $stageVerification = Invoke-EpochFourVerification `
                -VerifierPath $verifierPath -AttemptPath $stageRoot `
                -RecoveryPlan $epochFourPlan -SourcePlan $sourcePlan `
                -RecoveryPublicationStage
            Assert-PublicationCorrection `
                (-not (Test-Path -LiteralPath $destinationRoot)) `
                'recovery destination appeared before atomic move'
            [System.IO.Directory]::Move($stageRoot, $destinationRoot)
            $moved = $true
        }
        finally {
            Remove-OwnedEpochFiveStage -StageRoot $stageRoot `
                -Coordinate ([string]$plan.operation.coordinate)
        }
        Assert-PublicationCorrection $moved `
            'exact recovery destination was not atomically published'
        $destinationCheck = Test-EpochFiveExactTree -Root $destinationRoot `
            -ManifestAnchor $rawAnchor `
            -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
        Assert-PublicationCorrection ([bool]$destinationCheck.passed) `
            "published destination differs: $(@($destinationCheck.errors) -join '; ')"
        $publishedVerification = Invoke-EpochFourVerification `
            -VerifierPath $verifierPath -AttemptPath $destinationRoot `
            -RecoveryPlan $epochFourPlan -SourcePlan $sourcePlan
    }

    $legacyEnvelope = [ordered]@{
        schema = 'animus-ferric-runtime-recovery-publication-v4'
        task = 'T-11409'
        operation_id = [string]$epochFourPlan.operation.id
        execution_epoch = 3
        publication_epoch = 4
        timestamp_protocol = [string]$epochFourPlan.timestamp_protocol
        published_at_utc = (Get-Date).ToUniversalTime().ToString(
            "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
        )
        control_manifest_sha256 = $epochFourControlSha256
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
    $legacyCheck = Test-LegacyRecoveryEnvelope -Envelope $legacyEnvelope `
        -EpochFourPlan $epochFourPlan -SourcePlan $sourcePlan `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -DestinationPath $destinationRoot
    Assert-PublicationCorrection ([bool]$legacyCheck.passed) `
        "constructed legacy envelope differs: $(@($legacyCheck.errors) -join '; ')"
    Write-EpochFiveJsonAtomic -Path $legacyEnvelopePath -Value $legacyEnvelope
    $legacyItem = Get-Item -LiteralPath $legacyEnvelopePath
    $legacySha256 = Get-Sha256Lower -Path $legacyEnvelopePath
}

$correction = [ordered]@{
    schema = 'animus-ferric-runtime-publication-correction-v5'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    failed_operation_id = [string]$epochFourPlan.operation.id
    execution_epoch = 3
    failed_publication_epoch = 4
    correction_epoch = 5
    timestamp_protocol = [string]$plan.timestamp_protocol
    corrected_at_utc = (Get-Date).ToUniversalTime().ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    control_manifest_sha256 = $controlSha256
    failed_epoch_control_manifest_sha256 = $epochFourControlSha256
    legacy_envelope = [ordered]@{
        relative_path = [string]$plan.operation.legacy_envelope_relative_path
        bytes = [UInt64]$legacyItem.Length
        sha256 = $legacySha256
    }
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
    resumed_existing_destination = [bool]$legacyEnvelope.resumed_existing_destination
    legacy_envelope_validation = [ordered]@{
        contract = 'animus-ferric-runtime-recovery-publication-v4'
        passed = $true
    }
    passed = $true
}
$correctionCheck = Test-CorrectionEvidence -Evidence $correction `
    -EpochFiveControlSha256 $controlSha256 `
    -EpochFourControlSha256 $epochFourControlSha256 `
    -LegacyEnvelopeSha256 $legacySha256 `
    -LegacyEnvelopeBytes ([UInt64]$legacyItem.Length) `
    -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
    -ExpectedResumedExistingDestination `
        ([bool]$legacyEnvelope.resumed_existing_destination)
Assert-PublicationCorrection ([bool]$correctionCheck.passed) `
    "constructed correction evidence differs: $(@($correctionCheck.errors) -join '; ')"
Write-EpochFiveJsonAtomic -Path $correctionEvidencePath -Value $correction
$correction | ConvertTo-Json -Depth 64
