[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$runtimeRoot = Split-Path -Parent $artifactDir
$epochFourCommonPath = Join-Path $runtimeRoot 'epoch-4/runtime-common.ps1'
if (-not (Test-Path -LiteralPath $epochFourCommonPath -PathType Leaf)) {
    throw 'frozen epoch-4 runtime-common.ps1 is absent before bootstrap'
}
$epochFourCommonItem = Get-Item -LiteralPath $epochFourCommonPath -Force
if ($epochFourCommonItem.Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    ) -or [UInt64]$epochFourCommonItem.Length -ne 87625) {
    throw 'frozen epoch-4 runtime-common.ps1 bootstrap identity differs'
}
$epochFourCommonStream = [System.IO.File]::OpenRead($epochFourCommonPath)
try {
    $epochFourCommonHasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $epochFourCommonHash = [Convert]::ToHexString(
            $epochFourCommonHasher.ComputeHash($epochFourCommonStream)
        ).ToLowerInvariant()
    }
    finally {
        $epochFourCommonHasher.Dispose()
    }
}
finally {
    $epochFourCommonStream.Dispose()
}
if ($epochFourCommonHash -cne
        '322407fc52e2192cedf320d65e9c8029c75d1190e732e3d76a27394614eaf59c') {
    throw 'frozen epoch-4 runtime-common.ps1 bootstrap SHA-256 differs'
}
$epochFiveCommonPath = Join-Path $artifactDir 'publication-common.ps1'
if (-not (Test-Path -LiteralPath $epochFiveCommonPath -PathType Leaf)) {
    throw 'epoch-5 publication-common.ps1 is absent before bootstrap'
}
$epochFiveCommonItem = Get-Item -LiteralPath $epochFiveCommonPath -Force
if ($epochFiveCommonItem.Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    ) -or [UInt64]$epochFiveCommonItem.Length -ne 39136) {
    throw 'epoch-5 publication-common.ps1 bootstrap identity differs'
}
$epochFiveCommonStream = [System.IO.File]::OpenRead($epochFiveCommonPath)
try {
    $epochFiveCommonHasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $epochFiveCommonHash = [Convert]::ToHexString(
            $epochFiveCommonHasher.ComputeHash($epochFiveCommonStream)
        ).ToLowerInvariant()
    }
    finally {
        $epochFiveCommonHasher.Dispose()
    }
}
finally {
    $epochFiveCommonStream.Dispose()
}
if ($epochFiveCommonHash -cne
        '332475b8b83d5668ed9d7cb5d34fddfd720e4fb1328b91f9cbc7c44e64f994f1') {
    throw 'epoch-5 publication-common.ps1 bootstrap SHA-256 differs'
}
. $epochFourCommonPath
. $epochFiveCommonPath

$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$incidentPath = Join-Path $artifactDir 'incident.json'
$selfTestPath = Join-Path $artifactDir 'publication-self-test.json'
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
        "epoch-5 dependency anchor differs: $Label"
    $result
}

function Assert-ExactAnchoredEntry {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)]$Entry,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $relative = [string]$Entry.path
    Assert-FreezeCondition `
        (-not [string]::IsNullOrWhiteSpace($relative) -and
            -not [System.IO.Path]::IsPathRooted($relative) -and
            $relative -notmatch '(^|[\/])\.{1,2}([\/]|$)') `
        "unsafe frozen control path: $Label"
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $path = [System.IO.Path]::GetFullPath((Join-Path $rootFull $relative))
    $prefix = "$rootFull$([System.IO.Path]::DirectorySeparatorChar)"
    Assert-FreezeCondition `
        ($path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) `
        "frozen control escapes its root: $Label"
    Assert-FreezeCondition (Test-Path -LiteralPath $path -PathType Leaf) `
        "frozen control is absent: $Label"
    $item = Get-Item -LiteralPath $path -Force
    Assert-FreezeCondition `
        (-not $item.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            ) -and
            [UInt64]$item.Length -eq [UInt64]$Entry.bytes -and
            (Get-Sha256Lower -Path $path) -ceq [string]$Entry.sha256) `
        "frozen control bytes differ: $Label"
}

Assert-FreezeCondition (-not (Test-Path -LiteralPath $controlPath)) `
    'epoch-5 controls already exist and will not be overwritten'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $digestPath)) `
    'epoch-5 control digest already exists and will not be overwritten'
Assert-FreezeCondition (Test-Path -LiteralPath $selfTestPath -PathType Leaf) `
    'epoch-5 publication self-test is absent'

$plan = Read-FreezeJson -Path $planPath
$incident = Read-FreezeJson -Path $incidentPath
$selfTest = Read-FreezeJson -Path $selfTestPath
Assert-FreezeCondition (Test-EpochFivePlanIdentity -Plan $plan) `
    'epoch-5 publication correction plan identity differs'

$expectedHead = [string]$plan.repository_commit_before_epoch_5_controls
$head = (& git -C $repoRoot rev-parse HEAD).Trim()
Assert-FreezeCondition ($LASTEXITCODE -eq 0) 'could not resolve repository HEAD'
Assert-FreezeCondition ($head -ceq $expectedHead) `
    'repository HEAD differs from the epoch-5 baseline'

Assert-FreezeCondition `
    ([string]$incident.schema -ceq
        'animus-ferric-runtime-publication-incident-v5' -and
        [string]$incident.task -ceq 'T-11409' -and
        [string]$incident.operation_id -ceq [string]$plan.operation.id -and
        [string]$incident.failed_operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [int]$incident.execution_epoch -eq 3 -and
        [int]$incident.failed_publication_epoch -eq 4 -and
        [int]$incident.correction_epoch -eq 5 -and
        [string]$incident.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [string]$incident.failed_control_manifest_sha256 -ceq
            [string]$plan.epoch_4.control_manifest.sha256 -and
        [string]$incident.failure.script_sha256 -ceq
            [string]$plan.epoch_4.frozen_failed_publisher.sha256 -and
        [int]$incident.state_after_failure.source_manifest_entries -eq
            [int]$plan.operation.exact_manifest_entries -and
        [bool]$incident.state_after_failure.destination_absent -and
        [bool]$incident.state_after_failure.legacy_epoch_4_publication_envelope_absent -and
        [bool]$incident.state_after_failure.epoch_5_correction_evidence_absent -and
        [bool]$incident.state_after_failure.stage_not_promoted -and
        -not [bool]$incident.state_after_failure.model_execution_repeated -and
        [bool]$incident.resolution.preserve_epoch_4_immutable -and
        [bool]$incident.resolution.bind_source_protocols_from_epoch_3_plan -and
        [bool]$incident.resolution.publish_legacy_epoch_4_envelope -and
        [string]$incident.resolution.correction_evidence_relative_path -ceq
            [string]$plan.operation.correction_evidence_relative_path) `
    'epoch-5 incident record identity differs'

$anchorResults = [ordered]@{}
foreach ($anchorSpec in @(
        [pscustomobject]@{ key = 'epoch_4_runtime_plan'; anchor = $plan.epoch_4.runtime_plan },
        [pscustomobject]@{ key = 'epoch_4_raw_source_anchor'; anchor = $plan.epoch_4.raw_source_anchor },
        [pscustomobject]@{ key = 'epoch_4_control_manifest'; anchor = $plan.epoch_4.control_manifest },
        [pscustomobject]@{ key = 'epoch_4_control_digest'; anchor = $plan.epoch_4.control_digest },
        [pscustomobject]@{ key = 'epoch_4_runtime_self_test'; anchor = $plan.epoch_4.runtime_self_test },
        [pscustomobject]@{ key = 'epoch_4_verifier'; anchor = $plan.epoch_4.verifier },
        [pscustomobject]@{ key = 'epoch_4_frozen_failed_publisher'; anchor = $plan.epoch_4.frozen_failed_publisher },
        [pscustomobject]@{ key = 'epoch_3_control_manifest'; anchor = $plan.epoch_3.control_manifest },
        [pscustomobject]@{ key = 'epoch_3_control_digest'; anchor = $plan.epoch_3.control_digest },
        [pscustomobject]@{ key = 'epoch_3_runtime_plan'; anchor = $plan.epoch_3.runtime_plan },
        [pscustomobject]@{ key = 'epoch_3_runtime_self_test'; anchor = $plan.epoch_3.runtime_self_test },
        [pscustomobject]@{ key = 'raw_manifest'; anchor = $plan.operation.manifest },
        [pscustomobject]@{ key = 'raw_attempt'; anchor = $plan.operation.attempt },
        [pscustomobject]@{ key = 'raw_attestation'; anchor = $plan.operation.attestation }
    )) {
    $anchorResults[$anchorSpec.key] = Assert-PlanAnchor `
        -Anchor $anchorSpec.anchor -Label $anchorSpec.key
}

$epoch4PlanPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.runtime_plan.relative_path)
$rawAnchorPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.raw_source_anchor.relative_path)
$epoch4ControlPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.control_manifest.relative_path)
$epoch4DigestPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_4.control_digest.relative_path)
$epoch3PlanPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.epoch_3.runtime_plan.relative_path)

$epoch4Plan = Read-FreezeJson -Path $epoch4PlanPath
$rawAnchor = Read-FreezeJson -Path $rawAnchorPath
$epoch4Controls = Read-FreezeJson -Path $epoch4ControlPath
$epoch3Plan = Read-FreezeJson -Path $epoch3PlanPath
Assert-FreezeCondition (Test-RecoveryPlanIdentity -Plan $epoch4Plan) `
    'anchored epoch-4 recovery plan identity differs'
Assert-FreezeCondition (Test-RuntimePlanIdentity -Plan $epoch3Plan) `
    'anchored epoch-3 source runtime plan identity differs'
Assert-FreezeCondition `
    ([string]$epoch4Plan.operation.id -ceq
        [string]$plan.operation.failed_operation_id -and
        [string]$epoch4Plan.operation.coordinate -ceq
            [string]$plan.operation.coordinate -and
        [string]$epoch4Plan.operation.source_raw_relative_path -ceq
            [string]$plan.operation.source_raw_relative_path -and
        [string]$epoch4Plan.operation.destination_relative_path -ceq
            [string]$plan.operation.destination_relative_path -and
        [int]$epoch4Plan.operation.exact_manifest_entries -eq
            [int]$plan.operation.exact_manifest_entries -and
        [string]$epoch4Plan.operation.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [string]$epoch4Plan.operation.attempt.sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$epoch4Plan.operation.attestation.sha256 -ceq
            [string]$plan.operation.attestation.sha256 -and
        [string]$epoch4Plan.epoch_3.runtime_plan.sha256 -ceq
            [string]$plan.epoch_3.runtime_plan.sha256 -and
        [string]$epoch4Plan.epoch_3.control_manifest.sha256 -ceq
            [string]$plan.epoch_3.control_manifest.sha256 -and
        [string]$epoch4Plan.epoch_3.control_digest.sha256 -ceq
            [string]$plan.epoch_3.control_digest.sha256 -and
        [string]$epoch4Plan.epoch_3.runtime_self_test.sha256 -ceq
            [string]$plan.epoch_3.runtime_self_test.sha256 -and
        [string]$epoch4Plan.epoch_3.control_manifest_digest_line -ceq
            [string]$plan.epoch_3.control_manifest_digest_line -and
        [string]$epoch4Plan.model.sha256 -ceq [string]$plan.model.sha256) `
    'epoch-5 plan is not an exact correction of the epoch-4 operation'
foreach ($name in @(
        'control_manifest',
        'control_digest',
        'runtime_plan',
        'runtime_self_test'
    )) {
    $epoch5Anchor = $plan.epoch_3.$name
    $epoch4Anchor = $epoch4Plan.epoch_3.$name
    Assert-FreezeCondition `
        ([string]$epoch5Anchor.relative_path -ceq
            [string]$epoch4Anchor.relative_path -and
            [UInt64]$epoch5Anchor.bytes -eq [UInt64]$epoch4Anchor.bytes -and
            [string]$epoch5Anchor.sha256 -ceq [string]$epoch4Anchor.sha256) `
        "epoch-5 explicit epoch-3 anchor differs: $name"
}
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
        [string]$rawAnchor.manifest.sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [string]$rawAnchor.selected.attempt.sha256 -ceq
            [string]$plan.operation.attempt.sha256 -and
        [string]$rawAnchor.selected.attestation.sha256 -ceq
            [string]$plan.operation.attestation.sha256) `
    'anchored raw-source identity differs from the correction plan'

$epoch4DigestLine = (Get-Content -Raw -LiteralPath $epoch4DigestPath).
    TrimEnd("`r", "`n")
Assert-FreezeCondition `
    ($epoch4DigestLine -ceq [string]$plan.epoch_4.control_manifest_digest_line) `
    'epoch-4 control digest line differs'
Assert-FreezeCondition `
    ([string]$epoch4Controls.schema -ceq
        'animus-ferric-runtime-recovery-control-inputs-v4' -and
        [string]$epoch4Controls.task -ceq 'T-11409' -and
        [string]$epoch4Controls.operation_id -ceq
            [string]$plan.operation.failed_operation_id -and
        [int]$epoch4Controls.execution_epoch -eq 3 -and
        [int]$epoch4Controls.publication_epoch -eq 4 -and
        [string]$epoch4Controls.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [string]$epoch4Controls.repository.head_at_freeze -ceq $head -and
        [string]$epoch4Controls.runtime_plan_sha256 -ceq
            [string]$plan.epoch_4.runtime_plan.sha256 -and
        [string]$epoch4Controls.raw_source_anchor_sha256 -ceq
            [string]$plan.epoch_4.raw_source_anchor.sha256 -and
        [bool]$epoch4Controls.epoch_3.passed -and
        [bool]$epoch4Controls.runtime_self_test.passed -and
        [bool]$epoch4Controls.source_verification.passed -and
        -not [bool]$epoch4Controls.source_verification.hash_deferral_used -and
        [bool]$epoch4Controls.model.passed -and
        [bool]$epoch4Controls.model.independently_rehashed -and
        [bool]$epoch4Controls.destination.absent_at_freeze) `
    'frozen epoch-4 control manifest identity differs'

$epoch4Root = Split-Path -Parent $epoch4ControlPath
$epoch4Names = @(Get-EpochFourStaticControlNames)
$epoch4Entries = @($epoch4Controls.static_controls)
Assert-FreezeCondition `
    ($epoch4Names.Count -eq 12 -and $epoch4Entries.Count -eq 12 -and
        @($epoch4Names | Select-Object -Unique).Count -eq 12 -and
        @($epoch4Entries.path | Select-Object -Unique).Count -eq 12) `
    'frozen epoch-4 static-control set differs'
foreach ($name in $epoch4Names) {
    $matches = @($epoch4Entries | Where-Object {
            [string]$_.path -ceq [string]$name
        })
    Assert-FreezeCondition ($matches.Count -eq 1) `
        "epoch-4 control has no unique frozen entry: $name"
    Assert-ExactAnchoredEntry -Root $epoch4Root -Entry $matches[0] `
        -Label "epoch-4/$name"
}
Assert-PlanAnchor -Anchor $epoch4Controls.runtime_self_test `
    -Label 'epoch-4/control-runtime-self-test' | Out-Null

$epoch3AnchorPairs = @(
    [pscustomobject]@{
        left = $epoch4Controls.epoch_3.control_manifest
        right = $epoch4Plan.epoch_3.control_manifest
        label = 'epoch-3-control-manifest'
    },
    [pscustomobject]@{
        left = $epoch4Controls.epoch_3.control_digest
        right = $epoch4Plan.epoch_3.control_digest
        label = 'epoch-3-control-digest'
    },
    [pscustomobject]@{
        left = $epoch4Controls.epoch_3.runtime_plan
        right = $epoch4Plan.epoch_3.runtime_plan
        label = 'epoch-3-runtime-plan'
    },
    [pscustomobject]@{
        left = $epoch4Controls.epoch_3.runtime_self_test
        right = $epoch4Plan.epoch_3.runtime_self_test
        label = 'epoch-3-runtime-self-test'
    }
)
foreach ($pair in $epoch3AnchorPairs) {
    Assert-FreezeCondition `
        ([string]$pair.left.relative_path -ceq
            [string]$pair.right.relative_path -and
            [UInt64]$pair.left.bytes -eq [UInt64]$pair.right.bytes -and
            [string]$pair.left.sha256 -ceq [string]$pair.right.sha256) `
        "epoch-4 transitive anchor differs: $($pair.label)"
    Assert-PlanAnchor -Anchor $pair.left -Label $pair.label | Out-Null
}
Assert-FreezeCondition `
    ([string]$epoch4Controls.epoch_3.control_manifest_digest_line -ceq
        [string]$epoch4Plan.epoch_3.control_manifest_digest_line) `
    'epoch-4 transitive epoch-3 digest line differs'
$epoch3DigestPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$epoch4Plan.epoch_3.control_digest.relative_path)
$epoch3DigestLine = (Get-Content -Raw -LiteralPath $epoch3DigestPath).
    TrimEnd("`r", "`n")
Assert-FreezeCondition `
    ($epoch3DigestLine -ceq [string]$epoch4Plan.epoch_3.control_manifest_digest_line) `
    'epoch-3 control digest line differs'
$epoch3ControlPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$epoch4Plan.epoch_3.control_manifest.relative_path)
$epoch3Controls = Read-FreezeJson -Path $epoch3ControlPath
$epoch3Root = Split-Path -Parent $epoch3ControlPath
$epoch3Entries = @($epoch3Controls.controls)
Assert-FreezeCondition `
    ([string]$epoch3Controls.schema -ceq
        'animus-ferric-runtime-control-inputs-v3' -and
        [string]$epoch3Controls.task -ceq 'T-11409' -and
        [int]$epoch3Controls.control_epoch -eq 3 -and
        [string]$epoch3Controls.repository.head_at_freeze -ceq $head -and
        [bool]$epoch3Controls.recovery_anchors.passed -and
        [bool]$epoch3Controls.measurement_continuity.passed -and
        $epoch3Entries.Count -eq
            [int]$epoch4Controls.epoch_3.transitive_controls_checked -and
        @($epoch3Entries.path | Select-Object -Unique).Count -eq
            $epoch3Entries.Count) `
    'transitive epoch-3 frozen control set differs'
foreach ($entry in $epoch3Entries) {
    Assert-ExactAnchoredEntry -Root $epoch3Root -Entry $entry `
        -Label "epoch-3/$([string]$entry.path)"
}

$sourceRoot = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.source_raw_relative_path)
$treeCheck = Test-EpochFiveExactTree -Root $sourceRoot `
    -ManifestAnchor $rawAnchor `
    -ExpectedEntries ([int]$plan.operation.exact_manifest_entries)
Assert-FreezeCondition `
    ([bool]$treeCheck.passed -and
        [string]$treeCheck.manifest_sha256 -ceq
            [string]$plan.operation.manifest.sha256 -and
        [int]$treeCheck.entries -eq
            [int]$plan.operation.exact_manifest_entries -and
        [UInt64]$treeCheck.payload_bytes -eq
            [UInt64]$rawAnchor.manifest.payload_bytes) `
    'raw source exact-tree verification failed'
$attempt = Read-FreezeJson -Path (Join-Path $sourceRoot 'attempt.json')
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
    'raw source terminal facts differ from the correction plan'

$staticNames = @(Get-EpochFiveStaticControlNames)
$expectedStaticNames = @(
    '.gitattributes',
    'README.md',
    'incident.json',
    'runtime-plan.json',
    'publication-common.ps1',
    'test-publication.ps1',
    'freeze-publication.ps1',
    'publish-e04-correction.ps1'
)
$selfTestStatic = @($selfTest.static_controls)
Assert-FreezeCondition `
    (($staticNames -join "`n") -ceq ($expectedStaticNames -join "`n") -and
        $staticNames.Count -eq 8 -and
        @($staticNames | Select-Object -Unique).Count -eq 8 -and
        $selfTestStatic.Count -eq 8 -and
        @($selfTestStatic.path | Select-Object -Unique).Count -eq 8) `
    'epoch-5 static-control set must contain exactly eight unique files'
Assert-FreezeCondition `
    ([string]$selfTest.schema -ceq
        'animus-ferric-runtime-publication-correction-self-test-v5' -and
        [string]$selfTest.task -ceq 'T-11409' -and
        [string]$selfTest.operation_id -ceq [string]$plan.operation.id -and
        [int]$selfTest.execution_epoch -eq 3 -and
        [int]$selfTest.failed_publication_epoch -eq 4 -and
        [int]$selfTest.correction_epoch -eq 5 -and
        [string]$selfTest.timestamp_protocol -ceq
            [string]$plan.timestamp_protocol -and
        [bool]$selfTest.passed) `
    'epoch-5 publication self-test is not green'
$selfTestResults = @($selfTest.results)
Assert-FreezeCondition `
    ($selfTestResults.Count -gt 0 -and
        @($selfTestResults.name | Select-Object -Unique).Count -eq
            $selfTestResults.Count -and
        @($selfTestResults | Where-Object { -not [bool]$_.passed }).Count -eq 0) `
    'epoch-5 publication self-test results are incomplete or failed'
$frozenStaticControls = @()
foreach ($name in $staticNames) {
    $matches = @($selfTestStatic | Where-Object {
            [string]$_.path -ceq [string]$name
        })
    Assert-FreezeCondition ($matches.Count -eq 1) `
        "publication self-test has no unique static identity for $name"
    $path = Join-Path $artifactDir $name
    Assert-FreezeCondition (Test-Path -LiteralPath $path -PathType Leaf) `
        "epoch-5 static control is absent: $name"
    $item = Get-Item -LiteralPath $path -Force
    $hash = Get-Sha256Lower -Path $path
    Assert-FreezeCondition `
        (-not $item.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            ) -and
            [UInt64]$matches[0].bytes -eq [UInt64]$item.Length -and
            [string]$matches[0].sha256 -ceq $hash) `
        "epoch-5 static control differs from the green self-test: $name"
    $frozenStaticControls += [ordered]@{
        path = [string]$name
        bytes = [UInt64]$item.Length
        sha256 = $hash
    }
}
Assert-FreezeCondition `
    ([bool]$selfTest.live_q4_identity.checked -and
        [bool]$selfTest.live_q4_identity.passed -and
        [UInt64]$selfTest.live_q4_identity.bytes -eq
            [UInt64]$plan.model.bytes -and
        [string]$selfTest.live_q4_identity.sha256 -ceq
            [string]$plan.model.sha256) `
    'epoch-5 self-test Q4 identity differs'

$destinationPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.destination_relative_path)
$legacyEnvelopePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.legacy_envelope_relative_path)
$correctionEvidencePath = Resolve-EpochFiveRepoRelativePath `
    -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.operation.correction_evidence_relative_path)
Assert-FreezeCondition (-not (Test-Path -LiteralPath $destinationPath)) `
    'publication destination already exists at epoch-5 freeze'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $legacyEnvelopePath)) `
    'legacy epoch-4 publication envelope already exists at epoch-5 freeze'
Assert-FreezeCondition (-not (Test-Path -LiteralPath $correctionEvidencePath)) `
    'epoch-5 correction evidence already exists at freeze'

$coldState = Get-EpochFiveColdState -RepositoryRoot $repoRoot
Assert-FreezeCondition ([bool]$coldState.passed) `
    'Ferric/llama-server state is not cold at epoch-5 freeze'

$modelPath = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath ([string]$plan.model.relative_path)
Assert-FreezeCondition (Test-Path -LiteralPath $modelPath -PathType Leaf) `
    'frozen Q4 model is absent'
$modelItem = Get-Item -LiteralPath $modelPath -Force
Assert-FreezeCondition `
    (-not $modelItem.Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        ) -and
        [UInt64]$modelItem.Length -eq [UInt64]$plan.model.bytes) `
    'live Q4 model byte identity differs'
$modelHash = Get-Sha256Lower -Path $modelPath
Assert-FreezeCondition ($modelHash -ceq [string]$plan.model.sha256) `
    'independent freeze-time Q4 hash differs'

$controlManifest = [ordered]@{
    schema = 'animus-ferric-runtime-publication-correction-control-inputs-v5'
    task = 'T-11409'
    operation_id = [string]$plan.operation.id
    failed_operation_id = [string]$plan.operation.failed_operation_id
    execution_epoch = 3
    failed_publication_epoch = 4
    correction_epoch = 5
    timestamp_protocol = [string]$plan.timestamp_protocol
    frozen_at_utc = (Get-Date).ToUniversalTime().ToString(
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'"
    )
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    incident_sha256 = Get-Sha256Lower -Path $incidentPath
    repository = [ordered]@{
        head_at_freeze = $head
        epoch_5_pre_control_base = $expectedHead
    }
    static_controls = @($frozenStaticControls)
    publication_self_test = [ordered]@{
        relative_path = 'docs/sprints/s114/control-artifacts/runtime/epoch-5/publication-self-test.json'
        bytes = [UInt64](Get-Item -LiteralPath $selfTestPath).Length
        sha256 = Get-Sha256Lower -Path $selfTestPath
        passed = $true
    }
    epoch_4 = [ordered]@{
        runtime_plan_sha256 = [string]$plan.epoch_4.runtime_plan.sha256
        raw_source_anchor_sha256 = [string]$plan.epoch_4.raw_source_anchor.sha256
        control_manifest_sha256 = [string]$plan.epoch_4.control_manifest.sha256
        control_digest_sha256 = [string]$plan.epoch_4.control_digest.sha256
        runtime_self_test_sha256 = [string]$plan.epoch_4.runtime_self_test.sha256
        verifier_sha256 = [string]$plan.epoch_4.verifier.sha256
        frozen_failed_publisher_sha256 =
            [string]$plan.epoch_4.frozen_failed_publisher.sha256
        control_manifest_digest_line =
            [string]$plan.epoch_4.control_manifest_digest_line
        static_controls_checked = $epoch4Entries.Count
        transitive_epoch_3_controls_checked = $epoch3Entries.Count
        passed = $true
    }
    raw_source = [ordered]@{
        relative_path = [string]$plan.operation.source_raw_relative_path
        manifest_sha256 = [string]$treeCheck.manifest_sha256
        entries = [int]$treeCheck.entries
        payload_bytes = [UInt64]$treeCheck.payload_bytes
        attempt_sha256 = [string]$plan.operation.attempt.sha256
        attestation_sha256 = [string]$plan.operation.attestation.sha256
        terminal_facts_passed = $true
        passed = $true
    }
    model = [ordered]@{
        relative_path = [string]$plan.model.relative_path
        bytes = [UInt64]$modelItem.Length
        sha256 = $modelHash
        independently_rehashed = $true
        passed = $true
    }
    cold_state = $coldState
    publication_preconditions = [ordered]@{
        destination_relative_path =
            [string]$plan.operation.destination_relative_path
        destination_absent_at_freeze = $true
        legacy_envelope_relative_path =
            [string]$plan.operation.legacy_envelope_relative_path
        legacy_envelope_absent_at_freeze = $true
        correction_evidence_relative_path =
            [string]$plan.operation.correction_evidence_relative_path
        correction_evidence_absent_at_freeze = $true
        passed = $true
    }
    passed = $true
}

$stageParent = Resolve-EpochFiveRepoRelativePath -RepositoryRoot $repoRoot `
    -RelativePath 'target/s114-experiment/publication-correction-control-stage'
[System.IO.Directory]::CreateDirectory($stageParent) | Out-Null
$stageOwner = Join-Path $stageParent ([guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($stageOwner) | Out-Null
$stageControl = Join-Path $stageOwner 'control-inputs.json'
$stageDigest = Join-Path $stageOwner 'control-inputs.sha256'
$controlPublished = $false
try {
    Write-EpochFiveJsonAtomic -Path $stageControl -Value $controlManifest -Depth 64
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
    if (Test-Path -LiteralPath $stageOwner -PathType Container) {
        [System.IO.Directory]::Delete($stageOwner, $true)
    }
}
Assert-FreezeCondition $controlPublished 'epoch-5 controls were not published'

[ordered]@{
    control_manifest = 'control-inputs.json'
    control_manifest_sha256 = Get-Sha256Lower -Path $controlPath
    control_digest = 'control-inputs.sha256'
    operation_id = [string]$plan.operation.id
    source_entries = [int]$treeCheck.entries
    independently_rehashed_q4 = $true
    passed = $true
} | ConvertTo-Json -Depth 8
