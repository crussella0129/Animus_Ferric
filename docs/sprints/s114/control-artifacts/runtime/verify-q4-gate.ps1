[CmdletBinding()]
param(
    [switch]$DeriveOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$attemptsRoot = Join-Path $artifactDir 'attempts'
$gatePath = Join-Path $artifactDir 'q4-viability.json'
$errors = [System.Collections.Generic.List[string]]::new()

function Add-GateError {
    param([Parameter(Mandatory = $true)][string]$Message)
    $errors.Add($Message)
}

function Test-JsonEqual {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Left,
        [AllowNull()][Parameter(Mandatory = $true)]$Right
    )
    ($Left | ConvertTo-Json -Depth 100 -Compress) -eq
        ($Right | ConvertTo-Json -Depth 100 -Compress)
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
        $verification = $verificationText | ConvertFrom-Json
    }
    catch {
        Add-GateError "Q4 attempt validator returned malformed JSON: $Id"
        return $null
    }
    if ($verificationCode -ne 0 -or -not $verification.passed) {
        Add-GateError "Q4 attempt failed verification: $Id"
    }
    $attempt = Get-Content -Raw -LiteralPath $attemptPath | ConvertFrom-Json
    if ($attempt.quant -ne 'Q4_K_M') {
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
$primary = Get-VerifiedAttempt -Id '01-q4-32768'
if ($null -ne $primary) {
    $chain.Add($primary)
}
$requiresRetry = $null -ne $primary -and
    $primary.attempt.failure_classification -eq 'startup_memory_pressure'
$retryPath = Join-Path $attemptsRoot '02-q4-16384'
if ($requiresRetry) {
    $retry = Get-VerifiedAttempt -Id '02-q4-16384'
    if ($null -ne $retry) {
        $chain.Add($retry)
    }
}
elseif (Test-Path -LiteralPath $retryPath) {
    Add-GateError 'Q4 16384 attempt exists without a verified memory-pressure predecessor'
}

$expectedQ4Ids = @($chain | ForEach-Object { $_.id })
$actualQ4Ids = if (Test-Path -LiteralPath $attemptsRoot) {
    @(
        Get-ChildItem -LiteralPath $attemptsRoot -Directory -Force |
            Where-Object { $_.Name -match '^\d{2}-q4-' } |
            Select-Object -ExpandProperty Name |
            Sort-Object
    )
}
else {
    @()
}
if (($actualQ4Ids -join "`n") -ne
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
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    validator_sha256 = Get-Sha256Lower -Path $validatorPath
}

$gate = $null
if (-not $DeriveOnly) {
    if (-not (Test-Path -LiteralPath $gatePath -PathType Leaf)) {
        Add-GateError 'q4-viability.json is absent'
    }
    else {
        try {
            $gate = Get-Content -Raw -LiteralPath $gatePath | ConvertFrom-Json
        }
        catch {
            Add-GateError 'q4-viability.json is malformed'
        }
    }
    if ($null -ne $gate) {
        if ($gate.schema -ne 'animus-ferric-qwen38-viability-v1' -or
            $gate.gate -ne 'E09-D' -or
            $gate.q4_file -ne $plan.models.Q4_K_M.file -or
            $gate.q4_sha256 -ne $plan.models.Q4_K_M.sha256 -or
            $gate.selected_attempt -ne $derivation.selected_attempt -or
            $gate.q4_verdict -ne $derivation.q4_verdict -or
            [bool]$gate.q3_fallback_authorized -ne
                [bool]$derivation.q3_fallback_authorized -or
            $gate.fallback_basis -ne $derivation.fallback_basis -or
            $gate.attempt_manifest_sha256 -ne
                $derivation.attempt_manifest_sha256 -or
            $gate.runtime_plan_sha256 -ne $derivation.runtime_plan_sha256 -or
            $gate.validator_sha256 -ne $derivation.validator_sha256 -or
            (@($gate.reason_codes) -join "`n") -ne
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
    schema = 'animus-ferric-q4-gate-verification-v1'
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
