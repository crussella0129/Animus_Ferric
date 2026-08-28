#requires -Version 7.5
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$runtimeDir = Split-Path -Parent $artifactDir
$epochFourDir = Join-Path $runtimeDir 'epoch-4'
$epochFiveDir = Join-Path $runtimeDir 'epoch-5'
. (Join-Path $epochFourDir 'runtime-common.ps1')
. (Join-Path $epochFiveDir 'publication-common.ps1')
. (Join-Path $artifactDir 'materialization-common.ps1')

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$resultPath = Join-Path $artifactDir 'materialization-self-test.json'
$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'

function Read-EpochSixJson {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$Path)

    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Copy-EpochSixJsonValue {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Value)

    $Value | ConvertTo-Json -Depth 100 | ConvertFrom-Json -DateKind String
}

function ConvertTo-EpochSixJsonIdentity {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Value)

    $json = ($Value | ConvertTo-Json -Depth 100).Replace("`r`n", "`n")
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $sha256 = [Convert]::ToHexString(
            $algorithm.ComputeHash($bytes)
        ).ToLowerInvariant()
    }
    finally {
        $algorithm.Dispose()
    }
    [pscustomobject][ordered]@{
        bytes = [UInt64]$bytes.Length
        sha256 = $sha256
    }
}

function ConvertTo-EpochSixTreeEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Check)

    [pscustomobject][ordered]@{
        passed = [bool]$Check.passed
        manifest_sha256 = [string]$Check.manifest_sha256
        entries = [int]$Check.entries
        payload_bytes = [UInt64]$Check.payload_bytes
        errors = @($Check.errors)
    }
}

function ConvertTo-EpochSixAnchorEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)]$Check
    )

    [pscustomobject][ordered]@{
        label = $Label
        relative_path = [string]$Check.relative_path
        bytes = $Check.bytes
        sha256 = [string]$Check.sha256
        passed = [bool]$Check.passed
        errors = @($Check.errors)
    }
}

function Test-EpochSixLfNoBom {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$Path)

    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $hasUtf8Bom = ($bytes.Length -ge 3 -and
        $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and
        $bytes[2] -eq 0xBF)
    $hasUtf16Bom = ($bytes.Length -ge 2 -and (
            ($bytes[0] -eq 0xFF -and $bytes[1] -eq 0xFE) -or
            ($bytes[0] -eq 0xFE -and $bytes[1] -eq 0xFF)
        ))
    $hasCarriageReturn = [Array]::IndexOf($bytes, [byte]0x0D) -ge 0
    $endsInLf = $bytes.Length -gt 0 -and $bytes[$bytes.Length - 1] -eq 0x0A
    [pscustomobject][ordered]@{
        passed = (-not $hasUtf8Bom -and -not $hasUtf16Bom -and
            -not $hasCarriageReturn -and $endsInLf)
        utf8_bom_absent = -not $hasUtf8Bom
        utf16_bom_absent = -not $hasUtf16Bom
        carriage_return_absent = -not $hasCarriageReturn
        trailing_lf_present = $endsInLf
    }
}

function New-EpochSixReorderedObject {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Value)

    $copy = Copy-EpochSixJsonValue -Value $Value
    $names = @($copy.PSObject.Properties.Name)
    if ($names.Count -lt 2) {
        throw 'cannot reorder an object with fewer than two properties'
    }
    $first = $names[0]
    $names[0] = $names[1]
    $names[1] = $first
    $ordered = [ordered]@{}
    foreach ($name in $names) {
        $ordered[[string]$name] = $copy.$name
    }
    [pscustomobject]$ordered
}

function Remove-EpochSixOwnedDirectory {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedParent,
        [Parameter(Mandatory = $true)][string]$RepositoryRoot
    )

    $existingItem = Get-Item -LiteralPath $Path -Force `
        -ErrorAction SilentlyContinue
    if ($null -eq $existingItem) {
        return $true
    }
    if (-not [bool]$existingItem.PSIsContainer) {
        throw 'refusing to treat a wrong-type owned-test path as clean'
    }
    $resolvedPath = [System.IO.Path]::GetFullPath($Path)
    $resolvedParent = [System.IO.Path]::GetFullPath($ExpectedParent)
    if ((Split-Path -Parent $resolvedPath) -cne $resolvedParent -or
        (Split-Path -Leaf $resolvedPath) -cnotmatch '^[0-9a-f]{32}$') {
        throw 'refusing to remove a directory outside the exact owned-test shape'
    }
    $chain = Test-EpochSixNonReparseDirectoryChain `
        -RepositoryRoot $RepositoryRoot -Path $resolvedPath
    if (-not [bool]$chain.passed) {
        throw "refusing cleanup through an unsafe directory chain: $(@($chain.errors) -join '; ')"
    }
    $item = Get-Item -LiteralPath $resolvedPath -Force
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw 'refusing to remove an owned-test reparse point'
    }
    $reparseEntries = @(Get-ChildItem -LiteralPath $resolvedPath -Recurse -Force |
            Where-Object {
                $_.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            })
    if ($reparseEntries.Count -ne 0) {
        throw 'refusing to recursively remove owned test content with a reparse point'
    }
    [System.IO.Directory]::Delete($resolvedPath, $true)
    -not (Test-Path -LiteralPath $resolvedPath)
}

function Get-EpochSixDirectAnchors {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Plan)

    $anchors = [System.Collections.Generic.List[object]]::new()
    function Add-AnchorsRecursive {
        param(
            [AllowNull()]$Value,
            [Parameter(Mandatory = $true)][string]$Label
        )
        if ($null -eq $Value) { return }
        if ($Value -is [string] -or $Value -is [ValueType]) { return }
        $properties = @($Value.PSObject.Properties)
        $propertyNames = @($properties.Name)
        if ($propertyNames -contains 'relative_path' -and
            $propertyNames -contains 'bytes' -and
            $propertyNames -contains 'sha256') {
            $anchors.Add([pscustomobject][ordered]@{
                label = $Label
                anchor = $Value
            })
            return
        }
        foreach ($property in $properties) {
            if ($property.Value -is [System.Array]) { continue }
            Add-AnchorsRecursive -Value $property.Value `
                -Label "$Label.$($property.Name)"
        }
    }
    Add-AnchorsRecursive -Value $Plan -Label 'plan'
    @($anchors)
}

function Add-EpochSixResult {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()][Parameter(Mandatory = $true)]
        [System.Collections.Generic.List[object]]$Results,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$Passed,
        [AllowNull()]$Evidence
    )

    $Results.Add([pscustomobject][ordered]@{
        name = $Name
        passed = $Passed
        evidence = $Evidence
    })
}

if (Test-Path -LiteralPath $resultPath) {
    throw 'materialization-self-test.json already exists and will not be overwritten'
}
if ((Test-Path -LiteralPath $controlManifestPath) -or
    (Test-Path -LiteralPath $controlDigestPath)) {
    throw 'epoch-6 materialization self-test must run before epoch-6 controls exist'
}

$plan = Read-EpochSixJson -Path $planPath
$incident = Read-EpochSixJson -Path $incidentPath
if (-not (Test-EpochSixPlanIdentity -Plan $plan)) {
    throw 'runtime plan is not the exact epoch-6 evidence-materialization protocol'
}

$epochFivePlanPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_5.runtime_plan.relative_path)
$epochFiveControlPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_5.control_manifest.relative_path)
$epochFivePublisherPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_5.frozen_failed_publisher.relative_path)
$recoveryPlanPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.runtime_plan.relative_path)
$sourcePlanPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_3.runtime_plan.relative_path)
$rawAnchorPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.raw_source_anchor.relative_path)
$verifierPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.verifier.relative_path)
$sourceRoot = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$destinationRoot = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$legacyEnvelopePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.legacy_envelope_relative_path)
$correctionEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.correction_evidence_relative_path)
$materializationEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.materialization_evidence_relative_path)

$epochFivePlan = Read-EpochSixJson -Path $epochFivePlanPath
$epochFiveControls = Read-EpochSixJson -Path $epochFiveControlPath
$recoveryPlan = Read-EpochSixJson -Path $recoveryPlanPath
$sourcePlan = Read-EpochSixJson -Path $sourcePlanPath
$rawAnchor = Read-EpochSixJson -Path $rawAnchorPath
$results = [System.Collections.Generic.List[object]]::new()

$directAnchorDefinitions = @(Get-EpochSixDirectAnchors -Plan $plan)
$directAnchorChecks = [System.Collections.Generic.List[object]]::new()
foreach ($definition in $directAnchorDefinitions) {
    if ([string]$definition.label -ceq 'plan.model') {
        $modelAnchorPath = Resolve-EpochSixRepoRelativePath `
            -RepositoryRoot $repoRoot `
            -RelativePath ([string]$definition.anchor.relative_path)
        $modelAnchorItem = Get-Item -LiteralPath $modelAnchorPath -Force
        $modelAnchorErrors = @()
        if ($modelAnchorItem.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) {
            $modelAnchorErrors += 'plan.model: file is a reparse point'
        }
        if ([UInt64]$modelAnchorItem.Length -ne
            [UInt64]$definition.anchor.bytes) {
            $modelAnchorErrors += 'plan.model: byte length differs'
        }
        $check = [pscustomobject][ordered]@{
            passed = ($modelAnchorErrors.Count -eq 0)
            relative_path = [string]$definition.anchor.relative_path
            resolved_path = $modelAnchorPath
            bytes = [UInt64]$modelAnchorItem.Length
            sha256 = [string]$definition.anchor.sha256
            hash_deferred_to_frozen_verifier = $true
            errors = $modelAnchorErrors
        }
    }
    else {
        $check = Test-EpochSixFileAnchor -RepositoryRoot $repoRoot `
            -Anchor $definition.anchor -Label ([string]$definition.label)
    }
    $directAnchorChecks.Add((ConvertTo-EpochSixAnchorEvidence `
            -Label ([string]$definition.label) -Check $check))
}
$directAnchorsPassed = (
    $directAnchorChecks.Count -eq 23 -and
    @($directAnchorChecks | Where-Object { -not [bool]$_.passed }).Count -eq 0
)
Add-EpochSixResult -Results $results `
    -Name 'all_23_direct_plan_anchors_are_exact' `
    -Passed $directAnchorsPassed `
    -Evidence ([pscustomobject][ordered]@{
        expected = 23
        observed = $directAnchorChecks.Count
        anchors = @($directAnchorChecks)
    })

$failedSelfTestPath = Resolve-EpochSixRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.prefreeze_self_test_failure.relative_path)
$failedSelfTestAnchorEvidence = @($directAnchorChecks | Where-Object {
        [string]$_.label -ceq 'plan.prefreeze_self_test_failure'
    })
$failedSelfTestReport = $null
$failedSelfTestReportCheck = [pscustomobject][ordered]@{
    passed = $false
    errors = @('archived failed self-test was not loaded')
}
$failedSelfTestLoadError = $null
if ($failedSelfTestAnchorEvidence.Count -eq 1 -and
    [bool]$failedSelfTestAnchorEvidence[0].passed) {
    try {
        $failedSelfTestReport = Read-EpochSixJson -Path $failedSelfTestPath
        $failedSelfTestReportCheck = Test-EpochSixFailedSelfTestReport `
            -Report $failedSelfTestReport -Plan $plan
    }
    catch { $failedSelfTestLoadError = $_.Exception.Message }
}
$failedSelfTestErrors = [System.Collections.Generic.List[string]]::new()
foreach ($anchorEvidence in $failedSelfTestAnchorEvidence) {
    foreach ($errorMessage in @($anchorEvidence.errors)) {
        if (-not [string]::IsNullOrWhiteSpace([string]$errorMessage)) {
            $failedSelfTestErrors.Add([string]$errorMessage)
        }
    }
}
foreach ($errorMessage in @($failedSelfTestReportCheck.errors)) {
    if (-not [string]::IsNullOrWhiteSpace([string]$errorMessage)) {
        $failedSelfTestErrors.Add([string]$errorMessage)
    }
}
if (-not [string]::IsNullOrWhiteSpace([string]$failedSelfTestLoadError)) {
    $failedSelfTestErrors.Add([string]$failedSelfTestLoadError)
}
$failedSelfTestPassed = (
    -not (Test-Path -LiteralPath $resultPath) -and
    $failedSelfTestAnchorEvidence.Count -eq 1 -and
    [bool]$failedSelfTestAnchorEvidence[0].passed -and
    $null -ne $failedSelfTestReport -and
    [bool]$failedSelfTestReportCheck.passed -and
    $failedSelfTestErrors.Count -eq 0
)
Add-EpochSixResult -Results $results `
    -Name 'canonical_result_is_absent_and_archived_failed_self_test_is_exact_non_reparse' `
    -Passed $failedSelfTestPassed `
    -Evidence ([pscustomobject][ordered]@{
        canonical_result_absent = -not (Test-Path -LiteralPath $resultPath)
        archive_relative_path =
            [string]$plan.prefreeze_self_test_failure.relative_path
        archive_exact_non_reparse = (
            $failedSelfTestAnchorEvidence.Count -eq 1 -and
            [bool]$failedSelfTestAnchorEvidence[0].passed
        )
        archived_report_contract_passed =
            [bool]$failedSelfTestReportCheck.passed
        errors = @($failedSelfTestErrors)
    })

$dependencyCheck = Test-EpochSixFrozenDependencySet `
    -RepositoryRoot $repoRoot -Plan $plan `
    -ExpectedHead ([string]$plan.repository_commit_before_epoch_6_controls)
$dependencyPassed = (
    [bool]$dependencyCheck.passed -and
    [int]$dependencyCheck.epoch_5_static_controls_checked -eq 8 -and
    [int]$dependencyCheck.epoch_4_static_controls_checked -eq 12 -and
    [int]$dependencyCheck.transitive_epoch_3_controls_checked -eq 20
)
Add-EpochSixResult -Results $results `
    -Name 'shared_dependency_rewalk_checks_epoch_5_and_12_20_transitive_controls' `
    -Passed $dependencyPassed `
    -Evidence ([pscustomobject][ordered]@{
        epoch_5_static_controls_checked =
            [int]$dependencyCheck.epoch_5_static_controls_checked
        epoch_4_static_controls_checked =
            [int]$dependencyCheck.epoch_4_static_controls_checked
        transitive_epoch_3_controls_checked =
            [int]$dependencyCheck.transitive_epoch_3_controls_checked
        errors = @($dependencyCheck.errors)
    })

$wrongPlanOperation = Copy-EpochSixJsonValue -Value $plan
$wrongPlanOperation.operation.id = 'r06-cross-operation'
$wrongPlanProtocol = Copy-EpochSixJsonValue -Value $plan
$wrongPlanProtocol.timestamp_protocol = 'powershell-json-implicit-local-v0'
$missingPlanAnchor = Copy-EpochSixJsonValue -Value $plan
$missingPlanAnchor.epoch_5.PSObject.Properties.Remove('control_manifest')
$wrongPublishedDestination = Copy-EpochSixJsonValue -Value $plan
$wrongPublishedDestination.published_destination.manifest_sha256 = ('0' * 64)
Add-EpochSixResult -Results $results `
    -Name 'plan_identity_rejects_operation_protocol_anchor_and_destination_mutations' `
    -Passed (-not (Test-EpochSixPlanIdentity -Plan $wrongPlanOperation) -and
        -not (Test-EpochSixPlanIdentity -Plan $wrongPlanProtocol) -and
        -not (Test-EpochSixPlanIdentity -Plan $missingPlanAnchor) -and
        -not (Test-EpochSixPlanIdentity -Plan $wrongPublishedDestination)) `
    -Evidence ([pscustomobject][ordered]@{
        cross_operation_rejected = -not (
            Test-EpochSixPlanIdentity -Plan $wrongPlanOperation
        )
        wrong_protocol_rejected = -not (
            Test-EpochSixPlanIdentity -Plan $wrongPlanProtocol
        )
        missing_anchor_rejected = -not (
            Test-EpochSixPlanIdentity -Plan $missingPlanAnchor
        )
        wrong_published_destination_rejected = -not (
            Test-EpochSixPlanIdentity -Plan $wrongPublishedDestination
        )
    })

$sourceTreeCheck = Test-EpochSixExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$destinationTreeCheck = Test-EpochSixExactTree -Root $destinationRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$sourceBindingPassed = (
    [bool]$sourceTreeCheck.passed -and
    [string]$sourceTreeCheck.manifest_sha256 -ceq
        [string]$plan.operation.manifest.sha256 -and
    [int]$sourceTreeCheck.entries -eq
        [int]$plan.operation.exact_manifest_entries -and
    [UInt64]$sourceTreeCheck.payload_bytes -eq
        [UInt64]$plan.published_destination.payload_bytes
)
$destinationBindingPassed = (
    [bool]$destinationTreeCheck.passed -and
    [string]$destinationTreeCheck.manifest_sha256 -ceq
        [string]$plan.published_destination.manifest_sha256 -and
    [int]$destinationTreeCheck.entries -eq
        [int]$plan.published_destination.entries -and
    [UInt64]$destinationTreeCheck.payload_bytes -eq
        [UInt64]$plan.published_destination.payload_bytes
)
Add-EpochSixResult -Results $results `
    -Name 'raw_source_tree_remains_exact' -Passed $sourceBindingPassed `
    -Evidence (ConvertTo-EpochSixTreeEvidence -Check $sourceTreeCheck)
Add-EpochSixResult -Results $results `
    -Name 'published_destination_tree_is_exact_before_materialization' `
    -Passed $destinationBindingPassed `
    -Evidence (ConvertTo-EpochSixTreeEvidence -Check $destinationTreeCheck)

$outputsAbsent = (
    -not (Test-Path -LiteralPath $legacyEnvelopePath) -and
    -not (Test-Path -LiteralPath $correctionEvidencePath) -and
    -not (Test-Path -LiteralPath $materializationEvidencePath)
)
Add-EpochSixResult -Results $results `
    -Name 'all_three_official_evidence_outputs_are_absent_before_freeze' `
    -Passed $outputsAbsent `
    -Evidence ([pscustomobject][ordered]@{
        legacy_envelope_absent = -not (Test-Path -LiteralPath $legacyEnvelopePath)
        correction_evidence_absent = -not (Test-Path -LiteralPath $correctionEvidencePath)
        materialization_evidence_absent = -not (
            Test-Path -LiteralPath $materializationEvidencePath
        )
    })

$incidentPassed = (
    [string]$incident.schema -ceq
        'animus-ferric-runtime-materialization-incident-v6' -and
    [string]$incident.task -ceq 'T-11409' -and
    [string]$incident.operation_id -ceq [string]$plan.operation.id -and
    [string]$incident.correction_operation_id -ceq
        [string]$plan.operation.correction_operation_id -and
    [string]$incident.failed_operation_id -ceq
        [string]$plan.operation.failed_operation_id -and
    [string]$incident.failure.script_relative_path -ceq
        [string]$plan.epoch_5.frozen_failed_publisher.relative_path -and
    [UInt64]$incident.failure.script_bytes -eq
        [UInt64]$plan.epoch_5.frozen_failed_publisher.bytes -and
    [string]$incident.failure.script_sha256 -ceq
        [string]$plan.epoch_5.frozen_failed_publisher.sha256 -and
    [string]$incident.failure.message -ceq
        'constructed legacy envelope differs: legacy envelope does not have the exact 14-field contract; legacy source binding differs; legacy destination binding differs' -and
    (Test-EpochSixExactPropertySequence `
        -Value $incident.prefreeze_self_test_failure -Expected @(
            'stage', 'relative_path', 'bytes', 'sha256', 'schema',
            'tested_at_utc', 'passed', 'test_count', 'exact_model_hashes',
            'sole_failed_test', 'cause', 'controls_frozen',
            'official_outputs_created', 'archive_preserved_byte_for_byte',
            'canonical_result_path_released_for_corrected_rerun'
        )) -and
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
    [bool]$incident.prefreeze_self_test_failure.passed -eq
        [bool]$plan.prefreeze_self_test_failure.passed -and
    [int]$incident.prefreeze_self_test_failure.test_count -eq
        [int]$plan.prefreeze_self_test_failure.test_count -and
    [int]$incident.prefreeze_self_test_failure.exact_model_hashes -eq
        [int]$plan.prefreeze_self_test_failure.exact_model_hashes -and
    [string]$incident.prefreeze_self_test_failure.sole_failed_test -ceq
        [string]$plan.prefreeze_self_test_failure.sole_failed_test -and
    [string]$incident.prefreeze_self_test_failure.cause -ceq
        [string]$plan.prefreeze_self_test_failure.cause -and
    $incident.prefreeze_self_test_failure.controls_frozen -is [bool] -and
    [bool]$incident.prefreeze_self_test_failure.controls_frozen -eq
        [bool]$plan.prefreeze_self_test_failure.controls_frozen -and
    $incident.prefreeze_self_test_failure.official_outputs_created -is [bool] -and
    [bool]$incident.prefreeze_self_test_failure.official_outputs_created -eq
        [bool]$plan.prefreeze_self_test_failure.official_outputs_created -and
    $incident.prefreeze_self_test_failure.archive_preserved_byte_for_byte `
        -is [bool] -and
    [bool]$incident.prefreeze_self_test_failure.archive_preserved_byte_for_byte -and
    $incident.prefreeze_self_test_failure.canonical_result_path_released_for_corrected_rerun `
        -is [bool] -and
    [bool]$incident.prefreeze_self_test_failure.canonical_result_path_released_for_corrected_rerun -and
    [bool]$incident.state_after_failure.destination_exact -and
    [bool]$incident.state_after_failure.legacy_epoch_4_publication_envelope_absent -and
    [bool]$incident.state_after_failure.epoch_5_correction_evidence_absent -and
    [bool]$incident.state_after_failure.epoch_6_materialization_evidence_absent -and
    [bool]$incident.resolution.preserve_epoch_5_immutable -and
    [bool]$incident.resolution.revalidate_with_frozen_epoch_5_publisher
)
$frozenPublisherText = Get-Content -Raw -LiteralPath $epochFivePublisherPath
$orderedConstructorIndex = $frozenPublisherText.IndexOf(
    '$legacyEnvelope = [ordered]@{',
    [System.StringComparison]::Ordinal
)
$legacyValidationIndex = if ($orderedConstructorIndex -ge 0) {
    $frozenPublisherText.IndexOf(
        '$legacyCheck = Test-LegacyRecoveryEnvelope',
        $orderedConstructorIndex,
        [System.StringComparison]::Ordinal
    )
}
else { -1 }
Add-EpochSixResult -Results $results `
    -Name 'frozen_epoch_5_ordered_dictionary_bug_remains_exact_incident_evidence' `
    -Passed ($incidentPassed -and $orderedConstructorIndex -ge 0 -and
        $legacyValidationIndex -gt $orderedConstructorIndex) `
    -Evidence ([pscustomobject][ordered]@{
        incident_identity_passed = $incidentPassed
        frozen_ordered_constructor_present = ($orderedConstructorIndex -ge 0)
        frozen_validator_follows_constructor = (
            $legacyValidationIndex -gt $orderedConstructorIndex
        )
        frozen_publisher_sha256 = Get-Sha256Lower -Path $epochFivePublisherPath
    })

$staticNames = @(Get-EpochSixStaticControlNames)
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
$staticNameSet = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
$staticIdentities = [System.Collections.Generic.List[object]]::new()
$staticFormatChecks = [System.Collections.Generic.List[object]]::new()
$duplicateStaticNames = [System.Collections.Generic.List[string]]::new()
foreach ($name in $staticNames) {
    if (-not $staticNameSet.Add([string]$name)) {
        $duplicateStaticNames.Add([string]$name)
        continue
    }
    $path = Join-Path $artifactDir ([string]$name)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "epoch-6 static file is absent: $name"
    }
    $item = Get-Item -LiteralPath $path -Force
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "epoch-6 static file is a reparse point: $name"
    }
    $formatCheck = Test-EpochSixLfNoBom -Path $path
    $staticFormatChecks.Add([pscustomobject][ordered]@{
        path = [string]$name
        passed = [bool]$formatCheck.passed
        utf8_bom_absent = [bool]$formatCheck.utf8_bom_absent
        utf16_bom_absent = [bool]$formatCheck.utf16_bom_absent
        carriage_return_absent = [bool]$formatCheck.carriage_return_absent
        trailing_lf_present = [bool]$formatCheck.trailing_lf_present
    })
    $staticIdentities.Add([pscustomobject][ordered]@{
        path = [string]$name
        bytes = [UInt64]$item.Length
        sha256 = Get-Sha256Lower -Path $path
    })
}
$staticOrderPassed = (
    $staticNames.Count -eq 8 -and
    $staticIdentities.Count -eq 8 -and
    $duplicateStaticNames.Count -eq 0 -and
    (Test-JsonEquivalent -Left $staticNames -Right $expectedStaticNames)
)
Add-EpochSixResult -Results $results `
    -Name 'epoch_6_static_control_identities_are_exact_ordered_and_unique' `
    -Passed $staticOrderPassed `
    -Evidence ([pscustomobject][ordered]@{
        expected_count = 8
        observed_count = $staticIdentities.Count
        duplicate_names = @($duplicateStaticNames)
        expected_names = $expectedStaticNames
        observed_names = $staticNames
    })
$staticFormattingPassed = (
    $staticFormatChecks.Count -eq 8 -and
    @($staticFormatChecks | Where-Object { -not [bool]$_.passed }).Count -eq 0
)
Add-EpochSixResult -Results $results `
    -Name 'all_epoch_6_static_controls_use_lf_without_bom' `
    -Passed $staticFormattingPassed `
    -Evidence ([pscustomobject][ordered]@{
        files = @($staticFormatChecks)
    })

$jsonParseChecks = [System.Collections.Generic.List[object]]::new()
foreach ($name in @('incident.json', 'runtime-plan.json')) {
    $path = Join-Path $artifactDir $name
    $errorMessage = $null
    try {
        [void](Read-EpochSixJson -Path $path)
    }
    catch { $errorMessage = $_.Exception.Message }
    $jsonParseChecks.Add([pscustomobject][ordered]@{
        path = $name
        passed = [string]::IsNullOrWhiteSpace([string]$errorMessage)
        error = $errorMessage
    })
}
$parseTargets = [System.Collections.Generic.List[object]]::new()
foreach ($name in $staticNames | Where-Object { $_ -like '*.ps1' }) {
    $parseTargets.Add([pscustomobject][ordered]@{
        relative_path = ([string]$plan.materialization_artifact_relative_path).
            TrimEnd('/', '\') + '/' + [string]$name
        path = Join-Path $artifactDir ([string]$name)
    })
}
$parseTargets.Add([pscustomobject][ordered]@{
    relative_path = [string]$plan.epoch_4.verifier.relative_path
    path = $verifierPath
})
$parseTargets.Add([pscustomobject][ordered]@{
    relative_path = [string]$plan.epoch_5.frozen_failed_publisher.relative_path
    path = $epochFivePublisherPath
})
$powershellParseChecks = [System.Collections.Generic.List[object]]::new()
foreach ($target in $parseTargets) {
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile(
        [string]$target.path,
        [ref]$tokens,
        [ref]$parseErrors
    )
    $powershellParseChecks.Add([pscustomobject][ordered]@{
        relative_path = [string]$target.relative_path
        passed = @($parseErrors).Count -eq 0
        errors = @($parseErrors | ForEach-Object { [string]$_.Message })
    })
}
$parsePassed = (
    @($jsonParseChecks | Where-Object { -not [bool]$_.passed }).Count -eq 0 -and
    @($powershellParseChecks | Where-Object { -not [bool]$_.passed }).Count -eq 0
)
Add-EpochSixResult -Results $results `
    -Name 'all_epoch_6_json_and_powershell_controls_parse' `
    -Passed $parsePassed `
    -Evidence ([pscustomobject][ordered]@{
        json = @($jsonParseChecks)
        powershell = @($powershellParseChecks)
    })

$destinationReport = $null
$destinationVerificationError = $null
try {
    $destinationReport = Invoke-EpochFourVerification `
        -VerifierPath $verifierPath -AttemptPath $destinationRoot `
        -RecoveryPlan $recoveryPlan -SourcePlan $sourcePlan
}
catch {
    $destinationVerificationError = $_.Exception.Message
}
$destinationVerificationErrors = if (
    [string]::IsNullOrWhiteSpace([string]$destinationVerificationError)
) {
    @()
}
else { @([string]$destinationVerificationError) }
$destinationReportCheck = if ($null -ne $destinationReport) {
    Test-EpochSixDestinationVerification -Report $destinationReport `
        -Plan $plan -EpochFourPlan $recoveryPlan -SourcePlan $sourcePlan `
        -DestinationPath $destinationRoot
}
else {
    [pscustomobject][ordered]@{
        passed = $false
        errors = @($destinationVerificationErrors)
    }
}
Add-EpochSixResult -Results $results `
    -Name 'existing_destination_uses_the_frozen_epoch_4_verifier_exactly_once' `
    -Passed ($null -ne $destinationReport -and
        [bool]$destinationReportCheck.passed -and
        [string]::IsNullOrWhiteSpace([string]$destinationVerificationError)) `
    -Evidence ([pscustomobject][ordered]@{
        report_validation_errors = @($destinationReportCheck.errors)
        invocation_error = $destinationVerificationError
    })

$epochFourControlSha256 = [string]$plan.epoch_4.control_manifest.sha256
$epochFiveControlSha256 = [string]$plan.epoch_5.control_manifest.sha256
$syntheticEpochSixControlSha256 = ('6' * 64)
$publishedAtUtc = '2026-08-27T23:41:01.1234567Z'
$correctedAtUtc = '2026-08-27T23:41:02.2345678Z'
$materializedAtUtc = '2026-08-27T23:41:03.3456789Z'

$legacyEnvelope = New-EpochSixLegacyRecoveryEnvelope `
    -Plan $plan -EpochFourPlan $recoveryPlan `
    -EpochFourControlSha256 $epochFourControlSha256 `
    -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
    -VerificationReport $destinationReport -PublishedAtUtc $publishedAtUtc `
    -ResumedExistingDestination $true
$legacyValidation = Test-EpochSixLegacyRecoveryEnvelope `
    -Envelope $legacyEnvelope -Plan $plan -EpochFourPlan $recoveryPlan `
    -SourcePlan $sourcePlan `
    -EpochFourControlSha256 $epochFourControlSha256 `
    -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
    -DestinationPath $destinationRoot -VerificationReport $destinationReport `
    -ResumedExistingDestination $true
$legacyIdentity = ConvertTo-EpochSixJsonIdentity -Value $legacyEnvelope

$correctionEvidence = New-EpochSixCorrectionEvidence `
    -Plan $plan -EpochFourPlan $recoveryPlan `
    -EpochFiveControlSha256 $epochFiveControlSha256 `
    -EpochFourControlSha256 $epochFourControlSha256 `
    -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
    -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
    -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
    -CorrectedAtUtc $correctedAtUtc -ResumedExistingDestination $true
$correctionValidation = Test-EpochSixCorrectionEvidence `
    -Evidence $correctionEvidence -Plan $plan `
    -EpochFourPlan $recoveryPlan `
    -EpochFiveControlSha256 $epochFiveControlSha256 `
    -EpochFourControlSha256 $epochFourControlSha256 `
    -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
    -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
    -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
    -ResumedExistingDestination $true
$correctionIdentity = ConvertTo-EpochSixJsonIdentity -Value $correctionEvidence

$materializationEvidence = New-EpochSixMaterializationEvidence `
    -Plan $plan -EpochSixControlSha256 $syntheticEpochSixControlSha256 `
    -EpochFiveControlSha256 $epochFiveControlSha256 `
    -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
    -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
    -CorrectionEvidenceSha256 ([string]$correctionIdentity.sha256) `
    -CorrectionEvidenceBytes ([UInt64]$correctionIdentity.bytes) `
    -DestinationCheck $destinationTreeCheck `
    -MaterializedAtUtc $materializedAtUtc `
    -ResumedExistingDestination $true
$materializationValidation = Test-EpochSixMaterializationEvidence `
    -Evidence $materializationEvidence -Plan $plan `
    -EpochSixControlSha256 $syntheticEpochSixControlSha256 `
    -EpochFiveControlSha256 $epochFiveControlSha256 `
    -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
    -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
    -CorrectionEvidenceSha256 ([string]$correctionIdentity.sha256) `
    -CorrectionEvidenceBytes ([UInt64]$correctionIdentity.bytes) `
    -DestinationCheck $destinationTreeCheck `
    -ResumedExistingDestination $true

$legacyTopProperties = @(
    'schema', 'task', 'operation_id', 'execution_epoch',
    'publication_epoch', 'timestamp_protocol', 'published_at_utc',
    'control_manifest_sha256', 'source', 'destination',
    'stage_verification', 'published_verification',
    'resumed_existing_destination', 'passed'
)
$correctionTopProperties = @(
    'schema', 'task', 'operation_id', 'failed_operation_id',
    'execution_epoch', 'failed_publication_epoch', 'correction_epoch',
    'timestamp_protocol', 'corrected_at_utc', 'control_manifest_sha256',
    'failed_epoch_control_manifest_sha256', 'legacy_envelope', 'source',
    'destination', 'resumed_existing_destination',
    'legacy_envelope_validation', 'passed'
)
$materializationTopProperties = @(
    'schema', 'task', 'operation_id', 'correction_operation_id',
    'failed_operation_id', 'execution_epoch', 'failed_publication_epoch',
    'failed_correction_epoch', 'materialization_epoch', 'timestamp_protocol',
    'materialized_at_utc', 'control_manifest_sha256',
    'correction_epoch_control_manifest_sha256', 'legacy_envelope',
    'correction_evidence', 'destination', 'resumed_existing_destination',
    'authoritative_revalidation', 'passed'
)
$treeBindingProperties = @('relative_path', 'manifest_sha256', 'entries')
$fileBindingProperties = @('relative_path', 'bytes', 'sha256')
$validationProperties = @('contract', 'passed')
$authoritativeProperties = @(
    'publisher_relative_path', 'publisher_bytes', 'publisher_sha256',
    'exit_code', 'correction_json_equivalent', 'passed'
)
$constructorsReturnJsonObjects = (
    $legacyEnvelope -is [pscustomobject] -and
    $correctionEvidence -is [pscustomobject] -and
    $materializationEvidence -is [pscustomobject] -and
    $legacyEnvelope.source -is [pscustomobject] -and
    $legacyEnvelope.destination -is [pscustomobject] -and
    $correctionEvidence.legacy_envelope -is [pscustomobject] -and
    $correctionEvidence.source -is [pscustomobject] -and
    $correctionEvidence.destination -is [pscustomobject] -and
    $correctionEvidence.legacy_envelope_validation -is [pscustomobject] -and
    $materializationEvidence.legacy_envelope -is [pscustomobject] -and
    $materializationEvidence.correction_evidence -is [pscustomobject] -and
    $materializationEvidence.destination -is [pscustomobject] -and
    $materializationEvidence.authoritative_revalidation -is [pscustomobject] -and
    $legacyEnvelope -isnot [System.Collections.Specialized.OrderedDictionary] -and
    $correctionEvidence -isnot [System.Collections.Specialized.OrderedDictionary] -and
    $materializationEvidence -isnot [System.Collections.Specialized.OrderedDictionary]
)
$constructorSequencesPassed = (
    (Test-EpochSixExactPropertySequence -Value $legacyEnvelope `
        -Expected $legacyTopProperties) -and
    (Test-EpochSixExactPropertySequence -Value $legacyEnvelope.source `
        -Expected $treeBindingProperties) -and
    (Test-EpochSixExactPropertySequence -Value $legacyEnvelope.destination `
        -Expected $treeBindingProperties) -and
    (Test-EpochSixExactPropertySequence -Value $correctionEvidence `
        -Expected $correctionTopProperties) -and
    (Test-EpochSixExactPropertySequence `
        -Value $correctionEvidence.legacy_envelope `
        -Expected $fileBindingProperties) -and
    (Test-EpochSixExactPropertySequence -Value $correctionEvidence.source `
        -Expected $treeBindingProperties) -and
    (Test-EpochSixExactPropertySequence -Value $correctionEvidence.destination `
        -Expected $treeBindingProperties) -and
    (Test-EpochSixExactPropertySequence `
        -Value $correctionEvidence.legacy_envelope_validation `
        -Expected $validationProperties) -and
    (Test-EpochSixExactPropertySequence -Value $materializationEvidence `
        -Expected $materializationTopProperties) -and
    (Test-EpochSixExactPropertySequence `
        -Value $materializationEvidence.legacy_envelope `
        -Expected $fileBindingProperties) -and
    (Test-EpochSixExactPropertySequence `
        -Value $materializationEvidence.correction_evidence `
        -Expected $fileBindingProperties) -and
    (Test-EpochSixExactPropertySequence -Value $materializationEvidence.destination `
        -Expected $treeBindingProperties) -and
    (Test-EpochSixExactPropertySequence `
        -Value $materializationEvidence.authoritative_revalidation `
        -Expected $authoritativeProperties)
)
Add-EpochSixResult -Results $results `
    -Name 'constructors_regress_ordered_dictionary_to_json_object_contracts' `
    -Passed ($constructorsReturnJsonObjects -and $constructorSequencesPassed -and
        [bool]$legacyValidation.passed -and
        [bool]$correctionValidation.passed -and
        [bool]$materializationValidation.passed) `
    -Evidence ([pscustomobject][ordered]@{
        json_object_types = $constructorsReturnJsonObjects
        exact_top_and_nested_sequences = $constructorSequencesPassed
        legacy_errors = @($legacyValidation.errors)
        correction_errors = @($correctionValidation.errors)
        materialization_errors = @($materializationValidation.errors)
    })

function Test-LegacyCandidate {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Candidate)

    Test-EpochSixLegacyRecoveryEnvelope -Envelope $Candidate -Plan $plan `
        -EpochFourPlan $recoveryPlan -SourcePlan $sourcePlan `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
        -DestinationPath $destinationRoot -VerificationReport $destinationReport `
        -ResumedExistingDestination $true
}

function Test-CorrectionCandidate {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Candidate)

    Test-EpochSixCorrectionEvidence -Evidence $Candidate -Plan $plan `
        -EpochFourPlan $recoveryPlan `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -EpochFourControlSha256 $epochFourControlSha256 `
        -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
        -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
        -SourceCheck $sourceTreeCheck -DestinationCheck $destinationTreeCheck `
        -ResumedExistingDestination $true
}

function Test-MaterializationCandidate {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Candidate)

    Test-EpochSixMaterializationEvidence -Evidence $Candidate -Plan $plan `
        -EpochSixControlSha256 $syntheticEpochSixControlSha256 `
        -EpochFiveControlSha256 $epochFiveControlSha256 `
        -LegacyEnvelopeSha256 ([string]$legacyIdentity.sha256) `
        -LegacyEnvelopeBytes ([UInt64]$legacyIdentity.bytes) `
        -CorrectionEvidenceSha256 ([string]$correctionIdentity.sha256) `
        -CorrectionEvidenceBytes ([UInt64]$correctionIdentity.bytes) `
        -DestinationCheck $destinationTreeCheck `
        -ResumedExistingDestination $true
}

$legacyMutations = [System.Collections.Generic.List[object]]::new()
$legacyMissing = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyMissing.PSObject.Properties.Remove('passed')
$legacyMutations.Add([pscustomobject]@{ name = 'missing'; value = $legacyMissing })
$legacyExtra = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyExtra | Add-Member -NotePropertyName 'unexpected' -NotePropertyValue $true
$legacyMutations.Add([pscustomobject]@{ name = 'extra'; value = $legacyExtra })
$legacyMutations.Add([pscustomobject]@{
    name = 'reordered'
    value = New-EpochSixReorderedObject -Value $legacyEnvelope
})
$legacyCrossOperation = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyCrossOperation.operation_id = [string]$plan.operation.correction_operation_id
$legacyMutations.Add([pscustomobject]@{
    name = 'cross_operation'; value = $legacyCrossOperation
})
$legacyWrongTimestamp = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyWrongTimestamp.published_at_utc = '2026-08-27T19:41:01.1234567-04:00'
$legacyMutations.Add([pscustomobject]@{
    name = 'wrong_timestamp'; value = $legacyWrongTimestamp
})
$legacyWrongProtocol = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyWrongProtocol.timestamp_protocol = 'powershell-json-implicit-local-v0'
$legacyMutations.Add([pscustomobject]@{
    name = 'wrong_protocol'; value = $legacyWrongProtocol
})
$legacyWrongReport = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyWrongReport.stage_verification.operation_id =
    [string]$plan.operation.correction_operation_id
$legacyMutations.Add([pscustomobject]@{
    name = 'wrong_report'; value = $legacyWrongReport
})
$legacyWrongPath = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyWrongPath.source.relative_path = [string]$plan.operation.destination_relative_path
$legacyMutations.Add([pscustomobject]@{
    name = 'wrong_path'; value = $legacyWrongPath
})
$legacyWrongHash = Copy-EpochSixJsonValue -Value $legacyEnvelope
$legacyWrongHash.source.manifest_sha256 = ('0' * 64)
$legacyMutations.Add([pscustomobject]@{
    name = 'wrong_hash'; value = $legacyWrongHash
})
$legacyMutationChecks = [System.Collections.Generic.List[object]]::new()
foreach ($mutation in $legacyMutations) {
    $check = Test-LegacyCandidate -Candidate $mutation.value
    $legacyMutationChecks.Add([pscustomobject][ordered]@{
        name = [string]$mutation.name
        rejected = -not [bool]$check.passed
        errors = @($check.errors)
    })
}
Add-EpochSixResult -Results $results `
    -Name 'legacy_v4_validator_accepts_exact_and_rejects_all_adversarial_mutations' `
    -Passed ([bool]$legacyValidation.passed -and
        $legacyMutationChecks.Count -eq 9 -and
        @($legacyMutationChecks | Where-Object { -not [bool]$_.rejected }).Count -eq 0) `
    -Evidence ([pscustomobject][ordered]@{
        exact_accepted = [bool]$legacyValidation.passed
        mutations = @($legacyMutationChecks)
    })

$correctionMutations = [System.Collections.Generic.List[object]]::new()
$correctionMissing = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionMissing.PSObject.Properties.Remove('passed')
$correctionMutations.Add([pscustomobject]@{
    name = 'missing'; value = $correctionMissing
})
$correctionExtra = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionExtra | Add-Member -NotePropertyName 'unexpected' -NotePropertyValue $true
$correctionMutations.Add([pscustomobject]@{
    name = 'extra'; value = $correctionExtra
})
$correctionMutations.Add([pscustomobject]@{
    name = 'reordered'
    value = New-EpochSixReorderedObject -Value $correctionEvidence
})
$correctionCrossOperation = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionCrossOperation.operation_id = [string]$plan.operation.id
$correctionMutations.Add([pscustomobject]@{
    name = 'cross_operation'; value = $correctionCrossOperation
})
$correctionWrongTimestamp = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionWrongTimestamp.corrected_at_utc = '2026-08-27T19:41:02.2345678-04:00'
$correctionMutations.Add([pscustomobject]@{
    name = 'wrong_timestamp'; value = $correctionWrongTimestamp
})
$correctionWrongProtocol = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionWrongProtocol.timestamp_protocol = 'powershell-json-implicit-local-v0'
$correctionMutations.Add([pscustomobject]@{
    name = 'wrong_protocol'; value = $correctionWrongProtocol
})
$correctionWrongPath = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionWrongPath.legacy_envelope.relative_path =
    [string]$plan.operation.correction_evidence_relative_path
$correctionMutations.Add([pscustomobject]@{
    name = 'wrong_path'; value = $correctionWrongPath
})
$correctionWrongHash = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionWrongHash.legacy_envelope.sha256 = ('0' * 64)
$correctionMutations.Add([pscustomobject]@{
    name = 'wrong_hash'; value = $correctionWrongHash
})
$correctionWrongControl = Copy-EpochSixJsonValue -Value $correctionEvidence
$correctionWrongControl.control_manifest_sha256 = ('1' * 64)
$correctionMutations.Add([pscustomobject]@{
    name = 'wrong_control_hash'; value = $correctionWrongControl
})
$correctionMutationChecks = [System.Collections.Generic.List[object]]::new()
foreach ($mutation in $correctionMutations) {
    $check = Test-CorrectionCandidate -Candidate $mutation.value
    $correctionMutationChecks.Add([pscustomobject][ordered]@{
        name = [string]$mutation.name
        rejected = -not [bool]$check.passed
        errors = @($check.errors)
    })
}
Add-EpochSixResult -Results $results `
    -Name 'correction_v5_validator_accepts_exact_and_rejects_all_adversarial_mutations' `
    -Passed ([bool]$correctionValidation.passed -and
        $correctionMutationChecks.Count -eq 9 -and
        @($correctionMutationChecks | Where-Object {
                -not [bool]$_.rejected
            }).Count -eq 0) `
    -Evidence ([pscustomobject][ordered]@{
        exact_accepted = [bool]$correctionValidation.passed
        mutations = @($correctionMutationChecks)
    })

$materializationMutations = [System.Collections.Generic.List[object]]::new()
$materializationMissing = Copy-EpochSixJsonValue -Value $materializationEvidence
$materializationMissing.PSObject.Properties.Remove('passed')
$materializationMutations.Add([pscustomobject]@{
    name = 'missing'; value = $materializationMissing
})
$materializationExtra = Copy-EpochSixJsonValue -Value $materializationEvidence
$materializationExtra | Add-Member -NotePropertyName 'unexpected' -NotePropertyValue $true
$materializationMutations.Add([pscustomobject]@{
    name = 'extra'; value = $materializationExtra
})
$materializationMutations.Add([pscustomobject]@{
    name = 'reordered'
    value = New-EpochSixReorderedObject -Value $materializationEvidence
})
$materializationWrongOperation = Copy-EpochSixJsonValue -Value $materializationEvidence
$materializationWrongOperation.operation_id = [string]$plan.operation.correction_operation_id
$materializationMutations.Add([pscustomobject]@{
    name = 'cross_operation'; value = $materializationWrongOperation
})
$materializationWrongTimestamp = Copy-EpochSixJsonValue `
    -Value $materializationEvidence
$materializationWrongTimestamp.materialized_at_utc = 'not-rfc3339'
$materializationMutations.Add([pscustomobject]@{
    name = 'wrong_timestamp'; value = $materializationWrongTimestamp
})
$materializationWrongHash = Copy-EpochSixJsonValue -Value $materializationEvidence
$materializationWrongHash.correction_evidence.sha256 = ('0' * 64)
$materializationMutations.Add([pscustomobject]@{
    name = 'wrong_hash'; value = $materializationWrongHash
})
$materializationMutationChecks = [System.Collections.Generic.List[object]]::new()
foreach ($mutation in $materializationMutations) {
    $check = Test-MaterializationCandidate -Candidate $mutation.value
    $materializationMutationChecks.Add([pscustomobject][ordered]@{
        name = [string]$mutation.name
        rejected = -not [bool]$check.passed
        errors = @($check.errors)
    })
}
Add-EpochSixResult -Results $results `
    -Name 'materialization_v6_validator_accepts_exact_and_rejects_mutations' `
    -Passed ([bool]$materializationValidation.passed -and
        $materializationMutationChecks.Count -eq 6 -and
        @($materializationMutationChecks | Where-Object {
                -not [bool]$_.rejected
            }).Count -eq 0) `
    -Evidence ([pscustomobject][ordered]@{
        exact_accepted = [bool]$materializationValidation.passed
        mutations = @($materializationMutationChecks)
    })

function Invoke-EpochSixSyntheticState {
    [CmdletBinding()]
    param(
        [bool]$DestinationExists = $true,
        [bool]$DestinationContainer = $true,
        [bool]$DestinationNonReparse = $true,
        [bool]$DestinationExact = $true,
        [bool]$LegacyEnvelopeExists = $false,
        [bool]$LegacyEnvelopeLeaf = $false,
        [bool]$LegacyEnvelopeNonReparse = $false,
        [bool]$LegacyEnvelopeExact = $false,
        [bool]$CorrectionEvidenceExists = $false,
        [bool]$CorrectionEvidenceLeaf = $false,
        [bool]$CorrectionEvidenceNonReparse = $false,
        [bool]$CorrectionEvidenceExact = $false,
        [bool]$MaterializationEvidenceExists = $false,
        [bool]$MaterializationEvidenceLeaf = $false,
        [bool]$MaterializationEvidenceNonReparse = $false,
        [bool]$MaterializationEvidenceExact = $false
    )

    Test-EpochSixMaterializationState `
        -DestinationExists $DestinationExists `
        -DestinationContainer $DestinationContainer `
        -DestinationNonReparse $DestinationNonReparse `
        -DestinationExact $DestinationExact `
        -LegacyEnvelopeExists $LegacyEnvelopeExists `
        -LegacyEnvelopeLeaf $LegacyEnvelopeLeaf `
        -LegacyEnvelopeNonReparse $LegacyEnvelopeNonReparse `
        -LegacyEnvelopeExact $LegacyEnvelopeExact `
        -CorrectionEvidenceExists $CorrectionEvidenceExists `
        -CorrectionEvidenceLeaf $CorrectionEvidenceLeaf `
        -CorrectionEvidenceNonReparse $CorrectionEvidenceNonReparse `
        -CorrectionEvidenceExact $CorrectionEvidenceExact `
        -MaterializationEvidenceExists $MaterializationEvidenceExists `
        -MaterializationEvidenceLeaf $MaterializationEvidenceLeaf `
        -MaterializationEvidenceNonReparse $MaterializationEvidenceNonReparse `
        -MaterializationEvidenceExact $MaterializationEvidenceExact
}

$freshDestinationState = Invoke-EpochSixSyntheticState
$legacyOnlyState = Invoke-EpochSixSyntheticState `
    -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
    -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true
$bothExactState = Invoke-EpochSixSyntheticState `
    -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
    -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
    -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
    -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true
$alreadyCompleteState = Invoke-EpochSixSyntheticState `
    -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
    -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
    -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
    -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true `
    -MaterializationEvidenceExists $true -MaterializationEvidenceLeaf $true `
    -MaterializationEvidenceNonReparse $true `
    -MaterializationEvidenceExact $true
$authorizedStatesPassed = (
    [bool]$freshDestinationState.passed -and
    [string]$freshDestinationState.action -ceq
        'materialize_legacy_correction_and_record' -and
    [bool]$legacyOnlyState.passed -and
    [string]$legacyOnlyState.action -ceq 'materialize_correction_and_record' -and
    [bool]$bothExactState.passed -and
    [string]$bothExactState.action -ceq 'revalidate_and_record' -and
    [bool]$alreadyCompleteState.passed -and
    [string]$alreadyCompleteState.action -ceq 'revalidate_complete'
)
Add-EpochSixResult -Results $results `
    -Name 'state_machine_covers_destination_resume_legacy_completion_and_revalidation' `
    -Passed $authorizedStatesPassed `
    -Evidence ([pscustomobject][ordered]@{
        exact_destination_resume = [string]$freshDestinationState.action
        legacy_only_completion = [string]$legacyOnlyState.action
        both_exact_materialization_missing = [string]$bothExactState.action
        already_complete = [string]$alreadyCompleteState.action
    })

$invalidStates = [System.Collections.Generic.List[object]]::new()
$invalidStates.Add([pscustomobject]@{
    name = 'destination_absent'
    value = Invoke-EpochSixSyntheticState -DestinationExists $false `
        -DestinationContainer $false -DestinationNonReparse $false `
        -DestinationExact $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'destination_wrong_type'
    value = Invoke-EpochSixSyntheticState -DestinationContainer $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'destination_reparse'
    value = Invoke-EpochSixSyntheticState -DestinationNonReparse $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'destination_differing'
    value = Invoke-EpochSixSyntheticState -DestinationExact $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'correction_without_legacy'
    value = Invoke-EpochSixSyntheticState `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'materialization_without_predecessors'
    value = Invoke-EpochSixSyntheticState `
        -MaterializationEvidenceExists $true -MaterializationEvidenceLeaf $true `
        -MaterializationEvidenceNonReparse $true `
        -MaterializationEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'legacy_wrong_type'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $false `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'legacy_reparse'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $false -LegacyEnvelopeExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'legacy_differing'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'correction_wrong_type'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $false `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'correction_reparse'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $false -CorrectionEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'correction_differing'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $false
})
$invalidStates.Add([pscustomobject]@{
    name = 'materialization_wrong_type'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true `
        -MaterializationEvidenceExists $true -MaterializationEvidenceLeaf $false `
        -MaterializationEvidenceNonReparse $true `
        -MaterializationEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'materialization_reparse'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true `
        -MaterializationEvidenceExists $true -MaterializationEvidenceLeaf $true `
        -MaterializationEvidenceNonReparse $false `
        -MaterializationEvidenceExact $true
})
$invalidStates.Add([pscustomobject]@{
    name = 'materialization_differing'
    value = Invoke-EpochSixSyntheticState `
        -LegacyEnvelopeExists $true -LegacyEnvelopeLeaf $true `
        -LegacyEnvelopeNonReparse $true -LegacyEnvelopeExact $true `
        -CorrectionEvidenceExists $true -CorrectionEvidenceLeaf $true `
        -CorrectionEvidenceNonReparse $true -CorrectionEvidenceExact $true `
        -MaterializationEvidenceExists $true -MaterializationEvidenceLeaf $true `
        -MaterializationEvidenceNonReparse $true `
        -MaterializationEvidenceExact $false
})
$invalidStateEvidence = @($invalidStates | ForEach-Object {
        [pscustomobject][ordered]@{
            name = [string]$_.name
            rejected = -not [bool]$_.value.passed
            action = $_.value.action
            errors = @($_.value.errors)
        }
    })
Add-EpochSixResult -Results $results `
    -Name 'state_machine_rejects_partial_wrong_type_reparse_and_differing_states' `
    -Passed ($invalidStateEvidence.Count -eq 15 -and
        @($invalidStateEvidence | Where-Object {
                -not [bool]$_.rejected
            }).Count -eq 0) `
    -Evidence ([pscustomobject][ordered]@{
        states = $invalidStateEvidence
    })

$selfTestParent = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath 'target/s114-experiment/runtime-epoch-6-selftest'
$selfTestBaseChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path (Split-Path -Parent $selfTestParent)
if (-not [bool]$selfTestBaseChain.passed) {
    throw "epoch-6 self-test base chain is unsafe: $(@($selfTestBaseChain.errors) -join '; ')"
}
[System.IO.Directory]::CreateDirectory($selfTestParent) | Out-Null
$selfTestParentChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path $selfTestParent
if (-not [bool]$selfTestParentChain.passed) {
    throw "epoch-6 self-test parent chain is unsafe: $(@($selfTestParentChain.errors) -join '; ')"
}
$selfTestOwner = Join-Path $selfTestParent ([guid]::NewGuid().ToString('N'))
if (Test-Path -LiteralPath $selfTestOwner) {
    throw 'generated epoch-6 self-test owner already exists'
}
[System.IO.Directory]::CreateDirectory($selfTestOwner) | Out-Null
$selfTestOwnerChain = Test-EpochSixNonReparseDirectoryChain `
    -RepositoryRoot $repoRoot -Path $selfTestOwner
if (-not [bool]$selfTestOwnerChain.passed) {
    throw "epoch-6 self-test owner chain is unsafe: $(@($selfTestOwnerChain.errors) -join '; ')"
}
$writerFormatting = $null
$writerRoundTripPassed = $false
$writerOverwriteRefused = $false
$ownedWorkspaceCleaned = $false
try {
    $writerTarget = Join-Path $selfTestOwner 'writer-contract.json'
    $writerValue = [pscustomobject][ordered]@{
        schema = 'epoch-6-self-test-writer-contract'
        passed = $true
    }
    Write-EpochSixJsonAtomic -Path $writerTarget -Value $writerValue
    $writerFormatting = Test-EpochSixLfNoBom -Path $writerTarget
    $writerRoundTrip = Read-EpochSixJson -Path $writerTarget
    $writerRoundTripPassed = (
        [string]$writerRoundTrip.schema -ceq
            'epoch-6-self-test-writer-contract' -and
        [bool]$writerRoundTrip.passed
    )
    try {
        Write-EpochSixJsonAtomic -Path $writerTarget -Value $writerValue
    }
    catch {
        $writerOverwriteRefused =
            $_.Exception.Message -match 'already exists|will not be overwritten'
    }
}
finally {
    $ownedWorkspaceCleaned = Remove-EpochSixOwnedDirectory `
        -Path $selfTestOwner -ExpectedParent $selfTestParent `
        -RepositoryRoot $repoRoot
}
Add-EpochSixResult -Results $results `
    -Name 'atomic_writer_uses_lf_no_bom_refuses_overwrite_and_cleans_temp' `
    -Passed ($null -ne $writerFormatting -and
        [bool]$writerFormatting.passed -and $writerRoundTripPassed -and
        $writerOverwriteRefused -and $ownedWorkspaceCleaned) `
    -Evidence ([pscustomobject][ordered]@{
        formatting = $writerFormatting
        json_round_trip = $writerRoundTripPassed
        overwrite_refused = $writerOverwriteRefused
        owned_workspace_absent = $ownedWorkspaceCleaned
    })

$freezePath = Join-Path $artifactDir 'freeze-materialization.ps1'
$freezeTextForStageSafety = Get-Content -Raw -LiteralPath $freezePath
$freezeTokensForBoundary = $null
$freezeParseErrorsForBoundary = $null
$freezeAstForBoundary = [System.Management.Automation.Language.Parser]::ParseFile(
    $freezePath,
    [ref]$freezeTokensForBoundary,
    [ref]$freezeParseErrorsForBoundary
)
$freezeCommandsForBoundary = @($freezeAstForBoundary.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst]
        }, $true) | ForEach-Object { $_.GetCommandName() })
$freezeVerifierInvocationCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Invoke-EpochFourVerification'
    }).Count
$freezeDirectFileHashCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Get-FileHash'
    }).Count
$freezeColdStateInvocationCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Get-EpochFiveColdState'
    }).Count
$freezeDependencyInvocationCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Test-EpochSixFrozenDependencySet'
    }).Count
$freezeRewalkInvocationCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Test-EpochFourFrozenDependencySet'
    }).Count
$freezeExactTreeInvocationCount = @($freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Test-EpochFiveExactTree'
    }).Count
$freezeReportValidationInvocationCount = @(
    $freezeCommandsForBoundary | Where-Object {
        $_ -ceq 'Test-EpochSixDestinationVerification'
    }
).Count
$freezeVerifierIndex = $freezeTextForStageSafety.IndexOf(
    '$destinationReport = Invoke-EpochFourVerification',
    [System.StringComparison]::Ordinal
)
$freezeOutputRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'legacy epoch-4 envelope appeared during epoch-6 verification',
    [System.StringComparison]::Ordinal
)
$freezeDependencyRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'post-verifier frozen dependency set differs',
    [System.StringComparison]::Ordinal
)
$freezeStaticRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'post-verifier epoch-6/',
    [System.StringComparison]::Ordinal
)
$freezeSourceRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'post-verifier raw source',
    [System.StringComparison]::Ordinal
)
$freezeReportRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'post-verifier frozen report validation differs',
    [System.StringComparison]::Ordinal
)
$freezeColdRefreshIndex = $freezeTextForStageSafety.IndexOf(
    'Ferric/llama-server state became non-cold during epoch-6 verification',
    [System.StringComparison]::Ordinal
)
$freezeManifestIndex = $freezeTextForStageSafety.IndexOf(
    '$controlManifest = [ordered]@{',
    [System.StringComparison]::Ordinal
)
$tempBoundaryGuardPassed = (
    [bool]$selfTestBaseChain.passed -and
    [bool]$selfTestParentChain.passed -and
    [bool]$selfTestOwnerChain.passed -and
    @($freezeParseErrorsForBoundary).Count -eq 0 -and
    $freezeVerifierInvocationCount -eq 1 -and
    $freezeDirectFileHashCount -eq 0 -and
    $freezeColdStateInvocationCount -eq 2 -and
    $freezeDependencyInvocationCount -eq 2 -and
    $freezeRewalkInvocationCount -eq 2 -and
    $freezeExactTreeInvocationCount -eq 4 -and
    $freezeReportValidationInvocationCount -eq 1 -and
    $freezeVerifierIndex -ge 0 -and
    $freezeStaticRefreshIndex -gt $freezeVerifierIndex -and
    $freezeDependencyRefreshIndex -gt $freezeStaticRefreshIndex -and
    $freezeSourceRefreshIndex -gt $freezeDependencyRefreshIndex -and
    $freezeReportRefreshIndex -gt $freezeSourceRefreshIndex -and
    $freezeOutputRefreshIndex -gt $freezeReportRefreshIndex -and
    $freezeColdRefreshIndex -gt $freezeOutputRefreshIndex -and
    $freezeManifestIndex -gt $freezeColdRefreshIndex -and
    $freezeTextForStageSafety.Contains(
        'Test-EpochSixNonReparseDirectoryChain',
        [System.StringComparison]::Ordinal
    ) -and
    $freezeTextForStageSafety.Contains(
        'refusing epoch-6 stage cleanup through an unsafe directory chain',
        [System.StringComparison]::Ordinal
    ) -and
    $freezeTextForStageSafety.Contains(
        'refusing to recursively remove epoch-6 stage content with a reparse point',
        [System.StringComparison]::Ordinal
    ) -and
    $freezeTextForStageSafety.Contains(
        'legacy epoch-4 envelope appeared during epoch-6 verification',
        [System.StringComparison]::Ordinal
    ) -and
    $freezeTextForStageSafety.Contains(
        'Ferric/llama-server state became non-cold during epoch-6 verification',
        [System.StringComparison]::Ordinal
    )
)
Add-EpochSixResult -Results $results `
    -Name 'temporary_workflows_enforce_non_reparse_parent_owner_and_cleanup_boundaries' `
    -Passed $tempBoundaryGuardPassed `
    -Evidence ([pscustomobject][ordered]@{
        self_test_parent_components_checked =
            [int]$selfTestParentChain.components_checked
        self_test_base_components_checked =
            [int]$selfTestBaseChain.components_checked
        self_test_owner_components_checked =
            [int]$selfTestOwnerChain.components_checked
        freeze_stage_guards_present = $tempBoundaryGuardPassed
        freeze_verifier_invocations = $freezeVerifierInvocationCount
        freeze_direct_get_file_hash_invocations = $freezeDirectFileHashCount
        freeze_cold_state_invocations = $freezeColdStateInvocationCount
        freeze_dependency_rewalk_invocations =
            $freezeDependencyInvocationCount
        freeze_epoch_4_rewalk_invocations = $freezeRewalkInvocationCount
        freeze_exact_tree_invocations = $freezeExactTreeInvocationCount
        freeze_report_validation_invocations =
            $freezeReportValidationInvocationCount
        freeze_post_verifier_refresh_precedes_manifest =
            ($freezeVerifierIndex -ge 0 -and
                $freezeStaticRefreshIndex -gt $freezeVerifierIndex -and
                $freezeDependencyRefreshIndex -gt $freezeStaticRefreshIndex -and
                $freezeSourceRefreshIndex -gt $freezeDependencyRefreshIndex -and
                $freezeReportRefreshIndex -gt $freezeSourceRefreshIndex -and
                $freezeOutputRefreshIndex -gt $freezeReportRefreshIndex -and
                $freezeColdRefreshIndex -gt $freezeOutputRefreshIndex -and
                $freezeManifestIndex -gt $freezeColdRefreshIndex)
    })

$selfTokens = $null
$selfParseErrors = $null
$selfAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $PSCommandPath,
    [ref]$selfTokens,
    [ref]$selfParseErrors
)
$selfCommands = @($selfAst.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst]
        }, $true) | ForEach-Object { $_.GetCommandName() })
$verifierInvocationCount = @($selfCommands | Where-Object {
        $_ -ceq 'Invoke-EpochFourVerification'
    }).Count
$directFileHashCount = @($selfCommands | Where-Object {
        $_ -ceq 'Get-FileHash'
    }).Count
$liveModelIdentityPassed = (
    $null -ne $destinationReport -and
    [bool]$destinationReport.live_model_identity.checked -and
    [string]$destinationReport.live_model_identity.mode -ceq
        'checked_in_verifier' -and
    [string]$destinationReport.live_model_identity.sha256 -ceq
        [string]$plan.model.sha256
)
Add-EpochSixResult -Results $results `
    -Name 'one_frozen_verifier_call_performs_the_only_live_model_hash' `
    -Passed ($verifierInvocationCount -eq 1 -and
        $directFileHashCount -eq 0 -and $liveModelIdentityPassed) `
    -Evidence ([pscustomobject][ordered]@{
        verifier_invocations_in_self_test = $verifierInvocationCount
        direct_get_file_hash_invocations_in_self_test = $directFileHashCount
        live_model_identity_checked = $liveModelIdentityPassed
        model_sha256 = if ($null -ne $destinationReport) {
            [string]$destinationReport.live_model_identity.sha256
        }
        else { $null }
    })

$materializerPath = Join-Path $artifactDir 'materialize-e05-evidence.ps1'
$materializerText = Get-Content -Raw -LiteralPath $materializerPath
$legacyWriteIndex = $materializerText.IndexOf(
    'Write-EpochSixJsonAtomic -Path $legacyEnvelopePath',
    [System.StringComparison]::Ordinal
)
$legacyPostWriteValidationIndex = if ($legacyWriteIndex -ge 0) {
    $materializerText.IndexOf(
        '$legacyCheck = Test-EpochSixLegacyRecoveryEnvelope',
        $legacyWriteIndex,
        [System.StringComparison]::Ordinal
    )
}
else { -1 }
$correctionWriteIndex = $materializerText.IndexOf(
    'Write-EpochSixJsonAtomic -Path $correctionEvidencePath',
    [System.StringComparison]::Ordinal
)
$correctionPostWriteValidationIndex = if ($correctionWriteIndex -ge 0) {
    $materializerText.IndexOf(
        '$correctionCheck = Test-EpochSixCorrectionEvidence',
        $correctionWriteIndex,
        [System.StringComparison]::Ordinal
    )
}
else { -1 }
$publisherInvocationIndex = $materializerText.IndexOf(
    '$publisherResult = Invoke-PowerShellFileBounded -ScriptPath $publisherPath',
    [System.StringComparison]::Ordinal
)
$publisherEquivalenceIndex = $materializerText.IndexOf(
    '(Test-JsonEquivalent -Left $publisherCorrection',
    [System.StringComparison]::Ordinal
)
$materializationWriteIndex = $materializerText.IndexOf(
    'Write-EpochSixJsonAtomic -Path $materializationEvidencePath',
    [System.StringComparison]::Ordinal
)
$materializerTokens = $null
$materializerParseErrors = $null
$materializerAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $materializerPath,
    [ref]$materializerTokens,
    [ref]$materializerParseErrors
)
$materializerCommands = @($materializerAst.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.CommandAst]
        }, $true) | ForEach-Object { $_.GetCommandName() })
$materializerPublisherInvocationCount = @($materializerCommands | Where-Object {
        $_ -ceq 'Invoke-PowerShellFileBounded'
    }).Count
$destinationMutationCommandCount = @($materializerCommands | Where-Object {
        $_ -in @('Copy-Item', 'Move-Item', 'Remove-Item')
    }).Count
$publisherAfterBothExactOutputs = (
    $legacyWriteIndex -ge 0 -and
    $legacyPostWriteValidationIndex -gt $legacyWriteIndex -and
    $correctionWriteIndex -gt $legacyPostWriteValidationIndex -and
    $correctionPostWriteValidationIndex -gt $correctionWriteIndex -and
    $publisherInvocationIndex -gt $correctionPostWriteValidationIndex -and
    $publisherEquivalenceIndex -gt $publisherInvocationIndex -and
    $materializationWriteIndex -gt $publisherEquivalenceIndex -and
    $materializerPublisherInvocationCount -eq 1 -and
    $destinationMutationCommandCount -eq 0
)
Add-EpochSixResult -Results $results `
    -Name 'materializer_invokes_frozen_publisher_only_after_both_exact_outputs' `
    -Passed $publisherAfterBothExactOutputs `
    -Evidence ([pscustomobject][ordered]@{
        legacy_write_index = $legacyWriteIndex
        legacy_post_write_validation_index = $legacyPostWriteValidationIndex
        correction_write_index = $correctionWriteIndex
        correction_post_write_validation_index = $correctionPostWriteValidationIndex
        frozen_publisher_invocation_index = $publisherInvocationIndex
        publisher_json_equivalence_index = $publisherEquivalenceIndex
        materialization_write_index = $materializationWriteIndex
        bounded_publisher_invocation_count = $materializerPublisherInvocationCount
        destination_mutation_command_count = $destinationMutationCommandCount
    })

$officialOutputsStillAbsent = (
    -not (Test-Path -LiteralPath $legacyEnvelopePath) -and
    -not (Test-Path -LiteralPath $correctionEvidencePath) -and
    -not (Test-Path -LiteralPath $materializationEvidencePath) -and
    -not (Test-Path -LiteralPath $controlManifestPath) -and
    -not (Test-Path -LiteralPath $controlDigestPath)
)
Add-EpochSixResult -Results $results `
    -Name 'self_test_does_not_materialize_official_outputs_or_controls' `
    -Passed $officialOutputsStillAbsent `
    -Evidence ([pscustomobject][ordered]@{
        legacy_envelope_absent = -not (Test-Path -LiteralPath $legacyEnvelopePath)
        correction_evidence_absent = -not (Test-Path -LiteralPath $correctionEvidencePath)
        materialization_evidence_absent = -not (
            Test-Path -LiteralPath $materializationEvidencePath
        )
        control_manifest_absent = -not (Test-Path -LiteralPath $controlManifestPath)
        control_digest_absent = -not (Test-Path -LiteralPath $controlDigestPath)
    })

$nameSet = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::Ordinal
)
$duplicateTestNames = [System.Collections.Generic.List[string]]::new()
foreach ($result in $results) {
    if ([string]::IsNullOrWhiteSpace([string]$result.name) -or
        -not $nameSet.Add([string]$result.name)) {
        $duplicateTestNames.Add([string]$result.name)
    }
}
$allPassed = (
    $results.Count -gt 0 -and
    $duplicateTestNames.Count -eq 0 -and
    @($results | Where-Object { -not [bool]$_.passed }).Count -eq 0
)
$exactModelHashes = if ($verifierInvocationCount -eq 1 -and
    $directFileHashCount -eq 0 -and $liveModelIdentityPassed) { 1 } else { 0 }
$dependencyEvidence = [pscustomobject][ordered]@{
    epoch_5_static_controls_checked =
        [int]$dependencyCheck.epoch_5_static_controls_checked
    epoch_4_static_controls_checked =
        [int]$dependencyCheck.epoch_4_static_controls_checked
    transitive_epoch_3_controls_checked =
        [int]$dependencyCheck.transitive_epoch_3_controls_checked
    passed = $dependencyPassed
    errors = @($dependencyCheck.errors)
}
$sourceEvidence = [pscustomobject][ordered]@{
    relative_path = [string]$plan.operation.source_raw_relative_path
    manifest_sha256 = [string]$sourceTreeCheck.manifest_sha256
    entries = [int]$sourceTreeCheck.entries
    payload_bytes = [UInt64]$sourceTreeCheck.payload_bytes
    passed = $sourceBindingPassed
    errors = @($sourceTreeCheck.errors)
}
$destinationEvidence = [pscustomobject][ordered]@{
    relative_path = [string]$plan.operation.destination_relative_path
    manifest_sha256 = [string]$destinationTreeCheck.manifest_sha256
    entries = [int]$destinationTreeCheck.entries
    payload_bytes = [UInt64]$destinationTreeCheck.payload_bytes
    report = $destinationReport
    passed = ($destinationBindingPassed -and
        [bool]$destinationReportCheck.passed)
    errors = @($destinationTreeCheck.errors) + @($destinationReportCheck.errors)
}
$frozenVerifierEvidence = [pscustomobject][ordered]@{
    relative_path = [string]$plan.epoch_4.verifier.relative_path
    invocations = $verifierInvocationCount
    direct_get_file_hash_invocations = $directFileHashCount
    live_model_sha256 = if ($null -ne $destinationReport) {
        [string]$destinationReport.live_model_identity.sha256
    }
    else { $null }
    report = $destinationReport
    passed = ($verifierInvocationCount -eq 1 -and
        $directFileHashCount -eq 0 -and $liveModelIdentityPassed -and
        [bool]$destinationReportCheck.passed)
    errors = @($destinationReportCheck.errors) + @($destinationVerificationErrors)
}
$report = [pscustomobject][ordered]@{
    schema = 'animus-ferric-runtime-materialization-self-test-v6'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    execution_epoch = [int]$plan.execution_epoch
    failed_publication_epoch = [int]$plan.failed_publication_epoch
    failed_correction_epoch = [int]$plan.failed_correction_epoch
    materialization_epoch = [int]$plan.materialization_epoch
    timestamp_protocol = [string]$plan.timestamp_protocol
    tested_at_utc = [DateTimeOffset]::UtcNow.ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    passed = $allPassed
    test_count = $results.Count
    exact_model_hashes = $exactModelHashes
    static_controls = @($staticIdentities)
    direct_anchors = @($directAnchorChecks)
    dependency_verification = $dependencyEvidence
    source_verification = $sourceEvidence
    destination_verification = $destinationEvidence
    frozen_epoch_4_verifier = $frozenVerifierEvidence
    duplicate_test_names = @($duplicateTestNames)
    results = @($results)
}

if ((Test-Path -LiteralPath $resultPath) -or
    (Test-Path -LiteralPath $controlManifestPath) -or
    (Test-Path -LiteralPath $controlDigestPath) -or
    (Test-Path -LiteralPath $legacyEnvelopePath) -or
    (Test-Path -LiteralPath $correctionEvidencePath) -or
    (Test-Path -LiteralPath $materializationEvidencePath)) {
    throw 'epoch-6 result, controls, or official evidence appeared; refusing self-test publication'
}
Write-EpochSixJsonAtomic -Path $resultPath -Value $report -Depth 100
$report | ConvertTo-Json -Depth 100
if (-not $allPassed) {
    exit 1
}
