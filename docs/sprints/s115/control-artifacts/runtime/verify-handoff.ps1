[CmdletBinding()]
param(
    [ValidatePattern('^(latest|[0-9]{3})$')]
    [string]$Attempt = 'latest',
    [switch]$CheckLive
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$requestedAttempt = $Attempt
$requestedLive = [bool]$CheckLive
. (Join-Path $PSScriptRoot 'verify-runtime.ps1') -Attempt $requestedAttempt
$Attempt = $requestedAttempt
$CheckLive = $requestedLive

function Invoke-S115HandoffVerification {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$AttemptId,
        [switch]$Live
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $offline = Invoke-S115RuntimeVerification -AttemptId $AttemptId
    if (-not $offline.passed) {
        $errors.Add("offline runtime verification failed: $($offline.errors -join '; ')")
    }
    $context = Get-S115Context
    $resolved = Resolve-S115AttemptDirectory -Context $context `
        -Attempt $AttemptId
    $handoffPath = Join-Path $resolved.path 'handoff.json'
    if (-not (Test-Path -LiteralPath $handoffPath -PathType Leaf)) {
        throw 'handoff.json is absent'
    }
    $handoff = Read-S115EvidenceJson -Path $handoffPath
    $result = Read-S115EvidenceJson -Path (Join-Path `
        $resolved.path 'result.json')
    $launch = Read-S115EvidenceJson -Path (Join-Path `
        $resolved.path 'launch-command.json')
    $log = Read-S115EvidenceJson -Path (Join-Path `
        $resolved.path 'server-log-attestation.json')
    $binding = Read-S115EvidenceJson -Path (Join-Path `
        $resolved.path 'final-binding.json')
    $control = Assert-S115ControlInputs -Context $context
    $attemptSourceManifestSha256 =
        [string]$offline.control_binding.attempt_source_manifest_sha256
    $attemptRuntimePlanSha256 =
        [string]$offline.control_binding.attempt_runtime_plan_sha256
    $controlCompatibility = Test-S115VerifierControlManifestCompatibility `
        -AttemptId $resolved.id `
        -AttemptSourceManifestSha256 $attemptSourceManifestSha256 `
        -CurrentManifestSha256 ([string]$control.manifest_sha256)
    $creationEquivalent = Test-S115UtcInstantEquivalent `
        -Left $handoff.process.creation_utc -Right $binding.process.creation_utc
    $canonicalCreationUtc = ConvertTo-S115CanonicalUtcInstant `
        -Value $handoff.process.creation_utc
    $coordinateEquivalent = Test-JsonEquivalent -Left $handoff.coordinate `
        -Right $context.plan.coordinate
    $handoffEnvironmentEquivalent = Test-JsonEquivalent `
        -Left $handoff.frozen_launch.environment -Right $launch.environment
    if ($handoff.state -cne 'qualified_running' -or
        $result.state -cne 'qualified_running' -or
        -not [bool]$result.passed -or
        [string]$result.handoff_sha256 -cne (Get-Sha256Lower -Path $handoffPath) -or
        [string]$handoff.endpoint -cne 'http://127.0.0.1:8080/v1' -or
        [string]$handoff.served_model_id -cne $context.model_path -or
        [UInt64]$handoff.served_n_params -ne
            [UInt64]$context.plan.model.parameters -or
        -not $coordinateEquivalent -or
        [string]$handoff.process.executable_sha256 -cne
            [string]$context.plan.engine.binary_sha256 -or
        [UInt32]$handoff.process.pid -ne [UInt32]$binding.process.pid -or
        -not $creationEquivalent.passed -or
        [string]$handoff.process.executable_path -cne
            [string]$binding.process.executable_path -or
        [string]$handoff.process.command_line -cne
            [string]$binding.process.command_line -or
        (@($handoff.process.expected_argv) -join "`n") -cne
            (@(Get-S115ExpectedChildArgv -Context $context) -join "`n") -or
        [int]$handoff.counts.launch -ne 1 -or
        [int]$handoff.counts.fallback -ne 0 -or
        [int]$handoff.counts.download -ne 0 -or
        [int]$handoff.counts.restart -ne 0 -or
        [int]$handoff.counts.smoke -ne 1 -or
        [int]$handoff.counts.throughput -ne 4 -or
        [int]$handoff.counts.throughput_replacement -ne 0 -or
        [string]$handoff.frozen_launch.declaration_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path 'launch-command.json')) -or
        -not $handoffEnvironmentEquivalent -or
        (@($launch.expected_child_argv) -join "`n") -cne
            (@($handoff.process.expected_argv) -join "`n") -or
        [UInt64]$handoff.server_log_prefix.bytes -ne
            [UInt64]$log.prefix.bytes -or
        [string]$handoff.server_log_prefix.sha256 -cne
            [string]$log.prefix.sha256 -or
        [string]$handoff.server_log_facts_sha256 -cne
            [string]$log.facts.sha256 -or
        [string]$handoff.runfiles.local_path -cne
            [string]$binding.runfiles.local.path -or
        [string]$handoff.runfiles.local_sha256 -cne
            [string]$binding.runfiles.local.sha256 -or
        [string]$handoff.runfiles.global_path -cne
            [string]$binding.runfiles.global.path -or
        [string]$handoff.runfiles.global_sha256 -cne
            [string]$binding.runfiles.global.sha256 -or
        [string]$handoff.identities.ferric_sha256 -cne
            [string]$context.plan.qualified_release.binary_sha256 -or
        [string]$handoff.identities.model_sha256 -cne
            [string]$context.plan.model.sha256 -or
        [string]$handoff.identities.engine_sha256 -cne
            [string]$context.plan.engine.binary_sha256 -or
        [string]$handoff.identities.cuda_backend_sha256 -cne
            [string]$context.plan.engine.cuda_backend_sha256 -or
        [string]$handoff.identities.runtime_tree_manifest_sha256 -cne
            [string]$context.plan.engine.source_manifest_sha256 -or
        [string]$handoff.identities.release_result_sha256 -cne
            [string]$context.plan.qualified_release.result_sha256 -or
        [string]$handoff.identities.release_source_commit -cne
            [string]$context.plan.qualified_release.source_commit -or
        -not $controlCompatibility.passed -or
        [string]$handoff.identities.control_manifest_sha256 -cne
            $attemptSourceManifestSha256 -or
        [string]$handoff.identities.runtime_plan_sha256 -cne
            $attemptRuntimePlanSha256 -or
        [string]$handoff.evidence.engine_resolution_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path `
                'engine-resolution.json')) -or
        @($handoff.listeners).Count -ne 1 -or
        [string]$handoff.listeners[0].LocalAddress -cne '127.0.0.1' -or
        [int]$handoff.listeners[0].LocalPort -ne 8080 -or
        [UInt32]$handoff.listeners[0].OwningProcess -ne
            [UInt32]$handoff.process.pid -or
        [string]$handoff.disposition -cne
            'leave_same_bound_process_running') {
        $errors.Add('handoff does not encode the exact immutable running identity')
    }
    $liveResult = $null
    if ($Live -and $errors.Count -eq 0) {
        $liveResult = Test-S115LiveHandoff -Context $context -Handoff $handoff
        if (-not $liveResult.passed) {
            $errors.Add("live handoff changed: $($liveResult.errors -join '; ')")
        }
    }
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-handoff-verification-v1'
        passed = $errors.Count -eq 0
        attempt = $resolved.id
        mode = if ($Live) { 'offline-plus-live' } else { 'offline' }
        pid = [UInt32]$handoff.process.pid
        creation_utc = $canonicalCreationUtc
        endpoint = [string]$handoff.endpoint
        offline_runtime = $offline
        live = $liveResult
        errors = @($errors)
    }
}

if ($MyInvocation.InvocationName -cne '.') {
    try {
        $verification = Invoke-S115HandoffVerification `
            -AttemptId $Attempt -Live:$CheckLive
        $verification | ConvertTo-Json -Depth 64
        if (-not $verification.passed) { exit 1 }
    }
    catch {
        [pscustomobject]@{
            schema = 'animus-ferric-s115-handoff-verification-v1'
            passed = $false
            attempt = $Attempt
            errors = @($_.Exception.ToString())
        } | ConvertTo-Json -Depth 16
        exit 1
    }
}
