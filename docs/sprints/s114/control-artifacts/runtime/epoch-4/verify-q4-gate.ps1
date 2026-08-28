[CmdletBinding()]
param(
    [switch]$DeriveOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$recoveryPlanPath = Join-Path $artifactDir 'runtime-plan.json'
$recoveryPlan = Get-Content -Raw -LiteralPath $recoveryPlanPath |
    ConvertFrom-Json -DateKind String
$sourceArtifactDir = Resolve-SafeRelativePath -Root $repoRoot `
    -RelativePath ([string]$recoveryPlan.source_artifact_relative_path)
$planPath = Join-Path $sourceArtifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath |
    ConvertFrom-Json -DateKind String
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$primaryAttemptPath = Resolve-SafeRelativePath -Root $repoRoot `
    -RelativePath ([string]$recoveryPlan.operation.destination_relative_path)
$attemptsRoot = Split-Path -Parent $primaryAttemptPath
$gatePath = Join-Path $artifactDir 'q4-viability.json'
$publicationPath = Join-Path $artifactDir 'recovery-publication.json'
$errors = [System.Collections.Generic.List[string]]::new()

if (-not (Test-RecoveryPlanIdentity -Plan $recoveryPlan)) {
    $errors.Add('runtime plan is not the epoch-4 recovery plan')
}
if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    $errors.Add('source runtime plan is not the frozen epoch-3 protocol')
}

function Add-GateError {
    param([Parameter(Mandatory = $true)][string]$Message)
    $errors.Add($Message)
}

function Test-JsonEqual {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Left,
        [AllowNull()][Parameter(Mandatory = $true)]$Right
    )
    ($Left | ConvertTo-Json -Depth 100 -Compress) -ceq
        ($Right | ConvertTo-Json -Depth 100 -Compress)
}

function Test-StrictPublishedUtc {
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
        [System.Globalization.DateTimeStyles]::AssumeUniversal -bor
            [System.Globalization.DateTimeStyles]::AdjustToUniversal,
        [ref]$instant
    )
}

function Test-EmbeddedRecoveryVerification {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Report,
        [Parameter(Mandatory = $true)][string]$ExpectedAnchorMode
    )

    $null -ne $Report -and
        $Report.schema -ceq
            'animus-ferric-runtime-recovery-verification-v4' -and
        $Report.task -ceq 'T-11409' -and
        $Report.operation_id -ceq [string]$recoveryPlan.operation.id -and
        [int]$Report.execution_epoch -eq 3 -and
        [int]$Report.publication_epoch -eq 4 -and
        $Report.source_attempt_schema -ceq
            [string]$recoveryPlan.operation.source_attempt_schema -and
        $Report.timestamp_protocol -ceq
            [string]$recoveryPlan.timestamp_protocol -and
        [int]$Report.control_epoch -eq 3 -and
        $Report.attestation_protocol -ceq
            [string]$plan.template_attestation.protocol -and
        $Report.process_command_protocol -ceq
            [string]$plan.process_command_attestation.protocol -and
        $Report.coordinate -ceq [string]$recoveryPlan.operation.coordinate -and
        $Report.verdict -ceq
            [string]$recoveryPlan.operation.expected_terminal.verdict -and
        $Report.control_anchor_mode -ceq $ExpectedAnchorMode -and
        [bool]$Report.live_model_identity.checked -and
        $Report.live_model_identity.mode -ceq 'checked_in_verifier' -and
        $Report.live_model_identity.sha256 -ceq
            [string]$recoveryPlan.model.sha256 -and
        [bool]$Report.manifest.passed -and
        [int]$Report.manifest.entries -eq
            [int]$recoveryPlan.operation.exact_manifest_entries -and
        [bool]$Report.recovery_anchor.applicable -and
        [bool]$Report.recovery_anchor.passed -and
        [int]$Report.recovery_anchor.observed_entries -eq
            [int]$recoveryPlan.operation.exact_manifest_entries -and
        [bool]$Report.passed
}

$publication = $null
if (-not (Test-Path -LiteralPath $publicationPath -PathType Leaf)) {
    Add-GateError 'epoch-4 recovery publication envelope is absent'
}
else {
    try {
        $publication = Get-Content -Raw -LiteralPath $publicationPath |
            ConvertFrom-Json -DateKind String
    }
    catch {
        Add-GateError 'epoch-4 recovery publication envelope is malformed'
    }
}
if ($null -ne $publication) {
    $publishedManifestPath = Join-Path $primaryAttemptPath 'files.sha256'
    $publishedManifestSha256 = if (Test-Path -LiteralPath $publishedManifestPath `
            -PathType Leaf) {
        Get-Sha256Lower -Path $publishedManifestPath
    }
    else {
        $null
    }
    $frozenControlPath = Join-Path $artifactDir 'control-inputs.json'
    $frozenControlSha256 = if (Test-Path -LiteralPath $frozenControlPath `
            -PathType Leaf) {
        Get-Sha256Lower -Path $frozenControlPath
    }
    else {
        $null
    }
    $stageAnchorMode = if (
        $publication.resumed_existing_destination -is [bool] -and
        [bool]$publication.resumed_existing_destination
    ) {
        'epoch_4_frozen_recovery'
    }
    else {
        'epoch_4_frozen_publication_stage'
    }
    if ($publication.schema -cne
            'animus-ferric-runtime-recovery-publication-v4' -or
        $publication.task -cne 'T-11409' -or
        $publication.operation_id -cne [string]$recoveryPlan.operation.id -or
        [int]$publication.execution_epoch -ne 3 -or
        [int]$publication.publication_epoch -ne 4 -or
        $publication.timestamp_protocol -cne
            [string]$recoveryPlan.timestamp_protocol -or
        -not (Test-StrictPublishedUtc -Value $publication.published_at_utc) -or
        $publication.resumed_existing_destination -isnot [bool] -or
        -not [bool]$publication.passed -or
        $publication.control_manifest_sha256 -cne $frozenControlSha256 -or
        $publication.source.relative_path -cne
            [string]$recoveryPlan.operation.source_raw_relative_path -or
        $publication.source.manifest_sha256 -cne
            [string]$recoveryPlan.operation.manifest.sha256 -or
        [int]$publication.source.entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        $publication.destination.relative_path -cne
            [string]$recoveryPlan.operation.destination_relative_path -or
        $publication.destination.manifest_sha256 -cne
            $publishedManifestSha256 -or
        [int]$publication.destination.entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        -not (Test-EmbeddedRecoveryVerification `
            -Report $publication.stage_verification `
            -ExpectedAnchorMode $stageAnchorMode) -or
        -not (Test-EmbeddedRecoveryVerification `
            -Report $publication.published_verification `
            -ExpectedAnchorMode 'epoch_4_frozen_recovery')) {
        Add-GateError 'epoch-4 recovery publication envelope is not derivable'
    }
}

function Get-VerifiedAttempt {
    param([Parameter(Mandatory = $true)][string]$Id)

    $directory = Join-Path $attemptsRoot $Id
    $attemptPath = Join-Path $directory 'attempt.json'
    $manifestPath = Join-Path $directory 'files.sha256'
    if (-not (Test-Path -LiteralPath $attemptPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        Add-GateError "Q4 attempt evidence is absent: $Id"
        return $null
    }
    $verificationProcess = Invoke-PowerShellFileBounded `
        -ScriptPath $validatorPath `
        -Arguments @('-AttemptPath', $directory)
    $verificationText = $verificationProcess.stdout
    $verificationCode = $verificationProcess.exit_code
    try {
        $verification = $verificationText | ConvertFrom-Json -DateKind String
    }
    catch {
        Add-GateError "Q4 attempt validator returned malformed JSON: $Id"
        return $null
    }
    if ($verificationCode -ne 0 -or
        $verification.schema -cne
            'animus-ferric-runtime-recovery-verification-v4' -or
        $verification.task -cne 'T-11409' -or
        $verification.operation_id -cne
            [string]$recoveryPlan.operation.id -or
        [int]$verification.execution_epoch -ne 3 -or
        [int]$verification.publication_epoch -ne 4 -or
        $verification.source_attempt_schema -cne
            'animus-ferric-runtime-attempt-v3' -or
        $verification.timestamp_protocol -cne
            [string]$recoveryPlan.timestamp_protocol -or
        $verification.attestation_protocol -cne
            [string]$plan.template_attestation.protocol -or
        $verification.process_command_protocol -cne
            [string]$plan.process_command_attestation.protocol -or
        $verification.coordinate -cne $Id -or
        $verification.control_anchor_mode -cne
            'epoch_4_frozen_recovery' -or
        -not [bool]$verification.live_model_identity.checked -or
        $verification.live_model_identity.mode -cne 'checked_in_verifier' -or
        $verification.live_model_identity.sha256 -cne
            [string]$recoveryPlan.model.sha256 -or
        -not [bool]$verification.manifest.passed -or
        [int]$verification.manifest.entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        -not [bool]$verification.recovery_anchor.applicable -or
        -not [bool]$verification.recovery_anchor.passed -or
        [int]$verification.recovery_anchor.observed_entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        -not [bool]$verification.passed) {
        Add-GateError "Q4 attempt failed verification: $Id"
    }
    $attempt = Get-Content -Raw -LiteralPath $attemptPath |
        ConvertFrom-Json -DateKind String
    if ($attempt.quant -cne 'Q4_K_M') {
        Add-GateError "Q4 chain contains a different quant: $Id"
    }
    [pscustomobject]@{
        id = $Id
        directory = $directory
        attempt = $attempt
        manifest_sha256 = Get-Sha256Lower -Path $manifestPath
        verification = $verification
        verification_code = $verificationCode
    }
}

$chain = [System.Collections.Generic.List[object]]::new()
$primary = Get-VerifiedAttempt -Id ([string]$recoveryPlan.operation.coordinate)
if ($null -ne $primary) {
    $chain.Add($primary)
}
$requiresRetry = $null -ne $primary -and
    $primary.attempt.failure_classification -ceq 'startup_memory_pressure'
$retryPath = Join-Path $attemptsRoot 'e03-02-q4-16384'
if ($requiresRetry) {
    Add-GateError 'the exact-byte recovery operation cannot invent a context retry'
}
elseif (Test-Path -LiteralPath $retryPath) {
    Add-GateError 'Q4 16384 attempt exists without a verified memory-pressure predecessor'
}

$expectedQ4Ids = @($chain | ForEach-Object { $_.id })
$actualQ4Ids = if (Test-Path -LiteralPath $attemptsRoot) {
    @(
        Get-ChildItem -LiteralPath $attemptsRoot -Directory -Force |
            Where-Object { $_.Name -match '^e03-\d{2}-q4-' } |
            Select-Object -ExpandProperty Name |
            Sort-Object
    )
}
else {
    @()
}
if (($actualQ4Ids -join "`n") -cne
    (@($expectedQ4Ids | Sort-Object) -join "`n")) {
    Add-GateError 'retained Q4 attempt directories do not equal the authorized chain'
}

$terminal = if ($chain.Count -gt 0) { $chain[$chain.Count - 1] } else { $null }
$fallbackBasis = $null
$q4Verdict = 'blocked'
$fallbackAuthorized = $false
if ($errors.Count -eq 0 -and $null -ne $terminal) {
    $terminalAttempt = $terminal.attempt
    if ($terminalAttempt.verdict -eq 'viable') {
        $q4Verdict = 'viable'
    }
    elseif ($terminalAttempt.verdict -eq 'non_viable' -and
        $terminalAttempt.startup.healthy -and
        $terminalAttempt.attestation.passed -and
        @($terminalAttempt.reason_codes).Count -eq 1) {
        $reason = [string]$terminalAttempt.reason_codes[0]
        if ($reason -eq 'functional_smoke_failed') {
            $fallbackBasis = 'q4_functional_smoke_failed'
        }
        elseif ($reason -eq 'throughput_median_below_floor') {
            $fallbackBasis = 'q4_throughput_median_below_floor'
        }
        elseif ($reason -eq 'invalid_throughput_sample_set') {
            $invalidRows = @($terminalAttempt.throughput.samples |
                Where-Object { -not $_.valid })
            $authorizingScoredRows = @($invalidRows | Where-Object {
                $_.scored -and
                $_.failure_cause -in
                    @($plan.selection.q3_authorizing_trial_failure_causes)
            })
            if ($authorizingScoredRows.Count -gt 0) {
                $fallbackBasis = 'q4_scored_trial_request_failure'
            }
        }
        if ($null -ne $fallbackBasis) {
            $q4Verdict = 'non_viable'
            $fallbackAuthorized = $true
        }
        else {
            $q4Verdict = 'non_viable'
            $fallbackAuthorized = $false
            $fallbackBasis = if ($reason -eq 'invalid_throughput_sample_set' -and
                @($invalidRows | Where-Object { -not $_.scored }).Count -gt 0) {
                'q4_non_authorizing_warmup_failure'
            }
            elseif ($reason -eq 'invalid_throughput_sample_set') {
                'q4_non_authorizing_sample_failure'
            }
            else {
                'q4_non_authorizing_failure'
            }
        }
    }
    else {
        Add-GateError 'terminal Q4 attempt is infrastructure-blocked or unsupported'
    }
}

$attemptChain = @(
    $chain | ForEach-Object {
        [ordered]@{
            id = $_.id
            manifest_sha256 = $_.manifest_sha256
        }
    }
)
$attemptVerifications = @(
    $chain | ForEach-Object {
        [ordered]@{
            id = $_.id
            report = $_.verification
        }
    }
)
$derivation = [ordered]@{
    operation_id = [string]$recoveryPlan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
    attempt_chain = $attemptChain
    attempt_verifications = $attemptVerifications
    selected_attempt = if ($null -ne $terminal) { $terminal.id } else { $null }
    q4_verdict = $q4Verdict
    q3_fallback_authorized = $fallbackAuthorized
    fallback_basis = $fallbackBasis
    reason_codes = if ($null -ne $terminal) {
        @($terminal.attempt.reason_codes)
    }
    else {
        @()
    }
    median_decoded_tokens_per_second = if ($null -ne $terminal) {
        $terminal.attempt.throughput.median_decoded_tokens_per_second
    }
    else {
        $null
    }
    attempt_manifest_sha256 = if ($null -ne $terminal) {
        $terminal.manifest_sha256
    }
    else {
        $null
    }
    attempt_verification = if ($null -ne $terminal) {
        $terminal.verification
    }
    else {
        $null
    }
    recovery_plan_sha256 = Get-Sha256Lower -Path $recoveryPlanPath
    source_runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    validator_sha256 = Get-Sha256Lower -Path $validatorPath
    recovery_publication_sha256 = if (Test-Path -LiteralPath $publicationPath `
            -PathType Leaf) {
        Get-Sha256Lower -Path $publicationPath
    }
    else {
        $null
    }
}

$gate = $null
if (-not $DeriveOnly) {
    if (-not (Test-Path -LiteralPath $gatePath -PathType Leaf)) {
        Add-GateError 'q4-viability.json is absent'
    }
    else {
        try {
            $gate = Get-Content -Raw -LiteralPath $gatePath |
                ConvertFrom-Json -DateKind String
        }
        catch {
            Add-GateError 'q4-viability.json is malformed'
        }
    }
    if ($null -ne $gate) {
        if ($gate.schema -cne 'animus-ferric-qwen38-viability-v4' -or
            $gate.task -cne 'T-11409' -or
            $gate.operation_id -cne [string]$recoveryPlan.operation.id -or
            [int]$gate.execution_epoch -ne 3 -or
            [int]$gate.publication_epoch -ne 4 -or
            $gate.timestamp_protocol -cne
                [string]$recoveryPlan.timestamp_protocol -or
            -not (Test-StrictPublishedUtc -Value $gate.derived_at_utc) -or
            $gate.attestation_protocol -cne
                [string]$plan.template_attestation.protocol -or
            $gate.process_command_protocol -cne
                [string]$plan.process_command_attestation.protocol -or
            $gate.gate -cne 'E09-D' -or
            $gate.q4_file -cne $plan.models.Q4_K_M.file -or
            $gate.q4_sha256 -cne $plan.models.Q4_K_M.sha256 -or
            $gate.selected_attempt -cne $derivation.selected_attempt -or
            $gate.q4_verdict -cne $derivation.q4_verdict -or
            [bool]$gate.q3_fallback_authorized -ne
                [bool]$derivation.q3_fallback_authorized -or
            $gate.fallback_basis -cne $derivation.fallback_basis -or
            $gate.attempt_manifest_sha256 -cne
                $derivation.attempt_manifest_sha256 -or
            $gate.recovery_plan_sha256 -cne
                $derivation.recovery_plan_sha256 -or
            $gate.source_runtime_plan_sha256 -cne
                $derivation.source_runtime_plan_sha256 -or
            $gate.validator_sha256 -cne $derivation.validator_sha256 -or
            $gate.recovery_publication_sha256 -cne
                $derivation.recovery_publication_sha256 -or
            (@($gate.reason_codes) -join "`n") -cne
                (@($derivation.reason_codes) -join "`n") -or
            -not (Test-JsonEqual -Left $gate.attempt_chain `
                -Right $derivation.attempt_chain) -or
            -not (Test-JsonEqual -Left $gate.attempt_verifications `
                -Right $derivation.attempt_verifications) -or
            -not (Test-JsonEqual -Left $gate.attempt_verification `
                -Right $derivation.attempt_verification)) {
            Add-GateError 'Q4 gate fields do not equal fresh attempt-chain derivation'
        }
        $gateMedian = $gate.median_decoded_tokens_per_second
        $derivedMedian = $derivation.median_decoded_tokens_per_second
        $medianEqual =
            ($null -eq $gateMedian -and $null -eq $derivedMedian) -or
            ($null -ne $gateMedian -and $null -ne $derivedMedian -and
                [double]::IsFinite([double]$gateMedian) -and
                [double]::IsFinite([double]$derivedMedian) -and
                [Math]::Abs([double]$gateMedian - [double]$derivedMedian) -le
                    0.000001)
        if (-not $medianEqual) {
            Add-GateError 'Q4 gate median differs from fresh derivation'
        }
    }
}

$report = [ordered]@{
    schema = 'animus-ferric-q4-gate-verification-v4'
    task = 'T-11409'
    operation_id = [string]$recoveryPlan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
    mode = if ($DeriveOnly) { 'derive_only' } else { 'verify_gate' }
    passed = ($errors.Count -eq 0)
    derivation = $derivation
    errors = @($errors)
}
$report | ConvertTo-Json -Depth 100
if ($errors.Count -gt 0) {
    exit 1
}
exit 0
