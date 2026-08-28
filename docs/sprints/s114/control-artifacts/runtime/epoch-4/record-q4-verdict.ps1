[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('e03-01-q4-32768', 'e03-02-q4-16384')]
    [string]$AttemptId
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
$gateVerifierPath = Join-Path $artifactDir 'verify-q4-gate.ps1'
$outputPath = Join-Path $artifactDir 'q4-viability.json'

if (-not (Test-RecoveryPlanIdentity -Plan $recoveryPlan)) {
    throw 'runtime plan is not the frozen epoch-4 recovery protocol'
}
if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    throw 'source runtime plan is not the frozen epoch-3 protocol'
}

if (Test-Path -LiteralPath $outputPath) {
    throw 'q4-viability.json already exists and will not be overwritten'
}
$derivationProcess = Invoke-PowerShellFileBounded `
    -ScriptPath $gateVerifierPath -Arguments @('-DeriveOnly')
$derivationCode = $derivationProcess.exit_code
$derivationReport = $derivationProcess.stdout |
    ConvertFrom-Json -DateKind String
if ($derivationCode -ne 0 -or
    $derivationReport.schema -cne
        'animus-ferric-q4-gate-verification-v4' -or
    $derivationReport.task -cne 'T-11409' -or
    $derivationReport.operation_id -cne [string]$recoveryPlan.operation.id -or
    [int]$derivationReport.execution_epoch -ne 3 -or
    [int]$derivationReport.publication_epoch -ne 4 -or
    $derivationReport.timestamp_protocol -cne
        [string]$recoveryPlan.timestamp_protocol -or
    $derivationReport.attestation_protocol -cne
        [string]$plan.template_attestation.protocol -or
    $derivationReport.process_command_protocol -cne
        [string]$plan.process_command_attestation.protocol -or
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
    schema = 'animus-ferric-qwen38-viability-v4'
    task = 'T-11409'
    operation_id = [string]$recoveryPlan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
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
    recovery_plan_sha256 = [string]$derivation.recovery_plan_sha256
    source_runtime_plan_sha256 =
        [string]$derivation.source_runtime_plan_sha256
    validator_sha256 = [string]$derivation.validator_sha256
    recovery_publication_sha256 =
        [string]$derivation.recovery_publication_sha256
    attempt_verifications = @($derivation.attempt_verifications)
    attempt_verification = $derivation.attempt_verification
    derived_at_utc = (Get-Date).ToUniversalTime().ToString('o')
}
$temporaryPath = Join-Path $artifactDir `
    ".q4-viability-$([guid]::NewGuid().ToString('N')).tmp"
try {
    Write-JsonLf -Path $temporaryPath -Value $gate
    if (Test-Path -LiteralPath $outputPath) {
        throw 'q4-viability.json appeared and will not be overwritten'
    }
    [System.IO.File]::Move($temporaryPath, $outputPath, $false)
}
finally {
    if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) {
        [System.IO.File]::Delete($temporaryPath)
    }
}
$gate
