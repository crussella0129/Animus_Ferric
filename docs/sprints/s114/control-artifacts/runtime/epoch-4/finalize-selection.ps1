[CmdletBinding()]
param(
    [ValidateSet('e03-03-q3-32768', 'e03-04-q3-16384')]
    [string]$Q3AttemptId
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
$gatePath = Join-Path $artifactDir 'q4-viability.json'
$gateVerifierPath = Join-Path $artifactDir 'verify-q4-gate.ps1'
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$publicationPath = Join-Path $artifactDir 'recovery-publication.json'
$primaryAttemptPath = Resolve-SafeRelativePath -Root $repoRoot `
    -RelativePath ([string]$recoveryPlan.operation.destination_relative_path)
$attemptsRoot = Split-Path -Parent $primaryAttemptPath
$coverageRoot = Split-Path -Parent $artifactDir
$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'
$finalDir = Join-Path $artifactDir 'final'
$finalNames = @(
    'selection.json',
    'runtime-verification.json',
    'artifact-manifest.json',
    'artifact-manifest.sha256'
)

function Test-OrdinalSequenceEqual {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Left,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Right
    )

    if ($Left.Count -ne $Right.Count) {
        return $false
    }
    for ($index = 0; $index -lt $Left.Count; $index++) {
        if ([string]$Left[$index] -cne [string]$Right[$index]) {
            return $false
        }
    }
    $true
}

function Get-OptionalPropertyValue {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Name,
        [AllowNull()]$Default = $null
    )

    if ($null -eq $Value) {
        return $Default
    }
    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $Default
    }
    $property.Value
}

function Assert-FrozenControls {
    if (-not (Test-Path -LiteralPath $controlManifestPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $controlDigestPath -PathType Leaf)) {
        throw 'runtime controls have not been frozen'
    }

    $digestLine = (Get-Content -Raw -LiteralPath $controlDigestPath).Trim()
    if ($digestLine -notmatch '^([0-9a-f]{64})  control-inputs\.json$') {
        throw 'malformed control-inputs.sha256'
    }
    $declaredDigest = $Matches[1]
    if ((Get-Sha256Lower -Path $controlManifestPath) -cne $declaredDigest) {
        throw 'control-inputs.json digest mismatch'
    }

    $controlManifest = Get-Content -Raw -LiteralPath $controlManifestPath |
        ConvertFrom-Json -DateKind String
    $head = (& git -C $repoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw 'could not resolve repository HEAD for finalization'
    }
    $anchorPath = Join-Path $artifactDir 'raw-source-anchor.json'
    if ($controlManifest.schema -cne
            'animus-ferric-runtime-recovery-control-inputs-v4' -or
        $controlManifest.task -cne 'T-11409' -or
        $controlManifest.operation_id -cne
            [string]$recoveryPlan.operation.id -or
        [int]$controlManifest.execution_epoch -ne 3 -or
        [int]$controlManifest.publication_epoch -ne 4 -or
        $controlManifest.timestamp_protocol -cne
            [string]$recoveryPlan.timestamp_protocol -or
        $controlManifest.runtime_plan_sha256 -cne
            (Get-Sha256Lower -Path $recoveryPlanPath) -or
        $controlManifest.raw_source_anchor_sha256 -cne
            (Get-Sha256Lower -Path $anchorPath) -or
        $controlManifest.repository.head_at_freeze -cne
            [string]$recoveryPlan.repository_commit_before_epoch_4_controls -or
        $head -cne [string]$controlManifest.repository.head_at_freeze -or
        -not [bool]$controlManifest.epoch_3.passed -or
        -not [bool]$controlManifest.raw_source.terminal_facts_passed -or
        $controlManifest.raw_source.relative_path -cne
            [string]$recoveryPlan.operation.source_raw_relative_path -or
        $controlManifest.raw_source.manifest_sha256 -cne
            [string]$recoveryPlan.operation.manifest.sha256 -or
        [int]$controlManifest.raw_source.entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        -not [bool]$controlManifest.source_verification.passed -or
        [bool]$controlManifest.source_verification.hash_deferral_used -or
        -not [bool]$controlManifest.model.passed -or
        -not [bool]$controlManifest.model.independently_rehashed -or
        $controlManifest.model.relative_path -cne
            [string]$recoveryPlan.model.relative_path -or
        [UInt64]$controlManifest.model.bytes -ne
            [UInt64]$recoveryPlan.model.bytes -or
        $controlManifest.model.sha256 -cne
            [string]$recoveryPlan.model.sha256 -or
        -not [bool]$controlManifest.destination.absent_at_freeze) {
        throw 'frozen epoch-4 recovery controls are malformed or name another operation'
    }

    $seen = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($entry in @($controlManifest.static_controls)) {
        $relative = [string]$entry.path
        if ([string]::IsNullOrWhiteSpace($relative) -or
            [System.IO.Path]::IsPathRooted($relative) -or
            $relative -notmatch '^[A-Za-z0-9._-]+$' -or
            -not $seen.Add($relative)) {
            throw "unsafe or duplicate frozen control path: $relative"
        }
        $path = Join-Path $artifactDir $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "frozen runtime control is absent: $relative"
        }
        $item = Get-Item -LiteralPath $path
        if ([UInt64]$item.Length -ne [UInt64]$entry.bytes -or
            (Get-Sha256Lower -Path $path) -cne [string]$entry.sha256) {
            throw "frozen runtime control changed: $relative"
        }
    }

    $selfTestEntry = $controlManifest.runtime_self_test
    $selfTestPath = Join-Path $artifactDir 'runtime-self-test.json'
    if ($selfTestEntry.relative_path -cne
            'docs/sprints/s114/control-artifacts/runtime/epoch-4/runtime-self-test.json' -or
        -not [bool]$selfTestEntry.passed -or
        [UInt64]$selfTestEntry.bytes -ne
            [UInt64](Get-Item -LiteralPath $selfTestPath).Length -or
        $selfTestEntry.sha256 -cne (Get-Sha256Lower -Path $selfTestPath)) {
        throw 'frozen epoch-4 runtime self-test identity differs'
    }
    $expectedControlNames = @(Get-EpochFourStaticControlNames | Sort-Object)
    $observedControlNames = @(
        $controlManifest.static_controls |
            ForEach-Object { [string]$_.path } |
            Sort-Object
    )
    if (($observedControlNames -join "`n") -cne
        ($expectedControlNames -join "`n")) {
        throw 'frozen control manifest does not name the exact control set'
    }
    $controlManifest
}

function Convert-BoundedJsonReport {
    param(
        [Parameter(Mandatory = $true)]$ProcessResult,
        [Parameter(Mandatory = $true)][string]$Label
    )

    try {
        $report = [string]$ProcessResult.stdout |
            ConvertFrom-Json -DateKind String
    }
    catch {
        throw "$Label returned malformed JSON: $($ProcessResult.stderr)"
    }
    if ($null -eq $report) {
        throw "$Label returned no JSON"
    }
    if (-not [string]::IsNullOrWhiteSpace([string]$ProcessResult.stderr)) {
        throw "$Label wrote unexpected stderr: $($ProcessResult.stderr)"
    }
    $report
}

function Invoke-Q4GateVerification {
    if (-not (Test-Path -LiteralPath $gatePath -PathType Leaf)) {
        throw 'q4-viability.json is required before final selection'
    }
    if (-not (Test-Path -LiteralPath $gateVerifierPath -PathType Leaf)) {
        throw 'verify-q4-gate.ps1 is absent'
    }

    $processResult = Invoke-PowerShellFileBounded `
        -ScriptPath $gateVerifierPath -TimeoutMilliseconds 300000
    $report = Convert-BoundedJsonReport -ProcessResult $processResult `
        -Label 'Q4 gate verifier'
    if ([int]$processResult.exit_code -ne 0 -or
        $report.schema -cne 'animus-ferric-q4-gate-verification-v4' -or
        $report.task -cne 'T-11409' -or
        $report.operation_id -cne [string]$recoveryPlan.operation.id -or
        [int]$report.execution_epoch -ne 3 -or
        [int]$report.publication_epoch -ne 4 -or
        $report.timestamp_protocol -cne
            [string]$recoveryPlan.timestamp_protocol -or
        $report.attestation_protocol -cne
            [string]$plan.template_attestation.protocol -or
        $report.process_command_protocol -cne
            [string]$plan.process_command_attestation.protocol -or
        $report.mode -cne 'verify_gate' -or
        -not [bool]$report.passed) {
        throw "Q4 gate verification failed: $($report.errors -join '; ')"
    }
    $report
}

function Get-VerifiedAttempt {
    param([Parameter(Mandatory = $true)][string]$Id)

    $declared = @($plan.coordinates | Where-Object { $_.id -ceq $Id })
    if ($declared.Count -ne 1) {
        throw "attempt is not uniquely declared by the runtime plan: $Id"
    }
    $directory = Join-Path $attemptsRoot $Id
    $attemptPath = Join-Path $directory 'attempt.json'
    $manifestPath = Join-Path $directory 'files.sha256'
    if (-not (Test-Path -LiteralPath $attemptPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "runtime attempt evidence is absent: $Id"
    }

    $processResult = Invoke-PowerShellFileBounded -ScriptPath $validatorPath `
        -Arguments @('-AttemptPath', $directory) -TimeoutMilliseconds 300000
    $report = Convert-BoundedJsonReport -ProcessResult $processResult `
        -Label "runtime attempt validator ($Id)"
    if ([int]$processResult.exit_code -ne 0 -or
        $report.schema -cne
            'animus-ferric-runtime-recovery-verification-v4' -or
        $report.task -cne 'T-11409' -or
        $report.operation_id -cne [string]$recoveryPlan.operation.id -or
        [int]$report.execution_epoch -ne 3 -or
        [int]$report.publication_epoch -ne 4 -or
        $report.source_attempt_schema -cne
            'animus-ferric-runtime-attempt-v3' -or
        $report.timestamp_protocol -cne
            [string]$recoveryPlan.timestamp_protocol -or
        $report.attestation_protocol -cne
            [string]$plan.template_attestation.protocol -or
        $report.process_command_protocol -cne
            [string]$plan.process_command_attestation.protocol -or
        $report.coordinate -cne $Id -or
        $report.control_anchor_mode -cne 'epoch_4_frozen_recovery' -or
        -not [bool]$report.live_model_identity.checked -or
        $report.live_model_identity.mode -cne 'checked_in_verifier' -or
        $report.live_model_identity.sha256 -cne
            [string]$recoveryPlan.model.sha256 -or
        -not [bool]$report.passed -or
        -not [bool]$report.manifest.passed -or
        [int]$report.manifest.entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries -or
        -not [bool]$report.recovery_anchor.applicable -or
        -not [bool]$report.recovery_anchor.passed -or
        [int]$report.recovery_anchor.observed_entries -ne
            [int]$recoveryPlan.operation.exact_manifest_entries) {
        throw "runtime attempt $Id failed verification: $($report.errors -join '; ')"
    }

    $attempt = Get-Content -Raw -LiteralPath $attemptPath |
        ConvertFrom-Json -DateKind String
    if ($attempt.schema -cne 'animus-ferric-runtime-attempt-v3' -or
        [int]$attempt.control_epoch -ne 3 -or
        $attempt.task -cne 'T-11409' -or
        $attempt.attestation_protocol -cne
            [string]$plan.template_attestation.protocol -or
        $attempt.process_command_protocol -cne
            [string]$plan.process_command_attestation.protocol -or
        $attempt.coordinate -cne $Id -or
        $report.verdict -cne $attempt.verdict) {
        throw "runtime attempt $Id disagrees with its fresh verification report"
    }

    [pscustomobject]@{
        id = $Id
        attempt = $attempt
        verification = $report
        manifest_sha256 = Get-Sha256Lower -Path $manifestPath
    }
}

function Get-TerminalQ3NonViabilityBasis {
    param([Parameter(Mandatory = $true)]$Attempt)

    if ($Attempt.verdict -cne 'non_viable' -or
        -not [bool]$Attempt.startup.healthy -or
        -not [bool]$Attempt.attestation.passed -or
        -not [bool]$Attempt.teardown.passed -or
        [bool]$Attempt.wall_cap_breached -or
        $null -ne $Attempt.fatal_error -or
        @($Attempt.reason_codes).Count -ne 1) {
        return $null
    }

    $reason = [string]$Attempt.reason_codes[0]
    if ($Attempt.failure_classification -ceq 'functional_smoke_failed' -and
        $reason -ceq 'functional_smoke_failed' -and
        -not [bool]$Attempt.smoke.passed -and
        $Attempt.throughput.reason -ceq 'not_run') {
        return 'functional_smoke_failed'
    }
    if ($Attempt.failure_classification -ceq 'throughput_non_viable' -and
        [bool]$Attempt.smoke.passed -and
        -not [bool]$Attempt.throughput.passed) {
        if ($reason -cin @(
            'invalid_throughput_sample_set',
            'throughput_median_below_floor',
            'throughput_failed'
        )) {
            return $reason
        }
    }
    $null
}

function Get-ColdState {
    $localRunfile = Join-Path $repoRoot '.ferric/server.json'
    $globalRunfile = Join-Path (Join-Path $env:APPDATA 'ferric') 'server.json'
    $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $plan.port `
        -ErrorAction SilentlyContinue)
    $llamaProcesses = @(
        Get-CimInstance Win32_Process `
            -Filter "Name = 'llama-server.exe'" -ErrorAction Stop |
            Select-Object ProcessId, Name, ExecutablePath, CommandLine
    )
    $state = [ordered]@{
        checked_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        passed = $false
        local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        global_runfile_absent = -not (Test-Path -LiteralPath $globalRunfile)
        listener_absent = ($listeners.Count -eq 0)
        listener_records = @($listeners | Select-Object LocalAddress,
            LocalPort, State, OwningProcess)
        llama_server_processes_absent = ($llamaProcesses.Count -eq 0)
        llama_server_processes = $llamaProcesses
    }
    $state.passed =
        $state.local_runfile_absent -and
        $state.global_runfile_absent -and
        $state.listener_absent -and
        $state.llama_server_processes_absent
    [pscustomobject]$state
}

function Get-ObservedAttemptIds {
    if (-not (Test-Path -LiteralPath $attemptsRoot -PathType Container)) {
        throw 'runtime attempt archive is absent'
    }
    $rootFiles = @(Get-ChildItem -LiteralPath $attemptsRoot -File -Force `
        -ErrorAction Stop)
    if ($rootFiles.Count -gt 0) {
        throw 'attempt archive root contains unauthorized files'
    }
    $directories = @(Get-ChildItem -LiteralPath $attemptsRoot -Directory `
        -Force -ErrorAction Stop)
    $reparseDirectories = @($directories | Where-Object {
        ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0
    })
    if ($reparseDirectories.Count -gt 0) {
        throw 'attempt archive contains a reparse-point directory'
    }
    @($directories | Select-Object -ExpandProperty Name | Sort-Object)
}

function Assert-NoForeignFinalStages {
    param([AllowNull()][string]$AllowedName)

    $foreign = @(Get-ChildItem -LiteralPath $artifactDir -Force `
        -ErrorAction Stop | Where-Object {
            $_.Name.StartsWith(
                '.final-stage-',
                [System.StringComparison]::Ordinal
            ) -and
            ([string]::IsNullOrWhiteSpace($AllowedName) -or
                $_.Name -cne $AllowedName)
        })
    if ($foreign.Count -gt 0) {
        throw "foreign or abandoned finalization stage exists: $($foreign.Name -join ',')"
    }
}

function Get-CoverageRecords {
    param([AllowNull()][string]$StageDirectory)
    @(Get-RuntimeCoverageRecords -CoverageRoot $coverageRoot `
        -EpochArtifactDirectory $artifactDir -StageDirectory $StageDirectory)
}

function Assert-CoverageEqual {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Expected,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Actual,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if ($Expected.Count -ne $Actual.Count) {
        throw "$Label file-count mismatch"
    }
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        if ([string]$Expected[$index].path -cne [string]$Actual[$index].path -or
            [UInt64]$Expected[$index].bytes -ne [UInt64]$Actual[$index].bytes -or
            [string]$Expected[$index].sha256 -cne
                [string]$Actual[$index].sha256) {
            throw "$Label mismatch at index $index"
        }
    }
}

if (-not (Test-RecoveryPlanIdentity -Plan $recoveryPlan)) {
    throw 'epoch-4 recovery plan identity is malformed'
}
if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    throw 'source epoch-3 runtime plan identity is malformed'
}
if (Test-Path -LiteralPath $finalDir) {
    throw 'runtime/epoch-4/final already exists and will not be overwritten'
}
Assert-NoForeignFinalStages -AllowedName $null
foreach ($name in $finalNames) {
    $legacyPath = Join-Path $artifactDir $name
    if (Test-Path -LiteralPath $legacyPath) {
        throw "legacy partial final artifact blocks atomic publication: $name"
    }
}
$actualIds = @(Get-ObservedAttemptIds)

$controlManifest = Assert-FrozenControls
$gateReport = Invoke-Q4GateVerification
$q4Derivation = $gateReport.derivation
$q4Chain = @($q4Derivation.attempt_chain)
$q4Ids = @($q4Chain | ForEach-Object { [string]$_.id })
if ($q4Ids.Count -lt 1 -or $q4Ids.Count -gt 2 -or
    $q4Ids[0] -cne 'e03-01-q4-32768' -or
    ($q4Ids.Count -eq 2 -and $q4Ids[1] -cne 'e03-02-q4-16384') -or
    [string]$q4Derivation.selected_attempt -cne $q4Ids[$q4Ids.Count - 1]) {
    throw 'Q4 gate derivation does not contain the declared ordered Q4 chain'
}

$expectedIds = [System.Collections.Generic.List[string]]::new()
foreach ($id in $q4Ids) {
    $expectedIds.Add($id)
}
$q3TerminalId = $null
if ($q4Derivation.q4_verdict -ceq 'viable') {
    if ([bool]$q4Derivation.q3_fallback_authorized) {
        throw 'viable Q4 derivation may not authorize Q3'
    }
    if (-not [string]::IsNullOrWhiteSpace($Q3AttemptId)) {
        throw 'Q3 attempt assertion may not be supplied when Q4 is viable'
    }
}
elseif ($q4Derivation.q4_verdict -ceq 'non_viable') {
    if ([bool]$q4Derivation.q3_fallback_authorized) {
        $q3PrimaryProbe = Get-VerifiedAttempt -Id 'e03-03-q3-32768'
        $expectedIds.Add('e03-03-q3-32768')
        $q3TerminalId = 'e03-03-q3-32768'
        if ($q3PrimaryProbe.attempt.failure_classification -ceq
                'startup_memory_pressure') {
            Get-VerifiedAttempt -Id 'e03-04-q3-16384' | Out-Null
            $expectedIds.Add('e03-04-q3-16384')
            $q3TerminalId = 'e03-04-q3-16384'
        }
        if (-not [string]::IsNullOrWhiteSpace($Q3AttemptId) -and
            $Q3AttemptId -cne $q3TerminalId) {
            throw "Q3 attempt assertion differs from derived terminal attempt: $q3TerminalId"
        }
    }
    elseif (-not [string]::IsNullOrWhiteSpace($Q3AttemptId)) {
        throw 'Q3 attempt assertion may not be supplied without Q3 authorization'
    }
}
else {
    throw "unsupported Q4 gate verdict: $($q4Derivation.q4_verdict)"
}

$expectedIdArray = @($expectedIds)
if (-not (Test-OrdinalSequenceEqual -Left $expectedIdArray -Right $actualIds)) {
    throw "retained attempt directories are not the exact authorized chain; expected=$($expectedIdArray -join ',') actual=$($actualIds -join ',')"
}

$records = [ordered]@{}
$attemptVerifications = [System.Collections.Generic.List[object]]::new()
$selectionAttemptChain = [System.Collections.Generic.List[object]]::new()
foreach ($id in $expectedIdArray) {
    $record = Get-VerifiedAttempt -Id $id
    $records.Add($id, $record)
    $attemptVerifications.Add([pscustomobject][ordered]@{
        id = $id
        report = $record.verification
    })
    $selectionAttemptChain.Add([pscustomobject][ordered]@{
        id = $id
        quant = [string]$record.attempt.quant
        context = [int]$record.attempt.context
        manifest_sha256 = [string]$record.manifest_sha256
    })
}

$q4HashCoveragePassed = $q4Chain.Count -eq $q4Ids.Count
for ($index = 0; $index -lt $q4Chain.Count; $index++) {
    $id = $q4Ids[$index]
    if ([string]$q4Chain[$index].manifest_sha256 -cne
        [string]$records[$id].manifest_sha256) {
        $q4HashCoveragePassed = $false
    }
}
if (-not $q4HashCoveragePassed) {
    throw 'fresh Q4 attempt hashes differ from the verified gate derivation'
}

$selectedAttempt = $null
$terminalAttempt = [string]$q4Derivation.selected_attempt
$finalLabel = $null
$q3Verdict = 'not_authorized'
$q3NonViabilityBasis = $null
if ($q4Derivation.q4_verdict -ceq 'viable') {
    $selectedAttempt = $terminalAttempt
    $finalLabel = 'selected_q4'
}
elseif ([bool]$q4Derivation.q3_fallback_authorized) {
    $terminalAttempt = $q3TerminalId
    $q3Attempt = $records[$q3TerminalId].attempt
    if ($q3Attempt.failure_classification -ceq 'startup_memory_pressure') {
        throw 'terminal 16384 Q3 memory pressure is infrastructure-blocked'
    }
    if ($q3Attempt.verdict -ceq 'infrastructure_blocked') {
        throw 'infrastructure-blocked Q3 evidence cannot produce a model verdict'
    }
    if ($q3Attempt.verdict -ceq 'viable') {
        $q3Verdict = 'viable'
        $selectedAttempt = $q3TerminalId
        $finalLabel = 'selected_q3'
    }
    else {
        $q3NonViabilityBasis = Get-TerminalQ3NonViabilityBasis `
            -Attempt $q3Attempt
        if ($null -eq $q3NonViabilityBasis) {
            throw 'terminal Q3 evidence is not an allowed functional or throughput verdict'
        }
        $q3Verdict = 'non_viable'
        $finalLabel = [string]$plan.selection.terminal_failure
        if ($finalLabel -cne 'no_viable_qwen38_coordinate') {
            throw 'runtime plan terminal failure label is malformed'
        }
    }
}
else {
    $finalLabel = [string]$plan.selection.terminal_failure
    if ($finalLabel -cne 'no_viable_qwen38_coordinate') {
        throw 'runtime plan terminal failure label is malformed'
    }
}

$recordList = @($expectedIdArray | ForEach-Object { $records[$_] })
$allVerificationsPassed = @($recordList | Where-Object {
    -not [bool]$_.verification.passed -or
    -not [bool]$_.verification.manifest.passed
}).Count -eq 0
$allAttemptTeardownsPassed = @($recordList | Where-Object {
    -not [bool]$_.attempt.teardown.passed
}).Count -eq 0

$attestationProtocolPassed = $allVerificationsPassed
$smokeProtocolPassed = $allVerificationsPassed
$fallbackProtocolPassed = $allVerificationsPassed
for ($index = 0; $index -lt $expectedIdArray.Count; $index++) {
    $id = $expectedIdArray[$index]
    $attempt = $records[$id].attempt
    $coordinate = @($plan.coordinates | Where-Object { $_.id -ceq $id })[0]
    if ([bool]$attempt.startup.healthy) {
        if (-not [bool]$attempt.attestation.passed) {
            $attestationProtocolPassed = $false
        }
        $smokeSchema = Get-OptionalPropertyValue -Value $attempt.smoke `
            -Name 'schema'
        if ($smokeSchema -cne 'animus-ferric-qwen38-smoke-v1') {
            $smokeProtocolPassed = $false
        }
        elseif ([bool]$attempt.smoke.passed) {
            if ([int]$attempt.throughput.observed_samples -ne
                @($plan.throughput.sequence).Count) {
                $smokeProtocolPassed = $false
            }
        }
        elseif ($attempt.failure_classification -cne
                'functional_smoke_failed' -or
            $attempt.throughput.reason -cne 'not_run') {
            $smokeProtocolPassed = $false
        }
    }
    else {
        $authorizedRetry = @($plan.coordinates | Where-Object {
            $_.predecessor -ceq $id -and
            $expectedIdArray -ccontains [string]$_.id
        })
        if ($attempt.failure_classification -cne
                'startup_memory_pressure' -or
            $authorizedRetry.Count -ne 1) {
            $attestationProtocolPassed = $false
            $fallbackProtocolPassed = $false
        }
        $smokeReason = Get-OptionalPropertyValue -Value $attempt.smoke `
            -Name 'reason'
        if ($smokeReason -cne 'not_run' -or
            $attempt.throughput.reason -cne 'not_run') {
            $smokeProtocolPassed = $false
        }
    }

    if ($null -ne $coordinate.predecessor) {
        $predecessor = [string]$coordinate.predecessor
        if (-not $records.Contains($predecessor) -or
            $records[$predecessor].attempt.failure_classification -cne
                'startup_memory_pressure') {
            $fallbackProtocolPassed = $false
        }
    }
}

$selectionProtocolPassed =
    [bool]$gateReport.passed -and
    $q4HashCoveragePassed -and
    (($q4Derivation.q4_verdict -ceq 'viable' -and
        $finalLabel -ceq 'selected_q4' -and
        $selectedAttempt -ceq [string]$q4Derivation.selected_attempt -and
        $q3Verdict -ceq 'not_authorized') -or
    ($q4Derivation.q4_verdict -ceq 'non_viable' -and
        -not [bool]$q4Derivation.q3_fallback_authorized -and
        $finalLabel -ceq 'no_viable_qwen38_coordinate' -and
        $null -eq $selectedAttempt -and
        $q3Verdict -ceq 'not_authorized') -or
    ($q4Derivation.q4_verdict -ceq 'non_viable' -and
        [bool]$q4Derivation.q3_fallback_authorized -and
        (($q3Verdict -ceq 'viable' -and
            $finalLabel -ceq 'selected_q3' -and
            $selectedAttempt -ceq $q3TerminalId) -or
        ($q3Verdict -ceq 'non_viable' -and
            $finalLabel -ceq 'no_viable_qwen38_coordinate' -and
            $null -eq $selectedAttempt -and
            $null -ne $q3NonViabilityBasis))))

$coldState = Get-ColdState
$teardownProtocolPassed =
    $allAttemptTeardownsPassed -and [bool]$coldState.passed
$overallProtocolPassed =
    $attestationProtocolPassed -and
    $smokeProtocolPassed -and
    $fallbackProtocolPassed -and
    $selectionProtocolPassed -and
    $teardownProtocolPassed
if (-not $overallProtocolPassed) {
    throw 'runtime evidence failed derived finalization protocol checks'
}

$selectedCoordinate = if ($null -ne $selectedAttempt) {
    $records[$selectedAttempt].attempt
}
else {
    $null
}
$finalizedAt = (Get-Date).ToUniversalTime().ToString('o')
$selection = [ordered]@{
    schema = 'animus-ferric-qwen38-selection-v4'
    task = 'T-11409'
    operation_id = [string]$recoveryPlan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
    result = $finalLabel
    selected_attempt = $selectedAttempt
    terminal_attempt = $terminalAttempt
    selected_quant = if ($null -ne $selectedCoordinate) {
        $selectedCoordinate.quant
    }
    else { $null }
    selected_context = if ($null -ne $selectedCoordinate) {
        $selectedCoordinate.context
    }
    else { $null }
    requested_gpu_layers = if ($null -ne $selectedCoordinate) {
        $selectedCoordinate.requested_gpu_layers
    }
    else { $null }
    effective_gpu_layers = if ($null -ne $selectedCoordinate) {
        $selectedCoordinate.attestation.effective.gpu_layers
    }
    else { $null }
    median_decoded_tokens_per_second = if ($null -ne $selectedCoordinate) {
        $selectedCoordinate.throughput.median_decoded_tokens_per_second
    }
    else { $null }
    q4_verdict = [string]$q4Derivation.q4_verdict
    q4_fallback_basis = $q4Derivation.fallback_basis
    q3_verdict = $q3Verdict
    q3_non_viability_basis = $q3NonViabilityBasis
    attempt_ids = $expectedIdArray
    attempt_chain = @($selectionAttemptChain)
    recovery_plan_sha256 = Get-Sha256Lower -Path $recoveryPlanPath
    source_runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    control_inputs_sha256 = Get-Sha256Lower -Path $controlManifestPath
    recovery_publication_sha256 = Get-Sha256Lower -Path $publicationPath
    q4_gate_sha256 = Get-Sha256Lower -Path $gatePath
    validator_sha256 = Get-Sha256Lower -Path $validatorPath
    q4_gate_validator_sha256 = Get-Sha256Lower -Path $gateVerifierPath
    finalized_at_utc = $finalizedAt
}

$smokeEvidence = @(
    foreach ($id in $expectedIdArray) {
        $attempt = $records[$id].attempt
        [ordered]@{
            id = $id
            startup_result = if ([bool]$attempt.startup.healthy) {
                'healthy'
            }
            else {
                [string]$attempt.failure_classification
            }
            functional_result = if (-not [bool]$attempt.startup.healthy) {
                'not_run'
            }
            elseif ([bool]$attempt.smoke.passed) {
                'passed'
            }
            else {
                'failed'
            }
            throughput_result = if ($attempt.throughput.reason -ceq 'not_run') {
                'not_run'
            }
            elseif ([bool]$attempt.throughput.passed) {
                'passed'
            }
            else {
                'failed'
            }
            throughput_failure_causes = @(
                @(Get-OptionalPropertyValue -Value $attempt.throughput `
                    -Name 'samples' -Default @()) |
                    Where-Object { -not [bool]$_.valid } |
                    ForEach-Object { $_.failure_cause }
            )
        }
    }
)
$verification = [ordered]@{
    schema = 'animus-ferric-runtime-tests-v4'
    task = 'T-11409'
    operation_id = [string]$recoveryPlan.operation.id
    execution_epoch = 3
    publication_epoch = 4
    timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
    completed_at_utc = $finalizedAt
    passed = $overallProtocolPassed
    tests = @(
        [ordered]@{
            name = 'managed_server_coordinate_attestation'
            passed = $attestationProtocolPassed
            evidence = @($attemptVerifications)
        },
        [ordered]@{
            name = 'qwen38_grammar_nonce_smoke'
            passed = $smokeProtocolPassed
            evidence = $smokeEvidence
        },
        [ordered]@{
            name = 'runtime_failure_classification_and_context_fallback'
            passed = $fallbackProtocolPassed
            evidence = [ordered]@{
                expected_attempts = $expectedIdArray
                observed_attempts = $actualIds
                unexpected_attempts = @($actualIds | Where-Object {
                    $expectedIdArray -cnotcontains $_
                })
            }
        },
        [ordered]@{
            name = 'qwen38_quant_viability_selection'
            passed = $selectionProtocolPassed
            evidence = [ordered]@{
                gate_verification = $gateReport
                selection = $selection
            }
        }
    )
    teardown = [ordered]@{
        passed = $teardownProtocolPassed
        all_attempt_teardowns_passed = $allAttemptTeardownsPassed
        final_cold_state = $coldState
    }
}

$stageName = ".final-stage-$PID-$([Guid]::NewGuid().ToString('N'))"
$stageDir = Join-Path $artifactDir $stageName
$stagePublished = $false
try {
    [System.IO.Directory]::CreateDirectory($stageDir) | Out-Null
    Assert-NoForeignFinalStages -AllowedName $stageName
    $stagedSelectionPath = Join-Path $stageDir 'selection.json'
    $stagedVerificationPath = Join-Path $stageDir 'runtime-verification.json'
    $stagedManifestPath = Join-Path $stageDir 'artifact-manifest.json'
    $stagedManifestDigestPath = Join-Path $stageDir 'artifact-manifest.sha256'
    Write-JsonLf -Path $stagedSelectionPath -Value $selection
    Write-JsonLf -Path $stagedVerificationPath -Value $verification

    $coverage = @(Get-CoverageRecords -StageDirectory $stageDir)
    $manifest = [ordered]@{
        schema = 'animus-ferric-runtime-artifact-manifest-v4'
        task = 'T-11409'
        operation_id = [string]$recoveryPlan.operation.id
        execution_epoch = 3
        publication_epoch = 4
        timestamp_protocol = [string]$recoveryPlan.timestamp_protocol
        attestation_protocol = [string]$plan.template_attestation.protocol
        process_command_protocol =
            [string]$plan.process_command_attestation.protocol
        generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        coverage_root = 'runtime/'
        excluded_self_paths = @(
            'epoch-4/final/artifact-manifest.json',
            'epoch-4/final/artifact-manifest.sha256'
        )
        reserved_ephemeral_directory_prefix = '.final-stage-'
        files = $coverage
    }
    Write-JsonLf -Path $stagedManifestPath -Value $manifest
    $manifestHash = Get-Sha256Lower -Path $stagedManifestPath
    Write-Utf8Lf -Path $stagedManifestDigestPath `
        -Text "$manifestHash  artifact-manifest.json`n"

    $manifestRoundTrip = Get-Content -Raw -LiteralPath $stagedManifestPath |
        ConvertFrom-Json -DateKind String
    $roundTripCoverage = @($manifestRoundTrip.files)
    $freshCoverage = @(Get-CoverageRecords -StageDirectory $stageDir)
    Assert-CoverageEqual -Expected $roundTripCoverage -Actual $freshCoverage `
        -Label 'prepublication artifact coverage'
    $digestLine = (Get-Content -Raw -LiteralPath $stagedManifestDigestPath).Trim()
    if ($digestLine -cne "$manifestHash  artifact-manifest.json" -or
        (Get-Sha256Lower -Path $stagedManifestPath) -cne $manifestHash) {
        throw 'staged artifact-manifest digest verification failed'
    }

    $lastColdState = Get-ColdState
    if (-not [bool]$lastColdState.passed) {
        throw 'runtime finalization lost cold managed-server state before publication'
    }
    $lastAttemptIds = @(Get-ObservedAttemptIds)
    if (-not (Test-OrdinalSequenceEqual -Left $expectedIdArray `
        -Right $lastAttemptIds)) {
        throw 'authorized attempt-directory set changed before publication'
    }
    Assert-NoForeignFinalStages -AllowedName $stageName
    $lastCoverage = @(Get-CoverageRecords -StageDirectory $stageDir)
    Assert-CoverageEqual -Expected $roundTripCoverage -Actual $lastCoverage `
        -Label 'atomic-publication artifact coverage'

    [System.IO.Directory]::Move($stageDir, $finalDir)
    $stagePublished = $true

    $publishedManifestPath = Join-Path $finalDir 'artifact-manifest.json'
    $publishedDigestPath = Join-Path $finalDir 'artifact-manifest.sha256'
    if ((Get-Sha256Lower -Path $publishedManifestPath) -cne $manifestHash -or
        (Get-Content -Raw -LiteralPath $publishedDigestPath).Trim() -cne
            "$manifestHash  artifact-manifest.json") {
        throw 'published artifact-manifest digest verification failed'
    }
    $publishedCoverage = @(Get-CoverageRecords -StageDirectory $null)
    Assert-CoverageEqual -Expected $roundTripCoverage -Actual $publishedCoverage `
        -Label 'published artifact coverage'
}
finally {
    if (-not $stagePublished -and
        (Test-Path -LiteralPath $stageDir -PathType Container)) {
        $fullStage = [System.IO.Path]::GetFullPath($stageDir)
        $fullArtifact = [System.IO.Path]::GetFullPath($artifactDir)
        if ((Split-Path -Parent $fullStage) -cne $fullArtifact -or
            -not (Split-Path -Leaf $fullStage).StartsWith(
                '.final-stage-',
                [System.StringComparison]::Ordinal
            )) {
            throw 'refusing to clean an unexpected staging directory'
        }
        [System.IO.Directory]::Delete($fullStage, $true)
    }
}

$selection
