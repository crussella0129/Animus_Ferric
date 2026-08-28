[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('e02-01-q4-32768', 'e02-02-q4-16384')]
    [string]$AttemptId
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
$gateVerifierPath = Join-Path $artifactDir 'verify-q4-gate.ps1'
$outputPath = Join-Path $artifactDir 'q4-viability.json'

if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    throw 'runtime plan is not the frozen epoch-2 recovery protocol'
}

if (Test-Path -LiteralPath $outputPath) {
    throw 'q4-viability.json already exists and will not be overwritten'
}
$derivationProcess = Invoke-PowerShellFileBounded `
    -ScriptPath $gateVerifierPath -Arguments @('-DeriveOnly')
$derivationCode = $derivationProcess.exit_code
$derivationReport = $derivationProcess.stdout | ConvertFrom-Json
if ($derivationCode -ne 0 -or
    $derivationReport.schema -cne
        'animus-ferric-q4-gate-verification-v2' -or
    $derivationReport.task -cne 'T-11409' -or
    [int]$derivationReport.control_epoch -ne 2 -or
    $derivationReport.attestation_protocol -cne
        [string]$plan.template_attestation.protocol -or
    -not $derivationReport.passed) {
    throw "Q4 chain derivation failed: $($derivationReport.errors -join '; ')"
}
$derivation = $derivationReport.derivation
if ($derivation.selected_attempt -ne $AttemptId) {
    throw "AttemptId does not equal the derived terminal Q4 attempt: $($derivation.selected_attempt)"
}
if ($derivation.q4_verdict -eq 'blocked') {
    throw 'Q4 evidence is infrastructure-blocked and cannot publish a viability gate'
}

$gate = [ordered]@{
    schema = 'animus-ferric-qwen38-viability-v2'
    task = 'T-11409'
    control_epoch = 2
    attestation_protocol = [string]$plan.template_attestation.protocol
    gate = 'E09-D'
    q4_verdict = [string]$derivation.q4_verdict
    q3_fallback_authorized = [bool]$derivation.q3_fallback_authorized
    q4_file = [string]$plan.models.Q4_K_M.file
    q4_sha256 = [string]$plan.models.Q4_K_M.sha256
    attempt_chain = @($derivation.attempt_chain)
    selected_attempt = [string]$derivation.selected_attempt
    fallback_basis = $derivation.fallback_basis
    reason_codes = @($derivation.reason_codes)
    median_decoded_tokens_per_second = `
        $derivation.median_decoded_tokens_per_second
    attempt_manifest_sha256 = [string]$derivation.attempt_manifest_sha256
    runtime_plan_sha256 = [string]$derivation.runtime_plan_sha256
    validator_sha256 = [string]$derivation.validator_sha256
    attempt_verifications = @($derivation.attempt_verifications)
    attempt_verification = $derivation.attempt_verification
    derived_at_utc = (Get-Date).ToUniversalTime().ToString('o')
}
$temporaryPath = Join-Path $artifactDir `
    ".q4-viability-$([guid]::NewGuid().ToString('N')).tmp"
Write-JsonLf -Path $temporaryPath -Value $gate
[System.IO.File]::Move($temporaryPath, $outputPath, $false)
$gate
