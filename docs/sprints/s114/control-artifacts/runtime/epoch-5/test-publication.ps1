[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$epochFourDir = Join-Path (Split-Path -Parent $artifactDir) 'epoch-4'
. (Join-Path $epochFourDir 'runtime-common.ps1')
. (Join-Path $artifactDir 'publication-common.ps1')

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$resultPath = Join-Path $artifactDir 'publication-self-test.json'
$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'

function Read-EpochFiveJson {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$Path)

    Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -DateKind String
}

function Copy-EpochFiveJsonValue {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Value)

    $Value | ConvertTo-Json -Depth 100 | ConvertFrom-Json -DateKind String
}

function ConvertTo-EpochFiveTreeEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Check)

    [ordered]@{
        passed = [bool]$Check.passed
        manifest_sha256 = [string]$Check.manifest_sha256
        entries = [int]$Check.entries
        payload_bytes = [UInt64]$Check.payload_bytes
        errors = @($Check.errors)
    }
}

function ConvertTo-EpochFiveAnchorEvidence {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$Check)

    [ordered]@{
        label = [string]$Check.label
        relative_path = [string]$Check.relative_path
        bytes = $Check.bytes
        sha256 = [string]$Check.sha256
        passed = [bool]$Check.passed
        errors = @($Check.errors)
    }
}

function ConvertTo-EpochFiveOffsetText {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][int]$OffsetHours
    )

    $instant = [DateTimeOffset]::Parse(
        [string]$Value,
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind
    )
    $instant.ToOffset([TimeSpan]::FromHours($OffsetHours)).ToString('o')
}

function Remove-EpochFiveOwnedDirectory {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedParent
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return $true
    }
    $resolvedPath = [System.IO.Path]::GetFullPath($Path)
    $resolvedParent = [System.IO.Path]::GetFullPath($ExpectedParent)
    if ((Split-Path -Parent $resolvedPath) -cne $resolvedParent -or
        (Split-Path -Leaf $resolvedPath) -cnotmatch '^[0-9a-f]{32}$') {
        throw 'refusing to remove a directory outside the exact owned-test shape'
    }
    $item = Get-Item -LiteralPath $resolvedPath -Force
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw 'refusing to remove an owned-test reparse point'
    }
    [System.IO.Directory]::Delete($resolvedPath, $true)
    -not (Test-Path -LiteralPath $resolvedPath)
}

function Get-EpochFourFrozenSnapshot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$CorrectionPlan,
        [Parameter(Mandatory = $true)]$EpochFourControlManifest
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $identities = [System.Collections.Generic.List[object]]::new()
    $staticNames = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    $staticEntries = @($EpochFourControlManifest.static_controls)
    if ($staticEntries.Count -ne 12) {
        $errors.Add("epoch-4 frozen static set has $($staticEntries.Count) entries, not 12")
    }
    foreach ($entry in $staticEntries) {
        $name = [string]$entry.path
        if (-not $staticNames.Add($name)) {
            $errors.Add("epoch-4 frozen static path is duplicated: $name")
            continue
        }
        $relativePath = (
            [string]$CorrectionPlan.failed_publication_artifact_relative_path
        ).TrimEnd('/', '\') + '/' + $name.Replace('\', '/')
        try {
            $path = Resolve-EpochFiveRepoRelativePath `
                -RepositoryRoot $repoRoot -RelativePath $relativePath
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                $errors.Add("epoch-4 frozen static file is absent: $name")
                continue
            }
            $item = Get-Item -LiteralPath $path -Force
            $sha256 = Get-Sha256Lower -Path $path
            $passed = (-not $item.Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                ) -and
                [UInt64]$item.Length -eq [UInt64]$entry.bytes -and
                $sha256 -ceq [string]$entry.sha256)
            if (-not $passed) {
                $errors.Add("epoch-4 frozen static identity differs: $name")
            }
            $identities.Add([ordered]@{
                path = $relativePath
                bytes = [UInt64]$item.Length
                sha256 = $sha256
                passed = $passed
            })
        }
        catch {
            $errors.Add("epoch-4 frozen static check failed for ${name}: $($_.Exception.Message)")
        }
    }

    foreach ($property in $CorrectionPlan.epoch_4.PSObject.Properties) {
        $anchor = $property.Value
        if (-not (Test-EpochFiveAnchorShape -Anchor $anchor)) {
            continue
        }
        $check = Test-EpochFiveFileAnchor -RepositoryRoot $repoRoot `
            -Anchor $anchor -Label "epoch-4 $($property.Name)"
        if (-not [bool]$check.passed) {
            foreach ($message in @($check.errors)) {
                $errors.Add([string]$message)
            }
        }
        $identities.Add([ordered]@{
            path = [string]$check.relative_path
            bytes = $check.bytes
            sha256 = [string]$check.sha256
            passed = [bool]$check.passed
        })
    }

    $digestPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$CorrectionPlan.epoch_4.control_digest.relative_path)
    $manifestPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
        -RelativePath ([string]$CorrectionPlan.epoch_4.control_manifest.relative_path)
    $digestLine = (Get-Content -Raw -LiteralPath $digestPath).TrimEnd("`r", "`n")
    $manifestSha256 = Get-Sha256Lower -Path $manifestPath
    if ($digestLine -cne
            [string]$CorrectionPlan.epoch_4.control_manifest_digest_line -or
        $digestLine -cne "$manifestSha256  control-inputs.json") {
        $errors.Add('epoch-4 control manifest digest line differs')
    }
    if ([string]$EpochFourControlManifest.schema -cne
            'animus-ferric-runtime-recovery-control-inputs-v4' -or
        [string]$EpochFourControlManifest.task -cne 'T-11409' -or
        -not [bool]$EpochFourControlManifest.runtime_self_test.passed -or
        -not [bool]$EpochFourControlManifest.source_verification.passed -or
        -not [bool]$EpochFourControlManifest.model.passed) {
        $errors.Add('epoch-4 control manifest is not the exact green v4 control set')
    }

    [ordered]@{
        passed = ($errors.Count -eq 0)
        control_manifest_sha256 = $manifestSha256
        control_digest_line = $digestLine
        static_count = $staticEntries.Count
        identities = @($identities | Sort-Object { $_.path }, { $_.sha256 })
        errors = @($errors)
    }
}

if (Test-Path -LiteralPath $resultPath) {
    throw 'publication-self-test.json already exists and will not be overwritten'
}
if ((Test-Path -LiteralPath $controlManifestPath) -or
    (Test-Path -LiteralPath $controlDigestPath)) {
    throw 'epoch-5 publication self-test must run before epoch-5 controls exist'
}

$plan = Read-EpochFiveJson -Path $planPath
$incident = Read-EpochFiveJson -Path $incidentPath
if (-not (Test-EpochFivePlanIdentity -Plan $plan)) {
    throw 'runtime plan is not the exact epoch-5 publication-correction protocol'
}

$epochFourPlanPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.runtime_plan.relative_path)
$sourcePlanPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_3.runtime_plan.relative_path)
$epochFourControlPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.control_manifest.relative_path)
$rawAnchorPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.raw_source_anchor.relative_path)
$verifierPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.verifier.relative_path)
$frozenPublisherPath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.frozen_failed_publisher.relative_path)
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

$recoveryPlan = Read-EpochFiveJson -Path $epochFourPlanPath
$sourcePlan = Read-EpochFiveJson -Path $sourcePlanPath
$epochFourControlManifest = Read-EpochFiveJson -Path $epochFourControlPath
$rawAnchor = Read-EpochFiveJson -Path $rawAnchorPath
$results = [System.Collections.Generic.List[object]]::new()

$anchorDefinitions = @(
    [pscustomobject]@{ label = 'raw manifest'; anchor = $plan.operation.manifest }
    [pscustomobject]@{ label = 'raw attempt'; anchor = $plan.operation.attempt }
    [pscustomobject]@{ label = 'raw attestation'; anchor = $plan.operation.attestation }
    [pscustomobject]@{ label = 'epoch-4 runtime plan'; anchor = $plan.epoch_4.runtime_plan }
    [pscustomobject]@{ label = 'epoch-4 raw source anchor'; anchor = $plan.epoch_4.raw_source_anchor }
    [pscustomobject]@{ label = 'epoch-4 control manifest'; anchor = $plan.epoch_4.control_manifest }
    [pscustomobject]@{ label = 'epoch-4 control digest'; anchor = $plan.epoch_4.control_digest }
    [pscustomobject]@{ label = 'epoch-4 runtime self-test'; anchor = $plan.epoch_4.runtime_self_test }
    [pscustomobject]@{ label = 'epoch-4 verifier'; anchor = $plan.epoch_4.verifier }
    [pscustomobject]@{ label = 'epoch-4 frozen failed publisher'; anchor = $plan.epoch_4.frozen_failed_publisher }
    [pscustomobject]@{ label = 'epoch-3 control manifest'; anchor = $plan.epoch_3.control_manifest }
    [pscustomobject]@{ label = 'epoch-3 control digest'; anchor = $plan.epoch_3.control_digest }
    [pscustomobject]@{ label = 'epoch-3 source runtime plan'; anchor = $plan.epoch_3.runtime_plan }
    [pscustomobject]@{ label = 'epoch-3 runtime self-test'; anchor = $plan.epoch_3.runtime_self_test }
)
$anchorChecks = [System.Collections.Generic.List[object]]::new()
foreach ($definition in $anchorDefinitions) {
    $check = Test-EpochFiveFileAnchor -RepositoryRoot $repoRoot `
        -Anchor $definition.anchor -Label ([string]$definition.label)
    $anchorChecks.Add((ConvertTo-EpochFiveAnchorEvidence -Check $check))
}
$results.Add([ordered]@{
    name = 'epoch_5_plan_and_dependency_anchors_are_exact'
    passed = (@($anchorChecks | Where-Object { -not [bool]$_.passed }).Count -eq 0)
    evidence = [ordered]@{
        plan_identity = $true
        anchor_count = $anchorChecks.Count
        anchors = @($anchorChecks)
    }
})

$frozenDependencyRewalk = Test-EpochFourFrozenDependencySet `
    -RepositoryRoot $repoRoot -EpochFivePlan $plan `
    -ExpectedHead ([string]$plan.repository_commit_before_epoch_5_controls)
$results.Add([ordered]@{
    name = 'shared_dependency_rewalk_checks_all_frozen_epoch_4_and_epoch_3_controls'
    passed = ([bool]$frozenDependencyRewalk.passed -and
        [int]$frozenDependencyRewalk.static_controls_checked -eq 12 -and
        [int]$frozenDependencyRewalk.transitive_epoch_3_controls_checked -eq 20)
    evidence = [ordered]@{
        static_controls_checked =
            [int]$frozenDependencyRewalk.static_controls_checked
        transitive_epoch_3_controls_checked =
            [int]$frozenDependencyRewalk.transitive_epoch_3_controls_checked
        errors = @($frozenDependencyRewalk.errors)
    }
})

$wrongPlanOperation = Copy-EpochFiveJsonValue -Value $plan
$wrongPlanOperation.operation.id = 'r05-cross-operation'
$wrongPlanProtocol = Copy-EpochFiveJsonValue -Value $plan
$wrongPlanProtocol.timestamp_protocol = 'powershell-json-implicit-local-v0'
$missingPlanAnchor = Copy-EpochFiveJsonValue -Value $plan
$missingPlanAnchor.epoch_3.PSObject.Properties.Remove('control_manifest')
$results.Add([ordered]@{
    name = 'plan_identity_rejects_cross_operation_protocol_and_missing_anchor_mutations'
    passed = (-not (Test-EpochFivePlanIdentity -Plan $wrongPlanOperation) -and
        -not (Test-EpochFivePlanIdentity -Plan $wrongPlanProtocol) -and
        -not (Test-EpochFivePlanIdentity -Plan $missingPlanAnchor))
    evidence = [ordered]@{
        cross_operation_rejected = -not (
            Test-EpochFivePlanIdentity -Plan $wrongPlanOperation
        )
        wrong_protocol_rejected = -not (
            Test-EpochFivePlanIdentity -Plan $wrongPlanProtocol
        )
        missing_epoch_3_anchor_rejected = -not (
            Test-EpochFivePlanIdentity -Plan $missingPlanAnchor
        )
    }
})

$epochThreeAnchorRolesPassed = $true
foreach ($name in @(
        'control_manifest',
        'control_digest',
        'runtime_plan',
        'runtime_self_test'
    )) {
    $epochFiveAnchor = $plan.epoch_3.$name
    $epochFourAnchor = $recoveryPlan.epoch_3.$name
    if ([string]$epochFiveAnchor.relative_path -cne
            [string]$epochFourAnchor.relative_path -or
        [UInt64]$epochFiveAnchor.bytes -ne [UInt64]$epochFourAnchor.bytes -or
        [string]$epochFiveAnchor.sha256 -cne
            [string]$epochFourAnchor.sha256) {
        $epochThreeAnchorRolesPassed = $false
    }
}
if ([string]$plan.epoch_3.control_manifest_digest_line -cne
    [string]$recoveryPlan.epoch_3.control_manifest_digest_line) {
    $epochThreeAnchorRolesPassed = $false
}
$planRolesPassed = (
    (Test-RecoveryPlanIdentity -Plan $recoveryPlan) -and
    (Test-RuntimePlanIdentity -Plan $sourcePlan) -and
    $epochThreeAnchorRolesPassed -and
    [string]$recoveryPlan.operation.id -ceq
        [string]$plan.operation.failed_operation_id -and
    [string]$recoveryPlan.operation.coordinate -ceq
        [string]$plan.operation.coordinate -and
    [string]$recoveryPlan.operation.source_attempt_schema -ceq
        [string]$plan.operation.source_attempt_schema -and
    $null -eq $recoveryPlan.PSObject.Properties['template_attestation'] -and
    $null -eq $recoveryPlan.PSObject.Properties['process_command_attestation'] -and
    $null -ne $sourcePlan.PSObject.Properties['template_attestation'] -and
    $null -ne $sourcePlan.PSObject.Properties['process_command_attestation'] -and
    -not [string]::IsNullOrWhiteSpace(
        [string]$sourcePlan.template_attestation.protocol
    ) -and
    -not [string]::IsNullOrWhiteSpace(
        [string]$sourcePlan.process_command_attestation.protocol
    )
)
$results.Add([ordered]@{
    name = 'recovery_and_source_plan_roles_are_explicitly_separated'
    passed = $planRolesPassed
    evidence = [ordered]@{
        recovery_schema = [string]$recoveryPlan.schema
        recovery_operation_id = [string]$recoveryPlan.operation.id
        recovery_has_source_protocol_objects = (
            $null -ne $recoveryPlan.PSObject.Properties['template_attestation'] -or
            $null -ne $recoveryPlan.PSObject.Properties['process_command_attestation']
        )
        source_schema = [string]$sourcePlan.schema
        epoch_3_anchors_match_recovery_plan = $epochThreeAnchorRolesPassed
        source_template_protocol = [string]$sourcePlan.template_attestation.protocol
        source_process_protocol = [string]$sourcePlan.process_command_attestation.protocol
    }
})

$rawAnchorMatchesPlan = (
    [string]$rawAnchor.operation_id -ceq
        [string]$plan.operation.failed_operation_id -and
    [string]$rawAnchor.source_relative_path -ceq
        [string]$plan.operation.source_raw_relative_path -and
    [string]$rawAnchor.destination_relative_path -ceq
        [string]$plan.operation.destination_relative_path -and
    [string]$rawAnchor.manifest.path -ceq 'files.sha256' -and
    [UInt64]$rawAnchor.manifest.bytes -eq
        [UInt64]$plan.operation.manifest.bytes -and
    [string]$rawAnchor.manifest.sha256 -ceq
        [string]$plan.operation.manifest.sha256 -and
    [int]$rawAnchor.manifest.entry_count -eq
        [int]$plan.operation.exact_manifest_entries -and
    [UInt64]$rawAnchor.selected.attempt.bytes -eq
        [UInt64]$plan.operation.attempt.bytes -and
    [string]$rawAnchor.selected.attempt.sha256 -ceq
        [string]$plan.operation.attempt.sha256 -and
    [UInt64]$rawAnchor.selected.attestation.bytes -eq
        [UInt64]$plan.operation.attestation.bytes -and
    [string]$rawAnchor.selected.attestation.sha256 -ceq
        [string]$plan.operation.attestation.sha256 -and
    @($rawAnchor.files).Count -eq [int]$plan.operation.exact_manifest_entries
)
$sourceTreeCheck = Test-EpochFiveExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
$results.Add([ordered]@{
    name = 'raw_source_anchor_and_exact_tree_match_the_correction_plan'
    passed = ($rawAnchorMatchesPlan -and [bool]$sourceTreeCheck.passed)
    evidence = [ordered]@{
        anchor_matches_plan = $rawAnchorMatchesPlan
        tree = ConvertTo-EpochFiveTreeEvidence -Check $sourceTreeCheck
    }
})

$epochFourBefore = Get-EpochFourFrozenSnapshot `
    -CorrectionPlan $plan -EpochFourControlManifest $epochFourControlManifest
$results.Add([ordered]@{
    name = 'epoch_4_static_and_control_hashes_are_frozen_before_testing'
    passed = [bool]$epochFourBefore.passed
    evidence = [ordered]@{
        control_manifest_sha256 = [string]$epochFourBefore.control_manifest_sha256
        control_digest_line = [string]$epochFourBefore.control_digest_line
        static_count = [int]$epochFourBefore.static_count
        errors = @($epochFourBefore.errors)
    }
})

$destinationStatePassed = (
    -not (Test-Path -LiteralPath $destinationRoot) -and
    -not (Test-Path -LiteralPath $legacyEnvelopePath) -and
    -not (Test-Path -LiteralPath $correctionEvidencePath)
)
$results.Add([ordered]@{
    name = 'destination_and_both_publication_envelopes_are_absent'
    passed = $destinationStatePassed
    evidence = [ordered]@{
        destination_absent = -not (Test-Path -LiteralPath $destinationRoot)
        legacy_epoch_4_envelope_absent = -not (Test-Path -LiteralPath $legacyEnvelopePath)
        epoch_5_correction_evidence_absent = -not (Test-Path -LiteralPath $correctionEvidencePath)
    }
})

$freshState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$destinationResumeState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$legacyResumeState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$true `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$completeState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$true `
    -CorrectionEvidenceExists:$true -CorrectionEvidenceExact:$true
$legacyOnlyState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$true `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$correctionOnlyState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$true -CorrectionEvidenceExact:$true
$differingDestinationState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$false `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$differingLegacyState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$differingCorrectionState = Test-EpochFivePublicationState `
    -DestinationExists:$true -DestinationExact:$true `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$true `
    -CorrectionEvidenceExists:$true -CorrectionEvidenceExact:$false
$nonLeafLegacyState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$reparseLegacyState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$true -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$false -CorrectionEvidenceExact:$false
$nonLeafCorrectionState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$true -CorrectionEvidenceExact:$false
$reparseCorrectionState = Test-EpochFivePublicationState `
    -DestinationExists:$false -DestinationExact:$false `
    -LegacyEnvelopeExists:$false -LegacyEnvelopeExact:$false `
    -CorrectionEvidenceExists:$true -CorrectionEvidenceExact:$false
$results.Add([ordered]@{
    name = 'shared_publication_state_machine_covers_fresh_resume_and_invalid_partial_states'
    passed = (
        [bool]$freshState.passed -and
        [string]$freshState.action -ceq 'publish_fresh' -and
        [bool]$destinationResumeState.passed -and
        [string]$destinationResumeState.action -ceq
            'resume_exact_destination' -and
        [bool]$legacyResumeState.passed -and
        [string]$legacyResumeState.action -ceq
            'complete_correction_evidence' -and
        [bool]$completeState.passed -and
        [string]$completeState.action -ceq 'already_complete' -and
        -not [bool]$legacyOnlyState.passed -and
        -not [bool]$correctionOnlyState.passed -and
        -not [bool]$differingDestinationState.passed -and
        -not [bool]$differingLegacyState.passed -and
        -not [bool]$differingCorrectionState.passed
    )
    evidence = [ordered]@{
        fresh_action = [string]$freshState.action
        exact_destination_resume_action = [string]$destinationResumeState.action
        exact_legacy_resume_action = [string]$legacyResumeState.action
        already_complete_action = [string]$completeState.action
        legacy_only_errors = @($legacyOnlyState.errors)
        correction_without_legacy_errors = @($correctionOnlyState.errors)
        differing_destination_errors = @($differingDestinationState.errors)
        differing_legacy_errors = @($differingLegacyState.errors)
        differing_correction_errors = @($differingCorrectionState.errors)
    }
})
$results.Add([ordered]@{
    name = 'present_nonleaf_or_reparse_evidence_is_never_treated_as_absent'
    passed = (
        -not [bool]$nonLeafLegacyState.passed -and
        -not [bool]$reparseLegacyState.passed -and
        -not [bool]$nonLeafCorrectionState.passed -and
        -not [bool]$reparseCorrectionState.passed -and
        @($nonLeafLegacyState.errors | Where-Object {
                [string]$_ -match 'without its destination|not exact'
            }).Count -gt 0 -and
        @($nonLeafCorrectionState.errors | Where-Object {
                [string]$_ -match 'without its destination|without its legacy|not exact'
            }).Count -gt 0
    )
    evidence = [ordered]@{
        production_mapping =
            'present=true, exact=false for non-leaf or reparse evidence paths'
        nonleaf_legacy_errors = @($nonLeafLegacyState.errors)
        reparse_legacy_errors = @($reparseLegacyState.errors)
        nonleaf_correction_errors = @($nonLeafCorrectionState.errors)
        reparse_correction_errors = @($reparseCorrectionState.errors)
    }
})

$staticNames = @(Get-EpochFiveStaticControlNames)
$staticNameSet = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
$staticIdentities = [System.Collections.Generic.List[object]]::new()
$duplicateStaticNames = [System.Collections.Generic.List[string]]::new()
foreach ($name in $staticNames) {
    if (-not $staticNameSet.Add([string]$name)) {
        $duplicateStaticNames.Add([string]$name)
        continue
    }
    $path = Join-Path $artifactDir ([string]$name)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "epoch-5 static file is absent: $name"
    }
    $item = Get-Item -LiteralPath $path -Force
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "epoch-5 static file is a reparse point: $name"
    }
    $staticIdentities.Add([ordered]@{
        path = [string]$name
        bytes = [UInt64]$item.Length
        sha256 = Get-Sha256Lower -Path $path
    })
}
$staticIdentityPassed = (
    $staticNames.Count -eq 8 -and
    $staticIdentities.Count -eq 8 -and
    $duplicateStaticNames.Count -eq 0
)
$results.Add([ordered]@{
    name = 'epoch_5_static_control_identities_are_exact_ordered_and_unique'
    passed = $staticIdentityPassed
    evidence = [ordered]@{
        expected_count = 8
        observed_count = $staticIdentities.Count
        duplicate_names = @($duplicateStaticNames)
        names = @($staticNames)
    }
})

$parseTargets = [System.Collections.Generic.List[object]]::new()
foreach ($name in $staticNames | Where-Object { $_ -like '*.ps1' }) {
    $parseTargets.Add([ordered]@{
        relative_path = ([string]$plan.correction_artifact_relative_path).TrimEnd('/', '\') + '/' + $name
        path = Join-Path $artifactDir $name
    })
}
$parseTargets.Add([ordered]@{
    relative_path = [string]$plan.epoch_4.verifier.relative_path
    path = $verifierPath
})
$parseTargets.Add([ordered]@{
    relative_path = [string]$plan.epoch_4.frozen_failed_publisher.relative_path
    path = $frozenPublisherPath
})
$parseEvidence = [System.Collections.Generic.List[object]]::new()
$allParsersPassed = $true
foreach ($target in $parseTargets) {
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile(
        [string]$target.path,
        [ref]$tokens,
        [ref]$parseErrors
    )
    $passed = @($parseErrors).Count -eq 0
    if (-not $passed) { $allParsersPassed = $false }
    $parseEvidence.Add([ordered]@{
        relative_path = [string]$target.relative_path
        passed = $passed
        errors = @($parseErrors | ForEach-Object { [string]$_.Message })
    })
}
$results.Add([ordered]@{
    name = 'all_epoch_5_and_invoked_frozen_powershell_files_parse'
    passed = $allParsersPassed
    evidence = [ordered]@{
        files = @($parseEvidence)
    }
})

$incidentIdentityPassed = (
    [string]$incident.schema -ceq
        'animus-ferric-runtime-publication-incident-v5' -and
    [string]$incident.task -ceq 'T-11409' -and
    [string]$incident.operation_id -ceq [string]$plan.operation.id -and
    [string]$incident.failed_operation_id -ceq
        [string]$plan.operation.failed_operation_id -and
    [string]$incident.failure.script_relative_path -ceq
        [string]$plan.epoch_4.frozen_failed_publisher.relative_path -and
    [UInt64]$incident.failure.script_bytes -eq
        [UInt64]$plan.epoch_4.frozen_failed_publisher.bytes -and
    [string]$incident.failure.script_sha256 -ceq
        [string]$plan.epoch_4.frozen_failed_publisher.sha256 -and
    [bool]$incident.resolution.preserve_epoch_4_immutable -and
    [bool]$incident.resolution.bind_source_protocols_from_epoch_3_plan -and
    [bool]$incident.resolution.publish_legacy_epoch_4_envelope
)
$frozenPublisherText = Get-Content -Raw -LiteralPath $frozenPublisherPath
$correctedCommonText = Get-Content -Raw -LiteralPath (
    Join-Path $artifactDir 'publication-common.ps1'
)
$correctedPublisherPath = Join-Path $artifactDir 'publish-e04-correction.ps1'
$correctedPublisherText = Get-Content -Raw -LiteralPath $correctedPublisherPath
$incorrectFragments = @(
    ([string]$incident.failure.incorrect_binding -split ' and ')
)
$requiredFragments = @(
    ([string]$incident.failure.required_binding -split ' and ')
)
$frozenContainsIncorrect = @($incorrectFragments | Where-Object {
        $frozenPublisherText.IndexOf(
            [string]$_,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -lt 0
    }).Count -eq 0
$correctedContainsRequired = @($requiredFragments | Where-Object {
        $correctedCommonText.IndexOf(
            [string]$_,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -lt 0
    }).Count -eq 0
$correctedPublisherAvoidsIncorrect = @($incorrectFragments | Where-Object {
        $correctedPublisherText.IndexOf(
            [string]$_,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -ge 0
    }).Count -eq 0
$legacyPresenceIndex = $correctedPublisherText.IndexOf(
    '$legacyExists = Test-Path -LiteralPath $legacyEnvelopePath',
    [System.StringComparison]::Ordinal
)
$correctionPresenceIndex = $correctedPublisherText.IndexOf(
    '$correctionExists = Test-Path -LiteralPath $correctionEvidencePath',
    [System.StringComparison]::Ordinal
)
$evidenceGuardIndex = $correctedPublisherText.IndexOf(
    'foreach ($evidencePath in @(',
    [System.StringComparison]::Ordinal
)
$evidenceLeafGuardIndex = $correctedPublisherText.IndexOf(
    'Test-Path -LiteralPath $evidencePath.path -PathType Leaf',
    [System.StringComparison]::Ordinal
)
$stateHelperIndex = $correctedPublisherText.IndexOf(
    '$publicationState = Test-EpochFivePublicationState',
    [System.StringComparison]::Ordinal
)
$destinationMoveIndex = $correctedPublisherText.IndexOf(
    '[System.IO.Directory]::Move($stageRoot, $destinationRoot)',
    [System.StringComparison]::Ordinal
)
$evidenceReparseGuardIndex = if ($evidenceGuardIndex -ge 0) {
    $correctedPublisherText.IndexOf(
        '[System.IO.FileAttributes]::ReparsePoint',
        $evidenceGuardIndex,
        [System.StringComparison]::Ordinal
    )
}
else { -1 }
$productionPathRepresentationPassed = (
    $legacyPresenceIndex -ge 0 -and
    $correctionPresenceIndex -ge 0 -and
    $evidenceGuardIndex -gt $legacyPresenceIndex -and
    $evidenceLeafGuardIndex -gt $evidenceGuardIndex -and
    $evidenceReparseGuardIndex -gt $evidenceGuardIndex -and
    $stateHelperIndex -gt $evidenceLeafGuardIndex -and
    $destinationMoveIndex -gt $stateHelperIndex
)
$results.Add([ordered]@{
    name = 'frozen_publisher_bug_is_preserved_only_as_incident_evidence'
    passed = ($incidentIdentityPassed -and $frozenContainsIncorrect -and
        $correctedContainsRequired -and $correctedPublisherAvoidsIncorrect)
    evidence = [ordered]@{
        incident_identity_passed = $incidentIdentityPassed
        frozen_publisher_contains_recorded_incorrect_binding = $frozenContainsIncorrect
        corrected_common_contains_source_plan_binding = $correctedContainsRequired
        corrected_publisher_avoids_recorded_incorrect_binding = $correctedPublisherAvoidsIncorrect
    }
})
$results.Add([ordered]@{
    name = 'publisher_maps_present_nonleaf_and_reparse_evidence_before_any_destination_move'
    passed = $productionPathRepresentationPassed
    evidence = [ordered]@{
        plain_legacy_presence_check = ($legacyPresenceIndex -ge 0)
        plain_correction_presence_check = ($correctionPresenceIndex -ge 0)
        leaf_and_reparse_guard_precedes_state_decision = (
            $evidenceGuardIndex -ge 0 -and
            $evidenceLeafGuardIndex -gt $evidenceGuardIndex -and
            $stateHelperIndex -gt $evidenceLeafGuardIndex
        )
        state_decision_precedes_destination_move = (
            $stateHelperIndex -ge 0 -and
            $destinationMoveIndex -gt $stateHelperIndex
        )
    }
})

$selfTestParent = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath 'target/s114-experiment/runtime-epoch-5-selftest'
[System.IO.Directory]::CreateDirectory($selfTestParent) | Out-Null
$selfTestOwner = Join-Path $selfTestParent ([guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($selfTestOwner) | Out-Null
$selfTestCleanupPassed = $false
try {
    function Copy-EpochFiveCase {
        [CmdletBinding()]
        param([Parameter(Mandatory = $true)][string]$Name)

        $casePath = Join-Path $selfTestOwner $Name
        Copy-Item -LiteralPath $sourceRoot -Destination $casePath -Recurse
        $casePath
    }

    $attemptRewritePath = Copy-EpochFiveCase -Name 'attempt-rewrite'
    $attemptRewriteFile = Join-Path $attemptRewritePath 'attempt.json'
    $attemptRewrite = Read-EpochFiveJson -Path $attemptRewriteFile
    $attemptRewrite.started_at_utc = ConvertTo-EpochFiveOffsetText `
        -Value $attemptRewrite.started_at_utc -OffsetHours -4
    Write-JsonLf -Path $attemptRewriteFile -Value $attemptRewrite -Depth 100
    Write-HashManifest -Root $attemptRewritePath `
        -OutputPath (Join-Path $attemptRewritePath 'files.sha256')
    $attemptRewriteCheck = Test-EpochFiveExactTree -Root $attemptRewritePath `
        -ManifestAnchor $rawAnchor `
        -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)

    $attestationRewritePath = Copy-EpochFiveCase -Name 'attestation-rewrite'
    $attestationRewriteFile = Join-Path $attestationRewritePath 'attestation.json'
    $attestationAttemptFile = Join-Path $attestationRewritePath 'attempt.json'
    $attestationRewrite = Read-EpochFiveJson -Path $attestationRewriteFile
    $attestationAttempt = Read-EpochFiveJson -Path $attestationAttemptFile
    $offsetCaptured = ConvertTo-EpochFiveOffsetText `
        -Value $attestationRewrite.captured_at_utc -OffsetHours -4
    $attestationRewrite.captured_at_utc = $offsetCaptured
    $attestationAttempt.attestation.captured_at_utc = $offsetCaptured
    Write-JsonLf -Path $attestationRewriteFile -Value $attestationRewrite -Depth 100
    Write-JsonLf -Path $attestationAttemptFile -Value $attestationAttempt -Depth 100
    Write-HashManifest -Root $attestationRewritePath `
        -OutputPath (Join-Path $attestationRewritePath 'files.sha256')
    $attestationRewriteCheck = Test-EpochFiveExactTree `
        -Root $attestationRewritePath -ManifestAnchor $rawAnchor `
        -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)

    $extraFilePath = Copy-EpochFiveCase -Name 'extra-file'
    Write-Utf8Lf -Path (Join-Path $extraFilePath 'unlisted-evidence.txt') `
        -Text "unlisted`n"
    $extraFileCheck = Test-EpochFiveExactTree -Root $extraFilePath `
        -ManifestAnchor $rawAnchor `
        -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)

    $results.Add([ordered]@{
        name = 'exact_tree_rejects_self_consistent_attempt_rewrite'
        passed = (-not [bool]$attemptRewriteCheck.passed -and
            @($attemptRewriteCheck.errors | Where-Object {
                    [string]$_ -match 'manifest differs|file differs'
                }).Count -gt 0)
        evidence = ConvertTo-EpochFiveTreeEvidence -Check $attemptRewriteCheck
    })
    $results.Add([ordered]@{
        name = 'exact_tree_rejects_self_consistent_attestation_rewrite'
        passed = (-not [bool]$attestationRewriteCheck.passed -and
            @($attestationRewriteCheck.errors | Where-Object {
                    [string]$_ -match 'manifest differs|file differs'
                }).Count -gt 0)
        evidence = ConvertTo-EpochFiveTreeEvidence -Check $attestationRewriteCheck
    })
    $results.Add([ordered]@{
        name = 'exact_tree_rejects_an_unlisted_extra_file'
        passed = (-not [bool]$extraFileCheck.passed -and
            @($extraFileCheck.errors | Where-Object {
                    [string]$_ -match 'unlisted|file count'
                }).Count -gt 0)
        evidence = ConvertTo-EpochFiveTreeEvidence -Check $extraFileCheck
    })
}
finally {
    $selfTestCleanupPassed = Remove-EpochFiveOwnedDirectory `
        -Path $selfTestOwner -ExpectedParent $selfTestParent
}
$results.Add([ordered]@{
    name = 'adversarial_exact_tree_workspace_was_cleaned'
    passed = $selfTestCleanupPassed
    evidence = [ordered]@{ owner_absent = $selfTestCleanupPassed }
})

$stageParent = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath 'target/s114-experiment/recovery-stage'
[System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
$stageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
$stageRoot = Join-Path $stageOwner ([string]$plan.operation.coordinate)
$validStagePolicy = Test-EpochFiveStagePathPolicy -RepositoryRoot $repoRoot `
    -StageRoot $stageRoot -Coordinate ([string]$plan.operation.coordinate)
$arbitraryStage = Join-Path $repoRoot (
    'target/s114-experiment/arbitrary/' +
    [guid]::NewGuid().ToString('N') + '/' +
    [string]$plan.operation.coordinate
)
$wrongOwnerStage = Join-Path $stageParent (
    'ABCDEFABCDEFABCDEFABCDEFABCDEFAB/' +
    [string]$plan.operation.coordinate
)
$wrongLeafStage = Join-Path (
    Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
) 'e03-99-q4-32768'
$arbitraryStagePolicy = Test-EpochFiveStagePathPolicy `
    -RepositoryRoot $repoRoot -StageRoot $arbitraryStage `
    -Coordinate ([string]$plan.operation.coordinate)
$wrongOwnerPolicy = Test-EpochFiveStagePathPolicy `
    -RepositoryRoot $repoRoot -StageRoot $wrongOwnerStage `
    -Coordinate ([string]$plan.operation.coordinate)
$wrongLeafPolicy = Test-EpochFiveStagePathPolicy `
    -RepositoryRoot $repoRoot -StageRoot $wrongLeafStage `
    -Coordinate ([string]$plan.operation.coordinate)
$results.Add([ordered]@{
    name = 'stage_policy_accepts_only_the_exact_owned_coordinate_shape'
    passed = ([bool]$validStagePolicy.passed -and
        -not [bool]$arbitraryStagePolicy.passed -and
        -not [bool]$wrongOwnerPolicy.passed -and
        -not [bool]$wrongLeafPolicy.passed)
    evidence = [ordered]@{
        valid_errors = @($validStagePolicy.errors)
        arbitrary_errors = @($arbitraryStagePolicy.errors)
        wrong_owner_errors = @($wrongOwnerPolicy.errors)
        wrong_leaf_errors = @($wrongLeafPolicy.errors)
    }
})

$stageReport = $null
$stageTreeCheck = $null
$stageInvocationError = $null
$stageCleanupPassed = $false
[System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
try {
    Copy-Item -LiteralPath $sourceRoot -Destination $stageRoot -Recurse
    $stageTreeCheck = Test-EpochFiveExactTree -Root $stageRoot `
        -ManifestAnchor $rawAnchor `
        -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
    if (-not [bool]$stageTreeCheck.passed) {
        throw "valid owned stage failed exact-tree verification: $(@($stageTreeCheck.errors) -join '; ')"
    }
    $stageReport = Invoke-EpochFourVerification -VerifierPath $verifierPath `
        -AttemptPath $stageRoot -RecoveryPlan $recoveryPlan `
        -SourcePlan $sourcePlan -RecoveryPublicationStage
}
catch {
    $stageInvocationError = $_.Exception.Message
}
finally {
    $stageCleanupPassed = Remove-EpochFiveOwnedDirectory `
        -Path $stageOwner -ExpectedParent $stageParent
}

$stageReportValidation = if ($null -ne $stageReport) {
    Test-EpochFourVerificationReport -Report $stageReport `
        -RecoveryPlan $recoveryPlan -SourcePlan $sourcePlan `
        -ExpectedAttemptPath $stageRoot `
        -ExpectedAnchorMode 'epoch_4_frozen_publication_stage'
}
else {
    [ordered]@{
        passed = $false
        errors = @('shared epoch-4 verifier wrapper did not return a report')
    }
}
$results.Add([ordered]@{
    name = 'owned_stage_uses_the_shared_epoch_4_verifier_wrapper_once'
    passed = ($null -ne $stageReport -and
        [bool]$stageReportValidation.passed -and
        [bool]$stageTreeCheck.passed -and
        $stageCleanupPassed -and
        [string]::IsNullOrWhiteSpace([string]$stageInvocationError))
    evidence = [ordered]@{
        exact_tree = if ($null -ne $stageTreeCheck) {
            ConvertTo-EpochFiveTreeEvidence -Check $stageTreeCheck
        }
        else { $null }
        report_validation_errors = @($stageReportValidation.errors)
        invocation_error = $stageInvocationError
        owned_stage_cleaned = $stageCleanupPassed
    }
})

if ($null -ne $stageReport) {
    $missingFields = Copy-EpochFiveJsonValue -Value $stageReport
    $missingFields.PSObject.Properties.Remove('operation_id')
    $missingFields.PSObject.Properties.Remove('source_attempt_schema')
    $missingValidation = Test-EpochFourVerificationReport `
        -Report $missingFields -RecoveryPlan $recoveryPlan `
        -SourcePlan $sourcePlan -ExpectedAttemptPath $stageRoot `
        -ExpectedAnchorMode 'epoch_4_frozen_publication_stage'

    $crossOperation = Copy-EpochFiveJsonValue -Value $stageReport
    $crossOperation.operation_id = [string]$plan.operation.id
    $crossOperation.coordinate = 'e03-99-q4-32768'
    $crossValidation = Test-EpochFourVerificationReport `
        -Report $crossOperation -RecoveryPlan $recoveryPlan `
        -SourcePlan $sourcePlan -ExpectedAttemptPath $stageRoot `
        -ExpectedAnchorMode 'epoch_4_frozen_publication_stage'

    $wrongProtocols = Copy-EpochFiveJsonValue -Value $stageReport
    $wrongProtocols.timestamp_protocol = 'powershell-json-implicit-local-v0'
    $wrongProtocols.attestation_protocol = 'wrong-template-protocol'
    $wrongProtocols.process_command_protocol = 'wrong-process-protocol'
    $protocolValidation = Test-EpochFourVerificationReport `
        -Report $wrongProtocols -RecoveryPlan $recoveryPlan `
        -SourcePlan $sourcePlan -ExpectedAttemptPath $stageRoot `
        -ExpectedAnchorMode 'epoch_4_frozen_publication_stage'

    $wrongMode = Copy-EpochFiveJsonValue -Value $stageReport
    $wrongMode.control_anchor_mode = 'epoch_4_frozen_recovery'
    $modeValidation = Test-EpochFourVerificationReport `
        -Report $wrongMode -RecoveryPlan $recoveryPlan `
        -SourcePlan $sourcePlan -ExpectedAttemptPath $stageRoot `
        -ExpectedAnchorMode 'epoch_4_frozen_publication_stage'
}
else {
    $missingValidation = [ordered]@{ passed = $true; errors = @('no baseline report') }
    $crossValidation = [ordered]@{ passed = $true; errors = @('no baseline report') }
    $protocolValidation = [ordered]@{ passed = $true; errors = @('no baseline report') }
    $modeValidation = [ordered]@{ passed = $true; errors = @('no baseline report') }
}
$results.Add([ordered]@{
    name = 'pure_report_validator_rejects_missing_required_fields'
    passed = ($null -ne $stageReport -and -not [bool]$missingValidation.passed)
    evidence = [ordered]@{ errors = @($missingValidation.errors) }
})
$results.Add([ordered]@{
    name = 'pure_report_validator_rejects_cross_operation_fields'
    passed = ($null -ne $stageReport -and -not [bool]$crossValidation.passed)
    evidence = [ordered]@{ errors = @($crossValidation.errors) }
})
$results.Add([ordered]@{
    name = 'pure_report_validator_rejects_wrong_protocols'
    passed = ($null -ne $stageReport -and -not [bool]$protocolValidation.passed)
    evidence = [ordered]@{ errors = @($protocolValidation.errors) }
})
$results.Add([ordered]@{
    name = 'pure_report_validator_rejects_wrong_anchor_mode'
    passed = ($null -ne $stageReport -and -not [bool]$modeValidation.passed)
    evidence = [ordered]@{ errors = @($modeValidation.errors) }
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
$wrapperInvocationCount = @($selfCommands | Where-Object {
        $_ -ceq 'Invoke-EpochFourVerification'
    }).Count
$directFileHashCount = @($selfCommands | Where-Object {
        $_ -ceq 'Get-FileHash'
    }).Count
$modelPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.model.relative_path)
$modelItem = Get-Item -LiteralPath $modelPath -Force
$modelByteIdentityPassed = (
    -not $modelItem.Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    ) -and
    [UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes
)
$liveModelIdentity = [ordered]@{
    relative_path = [string]$plan.model.relative_path
    bytes = [UInt64]$modelItem.Length
    sha256 = if ($null -ne $stageReport) {
        [string]$stageReport.live_model_identity.sha256
    }
    else { $null }
    checked = ($null -ne $stageReport -and
        [bool]$stageReport.live_model_identity.checked)
    passed = ($modelByteIdentityPassed -and $null -ne $stageReport -and
        [bool]$stageReport.live_model_identity.checked -and
        [string]$stageReport.live_model_identity.mode -ceq
            'checked_in_verifier' -and
        [string]$stageReport.live_model_identity.sha256 -ceq
            [string]$plan.model.sha256)
}
$results.Add([ordered]@{
    name = 'single_live_model_hash_is_delegated_to_the_one_shared_verifier_call'
    passed = ([bool]$liveModelIdentity.passed -and
        $wrapperInvocationCount -eq 1 -and $directFileHashCount -eq 0)
    evidence = [ordered]@{
        verifier_wrapper_invocations_in_self_test = $wrapperInvocationCount
        direct_get_file_hash_invocations_in_self_test = $directFileHashCount
        verifier_hash_mode = if ($null -ne $stageReport) {
            [string]$stageReport.live_model_identity.mode
        }
        else { $null }
    }
})

$epochFourAfter = Get-EpochFourFrozenSnapshot `
    -CorrectionPlan $plan -EpochFourControlManifest $epochFourControlManifest
$epochFourUnchanged = (
    [bool]$epochFourBefore.passed -and
    [bool]$epochFourAfter.passed -and
    (Test-JsonEquivalent -Left $epochFourBefore.identities `
        -Right $epochFourAfter.identities) -and
    [string]$epochFourBefore.control_manifest_sha256 -ceq
        [string]$epochFourAfter.control_manifest_sha256 -and
    [string]$epochFourBefore.control_digest_line -ceq
        [string]$epochFourAfter.control_digest_line
)
$results.Add([ordered]@{
    name = 'epoch_4_static_and_control_hashes_remain_unchanged_after_testing'
    passed = $epochFourUnchanged
    evidence = [ordered]@{
        before_passed = [bool]$epochFourBefore.passed
        after_passed = [bool]$epochFourAfter.passed
        before_errors = @($epochFourBefore.errors)
        after_errors = @($epochFourAfter.errors)
        control_manifest_sha256 = [string]$epochFourAfter.control_manifest_sha256
    }
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
$stageVerificationEvidence = if ($null -ne $stageReport) {
    [ordered]@{
        schema = [string]$stageReport.schema
        operation_id = [string]$stageReport.operation_id
        execution_epoch = [int]$stageReport.execution_epoch
        publication_epoch = [int]$stageReport.publication_epoch
        timestamp_protocol = [string]$stageReport.timestamp_protocol
        attestation_protocol = [string]$stageReport.attestation_protocol
        process_command_protocol = [string]$stageReport.process_command_protocol
        control_anchor_mode = [string]$stageReport.control_anchor_mode
        coordinate = [string]$stageReport.coordinate
        verdict = [string]$stageReport.verdict
        live_model_identity = $stageReport.live_model_identity
        manifest = $stageReport.manifest
        recovery_anchor = $stageReport.recovery_anchor
        passed = [bool]$stageReport.passed
        errors = @($stageReport.errors)
    }
}
else {
    [ordered]@{
        passed = $false
        errors = @($stageInvocationError)
    }
}
$report = [ordered]@{
    schema = 'animus-ferric-runtime-publication-correction-self-test-v5'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    execution_epoch = [int]$plan.execution_epoch
    failed_publication_epoch = [int]$plan.failed_publication_epoch
    correction_epoch = [int]$plan.correction_epoch
    timestamp_protocol = [string]$plan.timestamp_protocol
    generated_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    passed = $allPassed
    test_count = $results.Count
    duplicate_test_names = @($duplicateTestNames)
    static_controls = @($staticIdentities)
    plan_dependency_anchors = @($anchorChecks)
    stage_verification = $stageVerificationEvidence
    live_q4_identity = $liveModelIdentity
    results = @($results)
}

if ((Test-Path -LiteralPath $resultPath) -or
    (Test-Path -LiteralPath $controlManifestPath) -or
    (Test-Path -LiteralPath $controlDigestPath)) {
    throw 'epoch-5 result or controls appeared; refusing atomic self-test publication'
}
Write-EpochFiveJsonAtomic -Path $resultPath -Value $report -Depth 100
$report | ConvertTo-Json -Depth 100
if (-not $allPassed) {
    exit 1
}
