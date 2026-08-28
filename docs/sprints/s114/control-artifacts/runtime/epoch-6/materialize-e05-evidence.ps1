#requires -Version 7.5
param()

$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$runtimeDir = Split-Path -Parent $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$controlPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'
$selfTestPath = Join-Path $artifactDir 'materialization-self-test.json'
$epochSixCommonPath = Join-Path $artifactDir 'materialization-common.ps1'

function Assert-MaterializationCondition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Get-MaterializationBootstrapSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $hasher = [System.Security.Cryptography.SHA256]::Create()
        try {
            [Convert]::ToHexString($hasher.ComputeHash($stream)).ToLowerInvariant()
        }
        finally { $hasher.Dispose() }
    }
    finally { $stream.Dispose() }
}

function Read-MaterializationJson {
    param([Parameter(Mandatory = $true)][string]$Path)

    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Resolve-MaterializationBootstrapPath {
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    Assert-MaterializationCondition `
        (-not [string]::IsNullOrWhiteSpace($RelativePath) -and
            -not [System.IO.Path]::IsPathRooted($RelativePath) -and
            $RelativePath.IndexOf([char]0) -lt 0 -and
            $RelativePath.IndexOf(':') -lt 0 -and
            $RelativePath -notmatch '(^|[\\/])\.{1,2}([\\/]|$)') `
        "unsafe bootstrap relative path: $RelativePath"
    $root = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resolved = [System.IO.Path]::GetFullPath((Join-Path $root $RelativePath))
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-MaterializationCondition `
        ($resolved.StartsWith(
                $prefix,
                [System.StringComparison]::OrdinalIgnoreCase
            )) `
        "bootstrap path escaped repository: $RelativePath"
    $resolved
}

function Assert-MaterializationBootstrapLeaf {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][UInt64]$Bytes,
        [Parameter(Mandatory = $true)][string]$Sha256,
        [Parameter(Mandatory = $true)][string]$Label
    )

    Assert-MaterializationCondition `
        (Test-Path -LiteralPath $Path -PathType Leaf) `
        "$Label is absent or not a file"
    $item = Get-Item -LiteralPath $Path -Force
    Assert-MaterializationCondition `
        (-not $item.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) `
        "$Label is a reparse point"
    Assert-MaterializationCondition ([UInt64]$item.Length -eq $Bytes) `
        "$Label byte count differs"
    Assert-MaterializationCondition `
        ((Get-MaterializationBootstrapSha256 -Path $Path) -ceq $Sha256) `
        "$Label SHA-256 differs"
}

Assert-MaterializationCondition `
    ((Test-Path -LiteralPath $controlPath -PathType Leaf) -and
        (Test-Path -LiteralPath $digestPath -PathType Leaf)) `
    'epoch-6 frozen control pair is absent'
$controlItem = Get-Item -LiteralPath $controlPath -Force
$digestItem = Get-Item -LiteralPath $digestPath -Force
Assert-MaterializationCondition `
    (-not $controlItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and
        -not $digestItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) `
    'epoch-6 frozen control pair contains a reparse point'
$controlSha256 = Get-MaterializationBootstrapSha256 -Path $controlPath
$digestLine = (Get-Content -Raw -LiteralPath $digestPath).TrimEnd("`r", "`n")
Assert-MaterializationCondition `
    ($digestLine -ceq "$controlSha256  control-inputs.json") `
    'epoch-6 frozen control digest differs'

$controls = Read-MaterializationJson -Path $controlPath
$plan = Read-MaterializationJson -Path $planPath
$bootstrapHead = (& git -C $artifactDir rev-parse HEAD).Trim()
Assert-MaterializationCondition ($LASTEXITCODE -eq 0) `
    'could not determine repository HEAD during bootstrap'
$bootstrapRepoRoot = [System.IO.Path]::GetFullPath(
    (& git -C $artifactDir rev-parse --show-toplevel).Trim()
)
Assert-MaterializationCondition ($LASTEXITCODE -eq 0) `
    'could not determine repository root during bootstrap'

$expectedStaticNames = @(
    '.gitattributes',
    'README.md',
    'incident.json',
    'runtime-plan.json',
    'materialization-common.ps1',
    'test-materialization.ps1',
    'freeze-materialization.ps1',
    'materialize-e05-evidence.ps1'
)
$expectedBootstrapControlProperties = @(
    'schema',
    'task',
    'operation_id',
    'correction_operation_id',
    'failed_operation_id',
    'execution_epoch',
    'failed_publication_epoch',
    'failed_correction_epoch',
    'materialization_epoch',
    'timestamp_protocol',
    'frozen_at_utc',
    'runtime_plan_sha256',
    'incident_sha256',
    'repository',
    'static_controls',
    'prefreeze_self_test_failure_archive',
    'materialization_self_test',
    'dependency_verification',
    'source_verification',
    'destination_verification',
    'model',
    'cold_state',
    'materialization_preconditions',
    'passed'
)
$expectedFailureArchiveProperties = @(
    'relative_path',
    'bytes',
    'sha256',
    'schema',
    'tested_at_utc',
    'test_count',
    'exact_model_hashes',
    'sole_failed_test',
    'report_passed',
    'semantic_validation_passed'
)
$bootstrapControlProperties = @($controls.PSObject.Properties.Name)
$bootstrapFailureArchive = $controls.prefreeze_self_test_failure_archive
$bootstrapFailureArchiveProperties = if ($null -eq $bootstrapFailureArchive) {
    @()
}
else { @($bootstrapFailureArchive.PSObject.Properties.Name) }
$planFailureArchive = $plan.prefreeze_self_test_failure
$bootstrapStatic = @($controls.static_controls)
Assert-MaterializationCondition `
    (($bootstrapControlProperties -join "`n") -ceq
            ($expectedBootstrapControlProperties -join "`n") -and
        ($bootstrapFailureArchiveProperties -join "`n") -ceq
            ($expectedFailureArchiveProperties -join "`n") -and
        [string]$controls.schema -ceq
        'animus-ferric-runtime-evidence-materialization-control-inputs-v6' -and
        [string]$controls.task -ceq 'T-11409' -and
        [string]$controls.operation_id -ceq
            'r06-materialize-e05-publication-evidence' -and
        [string]$controls.correction_operation_id -ceq
            'r05-publish-e03-01-q4-32768-after-e04-wrapper-failure' -and
        [string]$controls.failed_operation_id -ceq
            'r04-publish-e03-01-q4-32768' -and
        [int]$controls.execution_epoch -eq 3 -and
        [int]$controls.failed_publication_epoch -eq 4 -and
        [int]$controls.failed_correction_epoch -eq 5 -and
        [int]$controls.materialization_epoch -eq 6 -and
        [string]$controls.timestamp_protocol -ceq
            'powershell-json-datekind-string-rfc3339-v1' -and
        [string]$controls.repository.head_at_freeze -ceq $bootstrapHead -and
        [string]$controls.repository.epoch_6_pre_control_base -ceq
            $bootstrapHead -and
        [string]$plan.repository_commit_before_epoch_6_controls -ceq
            $bootstrapHead -and
        [string]$controls.runtime_plan_sha256 -ceq
            (Get-MaterializationBootstrapSha256 -Path $planPath) -and
        [string]$bootstrapFailureArchive.relative_path -ceq
            [string]$planFailureArchive.relative_path -and
        [UInt64]$bootstrapFailureArchive.bytes -eq
            [UInt64]$planFailureArchive.bytes -and
        [string]$bootstrapFailureArchive.sha256 -ceq
            [string]$planFailureArchive.sha256 -and
        [string]$bootstrapFailureArchive.schema -ceq
            [string]$planFailureArchive.schema -and
        [string]$bootstrapFailureArchive.tested_at_utc -ceq
            [string]$planFailureArchive.tested_at_utc -and
        [int]$bootstrapFailureArchive.test_count -eq
            [int]$planFailureArchive.test_count -and
        [int]$bootstrapFailureArchive.exact_model_hashes -eq
            [int]$planFailureArchive.exact_model_hashes -and
        [string]$bootstrapFailureArchive.sole_failed_test -ceq
            [string]$planFailureArchive.sole_failed_test -and
        $bootstrapFailureArchive.report_passed -is [bool] -and
        -not [bool]$bootstrapFailureArchive.report_passed -and
        $bootstrapFailureArchive.semantic_validation_passed -is [bool] -and
        [bool]$bootstrapFailureArchive.semantic_validation_passed -and
        $bootstrapStatic.Count -eq $expectedStaticNames.Count -and
        [bool]$controls.passed) `
    'epoch-6 frozen control bootstrap identity differs'

for ($index = 0; $index -lt $expectedStaticNames.Count; $index++) {
    $name = [string]$expectedStaticNames[$index]
    $entry = $bootstrapStatic[$index]
    Assert-MaterializationCondition ([string]$entry.path -ceq $name) `
        "epoch-6 frozen static order differs at: $name"
    Assert-MaterializationBootstrapLeaf -Path (Join-Path $artifactDir $name) `
        -Bytes ([UInt64]$entry.bytes) -Sha256 ([string]$entry.sha256) `
        -Label "epoch-6 static control $name"
}

$epochFourCommonPath = Resolve-MaterializationBootstrapPath `
    -RepositoryRoot $bootstrapRepoRoot `
    -RelativePath ([string]$plan.epoch_4.runtime_common.relative_path)
$epochFiveCommonPath = Resolve-MaterializationBootstrapPath `
    -RepositoryRoot $bootstrapRepoRoot `
    -RelativePath ([string]$plan.epoch_5.publication_common.relative_path)
Assert-MaterializationBootstrapLeaf -Path $epochFourCommonPath `
    -Bytes ([UInt64]$plan.epoch_4.runtime_common.bytes) `
    -Sha256 ([string]$plan.epoch_4.runtime_common.sha256) `
    -Label 'frozen epoch-4 runtime common'
Assert-MaterializationBootstrapLeaf -Path $epochFiveCommonPath `
    -Bytes ([UInt64]$plan.epoch_5.publication_common.bytes) `
    -Sha256 ([string]$plan.epoch_5.publication_common.sha256) `
    -Label 'frozen epoch-5 publication common'

. $epochFourCommonPath
. $epochFiveCommonPath
. $epochSixCommonPath

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
Assert-MaterializationCondition `
    ([System.IO.Path]::GetFullPath($repoRoot).Equals(
            $bootstrapRepoRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) `
    'bootstrap and frozen helper repository roots differ'
Assert-MaterializationCondition (Test-EpochSixPlanIdentity -Plan $plan) `
    'epoch-6 runtime plan identity differs'
Assert-MaterializationCondition `
    (Test-EpochSixStrictUtc -Value $controls.frozen_at_utc) `
    'epoch-6 freeze timestamp is not strict UTC'
Assert-MaterializationCondition `
    (Test-EpochSixExactPropertySequence `
        -Value $controls.prefreeze_self_test_failure_archive `
        -Expected $expectedFailureArchiveProperties) `
    'epoch-6 frozen failed-self-test archive control has a different contract'

function Assert-MaterializationAnchor {
    param(
        [Parameter(Mandatory = $true)]$Anchor,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $check = Test-EpochSixFileAnchor -RepositoryRoot $repoRoot `
        -Anchor $Anchor -Label $Label
    Assert-MaterializationCondition ([bool]$check.passed) `
        "$Label differs: $(@($check.errors) -join '; ')"
    $check
}

function Assert-MaterializationNonReparseDirectoryChain {
    param([Parameter(Mandatory = $true)][string]$Path)

    $root = [System.IO.Path]::GetFullPath($repoRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resolved = [System.IO.Path]::GetFullPath($Path)
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-MaterializationCondition `
        ($resolved.StartsWith(
                $prefix,
                [System.StringComparison]::OrdinalIgnoreCase
            )) `
        'evidence parent is outside the repository'
    $relative = [System.IO.Path]::GetRelativePath($root, $resolved)
    $cursor = $root
    $segments = @($relative -split '[\\/]' | Where-Object {
            -not [string]::IsNullOrEmpty([string]$_) -and
            [string]$_ -cne '.'
        })
    foreach ($segment in $segments) {
        $cursor = Join-Path $cursor $segment
        Assert-MaterializationCondition `
            (Test-Path -LiteralPath $cursor -PathType Container) `
            "evidence parent component is absent: $cursor"
        Assert-MaterializationCondition `
            (-not (Get-Item -LiteralPath $cursor -Force).Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                )) `
            "evidence parent component is a reparse point: $cursor"
    }
}

function Get-MaterializationPathState {
    param([Parameter(Mandatory = $true)][string]$Path)

    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    $exists = $null -ne $item
    [pscustomobject][ordered]@{
        exists = $exists
        leaf = ($exists -and -not [bool]$item.PSIsContainer)
        container = ($exists -and [bool]$item.PSIsContainer)
        non_reparse = ($exists -and -not $item.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            ))
    }
}

$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-MaterializationCondition ($LASTEXITCODE -eq 0 -and $head -ceq $bootstrapHead) `
    'repository HEAD changed after bootstrap'
$dependencyCheck = Test-EpochSixFrozenDependencySet `
    -RepositoryRoot $repoRoot -Plan $plan -ExpectedHead $head
Assert-MaterializationCondition `
    ([bool]$dependencyCheck.passed -and
        [int]$dependencyCheck.epoch_5_static_controls_checked -eq 8 -and
        [int]$dependencyCheck.epoch_4_static_controls_checked -eq 12 -and
        [int]$dependencyCheck.transitive_epoch_3_controls_checked -eq 20) `
    "frozen dependency set differs: $(@($dependencyCheck.errors) -join '; ')"
Assert-MaterializationCondition `
    ([int]$controls.dependency_verification.epoch_5_static_controls_checked -eq 8 -and
        [int]$controls.dependency_verification.epoch_4_static_controls_checked -eq 12 -and
        [int]$controls.dependency_verification.transitive_epoch_3_controls_checked -eq 20 -and
        [int]$controls.dependency_verification.frozen_epoch_4_rewalk_static_controls_checked -eq 12 -and
        [int]$controls.dependency_verification.frozen_epoch_3_rewalk_controls_checked -eq 20 -and
        [bool]$controls.dependency_verification.passed) `
    'epoch-6 frozen dependency-verification record differs'

$selfTestCheck = Assert-MaterializationAnchor -Anchor (
    [pscustomobject]@{
        relative_path = [string]$controls.materialization_self_test.relative_path
        bytes = [UInt64]$controls.materialization_self_test.bytes
        sha256 = [string]$controls.materialization_self_test.sha256
    }
) -Label 'epoch-6 materialization self-test'
$selfTest = Read-MaterializationJson -Path ([string]$selfTestCheck.resolved_path)
Assert-MaterializationCondition `
    ([bool]$controls.materialization_self_test.passed -and
        [bool]$selfTest.passed -and
        [int]$controls.materialization_self_test.test_count -eq
            [int]$selfTest.test_count -and
        [int]$controls.materialization_self_test.exact_model_hashes -eq 1 -and
        [int]$selfTest.exact_model_hashes -eq 1) `
    'epoch-6 materialization self-test differs'
$failedSelfTestAnchor = Assert-MaterializationAnchor `
    -Anchor $plan.prefreeze_self_test_failure `
    -Label 'epoch-6 pre-freeze failed self-test archive'
$failedSelfTest = Read-MaterializationJson `
    -Path ([string]$failedSelfTestAnchor.resolved_path)
$failedSelfTestValidation = Test-EpochSixFailedSelfTestReport `
    -Report $failedSelfTest -Plan $plan
Assert-MaterializationCondition ([bool]$failedSelfTestValidation.passed) `
    "pre-freeze failed self-test archive differs: $(@($failedSelfTestValidation.errors) -join '; ')"
$failureArchiveControl = $controls.prefreeze_self_test_failure_archive
Assert-MaterializationCondition `
    ([string]$failureArchiveControl.relative_path -ceq
            [string]$plan.prefreeze_self_test_failure.relative_path -and
        [UInt64]$failureArchiveControl.bytes -eq
            [UInt64]$plan.prefreeze_self_test_failure.bytes -and
        [UInt64]$failedSelfTestAnchor.bytes -eq
            [UInt64]$failureArchiveControl.bytes -and
        [string]$failureArchiveControl.sha256 -ceq
            [string]$plan.prefreeze_self_test_failure.sha256 -and
        [string]$failedSelfTestAnchor.sha256 -ceq
            [string]$failureArchiveControl.sha256 -and
        [string]$failureArchiveControl.schema -ceq
            [string]$plan.prefreeze_self_test_failure.schema -and
        [string]$failedSelfTest.schema -ceq
            [string]$failureArchiveControl.schema -and
        [string]$failureArchiveControl.tested_at_utc -ceq
            [string]$plan.prefreeze_self_test_failure.tested_at_utc -and
        [string]$failedSelfTest.tested_at_utc -ceq
            [string]$failureArchiveControl.tested_at_utc -and
        [int]$failureArchiveControl.test_count -eq
            [int]$plan.prefreeze_self_test_failure.test_count -and
        [int]$failedSelfTest.test_count -eq
            [int]$failureArchiveControl.test_count -and
        [int]$failureArchiveControl.exact_model_hashes -eq
            [int]$plan.prefreeze_self_test_failure.exact_model_hashes -and
        [int]$failedSelfTest.exact_model_hashes -eq
            [int]$failureArchiveControl.exact_model_hashes -and
        [string]$failureArchiveControl.sole_failed_test -ceq
            [string]$plan.prefreeze_self_test_failure.sole_failed_test -and
        @($failedSelfTest.results | Where-Object {
                -not [bool]$_.passed
            }).Count -eq 1 -and
        [string](@($failedSelfTest.results | Where-Object {
                    -not [bool]$_.passed
                })[0].name) -ceq [string]$failureArchiveControl.sole_failed_test -and
        $failureArchiveControl.report_passed -is [bool] -and
        -not [bool]$failureArchiveControl.report_passed -and
        $failedSelfTest.passed -is [bool] -and
        -not [bool]$failedSelfTest.passed -and
        $failureArchiveControl.semantic_validation_passed -is [bool] -and
        [bool]$failureArchiveControl.semantic_validation_passed) `
    'pre-freeze failed self-test archive differs from plan or frozen controls'
Assert-MaterializationCondition `
    ((Get-Sha256Lower -Path $incidentPath) -ceq
        [string]$controls.incident_sha256) `
    'epoch-6 frozen incident differs'

$epochFivePlanPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_5.runtime_plan `
        -Label 'frozen epoch-5 runtime plan').resolved_path
$epochFourPlanPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_4.runtime_plan `
        -Label 'frozen epoch-4 runtime plan').resolved_path
$sourcePlanPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_3.runtime_plan `
        -Label 'frozen epoch-3 runtime plan').resolved_path
$rawAnchorPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_4.raw_source_anchor `
        -Label 'frozen raw-source anchor').resolved_path
$publisherPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_5.frozen_failed_publisher `
        -Label 'frozen epoch-5 publisher').resolved_path
[void](Assert-MaterializationAnchor -Anchor $plan.epoch_4.verifier `
        -Label 'frozen epoch-4 verifier')
$epochFivePlan = Read-MaterializationJson -Path $epochFivePlanPath
$epochFourPlan = Read-MaterializationJson -Path $epochFourPlanPath
$sourcePlan = Read-MaterializationJson -Path $sourcePlanPath
$rawAnchor = Read-MaterializationJson -Path $rawAnchorPath
Assert-MaterializationCondition `
    ([string]$epochFivePlan.operation.id -ceq
            [string]$plan.operation.correction_operation_id -and
        [string]$epochFourPlan.operation.id -ceq
            [string]$plan.operation.failed_operation_id) `
    'frozen predecessor operation tuple differs'

$epochFiveControlPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_5.control_manifest `
        -Label 'frozen epoch-5 control manifest').resolved_path
$epochFourControlPath = [string](Assert-MaterializationAnchor `
        -Anchor $plan.epoch_4.control_manifest `
        -Label 'frozen epoch-4 control manifest').resolved_path
$epochFiveControlSha256 = Get-Sha256Lower -Path $epochFiveControlPath
$epochFourControlSha256 = Get-Sha256Lower -Path $epochFourControlPath

$sourceRoot = Resolve-EpochSixRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-EpochSixRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$sourceCheck = Test-EpochSixExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$destinationCheck = Test-EpochSixExactTree -Root $destinationRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
Assert-MaterializationCondition ([bool]$sourceCheck.passed) `
    "raw source differs: $(@($sourceCheck.errors) -join '; ')"
Assert-MaterializationCondition ([bool]$destinationCheck.passed) `
    "published destination differs: $(@($destinationCheck.errors) -join '; ')"
Assert-MaterializationCondition `
    ([string]$sourceCheck.manifest_sha256 -ceq
            [string]$controls.source_verification.manifest_sha256 -and
        [int]$sourceCheck.entries -eq [int]$controls.source_verification.entries -and
        [UInt64]$sourceCheck.payload_bytes -eq
            [UInt64]$controls.source_verification.payload_bytes -and
        [string]$controls.source_verification.relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$controls.source_verification.attempt_sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$controls.source_verification.attestation_sha256 -ceq
            [string]$plan.operation.attestation.sha256 -and
        [bool]$controls.source_verification.terminal_facts_passed -and
        [bool]$controls.source_verification.passed) `
    'raw source differs from epoch-6 frozen controls'
Assert-MaterializationCondition `
    ([string]$destinationCheck.manifest_sha256 -ceq
            [string]$controls.destination_verification.manifest_sha256 -and
        [string]$destinationCheck.manifest_sha256 -ceq
            [string]$plan.published_destination.manifest_sha256 -and
        [int]$destinationCheck.entries -eq
            [int]$controls.destination_verification.entries -and
        [int]$destinationCheck.entries -eq
            [int]$plan.published_destination.entries -and
        [UInt64]$destinationCheck.payload_bytes -eq
            [UInt64]$controls.destination_verification.payload_bytes -and
        [UInt64]$destinationCheck.payload_bytes -eq
            [UInt64]$plan.published_destination.payload_bytes -and
        [string]$controls.destination_verification.relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$controls.destination_verification.verifier_invocations -eq 1 -and
        [bool]$controls.destination_verification.passed) `
    'published destination differs from epoch-6 frozen controls'
Assert-MaterializationCondition `
    ((Get-Sha256Lower -Path (Join-Path $destinationRoot 'attempt.json')) -ceq
            [string]$plan.published_destination.attempt_sha256 -and
        (Get-Sha256Lower -Path (Join-Path $destinationRoot 'attestation.json')) -ceq
            [string]$plan.published_destination.attestation_sha256) `
    'published destination selected identities differ'

$destinationReport = $controls.destination_verification.report
$destinationReportCheck = Test-EpochSixDestinationVerification `
    -Report $destinationReport -Plan $plan `
    -EpochFourPlan $epochFourPlan -SourcePlan $sourcePlan `
    -DestinationPath $destinationRoot
Assert-MaterializationCondition ([bool]$destinationReportCheck.passed) `
    "frozen destination report differs: $(@($destinationReportCheck.errors) -join '; ')"
Assert-MaterializationCondition `
    ([bool]$destinationReport.live_model_identity.checked -and
        [string]$destinationReport.live_model_identity.mode -ceq
            'checked_in_verifier' -and
        [string]$destinationReport.live_model_identity.sha256 -ceq
            [string]$plan.model.sha256 -and
        [string]$controls.model.sha256 -ceq [string]$plan.model.sha256 -and
        [UInt64]$controls.model.bytes -eq [UInt64]$plan.model.bytes -and
        [string]$controls.model.hash_mode -ceq 'checked_in_verifier' -and
        [bool]$controls.model.independently_rehashed_by_frozen_verifier -and
        [bool]$controls.model.passed) `
    'freeze-time model verification differs'
$modelPath = Resolve-EpochSixRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.model.relative_path)
Assert-MaterializationCondition (Test-Path -LiteralPath $modelPath -PathType Leaf) `
    'Q4 model is absent'
$modelItem = Get-Item -LiteralPath $modelPath -Force
Assert-MaterializationCondition `
    (-not $modelItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and [UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes) `
    'Q4 model live byte identity differs'

$preconditions = $controls.materialization_preconditions
Assert-MaterializationCondition `
    ([string]$preconditions.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [bool]$preconditions.destination_exact_at_freeze -and
        [string]$preconditions.legacy_envelope_relative_path -ceq
            [string]$plan.operation.legacy_envelope_relative_path -and
        [bool]$preconditions.legacy_envelope_absent_at_freeze -and
        [string]$preconditions.correction_evidence_relative_path -ceq
            [string]$plan.operation.correction_evidence_relative_path -and
        [bool]$preconditions.correction_evidence_absent_at_freeze -and
        [string]$preconditions.materialization_evidence_relative_path -ceq
            [string]$plan.operation.materialization_evidence_relative_path -and
        [bool]$preconditions.materialization_evidence_absent_at_freeze -and
        [bool]$preconditions.passed) `
    'epoch-6 materialization preconditions differ'

$legacyEnvelopePath = Resolve-EpochSixRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.legacy_envelope_relative_path)
$correctionEvidencePath = Resolve-EpochSixRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.correction_evidence_relative_path)
$materializationEvidencePath = Resolve-EpochSixRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.materialization_evidence_relative_path)
foreach ($parent in @(
        (Split-Path -Parent $legacyEnvelopePath),
        (Split-Path -Parent $correctionEvidencePath),
        (Split-Path -Parent $materializationEvidencePath)
    )) {
    Assert-MaterializationNonReparseDirectoryChain -Path $parent
}

$destinationState = Get-MaterializationPathState -Path $destinationRoot
$legacyState = Get-MaterializationPathState -Path $legacyEnvelopePath
$correctionState = Get-MaterializationPathState -Path $correctionEvidencePath
$materializationState = Get-MaterializationPathState `
    -Path $materializationEvidencePath
$legacyEnvelope = $null
$correctionEvidence = $null
$materializationEvidence = $null
$legacyExact = $false
$correctionExact = $false
$materializationExact = $false
$legacySha256 = $null
$legacyBytes = [UInt64]0
$correctionSha256 = $null
$correctionBytes = [UInt64]0

if ($legacyState.exists -and $legacyState.leaf -and $legacyState.non_reparse) {
    $legacyEnvelope = Read-MaterializationJson -Path $legacyEnvelopePath
    $legacyCheck = Test-EpochSixLegacyRecoveryEnvelope `
        -Envelope $legacyEnvelope -Plan $plan -EpochFourPlan $epochFourPlan `
        -SourcePlan $sourcePlan -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -DestinationPath $destinationRoot -VerificationReport $destinationReport `
        -ResumedExistingDestination $true
    $legacyExact = [bool]$legacyCheck.passed
    if ($legacyExact) {
        $legacyItem = Get-Item -LiteralPath $legacyEnvelopePath -Force
        $legacyBytes = [UInt64]$legacyItem.Length
        $legacySha256 = Get-Sha256Lower -Path $legacyEnvelopePath
    }
}
if ($correctionState.exists -and $correctionState.leaf -and
    $correctionState.non_reparse -and $legacyExact) {
    $correctionEvidence = Read-MaterializationJson -Path $correctionEvidencePath
    $correctionCheck = Test-EpochSixCorrectionEvidence `
        -Evidence $correctionEvidence -Plan $plan `
        -EpochFourPlan $epochFourPlan `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes -SourceCheck $sourceCheck `
        -DestinationCheck $destinationCheck `
        -ResumedExistingDestination $true
    $correctionExact = [bool]$correctionCheck.passed
    if ($correctionExact) {
        $correctionItem = Get-Item -LiteralPath $correctionEvidencePath -Force
        $correctionBytes = [UInt64]$correctionItem.Length
        $correctionSha256 = Get-Sha256Lower -Path $correctionEvidencePath
    }
}
if ($materializationState.exists -and $materializationState.leaf -and
    $materializationState.non_reparse -and $legacyExact -and $correctionExact) {
    $materializationEvidence = Read-MaterializationJson `
        -Path $materializationEvidencePath
    $materializationCheck = Test-EpochSixMaterializationEvidence `
        -Evidence $materializationEvidence -Plan $plan `
        -EpochSixControlSha256 $controlSha256 `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes `
        -CorrectionEvidenceSha256 $correctionSha256 `
        -CorrectionEvidenceBytes $correctionBytes `
        -DestinationCheck $destinationCheck `
        -ResumedExistingDestination $true
    $materializationExact = [bool]$materializationCheck.passed
}

$stateCheck = Test-EpochSixMaterializationState `
    -DestinationExists ([bool]$destinationState.exists) `
    -DestinationContainer ([bool]$destinationState.container) `
    -DestinationNonReparse ([bool]$destinationState.non_reparse) `
    -DestinationExact ([bool]$destinationCheck.passed) `
    -LegacyEnvelopeExists ([bool]$legacyState.exists) `
    -LegacyEnvelopeLeaf ([bool]$legacyState.leaf) `
    -LegacyEnvelopeNonReparse ([bool]$legacyState.non_reparse) `
    -LegacyEnvelopeExact $legacyExact `
    -CorrectionEvidenceExists ([bool]$correctionState.exists) `
    -CorrectionEvidenceLeaf ([bool]$correctionState.leaf) `
    -CorrectionEvidenceNonReparse ([bool]$correctionState.non_reparse) `
    -CorrectionEvidenceExact $correctionExact `
    -MaterializationEvidenceExists ([bool]$materializationState.exists) `
    -MaterializationEvidenceLeaf ([bool]$materializationState.leaf) `
    -MaterializationEvidenceNonReparse ([bool]$materializationState.non_reparse) `
    -MaterializationEvidenceExact $materializationExact
Assert-MaterializationCondition ([bool]$stateCheck.passed) `
    "materialization state is invalid: $(@($stateCheck.errors) -join '; ')"

if (-not $legacyState.exists) {
    $legacyEnvelope = New-EpochSixLegacyRecoveryEnvelope `
        -Plan $plan -EpochFourPlan $epochFourPlan `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -VerificationReport $destinationReport `
        -PublishedAtUtc ((Get-Date).ToUniversalTime().ToString(
                "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
            )) -ResumedExistingDestination $true
    $legacyCheck = Test-EpochSixLegacyRecoveryEnvelope `
        -Envelope $legacyEnvelope -Plan $plan -EpochFourPlan $epochFourPlan `
        -SourcePlan $sourcePlan -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -DestinationPath $destinationRoot -VerificationReport $destinationReport `
        -ResumedExistingDestination $true
    Assert-MaterializationCondition ([bool]$legacyCheck.passed) `
        "constructed legacy envelope differs: $(@($legacyCheck.errors) -join '; ')"
    Assert-MaterializationCondition `
        (Test-EpochSixExactPropertySequence -Value $legacyEnvelope.source `
            -Expected @('relative_path', 'manifest_sha256', 'entries')) `
        'constructed legacy source is not a JSON-native exact object'
    Assert-MaterializationCondition `
        (Test-EpochSixExactPropertySequence -Value $legacyEnvelope.destination `
            -Expected @('relative_path', 'manifest_sha256', 'entries')) `
        'constructed legacy destination is not a JSON-native exact object'
    Write-EpochSixJsonAtomic -Path $legacyEnvelopePath -Value $legacyEnvelope
    $legacyEnvelope = Read-MaterializationJson -Path $legacyEnvelopePath
    $legacyCheck = Test-EpochSixLegacyRecoveryEnvelope `
        -Envelope $legacyEnvelope -Plan $plan -EpochFourPlan $epochFourPlan `
        -SourcePlan $sourcePlan -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceCheck -DestinationCheck $destinationCheck `
        -DestinationPath $destinationRoot -VerificationReport $destinationReport `
        -ResumedExistingDestination $true
    Assert-MaterializationCondition ([bool]$legacyCheck.passed) `
        "published legacy envelope differs: $(@($legacyCheck.errors) -join '; ')"
    $legacyItem = Get-Item -LiteralPath $legacyEnvelopePath -Force
    $legacyBytes = [UInt64]$legacyItem.Length
    $legacySha256 = Get-Sha256Lower -Path $legacyEnvelopePath
}

if (-not $correctionState.exists) {
    $correctionEvidence = New-EpochSixCorrectionEvidence `
        -Plan $plan -EpochFourPlan $epochFourPlan `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes -SourceCheck $sourceCheck `
        -DestinationCheck $destinationCheck `
        -CorrectedAtUtc ((Get-Date).ToUniversalTime().ToString(
                "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
            )) -ResumedExistingDestination $true
    $correctionCheck = Test-EpochSixCorrectionEvidence `
        -Evidence $correctionEvidence -Plan $plan `
        -EpochFourPlan $epochFourPlan `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes -SourceCheck $sourceCheck `
        -DestinationCheck $destinationCheck `
        -ResumedExistingDestination $true
    Assert-MaterializationCondition ([bool]$correctionCheck.passed) `
        "constructed correction evidence differs: $(@($correctionCheck.errors) -join '; ')"
    Write-EpochSixJsonAtomic -Path $correctionEvidencePath `
        -Value $correctionEvidence
    $correctionEvidence = Read-MaterializationJson -Path $correctionEvidencePath
    $correctionCheck = Test-EpochSixCorrectionEvidence `
        -Evidence $correctionEvidence -Plan $plan `
        -EpochFourPlan $epochFourPlan `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes -SourceCheck $sourceCheck `
        -DestinationCheck $destinationCheck `
        -ResumedExistingDestination $true
    Assert-MaterializationCondition ([bool]$correctionCheck.passed) `
        "published correction evidence differs: $(@($correctionCheck.errors) -join '; ')"
    $correctionItem = Get-Item -LiteralPath $correctionEvidencePath -Force
    $correctionBytes = [UInt64]$correctionItem.Length
    $correctionSha256 = Get-Sha256Lower -Path $correctionEvidencePath
}

# Both predecessor records now exist and have already passed the epoch-6 exact
# validators. The frozen epoch-5 publisher must therefore take only its
# authoritative already-complete path and return the on-disk correction.
$publisherResult = Invoke-PowerShellFileBounded -ScriptPath $publisherPath `
    -TimeoutMilliseconds 300000
$publisherCorrection = try {
    $publisherResult.stdout | ConvertFrom-Json -DateKind String
}
catch { $null }
Assert-MaterializationCondition `
    ([int]$publisherResult.exit_code -eq 0 -and
        $null -ne $publisherCorrection -and
        (Test-JsonEquivalent -Left $publisherCorrection `
            -Right $correctionEvidence)) `
    "frozen epoch-5 authoritative revalidation failed: $([string]$publisherResult.stderr)"
$publisherIdentity = Test-EpochSixFileAnchor -RepositoryRoot $repoRoot `
    -Anchor $plan.epoch_5.frozen_failed_publisher `
    -Label 'post-run frozen epoch-5 publisher'
Assert-MaterializationCondition ([bool]$publisherIdentity.passed) `
    "frozen epoch-5 publisher changed: $(@($publisherIdentity.errors) -join '; ')"
Assert-MaterializationCondition `
    ((Get-Sha256Lower -Path $legacyEnvelopePath) -ceq $legacySha256 -and
        (Get-Sha256Lower -Path $correctionEvidencePath) -ceq $correctionSha256) `
    'predecessor evidence changed during authoritative revalidation'

if ($materializationState.exists) {
    $materializationEvidence = Read-MaterializationJson `
        -Path $materializationEvidencePath
}
else {
    $materializationEvidence = New-EpochSixMaterializationEvidence `
        -Plan $plan -EpochSixControlSha256 $controlSha256 `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes `
        -CorrectionEvidenceSha256 $correctionSha256 `
        -CorrectionEvidenceBytes $correctionBytes `
        -DestinationCheck $destinationCheck `
        -MaterializedAtUtc ((Get-Date).ToUniversalTime().ToString(
                "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
            )) -ResumedExistingDestination $true
    $newMaterializationCheck = Test-EpochSixMaterializationEvidence `
        -Evidence $materializationEvidence -Plan $plan `
        -EpochSixControlSha256 $controlSha256 `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -LegacyEnvelopeSha256 $legacySha256 `
        -LegacyEnvelopeBytes $legacyBytes `
        -CorrectionEvidenceSha256 $correctionSha256 `
        -CorrectionEvidenceBytes $correctionBytes `
        -DestinationCheck $destinationCheck `
        -ResumedExistingDestination $true
    Assert-MaterializationCondition ([bool]$newMaterializationCheck.passed) `
        "constructed materialization differs: $(@($newMaterializationCheck.errors) -join '; ')"
    Write-EpochSixJsonAtomic -Path $materializationEvidencePath `
        -Value $materializationEvidence
    $materializationEvidence = Read-MaterializationJson `
        -Path $materializationEvidencePath
}

$finalMaterializationCheck = Test-EpochSixMaterializationEvidence `
    -Evidence $materializationEvidence -Plan $plan `
    -EpochSixControlSha256 $controlSha256 `
    -EpochFiveControlSha256 $epochFiveControlSha256 `
    -LegacyEnvelopeSha256 $legacySha256 `
    -LegacyEnvelopeBytes $legacyBytes `
    -CorrectionEvidenceSha256 $correctionSha256 `
    -CorrectionEvidenceBytes $correctionBytes `
    -DestinationCheck $destinationCheck `
    -ResumedExistingDestination $true
Assert-MaterializationCondition ([bool]$finalMaterializationCheck.passed) `
    "final materialization evidence differs: $(@($finalMaterializationCheck.errors) -join '; ')"
$materializationEvidence | ConvertTo-Json -Depth 96
