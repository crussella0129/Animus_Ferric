#requires -Version 7.5
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$runtimeRoot = Split-Path -Parent $artifactDir
$epochFourCommonPath = Join-Path $runtimeRoot 'epoch-4/runtime-common.ps1'
$epochFiveCommonPath = Join-Path $runtimeRoot 'epoch-5/publication-common.ps1'
$epochSixCommonPath = Join-Path $artifactDir 'materialization-common.ps1'
$selfTestPath = Join-Path $artifactDir 'materialization-self-test.json'

function Get-BootstrapSha256 {
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

function Assert-BootstrapFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][UInt64]$Bytes,
        [Parameter(Mandatory = $true)][string]$Sha256,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is absent before bootstrap"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        [UInt64]$item.Length -ne $Bytes -or
        (Get-BootstrapSha256 -Path $Path) -cne $Sha256) {
        throw "$Label bootstrap identity differs"
    }
}

Assert-BootstrapFile -Path $epochFourCommonPath -Bytes 87625 `
    -Sha256 '322407fc52e2192cedf320d65e9c8029c75d1190e732e3d76a27394614eaf59c' `
    -Label 'frozen epoch-4 runtime-common.ps1'
Assert-BootstrapFile -Path $epochFiveCommonPath -Bytes 39136 `
    -Sha256 '332475b8b83d5668ed9d7cb5d34fddfd720e4fb1328b91f9cbc7c44e64f994f1' `
    -Label 'frozen epoch-5 publication-common.ps1'

if (-not (Test-Path -LiteralPath $selfTestPath -PathType Leaf)) {
    throw 'epoch-6 materialization self-test is absent before bootstrap'
}
$selfTestBootstrap = Get-Content -Raw -LiteralPath $selfTestPath |
    ConvertFrom-Json -DateKind String
$commonEntries = @($selfTestBootstrap.static_controls | Where-Object {
        [string]$_.path -ceq 'materialization-common.ps1'
    })
if ($commonEntries.Count -ne 1) {
    throw 'self-test has no unique epoch-6 common bootstrap identity'
}
Assert-BootstrapFile -Path $epochSixCommonPath `
    -Bytes ([UInt64]$commonEntries[0].bytes) `
    -Sha256 ([string]$commonEntries[0].sha256) `
    -Label 'epoch-6 materialization-common.ps1'

. $epochFourCommonPath
. $epochFiveCommonPath
. $epochSixCommonPath

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$controlPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'

function Assert-FreezeCondition {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) { throw $Message }
}

function Read-FreezeJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Assert-PlanAnchor {
    param(
        [Parameter(Mandatory = $true)]$Anchor,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $result = Test-EpochFiveFileAnchor -RepositoryRoot $repoRoot `
        -Anchor $Anchor -Label $Label
    Assert-FreezeCondition ([bool]$result.passed) `
        "epoch-6 dependency anchor differs: $Label"
    $result
}

function Assert-StaticEntry {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)]$Entry,
        [Parameter(Mandatory = $true)][string]$ExpectedName,
        [Parameter(Mandatory = $true)][string]$Label
    )
    Assert-FreezeCondition ([string]$Entry.path -ceq $ExpectedName) `
        "$Label static-control order differs"
    $path = Join-Path $Root $ExpectedName
    Assert-FreezeCondition (Test-Path -LiteralPath $path -PathType Leaf) `
        "$Label is absent"
    $item = Get-Item -LiteralPath $path -Force
    Assert-FreezeCondition `
        (-not $item.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            ) -and
            [UInt64]$item.Length -eq [UInt64]$Entry.bytes -and
            (Get-Sha256Lower -Path $path) -ceq [string]$Entry.sha256) `
        "$Label bytes differ"
}

Assert-FreezeCondition (-not (Test-Path -LiteralPath $controlPath)) `
    'epoch-6 controls already exist and will not be overwritten'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $digestPath)) `
    'epoch-6 control digest already exists and will not be overwritten'

$plan = Read-FreezeJson -Path $planPath
$incident = Read-FreezeJson -Path $incidentPath
$selfTest = Read-FreezeJson -Path $selfTestPath
Assert-FreezeCondition (Test-EpochSixPlanIdentity -Plan $plan) `
    'epoch-6 evidence-materialization plan identity differs'

$expectedHead = [string]$plan.repository_commit_before_epoch_6_controls
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-FreezeCondition ($LASTEXITCODE -eq 0) 'could not resolve repository HEAD'
Assert-FreezeCondition ($head -ceq $expectedHead) `
    'repository HEAD differs from the epoch-6 baseline'

Assert-FreezeCondition `
    ([string]$incident.schema -ceq
        'animus-ferric-runtime-materialization-incident-v6' -and
        [string]$incident.task -ceq 'T-11409' -and
        [string]$incident.operation_id -ceq [string]$plan.operation.id -and
        [string]$incident.correction_operation_id -ceq
            [string]$plan.operation.correction_operation_id -and
        [string]$incident.failed_operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [int]$incident.execution_epoch -eq 3 -and
        [int]$incident.failed_publication_epoch -eq 4 -and
        [int]$incident.failed_correction_epoch -eq 5 -and
        [int]$incident.materialization_epoch -eq 6 -and
        [string]$incident.failed_control_manifest_sha256 -ceq
            [string]$plan.epoch_5.control_manifest.sha256 -and
        [string]$incident.failure.script_relative_path -ceq
            [string]$plan.epoch_5.frozen_failed_publisher.relative_path -and
        [UInt64]$incident.failure.script_bytes -eq
            [UInt64]$plan.epoch_5.frozen_failed_publisher.bytes -and
        [string]$incident.failure.script_sha256 -ceq
            [string]$plan.epoch_5.frozen_failed_publisher.sha256 -and
        [string]$incident.failure.message -ceq
            'constructed legacy envelope differs: legacy envelope does not have the exact 14-field contract; legacy source binding differs; legacy destination binding differs' -and
        [string]$incident.prefreeze_self_test_failure.stage -ceq
            'pre_control_self_test_harness_assertion' -and
        [string]$incident.prefreeze_self_test_failure.relative_path -ceq
            [string]$plan.prefreeze_self_test_failure.relative_path -and
        [UInt64]$incident.prefreeze_self_test_failure.bytes -eq
            [UInt64]$plan.prefreeze_self_test_failure.bytes -and
        [string]$incident.prefreeze_self_test_failure.sha256 -ceq
            [string]$plan.prefreeze_self_test_failure.sha256 -and
        [string]$incident.prefreeze_self_test_failure.schema -ceq
            [string]$plan.prefreeze_self_test_failure.schema -and
        [string]$incident.prefreeze_self_test_failure.tested_at_utc -ceq
            [string]$plan.prefreeze_self_test_failure.tested_at_utc -and
        $incident.prefreeze_self_test_failure.passed -is [bool] -and
        -not [bool]$incident.prefreeze_self_test_failure.passed -and
        [int]$incident.prefreeze_self_test_failure.test_count -eq
            [int]$plan.prefreeze_self_test_failure.test_count -and
        [int]$incident.prefreeze_self_test_failure.exact_model_hashes -eq
            [int]$plan.prefreeze_self_test_failure.exact_model_hashes -and
        [string]$incident.prefreeze_self_test_failure.sole_failed_test -ceq
            [string]$plan.prefreeze_self_test_failure.sole_failed_test -and
        [string]$incident.prefreeze_self_test_failure.cause -ceq
            [string]$plan.prefreeze_self_test_failure.cause -and
        $incident.prefreeze_self_test_failure.controls_frozen -is [bool] -and
        -not [bool]$incident.prefreeze_self_test_failure.controls_frozen -and
        $incident.prefreeze_self_test_failure.official_outputs_created -is [bool] -and
        -not [bool]$incident.prefreeze_self_test_failure.official_outputs_created -and
        [bool]$incident.prefreeze_self_test_failure.archive_preserved_byte_for_byte -and
        [bool]$incident.prefreeze_self_test_failure.canonical_result_path_released_for_corrected_rerun -and
        [int]$incident.state_after_failure.destination_manifest_entries -eq
            [int]$plan.published_destination.entries -and
        [UInt64]$incident.state_after_failure.destination_payload_bytes -eq
            [UInt64]$plan.published_destination.payload_bytes -and
        [string]$incident.state_after_failure.destination_manifest_sha256 -ceq
            [string]$plan.published_destination.manifest_sha256 -and
        [bool]$incident.state_after_failure.destination_published -and
        [bool]$incident.state_after_failure.destination_exact -and
        [bool]$incident.state_after_failure.legacy_epoch_4_publication_envelope_absent -and
        [bool]$incident.state_after_failure.epoch_5_correction_evidence_absent -and
        [bool]$incident.state_after_failure.epoch_6_materialization_evidence_absent -and
        -not [bool]$incident.state_after_failure.model_execution_repeated -and
        [bool]$incident.resolution.preserve_epoch_5_immutable -and
        -not [bool]$incident.resolution.copy_or_move_destination -and
        -not [bool]$incident.resolution.repeat_model_execution -and
        [bool]$incident.resolution.reuse_freeze_time_destination_verification -and
        [bool]$incident.resolution.revalidate_with_frozen_epoch_5_publisher -and
        [string]$incident.resolution.materialization_evidence_relative_path -ceq
            [string]$plan.operation.materialization_evidence_relative_path) `
    'epoch-6 incident record identity differs'

$anchors = @(
    [pscustomobject]@{ label = 'epoch-5 runtime plan'; value = $plan.epoch_5.runtime_plan },
    [pscustomobject]@{ label = 'epoch-5 control manifest'; value = $plan.epoch_5.control_manifest },
    [pscustomobject]@{ label = 'epoch-5 control digest'; value = $plan.epoch_5.control_digest },
    [pscustomobject]@{ label = 'epoch-5 publication self-test'; value = $plan.epoch_5.publication_self_test },
    [pscustomobject]@{ label = 'epoch-5 publication common'; value = $plan.epoch_5.publication_common },
    [pscustomobject]@{ label = 'epoch-5 frozen failed publisher'; value = $plan.epoch_5.frozen_failed_publisher },
    [pscustomobject]@{ label = 'epoch-4 runtime plan'; value = $plan.epoch_4.runtime_plan },
    [pscustomobject]@{ label = 'epoch-4 raw source anchor'; value = $plan.epoch_4.raw_source_anchor },
    [pscustomobject]@{ label = 'epoch-4 runtime common'; value = $plan.epoch_4.runtime_common },
    [pscustomobject]@{ label = 'epoch-4 control manifest'; value = $plan.epoch_4.control_manifest },
    [pscustomobject]@{ label = 'epoch-4 control digest'; value = $plan.epoch_4.control_digest },
    [pscustomobject]@{ label = 'epoch-4 runtime self-test'; value = $plan.epoch_4.runtime_self_test },
    [pscustomobject]@{ label = 'epoch-4 verifier'; value = $plan.epoch_4.verifier },
    [pscustomobject]@{ label = 'epoch-4 frozen failed publisher'; value = $plan.epoch_4.frozen_failed_publisher },
    [pscustomobject]@{ label = 'epoch-3 control manifest'; value = $plan.epoch_3.control_manifest },
    [pscustomobject]@{ label = 'epoch-3 control digest'; value = $plan.epoch_3.control_digest },
    [pscustomobject]@{ label = 'epoch-3 runtime plan'; value = $plan.epoch_3.runtime_plan },
    [pscustomobject]@{ label = 'epoch-3 runtime self-test'; value = $plan.epoch_3.runtime_self_test },
    [pscustomobject]@{ label = 'raw manifest'; value = $plan.operation.manifest },
    [pscustomobject]@{ label = 'raw attempt'; value = $plan.operation.attempt },
    [pscustomobject]@{ label = 'raw attestation'; value = $plan.operation.attestation },
    [pscustomobject]@{ label = 'epoch-6 archived failed self-test'; value = $plan.prefreeze_self_test_failure }
)
$failedSelfTestAnchorCheck = $null
foreach ($anchor in $anchors) {
    $anchorCheck = Assert-PlanAnchor -Anchor $anchor.value -Label $anchor.label
    if ([string]$anchor.label -ceq 'epoch-6 archived failed self-test') {
        $failedSelfTestAnchorCheck = $anchorCheck
    }
}
Assert-FreezeCondition ($null -ne $failedSelfTestAnchorCheck) `
    'epoch-6 archived failed self-test anchor was not checked'
$failedSelfTestPath = [string]$failedSelfTestAnchorCheck.resolved_path
$failedSelfTest = Read-FreezeJson -Path $failedSelfTestPath
$failedSelfTestValidation = Test-EpochSixFailedSelfTestReport `
    -Report $failedSelfTest -Plan $plan
Assert-FreezeCondition ([bool]$failedSelfTestValidation.passed) `
    "epoch-6 archived failed self-test semantics differ: $(@($failedSelfTestValidation.errors) -join '; ')"
$failedSelfTestItemBeforeVerifier = Get-Item -LiteralPath $failedSelfTestPath -Force
$failedSelfTestSha256BeforeVerifier = [string]$failedSelfTestAnchorCheck.sha256

$epochFivePlan = Read-FreezeJson -Path (
    Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$plan.epoch_5.runtime_plan.relative_path)
)
$epochFourPlan = Read-FreezeJson -Path (
    Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$plan.epoch_4.runtime_plan.relative_path)
)
$sourcePlan = Read-FreezeJson -Path (
    Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$plan.epoch_3.runtime_plan.relative_path)
)
$rawAnchor = Read-FreezeJson -Path (
    Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$plan.epoch_4.raw_source_anchor.relative_path)
)
Assert-FreezeCondition (Test-EpochFivePlanIdentity -Plan $epochFivePlan) `
    'anchored epoch-5 plan identity differs'
Assert-FreezeCondition (Test-RecoveryPlanIdentity -Plan $epochFourPlan) `
    'anchored epoch-4 plan identity differs'
Assert-FreezeCondition (Test-RuntimePlanIdentity -Plan $sourcePlan) `
    'anchored epoch-3 plan identity differs'
Assert-FreezeCondition `
    ([string]$rawAnchor.schema -ceq
        'animus-ferric-runtime-raw-source-anchor-v1' -and
        [string]$rawAnchor.operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [string]$rawAnchor.source_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$rawAnchor.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$rawAnchor.manifest.entry_count -eq
            [int]$plan.operation.exact_manifest_entries -and
        [UInt64]$rawAnchor.manifest.payload_bytes -eq
            [UInt64]$plan.published_destination.payload_bytes -and
        [string]$rawAnchor.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [string]$rawAnchor.selected.attempt.sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$rawAnchor.selected.attestation.sha256 -ceq
            [string]$plan.operation.attestation.sha256) `
    'anchored raw-source identity differs from the epoch-6 plan'
Assert-FreezeCondition `
    ([string]$epochFivePlan.operation.id -ceq
            [string]$plan.operation.correction_operation_id -and
        [string]$epochFivePlan.operation.failed_operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [string]$epochFourPlan.operation.id -ceq
            [string]$plan.operation.failed_operation_id -and
        [string]$epochFivePlan.operation.coordinate -ceq
            [string]$plan.operation.coordinate -and
        [string]$epochFourPlan.operation.coordinate -ceq
            [string]$plan.operation.coordinate -and
        [string]$epochFivePlan.operation.source_raw_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$epochFivePlan.operation.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [string]$epochFivePlan.operation.legacy_envelope_relative_path -ceq
            [string]$plan.operation.legacy_envelope_relative_path -and
        [string]$epochFivePlan.operation.correction_evidence_relative_path -ceq
            [string]$plan.operation.correction_evidence_relative_path -and
        [string]$epochFourPlan.operation.source_raw_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$epochFourPlan.operation.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$epochFivePlan.operation.exact_manifest_entries -eq
            [int]$plan.operation.exact_manifest_entries -and
        [string]$epochFivePlan.operation.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [string]$epochFivePlan.operation.attempt.sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$epochFivePlan.operation.attestation.sha256 -ceq
            [string]$plan.operation.attestation.sha256 -and
        [string]$epochFivePlan.model.sha256 -ceq [string]$plan.model.sha256 -and
        [string]$epochFourPlan.model.sha256 -ceq [string]$plan.model.sha256) `
    'epoch-6 plan does not bind the exact failed publication chain'

$dependencyCheck = Test-EpochSixFrozenDependencySet `
    -RepositoryRoot $repoRoot -Plan $plan -ExpectedHead $head
Assert-FreezeCondition ([bool]$dependencyCheck.passed) `
    "epoch-6 frozen dependency set differs: $(@($dependencyCheck.errors) -join '; ')"
$frozenRewalk = Test-EpochFourFrozenDependencySet `
    -RepositoryRoot $repoRoot -EpochFivePlan $epochFivePlan `
    -ExpectedHead $head
Assert-FreezeCondition ([bool]$frozenRewalk.passed) `
    "frozen epoch-4/epoch-3 dependency rewalk failed: $(@($frozenRewalk.errors) -join '; ')"

$sourceRoot = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$sourceCheck = Test-EpochFiveExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$destinationCheck = Test-EpochFiveExactTree -Root $destinationRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
foreach ($tree in @(
        [pscustomobject]@{ label = 'raw source'; value = $sourceCheck },
        [pscustomobject]@{ label = 'published destination'; value = $destinationCheck }
    )) {
    Assert-FreezeCondition `
        ([bool]$tree.value.passed -and
            [string]$tree.value.manifest_sha256 -ceq
                [string]$plan.published_destination.manifest_sha256 -and
            [int]$tree.value.entries -eq
                [int]$plan.published_destination.entries -and
            [UInt64]$tree.value.payload_bytes -eq
                [UInt64]$plan.published_destination.payload_bytes) `
        "$($tree.label) exact-tree verification failed"
}
$attempt = Read-FreezeJson -Path (Join-Path $destinationRoot 'attempt.json')
$terminal = $plan.operation.expected_terminal
Assert-FreezeCondition `
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
    'published destination terminal facts differ from the epoch-6 plan'

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
$staticNames = @(Get-EpochSixStaticControlNames)
$selfTestStatic = @($selfTest.static_controls)
Assert-FreezeCondition `
    (($staticNames -join "`n") -ceq ($expectedStaticNames -join "`n") -and
        $staticNames.Count -eq 8 -and
        @($staticNames | Select-Object -Unique).Count -eq 8 -and
        $selfTestStatic.Count -eq 8 -and
        @($selfTestStatic.path | Select-Object -Unique).Count -eq 8) `
    'epoch-6 static-control set must contain exactly eight unique files'
Assert-FreezeCondition `
    ([string]$selfTest.schema -ceq
        'animus-ferric-runtime-materialization-self-test-v6' -and
        [string]$selfTest.task -ceq 'T-11409' -and
        [string]$selfTest.operation_id -ceq [string]$plan.operation.id -and
        [int]$selfTest.execution_epoch -eq 3 -and
        [int]$selfTest.failed_publication_epoch -eq 4 -and
        [int]$selfTest.failed_correction_epoch -eq 5 -and
        [int]$selfTest.materialization_epoch -eq 6 -and
        [string]$selfTest.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        (Test-EpochSixStrictUtc -Value $selfTest.tested_at_utc) -and
        [int]$selfTest.test_count -eq 23 -and
        [int]$selfTest.exact_model_hashes -eq 1 -and
        [bool]$selfTest.dependency_verification.passed -and
        [bool]$selfTest.source_verification.passed -and
        [bool]$selfTest.destination_verification.passed -and
        [bool]$selfTest.frozen_epoch_4_verifier.passed -and
        [bool]$selfTest.passed) `
    'epoch-6 materialization self-test identity is not green'
$selfTestResults = @($selfTest.results)
Assert-FreezeCondition `
    ($selfTestResults.Count -eq [int]$selfTest.test_count -and
        @($selfTestResults.name | Select-Object -Unique).Count -eq
            $selfTestResults.Count -and
        @($selfTestResults | Where-Object { -not [bool]$_.passed }).Count -eq 0) `
    'epoch-6 materialization self-test results are incomplete or failed'
$selfTestItemBeforeVerifier = Get-Item -LiteralPath $selfTestPath -Force
$selfTestSha256BeforeVerifier = Get-Sha256Lower -Path $selfTestPath
$frozenStaticControls = @()
for ($index = 0; $index -lt $staticNames.Count; $index++) {
    Assert-StaticEntry -Root $artifactDir -Entry $selfTestStatic[$index] `
        -ExpectedName ([string]$staticNames[$index]) `
        -Label "epoch-6/$([string]$staticNames[$index])"
    $path = Join-Path $artifactDir ([string]$staticNames[$index])
    $item = Get-Item -LiteralPath $path -Force
    $frozenStaticControls += [ordered]@{
        path = [string]$staticNames[$index]
        bytes = [UInt64]$item.Length
        sha256 = Get-Sha256Lower -Path $path
    }
}

$legacyEnvelopePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.legacy_envelope_relative_path)
$correctionEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.correction_evidence_relative_path)
$materializationEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.materialization_evidence_relative_path)
Assert-FreezeCondition (-not (Test-Path -LiteralPath $legacyEnvelopePath)) `
    'legacy epoch-4 envelope already exists at epoch-6 freeze'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $correctionEvidencePath)) `
    'epoch-5 correction evidence already exists at epoch-6 freeze'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $materializationEvidencePath)) `
    'epoch-6 materialization evidence already exists at freeze'

$coldState = Get-EpochFiveColdState -RepositoryRoot $repoRoot
Assert-FreezeCondition ([bool]$coldState.passed) `
    'Ferric/llama-server state is not cold at epoch-6 freeze'
$modelPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.model.relative_path)
Assert-FreezeCondition (Test-Path -LiteralPath $modelPath -PathType Leaf) `
    'frozen Q4 model is absent'
$modelItem = Get-Item -LiteralPath $modelPath -Force
Assert-FreezeCondition `
    (-not $modelItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and [UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes) `
    'live Q4 model byte identity differs before verifier'
$verifierPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.verifier.relative_path)
$destinationReport = Invoke-EpochFourVerification `
    -VerifierPath $verifierPath -AttemptPath $destinationRoot `
    -RecoveryPlan $epochFourPlan -SourcePlan $sourcePlan
Assert-FreezeCondition `
    ([bool]$destinationReport.passed -and
        [bool]$destinationReport.live_model_identity.checked -and
        [string]$destinationReport.live_model_identity.sha256 -ceq
            [string]$plan.model.sha256 -and
        [string]$destinationReport.control_anchor_mode -ceq
            'epoch_4_frozen_recovery' -and
        [int]$destinationReport.manifest.entries -eq
            [int]$plan.operation.exact_manifest_entries) `
    'freeze-time frozen verifier report differs from the epoch-6 plan'

# The verifier's model hash is intentionally expensive. Refresh the complete
# cheap control surface after it returns so the immutable manifest cannot
# describe state captured several minutes before frozen_at_utc.
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-FreezeCondition `
    ($LASTEXITCODE -eq 0 -and $head -ceq $expectedHead) `
    'repository HEAD changed during epoch-6 verification'

$planAfterVerifier = Read-FreezeJson -Path $planPath
$incidentAfterVerifier = Read-FreezeJson -Path $incidentPath
$selfTestAfterVerifier = Read-FreezeJson -Path $selfTestPath
Assert-FreezeCondition `
    ((Test-JsonEquivalent -Left $planAfterVerifier -Right $plan) -and
        (Test-JsonEquivalent -Left $incidentAfterVerifier -Right $incident) -and
        (Test-JsonEquivalent -Left $selfTestAfterVerifier -Right $selfTest)) `
    'epoch-6 plan, incident, or self-test changed during verification'
$selfTestItemAfterVerifier = Get-Item -LiteralPath $selfTestPath -Force
Assert-FreezeCondition `
    (-not $selfTestItemAfterVerifier.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and
        [UInt64]$selfTestItemAfterVerifier.Length -eq
            [UInt64]$selfTestItemBeforeVerifier.Length -and
        (Get-Sha256Lower -Path $selfTestPath) -ceq
            $selfTestSha256BeforeVerifier) `
    'epoch-6 materialization self-test byte identity changed during verification'

$frozenStaticControls = @()
for ($index = 0; $index -lt $staticNames.Count; $index++) {
    Assert-StaticEntry -Root $artifactDir -Entry $selfTestStatic[$index] `
        -ExpectedName ([string]$staticNames[$index]) `
        -Label "post-verifier epoch-6/$([string]$staticNames[$index])"
    $path = Join-Path $artifactDir ([string]$staticNames[$index])
    $item = Get-Item -LiteralPath $path -Force
    $frozenStaticControls += [ordered]@{
        path = [string]$staticNames[$index]
        bytes = [UInt64]$item.Length
        sha256 = Get-Sha256Lower -Path $path
    }
}

$dependencyCheck = Test-EpochSixFrozenDependencySet `
    -RepositoryRoot $repoRoot -Plan $plan -ExpectedHead $head
Assert-FreezeCondition `
    ([bool]$dependencyCheck.passed -and
        [int]$dependencyCheck.epoch_5_static_controls_checked -eq 8 -and
        [int]$dependencyCheck.epoch_4_static_controls_checked -eq 12 -and
        [int]$dependencyCheck.transitive_epoch_3_controls_checked -eq 20) `
    "post-verifier frozen dependency set differs: $(@($dependencyCheck.errors) -join '; ')"
$frozenRewalk = Test-EpochFourFrozenDependencySet `
    -RepositoryRoot $repoRoot -EpochFivePlan $epochFivePlan `
    -ExpectedHead $head
Assert-FreezeCondition `
    ([bool]$frozenRewalk.passed -and
        [int]$frozenRewalk.static_controls_checked -eq 12 -and
        [int]$frozenRewalk.transitive_epoch_3_controls_checked -eq 20) `
    "post-verifier epoch-4/epoch-3 dependency rewalk failed: $(@($frozenRewalk.errors) -join '; ')"

$failedSelfTestAnchorAfterVerifier = Assert-PlanAnchor `
    -Anchor $plan.prefreeze_self_test_failure `
    -Label 'post-verifier epoch-6 archived failed self-test'
$failedSelfTestAfterVerifier = Read-FreezeJson -Path (
    [string]$failedSelfTestAnchorAfterVerifier.resolved_path
)
$failedSelfTestValidationAfterVerifier = Test-EpochSixFailedSelfTestReport `
    -Report $failedSelfTestAfterVerifier -Plan $plan
$failedSelfTestItemAfterVerifier = Get-Item `
    -LiteralPath ([string]$failedSelfTestAnchorAfterVerifier.resolved_path) -Force
Assert-FreezeCondition `
    ([bool]$failedSelfTestValidationAfterVerifier.passed -and
        (Test-JsonEquivalent `
            -Left $failedSelfTestAfterVerifier -Right $failedSelfTest) -and
        -not $failedSelfTestItemAfterVerifier.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and
        [UInt64]$failedSelfTestItemAfterVerifier.Length -eq
            [UInt64]$failedSelfTestItemBeforeVerifier.Length -and
        [UInt64]$failedSelfTestItemAfterVerifier.Length -eq
            [UInt64]$plan.prefreeze_self_test_failure.bytes -and
        [string]$failedSelfTestAnchorAfterVerifier.sha256 -ceq
            $failedSelfTestSha256BeforeVerifier -and
        [string]$failedSelfTestAnchorAfterVerifier.sha256 -ceq
            [string]$plan.prefreeze_self_test_failure.sha256) `
    "post-verifier archived failed self-test identity or semantics differ: $(@($failedSelfTestValidationAfterVerifier.errors) -join '; ')"

$sourceCheck = Test-EpochFiveExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$destinationCheck = Test-EpochFiveExactTree -Root $destinationRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
foreach ($tree in @(
        [pscustomobject]@{ label = 'post-verifier raw source'; value = $sourceCheck },
        [pscustomobject]@{ label = 'post-verifier published destination'; value = $destinationCheck }
    )) {
    Assert-FreezeCondition `
        ([bool]$tree.value.passed -and
            [string]$tree.value.manifest_sha256 -ceq
                [string]$plan.published_destination.manifest_sha256 -and
            [int]$tree.value.entries -eq
                [int]$plan.published_destination.entries -and
            [UInt64]$tree.value.payload_bytes -eq
                [UInt64]$plan.published_destination.payload_bytes) `
        "$($tree.label) exact-tree verification failed"
}
foreach ($root in @($sourceRoot, $destinationRoot)) {
    Assert-FreezeCondition `
        ((Get-Sha256Lower -Path (Join-Path $root 'attempt.json')) -ceq
                [string]$plan.operation.attempt.sha256 -and
            (Get-Sha256Lower -Path (Join-Path $root 'attestation.json')) -ceq
                [string]$plan.operation.attestation.sha256) `
        "post-verifier selected evidence identity differs: $root"
}
$attempt = Read-FreezeJson -Path (Join-Path $destinationRoot 'attempt.json')
Assert-FreezeCondition `
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
    'post-verifier destination terminal facts differ from the epoch-6 plan'
$destinationReportCheck = Test-EpochSixDestinationVerification `
    -Report $destinationReport -Plan $plan `
    -EpochFourPlan $epochFourPlan -SourcePlan $sourcePlan `
    -DestinationPath $destinationRoot
Assert-FreezeCondition ([bool]$destinationReportCheck.passed) `
    "post-verifier frozen report validation differs: $(@($destinationReportCheck.errors) -join '; ')"

Assert-FreezeCondition (-not (Test-Path -LiteralPath $legacyEnvelopePath)) `
    'legacy epoch-4 envelope appeared during epoch-6 verification'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $correctionEvidencePath)) `
    'epoch-5 correction evidence appeared during epoch-6 verification'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $materializationEvidencePath)) `
    'epoch-6 materialization evidence appeared during freeze verification'
$coldState = Get-EpochFiveColdState -RepositoryRoot $repoRoot
Assert-FreezeCondition ([bool]$coldState.passed) `
    'Ferric/llama-server state became non-cold during epoch-6 verification'
$modelItem = Get-Item -LiteralPath $modelPath -Force
Assert-FreezeCondition `
    (-not $modelItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and [UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes) `
    'live Q4 model byte identity changed during verifier execution'

$controlManifest = [ordered]@{
    schema = 'animus-ferric-runtime-evidence-materialization-control-inputs-v6'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    correction_operation_id = [string]$plan.operation.correction_operation_id
    failed_operation_id = [string]$plan.operation.failed_operation_id
    execution_epoch = 3
    failed_publication_epoch = 4
    failed_correction_epoch = 5
    materialization_epoch = 6
    timestamp_protocol = [string]$plan.timestamp_protocol
    frozen_at_utc = (Get-Date).ToUniversalTime().ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    incident_sha256 = Get-Sha256Lower -Path $incidentPath
    repository = [ordered]@{
        head_at_freeze = $head
        epoch_6_pre_control_base = $expectedHead
    }
    static_controls = @($frozenStaticControls)
    prefreeze_self_test_failure_archive = [ordered]@{
        relative_path = [string]$plan.prefreeze_self_test_failure.relative_path
        bytes = [UInt64]$failedSelfTestItemAfterVerifier.Length
        sha256 = [string]$failedSelfTestAnchorAfterVerifier.sha256
        schema = [string]$failedSelfTestAfterVerifier.schema
        tested_at_utc = [string]$failedSelfTestAfterVerifier.tested_at_utc
        test_count = [int]$failedSelfTestAfterVerifier.test_count
        exact_model_hashes = [int]$failedSelfTestAfterVerifier.exact_model_hashes
        sole_failed_test =
            [string]$plan.prefreeze_self_test_failure.sole_failed_test
        report_passed = [bool]$failedSelfTestAfterVerifier.passed
        semantic_validation_passed =
            [bool]$failedSelfTestValidationAfterVerifier.passed
    }
    materialization_self_test = [ordered]@{
        relative_path = 'docs/sprints/s114/control-artifacts/runtime/epoch-6/materialization-self-test.json'
        bytes = [UInt64](Get-Item -LiteralPath $selfTestPath).Length
        sha256 = Get-Sha256Lower -Path $selfTestPath
        test_count = [int]$selfTest.test_count
        exact_model_hashes = [int]$selfTest.exact_model_hashes
        passed = $true
    }
    dependency_verification = [ordered]@{
        epoch_5_static_controls_checked =
            [int]$dependencyCheck.epoch_5_static_controls_checked
        epoch_4_static_controls_checked =
            [int]$dependencyCheck.epoch_4_static_controls_checked
        transitive_epoch_3_controls_checked =
            [int]$dependencyCheck.transitive_epoch_3_controls_checked
        frozen_epoch_4_rewalk_static_controls_checked =
            [int]$frozenRewalk.static_controls_checked
        frozen_epoch_3_rewalk_controls_checked =
            [int]$frozenRewalk.transitive_epoch_3_controls_checked
        passed = $true
    }
    source_verification = [ordered]@{
        relative_path = [string]$plan.operation.source_raw_relative_path
        manifest_sha256 = [string]$sourceCheck.manifest_sha256
        entries = [int]$sourceCheck.entries
        payload_bytes = [UInt64]$sourceCheck.payload_bytes
        attempt_sha256 = [string]$plan.operation.attempt.sha256
        attestation_sha256 = [string]$plan.operation.attestation.sha256
        terminal_facts_passed = $true
        passed = $true
    }
    destination_verification = [ordered]@{
        relative_path = [string]$plan.operation.destination_relative_path
        manifest_sha256 = [string]$destinationCheck.manifest_sha256
        entries = [int]$destinationCheck.entries
        payload_bytes = [UInt64]$destinationCheck.payload_bytes
        verifier_invocations = 1
        report = $destinationReport
        passed = $true
    }
    model = [ordered]@{
        relative_path = [string]$plan.model.relative_path
        bytes = [UInt64]$modelItem.Length
        sha256 = [string]$destinationReport.live_model_identity.sha256
        hash_mode = [string]$destinationReport.live_model_identity.mode
        independently_rehashed_by_frozen_verifier = $true
        passed = $true
    }
    cold_state = $coldState
    materialization_preconditions = [ordered]@{
        destination_relative_path =
            [string]$plan.operation.destination_relative_path
        destination_exact_at_freeze = $true
        legacy_envelope_relative_path =
            [string]$plan.operation.legacy_envelope_relative_path
        legacy_envelope_absent_at_freeze = $true
        correction_evidence_relative_path =
            [string]$plan.operation.correction_evidence_relative_path
        correction_evidence_absent_at_freeze = $true
        materialization_evidence_relative_path =
            [string]$plan.operation.materialization_evidence_relative_path
        materialization_evidence_absent_at_freeze = $true
        passed = $true
    }
    passed = $true
}

$stageParent = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath 'target/s114-experiment/materialization-control-stage'
$stageBaseChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path (Split-Path -Parent $stageParent)
Assert-FreezeCondition ([bool]$stageBaseChain.passed) `
    "epoch-6 stage base chain is unsafe: $(@($stageBaseChain.errors) -join '; ')"
[System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
$stageParentChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path $stageParent
Assert-FreezeCondition ([bool]$stageParentChain.passed) `
    "epoch-6 stage parent chain is unsafe: $(@($stageParentChain.errors) -join '; ')"
$stageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
Assert-FreezeCondition (-not (Test-Path -LiteralPath $stageOwner)) `
    'generated epoch-6 stage owner already exists'
[System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
$stageOwnerChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path $stageOwner
Assert-FreezeCondition ([bool]$stageOwnerChain.passed) `
    "epoch-6 stage owner chain is unsafe: $(@($stageOwnerChain.errors) -join '; ')"
$stageControl = Join-Path $stageOwner 'control-inputs.json'
$stageDigest = Join-Path $stageOwner 'control-inputs.sha256'
$controlPublished = $false
try {
    Write-EpochSixJsonAtomic -Path $stageControl -Value $controlManifest -Depth 96
    $controlHash = Get-Sha256Lower -Path $stageControl
    Write-Utf8Lf -Path $stageDigest -Text "$controlHash  control-inputs.json`n"
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
    $stageOwnerItem = Get-Item -LiteralPath $stageOwner -Force `
        -ErrorAction SilentlyContinue
    if ($null -ne $stageOwnerItem) {
        Assert-FreezeCondition ([bool]$stageOwnerItem.PSIsContainer) `
            'refusing to treat a wrong-type epoch-6 stage owner as clean'
        $resolvedStageOwner = [System.IO.Path]::GetFullPath($stageOwner)
        $resolvedStageParent = [System.IO.Path]::GetFullPath($stageParent)
        Assert-FreezeCondition `
            ((Split-Path -Parent $resolvedStageOwner) -ceq $resolvedStageParent -and
                (Split-Path -Leaf $resolvedStageOwner) -cmatch '^[0-9a-f]{32}$') `
            'refusing to remove a directory outside the exact epoch-6 stage-owner shape'
        $cleanupChain = Test-EpochSixNonReparseDirectoryChain `
            -RepositoryRoot $repoRoot -Path $resolvedStageOwner
        Assert-FreezeCondition ([bool]$cleanupChain.passed) `
            "refusing epoch-6 stage cleanup through an unsafe directory chain: $(@($cleanupChain.errors) -join '; ')"
        $cleanupReparseEntries = @(
            Get-ChildItem -LiteralPath $resolvedStageOwner -Recurse -Force |
                Where-Object {
                    $_.Attributes.HasFlag(
                        [System.IO.FileAttributes]::ReparsePoint
                    )
                }
        )
        Assert-FreezeCondition ($cleanupReparseEntries.Count -eq 0) `
            'refusing to recursively remove epoch-6 stage content with a reparse point'
        [System.IO.Directory]::Delete($stageOwner, $true)
    }
}
Assert-FreezeCondition $controlPublished 'epoch-6 controls were not published'

[ordered]@{
    control_manifest = 'control-inputs.json'
    control_manifest_sha256 = Get-Sha256Lower -Path $controlPath
    control_digest = 'control-inputs.sha256'
    operation_id = [string]$plan.operation.id
    destination_entries = [int]$destinationCheck.entries
    frozen_verifier_model_hashes = 1
    passed = $true
} | ConvertTo-Json -Depth 8
