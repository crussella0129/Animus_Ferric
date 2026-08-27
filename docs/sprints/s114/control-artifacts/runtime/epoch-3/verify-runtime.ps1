[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AttemptPath,
    [switch]$DeferLiveModelHashToFreeze
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath |
    ConvertFrom-Json
$resolvedAttempt = (Resolve-Path -LiteralPath $AttemptPath).Path
$errors = [System.Collections.Generic.List[string]]::new()

if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    $errors.Add('runtime plan does not declare the frozen epoch-3 protocol')
}
$recoveryAnchors = Test-RecoveryAnchors -Plan $plan -RepositoryRoot $repoRoot
foreach ($anchorError in @($recoveryAnchors.errors)) {
    $errors.Add([string]$anchorError)
}
$measurementContinuity = Test-EpochThreeMeasurementContinuity `
    -Plan $plan -RepositoryRoot $repoRoot
foreach ($continuityError in @($measurementContinuity.errors)) {
    $errors.Add([string]$continuityError)
}

function Add-Error {
    param([Parameter(Mandatory = $true)][string]$Message)
    $errors.Add($Message)
}

function Read-JsonLines {
    param([Parameter(Mandatory = $true)][string]$Path)
    @(
        Get-Content -LiteralPath $Path |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
}

function Get-RetainedJson {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $path = Join-Path $resolvedAttempt $Name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        Add-Error "$Label artifact is absent: $Name"
        return $null
    }
    try {
        Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
    }
    catch {
        Add-Error "$Label artifact is not valid JSON: $Name"
        $null
    }
}

function Test-ExchangeFile {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Exchange,
        [Parameter(Mandatory = $true)][string]$Label,
        [string]$ExpectedMethod,
        [string]$ExpectedUri,
        [string]$ExpectedResponseFile,
        [Nullable[int]]$ExpectedTimeoutSeconds,
        [switch]$RequireSuccess
    )

    $responseFile = Get-OptionalProperty -Value $Exchange -Name 'response_file'
    if ([string]::IsNullOrWhiteSpace([string]$responseFile)) {
        Add-Error "$Label exchange has no response file"
        return $false
    }
    $responsePath = Join-Path $resolvedAttempt ([string]$responseFile)
    if (-not (Test-Path -LiteralPath $responsePath -PathType Leaf)) {
        Add-Error "$Label response file is absent"
        return $false
    }
    $item = Get-Item -LiteralPath $responsePath
    $passed = $true
    $recordedBytes = Get-OptionalProperty -Value $Exchange -Name 'response_bytes'
    $recordedHash = Get-OptionalProperty -Value $Exchange -Name 'response_sha256'
    if ($null -eq $recordedBytes -or
        [UInt64]$item.Length -ne [UInt64]$recordedBytes) {
        Add-Error "$Label response byte count differs"
        $passed = $false
    }
    if ([string]::IsNullOrWhiteSpace([string]$recordedHash) -or
        (Get-Sha256Lower -Path $responsePath) -ne [string]$recordedHash) {
        Add-Error "$Label response hash differs"
        $passed = $false
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedMethod) -and
        [string](Get-OptionalProperty -Value $Exchange -Name 'method') -ne
            $ExpectedMethod) {
        Add-Error "$Label method differs"
        $passed = $false
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedUri) -and
        [string](Get-OptionalProperty -Value $Exchange -Name 'uri') -ne
            $ExpectedUri) {
        Add-Error "$Label URI differs"
        $passed = $false
    }
    if (-not [string]::IsNullOrWhiteSpace($ExpectedResponseFile) -and
        [string]$responseFile -ne $ExpectedResponseFile) {
        Add-Error "$Label response filename differs"
        $passed = $false
    }
    $recordedTimeout = Get-OptionalProperty -Value $Exchange `
        -Name 'timeout_seconds'
    if ($null -ne $ExpectedTimeoutSeconds -and
        ($null -eq $recordedTimeout -or
            [int]$recordedTimeout -ne [int]$ExpectedTimeoutSeconds)) {
        Add-Error "$Label timeout differs"
        $passed = $false
    }
    $startedText = Get-OptionalProperty -Value $Exchange -Name 'started_at_utc'
    $completedText = Get-OptionalProperty -Value $Exchange -Name 'completed_at_utc'
    $wallMs = Get-OptionalProperty -Value $Exchange -Name 'wall_ms'
    $started = [DateTimeOffset]::MinValue
    $completed = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse(
            [string]$startedText,
            [ref]$started
        ) -or
        -not [DateTimeOffset]::TryParse(
            [string]$completedText,
            [ref]$completed
        ) -or
        $completed -lt $started -or
        $null -eq $wallMs -or
        -not [double]::IsFinite([double]$wallMs) -or
        [double]$wallMs -lt 0 -or
        [Math]::Abs(
            [double]$wallMs - ($completed - $started).TotalMilliseconds
        ) -gt 5000 -or
        ($null -ne $recordedTimeout -and
            [double]$wallMs -gt ([double]$recordedTimeout * 1000.0 + 5000.0))) {
        Add-Error "$Label timing metadata is invalid"
        $passed = $false
    }
    $status = Get-OptionalProperty -Value $Exchange -Name 'status_code'
    $exchangeError = Get-OptionalProperty -Value $Exchange -Name 'error'
    if ($RequireSuccess) {
        if ($null -ne $exchangeError -or $null -eq $status -or
            [int]$status -lt 200 -or [int]$status -ge 300) {
            Add-Error "$Label did not retain a successful exchange"
            $passed = $false
        }
    }
    elseif (($null -eq $exchangeError -and $null -eq $status) -or
        ($null -ne $exchangeError -and $null -ne $status)) {
        Add-Error "$Label must record exactly one HTTP or transport outcome"
        $passed = $false
    }
    $passed
}

function Invoke-VerificationChild {
    param([Parameter(Mandatory = $true)][string]$ChildAttemptPath)

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = [Environment]::ProcessPath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-File', $PSCommandPath,
        '-AttemptPath', $ChildAttemptPath
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw 'could not start predecessor verifier'
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(300000)) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit(5000)
            throw 'predecessor verifier exceeded 300 seconds'
        }
        $tasks = [System.Threading.Tasks.Task[]]@($stdoutTask, $stderrTask)
        if (-not [System.Threading.Tasks.Task]::WaitAll($tasks, 5000)) {
            throw 'predecessor verifier output did not close promptly'
        }
        [pscustomobject]@{
            exit_code = $process.ExitCode
            stdout = $stdoutTask.GetAwaiter().GetResult()
            stderr = $stderrTask.GetAwaiter().GetResult()
        }
    }
    finally {
        $process.Dispose()
    }
}

function Test-MemorySnapshot {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Snapshot,
        [AllowNull()][Parameter(Mandatory = $true)]$ExpectedGpu,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if ($null -eq $Snapshot) {
        Add-Error "$Label memory snapshot is absent"
        return $false
    }
    $passed = $true
    $capturedAt = Get-OptionalProperty -Value $Snapshot -Name 'captured_at_utc'
    $captured = [DateTimeOffset]::MinValue
    if (-not [DateTimeOffset]::TryParse([string]$capturedAt, [ref]$captured)) {
        Add-Error "$Label memory timestamp is invalid"
        $passed = $false
    }
    $ram = Get-OptionalProperty -Value $Snapshot -Name 'ram'
    $totalPhysical = Get-OptionalProperty -Value $ram -Name 'total_visible_bytes'
    $freePhysical = Get-OptionalProperty -Value $ram -Name 'free_physical_bytes'
    $totalVirtual = Get-OptionalProperty -Value $ram -Name 'total_virtual_bytes'
    $freeVirtual = Get-OptionalProperty -Value $ram -Name 'free_virtual_bytes'
    if ($null -eq $totalPhysical -or $null -eq $freePhysical -or
        $null -eq $totalVirtual -or $null -eq $freeVirtual -or
        [UInt64]$totalPhysical -eq 0 -or
        [UInt64]$freePhysical -gt [UInt64]$totalPhysical -or
        [UInt64]$totalVirtual -eq 0 -or
        [UInt64]$freeVirtual -gt [UInt64]$totalVirtual) {
        Add-Error "$Label RAM snapshot is invalid"
        $passed = $false
    }
    $gpu = Get-OptionalProperty -Value $Snapshot -Name 'gpu'
    $gpuTotal = Get-OptionalProperty -Value $gpu -Name 'total_mib'
    $gpuFree = Get-OptionalProperty -Value $gpu -Name 'free_mib'
    $gpuUsed = Get-OptionalProperty -Value $gpu -Name 'used_mib'
    $gpuUtilization = Get-OptionalProperty -Value $gpu -Name 'utilization_percent'
    if ($null -eq $gpu -or
        [string]::IsNullOrWhiteSpace(
            [string](Get-OptionalProperty -Value $gpu -Name 'name')
        ) -or
        [string]::IsNullOrWhiteSpace(
            [string](Get-OptionalProperty -Value $gpu -Name 'uuid')
        ) -or
        [string]::IsNullOrWhiteSpace(
            [string](Get-OptionalProperty -Value $gpu -Name 'driver_version')
        ) -or
        $null -eq $gpuTotal -or $null -eq $gpuFree -or
        $null -eq $gpuUsed -or $null -eq $gpuUtilization -or
        [UInt64]$gpuTotal -eq 0 -or
        [UInt64]$gpuFree -gt [UInt64]$gpuTotal -or
        [UInt64]$gpuUsed -gt [UInt64]$gpuTotal -or
        [UInt32]$gpuUtilization -gt 100) {
        Add-Error "$Label GPU snapshot is invalid"
        $passed = $false
    }
    $gpuCsv = @(Get-OptionalProperty -Value $Snapshot `
        -Name 'nvidia_smi_gpu_csv')
    $gpuCsvFields = if ($gpuCsv.Count -eq 1) {
        @([string]$gpuCsv[0] -split ',' | ForEach-Object { $_.Trim() })
    }
    else {
        @()
    }
    if ($null -ne $gpu -and
        ($gpuCsvFields.Count -ne 7 -or
            $gpuCsvFields[0] -ne [string]$gpu.name -or
            $gpuCsvFields[1] -ne [string]$gpu.uuid -or
            $gpuCsvFields[2] -ne [string]$gpu.driver_version -or
            [UInt64]$gpuCsvFields[3] -ne [UInt64]$gpu.total_mib -or
            [UInt64]$gpuCsvFields[4] -ne [UInt64]$gpu.free_mib -or
            [UInt64]$gpuCsvFields[5] -ne [UInt64]$gpu.used_mib -or
            [UInt32]$gpuCsvFields[6] -ne [UInt32]$gpu.utilization_percent)) {
        Add-Error "$Label GPU object is not derived from retained nvidia-smi CSV"
        $passed = $false
    }
    if ($null -ne $gpu -and $null -ne $ExpectedGpu -and
        (
            [string]$gpu.name -ne [string]$ExpectedGpu.name -or
            [string]$gpu.uuid -ne [string]$ExpectedGpu.uuid -or
            [string]$gpu.driver_version -ne
                [string]$ExpectedGpu.driver_version -or
            [UInt64]$gpu.total_mib -ne [UInt64]$ExpectedGpu.total_mib
        )) {
        Add-Error "$Label GPU identity differs from the frozen device"
        $passed = $false
    }
    $passed
}

function Test-ProcessRecord {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][string]$ExpectedFile,
        [Parameter(Mandatory = $true)][string[]]$ExpectedArguments,
        [Parameter(Mandatory = $true)][string]$ExpectedStdoutFile,
        [Parameter(Mandatory = $true)][string]$ExpectedStderrFile,
        [Parameter(Mandatory = $true)][string]$Label,
        [switch]$RequireArgumentLine
    )

    if ($null -eq $Record) {
        Add-Error "$Label process record is absent"
        return $false
    }
    $passed = $true
    $requiredRecordProperties = @(
        'file',
        'arguments',
        'pid',
        'started_at_utc',
        'completed_at_utc',
        'duration_ms',
        'timed_out',
        'execution_timed_out',
        'kill_attempted',
        'kill_succeeded',
        'post_process_alive',
        'exit_code',
        'stdout_file',
        'stderr_file'
    ) + $(if ($RequireArgumentLine) {
        @('argument_line', 'post_kill_wait_timed_out')
    }
    else {
        @('output_drain_timed_out')
    })
    $missingRecordProperties = @(
        $requiredRecordProperties | Where-Object {
            $null -eq $Record.PSObject.Properties[$_]
        }
    )
    if ($missingRecordProperties.Count -gt 0) {
        Add-Error "$Label process record lacks fields: $($missingRecordProperties -join ', ')"
        $passed = $false
    }
    $recordFile = Get-OptionalProperty -Value $Record -Name 'file'
    if ([string]::IsNullOrWhiteSpace([string]$recordFile) -or
        -not [System.IO.Path]::GetFullPath([string]$recordFile).Equals(
            [System.IO.Path]::GetFullPath($ExpectedFile),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -or
        (@(Get-OptionalProperty -Value $Record -Name 'arguments') -join "`n") -ne
            ($ExpectedArguments -join "`n") -or
        [string](Get-OptionalProperty -Value $Record -Name 'stdout_file') -ne
            $ExpectedStdoutFile -or
        [string](Get-OptionalProperty -Value $Record -Name 'stderr_file') -ne
            $ExpectedStderrFile) {
        Add-Error "$Label process identity differs from the declared command"
        $passed = $false
    }
    if ($RequireArgumentLine) {
        $expectedArgumentLine = (@($ExpectedArguments | ForEach-Object {
            ConvertTo-WindowsCommandLineArgument -Argument ([string]$_)
        }) -join ' ')
        if ([string](Get-OptionalProperty -Value $Record -Name 'argument_line') -ne
            $expectedArgumentLine) {
            Add-Error "$Label process argument line differs"
            $passed = $false
        }
    }
    $recordPid = Get-OptionalProperty -Value $Record -Name 'pid'
    $aggregateTimedOut = [bool](Get-OptionalProperty -Value $Record `
        -Name 'timed_out')
    $executionTimedOut = [bool](Get-OptionalProperty -Value $Record `
        -Name 'execution_timed_out')
    $killAttempted = [bool](Get-OptionalProperty -Value $Record `
        -Name 'kill_attempted')
    $killSucceeded = [bool](Get-OptionalProperty -Value $Record `
        -Name 'kill_succeeded')
    $postProcessAlive = [bool](Get-OptionalProperty -Value $Record `
        -Name 'post_process_alive')
    $secondaryTimeoutName = if ($RequireArgumentLine) {
        'post_kill_wait_timed_out'
    }
    else {
        'output_drain_timed_out'
    }
    $secondaryTimeoutProperty = $Record.PSObject.Properties[$secondaryTimeoutName]
    $secondaryTimedOut = [bool](Get-OptionalProperty -Value $Record `
        -Name $secondaryTimeoutName)
    $exitCode = Get-OptionalProperty -Value $Record -Name 'exit_code'
    if ($null -eq $recordPid -or [UInt32]$recordPid -eq 0 -or
        $null -eq $Record.PSObject.Properties['execution_timed_out'] -or
        $null -eq $secondaryTimeoutProperty -or
        $aggregateTimedOut -ne ($executionTimedOut -or $secondaryTimedOut) -or
        ($aggregateTimedOut -and $null -ne $exitCode) -or
        (-not $aggregateTimedOut -and $null -eq $exitCode) -or
        ($executionTimedOut -and -not $killAttempted) -or
        ($killSucceeded -and -not $killAttempted) -or
        (-not $executionTimedOut -and ($killAttempted -or $killSucceeded)) -or
        ($postProcessAlive -and -not $executionTimedOut)) {
        Add-Error "$Label process termination evidence is invalid"
        $passed = $false
    }
    $recordStarted = [DateTimeOffset]::MinValue
    $recordCompleted = [DateTimeOffset]::MinValue
    $recordDuration = Get-OptionalProperty -Value $Record -Name 'duration_ms'
    if (-not [DateTimeOffset]::TryParse(
            [string](Get-OptionalProperty -Value $Record -Name 'started_at_utc'),
            [ref]$recordStarted
        ) -or
        -not [DateTimeOffset]::TryParse(
            [string](Get-OptionalProperty -Value $Record -Name 'completed_at_utc'),
            [ref]$recordCompleted
        ) -or
        $recordCompleted -lt $recordStarted -or
        $null -eq $recordDuration -or
        -not [double]::IsFinite([double]$recordDuration) -or
        [double]$recordDuration -lt 0 -or
        [Math]::Abs(
            [double]$recordDuration -
            ($recordCompleted - $recordStarted).TotalMilliseconds
        ) -gt 5000 -or
        ($attemptChronologyPassed -and
            ($recordStarted -lt $attemptStarted -or
                $recordCompleted -gt $attemptCompleted.AddSeconds(2)))) {
        Add-Error "$Label process timing is invalid"
        $passed = $false
    }
    $passed
}

$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'
$controlManifest = $null
$controlAnchorMode = 'live_frozen'
$officialAttemptsRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $artifactDir 'attempts')
)
$isOfficialAttempt = [System.IO.Path]::GetFullPath(
    (Split-Path -Parent $resolvedAttempt)
).Equals($officialAttemptsRoot, [System.StringComparison]::OrdinalIgnoreCase)
$selfTestRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $repoRoot 'target/s114-experiment/runtime-epoch-3-selftest')
).TrimEnd(
    [System.IO.Path]::DirectorySeparatorChar,
    [System.IO.Path]::AltDirectorySeparatorChar
) + [System.IO.Path]::DirectorySeparatorChar
$controlsAbsent =
    -not (Test-Path -LiteralPath $controlManifestPath) -and
    -not (Test-Path -LiteralPath $controlDigestPath)
$isSelfTestAttempt = $resolvedAttempt.StartsWith(
    $selfTestRoot,
    [System.StringComparison]::OrdinalIgnoreCase
)
$deferLiveModelHash =
    [bool]$DeferLiveModelHashToFreeze -and
    $controlsAbsent -and
    -not $isOfficialAttempt -and
    $isSelfTestAttempt
if ($DeferLiveModelHashToFreeze -and -not $deferLiveModelHash) {
    throw 'live model hash deferral is restricted to unfrozen epoch-3 self-tests'
}
if (-not (Test-Path -LiteralPath $controlManifestPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $controlDigestPath -PathType Leaf)) {
    if ($isOfficialAttempt) {
        Add-Error 'official attempt lacks the frozen control-input manifest'
    }
    else {
        $controlAnchorMode = 'unfrozen_pure_evidence_self_test'
        $llamaBinForSelfTest = Join-Path $repoRoot `
            $plan.llama_cpp.ignored_runtime_relative_path
        $llamaForSelfTest = Join-Path $llamaBinForSelfTest 'llama-server.exe'
        $ferricForSelfTest = Join-Path $repoRoot $plan.ferric.relative_path
        $selfTestDeviceOutput = @(Invoke-BoundedTextProcess `
            -FilePath $llamaForSelfTest -Arguments @('--list-devices'))
        $selfTestDeviceObservation = Get-LlamaDeviceObservation `
            -Output $selfTestDeviceOutput
        $controlManifest = [pscustomobject]@{
            repository = [pscustomobject]@{
                head_at_freeze = 'self-test'
                epoch_1_pre_control_baseline =
                    [string]$plan.repository_commit_before_epoch_1_runtime_controls
                epoch_2_pre_control_baseline =
                    [string]$plan.repository_commit_before_epoch_2_runtime_controls
                epoch_3_pre_control_base =
                    [string]$plan.repository_commit_before_epoch_3_runtime_controls
                prior_evidence_checkpoint =
                    [string]$plan.recovery.prior_evidence_checkpoint
            }
            binaries = [pscustomobject]@{
                ferric = [pscustomobject]@{
                    bytes = [UInt64](Get-Item -LiteralPath $ferricForSelfTest).Length
                    sha256 = $plan.ferric.expected_sha256_at_freeze
                    version_output = @(Invoke-BoundedTextProcess `
                        -FilePath $ferricForSelfTest -Arguments @('--version'))
                }
                llama_server = [pscustomobject]@{
                    bytes = [UInt64](Get-Item -LiteralPath $llamaForSelfTest).Length
                    sha256 = $plan.llama_cpp.expected_server_sha256
                    version_output = @(Invoke-BoundedTextProcess `
                        -FilePath $llamaForSelfTest -Arguments @('--version'))
                    device_identity = $selfTestDeviceObservation.identity
                    device_output_at_freeze = $selfTestDeviceOutput
                    device_free_mib_at_freeze =
                        [UInt64]$selfTestDeviceObservation.free_mib
                    help_output_sha256 = Get-Sha256Text -Text (
                        (@(Invoke-BoundedTextProcess -FilePath $llamaForSelfTest `
                            -Arguments @('--help')) -join "`n") + "`n"
                    )
                }
                llama_runtime = [pscustomobject]@{
                    files = @(Get-FileIdentityManifest -Root $llamaBinForSelfTest)
                }
            }
            cold_state = [pscustomobject]@{
                memory = Get-MemorySnapshot
            }
        }
    }
}
else {
    $digestLine = (Get-Content -Raw -LiteralPath $controlDigestPath).Trim()
    if ($digestLine -notmatch '^([0-9a-f]{64})  control-inputs\.json$' -or
        (Get-Sha256Lower -Path $controlManifestPath) -ne $Matches[1]) {
        Add-Error 'frozen control-input manifest digest is invalid'
    }
    else {
        $controlManifest = Get-Content -Raw -LiteralPath $controlManifestPath |
            ConvertFrom-Json
        if ($controlManifest.schema -cne
                'animus-ferric-runtime-control-inputs-v3' -or
            $controlManifest.task -cne 'T-11409' -or
            [int]$controlManifest.control_epoch -ne 3 -or
            $controlManifest.attestation_protocol -cne
                [string]$plan.template_attestation.protocol -or
            $controlManifest.process_command_protocol -cne
                [string]$plan.process_command_attestation.protocol -or
            -not (Test-JsonEquivalent `
                -Left @($controlManifest.prior_epochs) `
                -Right @($plan.recovery.prior_epochs)) -or
            -not (Test-JsonEquivalent `
                -Left $controlManifest.recovery_anchors `
                -Right $recoveryAnchors) -or
            -not (Test-JsonEquivalent `
                -Left $controlManifest.measurement_continuity `
                -Right $measurementContinuity) -or
            $controlManifest.repository.head_at_freeze -cne
                [string]$plan.repository_commit_before_epoch_3_runtime_controls -or
            $controlManifest.repository.epoch_1_pre_control_baseline -cne
                [string]$plan.repository_commit_before_epoch_1_runtime_controls -or
            $controlManifest.repository.epoch_2_pre_control_baseline -cne
                [string]$plan.repository_commit_before_epoch_2_runtime_controls -or
            $controlManifest.repository.epoch_3_pre_control_base -cne
                [string]$plan.repository_commit_before_epoch_3_runtime_controls -or
            $controlManifest.repository.prior_evidence_checkpoint -cne
                [string]$plan.recovery.prior_evidence_checkpoint) {
            Add-Error 'frozen control-input manifest is not epoch 3'
        }
        if ($controlManifest.runtime_plan_sha256 -ne
            (Get-Sha256Lower -Path $planPath)) {
            Add-Error 'runtime plan differs from the frozen control input'
        }
        $expectedControlNames = @(
            @(Get-EpochThreeStaticControlNames) + @('runtime-self-test.json') |
                Sort-Object
        )
        $observedControlNames = @(
            $controlManifest.controls | ForEach-Object { [string]$_.path } |
                Sort-Object
        )
        if (($observedControlNames -join "`n") -cne
            ($expectedControlNames -join "`n")) {
            Add-Error 'frozen manifest does not name the exact control set'
        }
        $seenControls = [System.Collections.Generic.HashSet[string]]::new(
            [System.StringComparer]::OrdinalIgnoreCase
        )
        foreach ($control in @($controlManifest.controls)) {
            $relative = [string]$control.path
            if ($relative -notmatch '^[A-Za-z0-9._-]+$' -or
                -not $seenControls.Add($relative)) {
                Add-Error "unsafe or duplicate frozen control path: $relative"
                continue
            }
            $controlPath = Resolve-SafeRelativePath -Root $artifactDir `
                -RelativePath $relative
            if (-not (Test-Path -LiteralPath $controlPath -PathType Leaf) -or
                [UInt64](Get-Item -LiteralPath $controlPath).Length -ne
                    [UInt64]$control.bytes -or
                (Get-Sha256Lower -Path $controlPath) -cne
                    [string]$control.sha256) {
                Add-Error "frozen runtime control differs: $relative"
            }
        }
    }
}

$attemptPath = Join-Path $resolvedAttempt 'attempt.json'
$manifestPath = Join-Path $resolvedAttempt 'files.sha256'
if (-not (Test-Path -LiteralPath $attemptPath -PathType Leaf)) {
    Add-Error 'attempt.json is absent'
}
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Add-Error 'files.sha256 is absent'
}
if ($errors.Count -gt 0) {
    $early = [ordered]@{
        schema = 'animus-ferric-runtime-verification-v3'
        task = 'T-11409'
        control_epoch = 3
        attestation_protocol = [string]$plan.template_attestation.protocol
        process_command_protocol =
            [string]$plan.process_command_attestation.protocol
        attempt_path = $resolvedAttempt
        passed = $false
        errors = @($errors)
    }
    $early | ConvertTo-Json -Depth 16
    exit 1
}

$manifestCheck = Test-HashManifest -Root $resolvedAttempt `
    -ManifestPath $manifestPath `
    -RejectUnlistedFiles
if (-not $manifestCheck.passed) {
    foreach ($message in $manifestCheck.errors) {
        Add-Error "manifest: $message"
    }
}
$listedPaths = @(
    Get-Content -LiteralPath $manifestPath |
        Where-Object { $_ -match '^[0-9a-f]{64}  (.+)$' } |
        ForEach-Object {
            $_ -match '^[0-9a-f]{64}  (.+)$' | Out-Null
            $Matches[1]
        } |
        Sort-Object
)
$actualPaths = @(
    Get-ChildItem -LiteralPath $resolvedAttempt -Recurse -File -Force |
        Where-Object { $_.FullName -ne $manifestPath } |
        ForEach-Object {
            Get-RelativeSlashPath -Root $resolvedAttempt -Path $_.FullName
        } |
        Sort-Object
)
if (($listedPaths -join "`n") -ne ($actualPaths -join "`n")) {
    Add-Error 'manifest does not cover exactly every retained non-manifest file'
}

$attempt = Get-Content -Raw -LiteralPath $attemptPath | ConvertFrom-Json
if ($attempt.schema -ne 'animus-ferric-runtime-attempt-v3' -or
    [int]$attempt.control_epoch -ne 3 -or
    $attempt.task -cne 'T-11409' -or
    $attempt.attestation_protocol -ne
        [string]$plan.template_attestation.protocol -or
    $attempt.process_command_protocol -cne
        [string]$plan.process_command_attestation.protocol) {
    Add-Error 'attempt schema mismatch'
}
$coordinate = @($plan.coordinates | Where-Object { $_.id -eq $attempt.coordinate })
if ($coordinate.Count -ne 1) {
    Add-Error 'attempt coordinate is undeclared'
}
else {
    $coordinate = $coordinate[0]
    if ((Split-Path -Leaf $resolvedAttempt) -ne $attempt.coordinate) {
        Add-Error 'attempt directory does not match coordinate id'
    }
    if ($attempt.quant -ne $coordinate.quant -or
        [int]$attempt.context -ne [int]$coordinate.context) {
        Add-Error 'attempt quant/context differs from the frozen coordinate'
    }
    $modelSpec = $plan.models.($coordinate.quant)
    if ([int]$attempt.requested_gpu_layers -ne
        [int]$modelSpec.requested_gpu_layers) {
        Add-Error 'requested GPU-layer count differs from the frozen coordinate'
    }
}

$attemptStarted = [DateTimeOffset]::MinValue
$attemptCompleted = [DateTimeOffset]::MinValue
$attemptChronologyPassed =
    [DateTimeOffset]::TryParse(
        [string]$attempt.started_at_utc,
        [ref]$attemptStarted
    ) -and
    [DateTimeOffset]::TryParse(
        [string]$attempt.completed_at_utc,
        [ref]$attemptCompleted
    ) -and
    $attemptCompleted -ge $attemptStarted -and
    [double]::IsFinite([double]$attempt.duration_seconds) -and
    [double]$attempt.duration_seconds -ge 0 -and
    [Math]::Abs(
        ($attemptCompleted - $attemptStarted).TotalSeconds -
        [double]$attempt.duration_seconds
    ) -le 2.0
if (-not $attemptChronologyPassed) {
    Add-Error 'attempt duration is not derivable from its UTC timestamps'
}
$journalPath = Join-Path $resolvedAttempt 'command-journal.jsonl'
$journalRows = @()
if (-not (Test-Path -LiteralPath $journalPath -PathType Leaf)) {
    Add-Error 'command journal is absent'
}
else {
    $journalRows = Read-JsonLines -Path $journalPath
    if ($journalRows.Count -lt 1) {
        Add-Error 'command journal is empty'
    }
    $previousJournalElapsed = -1.0
    $previousJournalTime = [DateTimeOffset]::MinValue
    foreach ($journalRow in $journalRows) {
        $journalTime = [DateTimeOffset]::MinValue
        $journalElapsed = [double]$journalRow.elapsed_ms
        if ($journalRow.schema -ne 'animus-ferric-runtime-journal-row-v1' -or
            -not [double]::IsFinite($journalElapsed) -or
            $journalElapsed -lt 0 -or
            $journalElapsed + 0.001 -lt $previousJournalElapsed -or
            -not [DateTimeOffset]::TryParse(
                [string]$journalRow.at_utc,
                [ref]$journalTime
            ) -or
            $journalTime -lt $previousJournalTime -or
            ($attemptChronologyPassed -and
                ($journalTime -lt $attemptStarted -or
                    $journalTime -gt $attemptCompleted.AddSeconds(2))) -or
            $journalElapsed -gt
                ([double]$attempt.duration_seconds * 1000.0 + 2000.0)) {
            Add-Error 'command journal chronology is invalid'
            break
        }
        $previousJournalElapsed = $journalElapsed
        $previousJournalTime = $journalTime
    }
}

if (-not [double]::IsFinite([double]$attempt.wall_cap_seconds) -or
    [double]$attempt.wall_cap_seconds -ne
        [double]$plan.quant_wall_cap_seconds) {
    Add-Error 'attempt wall cap differs from the frozen plan'
}
$priorQuantElapsed = [double]$attempt.prior_quant_elapsed_seconds
$durationSeconds = [double]$attempt.duration_seconds
$quantElapsed = [double]$attempt.quant_elapsed_seconds
if (-not [double]::IsFinite($priorQuantElapsed) -or
    -not [double]::IsFinite($durationSeconds) -or
    -not [double]::IsFinite($quantElapsed) -or
    $priorQuantElapsed -lt 0 -or $quantElapsed -lt 0) {
    Add-Error 'quant elapsed values are not finite nonnegative numbers'
}
$expectedQuantElapsed = [double]$attempt.prior_quant_elapsed_seconds +
    [double]$attempt.duration_seconds
if ([Math]::Abs(
        [double]$attempt.quant_elapsed_seconds - $expectedQuantElapsed
    ) -gt 0.002) {
    Add-Error 'cumulative quant elapsed time is inconsistent'
}
$derivedWallCapBreached =
    [double]$attempt.quant_elapsed_seconds -gt
        [double]$attempt.wall_cap_seconds
if ([bool]$attempt.wall_cap_breached -ne $derivedWallCapBreached) {
    Add-Error 'wall-cap breach flag is not derivable from elapsed time'
}
if ($coordinate.Count -eq 1) {
    if ($null -eq $coordinate.predecessor) {
        if ([double]$attempt.prior_quant_elapsed_seconds -ne 0.0) {
            Add-Error 'primary coordinate has inherited quant time'
        }
    }
    else {
        $predecessorDir = Join-Path (Split-Path -Parent $resolvedAttempt) `
            $coordinate.predecessor
        $predecessorPath = Join-Path $predecessorDir 'attempt.json'
        $predecessorManifest = Join-Path $predecessorDir 'files.sha256'
        if (-not (Test-Path -LiteralPath $predecessorPath -PathType Leaf) -or
            -not (Test-Path -LiteralPath $predecessorManifest -PathType Leaf)) {
            Add-Error 'retry predecessor evidence is absent'
        }
        else {
            $predecessorManifestCheck = Test-HashManifest `
                -Root $predecessorDir `
                -ManifestPath $predecessorManifest `
                -RejectUnlistedFiles
            $predecessorAttempt = Get-Content -Raw `
                -LiteralPath $predecessorPath | ConvertFrom-Json
            $predecessorValidationProcess = Invoke-VerificationChild `
                -ChildAttemptPath $predecessorDir
            $predecessorValidation = try {
                $predecessorValidationProcess.stdout | ConvertFrom-Json
            }
            catch {
                $null
            }
            if (-not $predecessorManifestCheck.passed -or
                $predecessorValidationProcess.exit_code -ne 0 -or
                $null -eq $predecessorValidation -or
                -not $predecessorValidation.passed -or
                $predecessorAttempt.coordinate -ne $coordinate.predecessor -or
                $predecessorAttempt.quant -ne $attempt.quant -or
                $predecessorAttempt.failure_classification -ne
                    'startup_memory_pressure' -or
                -not $predecessorAttempt.teardown.passed -or
                [Math]::Abs(
                    [double]$attempt.prior_quant_elapsed_seconds -
                    [double]$predecessorAttempt.quant_elapsed_seconds
                ) -gt 0.002) {
                Add-Error 'retry does not inherit verified predecessor quant time'
            }
        }
    }
}
$startupFileEvidence = Get-RetainedJson -Name 'startup.json' -Label 'startup'
$launchProcessEvidence = Get-RetainedJson `
    -Name 'launch-process.json' -Label 'launch process'
$launchStdoutPath = Join-Path $resolvedAttempt 'launch.stdout.log'
$launchStderrPath = Join-Path $resolvedAttempt 'launch.stderr.log'
$startupInputsPresent =
    $null -ne $startupFileEvidence -and
    $null -ne $launchProcessEvidence -and
    (Test-Path -LiteralPath $launchStdoutPath -PathType Leaf) -and
    (Test-Path -LiteralPath $launchStderrPath -PathType Leaf)
if (-not $startupInputsPresent) {
    Add-Error 'startup classification lacks launch process/log evidence'
}
else {
    if (-not (Test-JsonEquivalent -Left $startupFileEvidence `
        -Right $attempt.startup) -or
        -not (Test-JsonEquivalent -Left $launchProcessEvidence `
            -Right $attempt.startup.launch_process)) {
        Add-Error 'startup summary differs from retained launch evidence'
    }
    $startupClassificationFile = Get-OptionalProperty `
        -Value $attempt.startup -Name 'classification_input_file'
    $startupClassificationPath = if (
        [string]::IsNullOrWhiteSpace([string]$startupClassificationFile)
    ) {
        $null
    }
    else {
        Join-Path $resolvedAttempt ([string]$startupClassificationFile)
    }
    $launchText = if ($null -ne $startupClassificationPath -and
        (Test-Path -LiteralPath $startupClassificationPath -PathType Leaf) -and
        [string]$startupClassificationFile -eq 'startup-classification.log' -and
        [UInt64](Get-Item -LiteralPath $startupClassificationPath).Length -eq
            [UInt64]$attempt.startup.classification_input_bytes -and
        (Get-Sha256Lower -Path $startupClassificationPath) -eq
            $attempt.startup.classification_input_sha256) {
        Get-Content -Raw -LiteralPath $startupClassificationPath
    }
    else {
        Add-Error 'startup classification input is absent or not hash-bound'
        ''
    }
    $derivedMemoryMatch = Test-StartupMemoryPressure -Text $launchText `
        -Patterns @($plan.startup_memory_patterns)
    $derivedStartupHealthy =
        (-not $launchProcessEvidence.timed_out) -and
        [int]$launchProcessEvidence.exit_code -eq 0 -and
        (Test-Path -LiteralPath (
            Join-Path $resolvedAttempt 'runfile.local.json'
        ) -PathType Leaf) -and
        (Test-Path -LiteralPath (
            Join-Path $resolvedAttempt 'runfile.global.json'
        ) -PathType Leaf)
    $derivedStartupClass = if ($derivedStartupHealthy) {
        'healthy'
    }
    elseif ($derivedMemoryMatch.matched) {
        'startup_memory_pressure'
    }
    elseif ($launchProcessEvidence.timed_out) {
        'startup_timeout'
    }
    else {
        'startup_other_failure'
    }
    if ([bool]$attempt.startup.healthy -ne $derivedStartupHealthy -or
        $attempt.startup.classification -ne $derivedStartupClass -or
        -not (Test-JsonEquivalent -Left $attempt.startup.memory_match `
            -Right $derivedMemoryMatch)) {
        Add-Error 'startup classification is not derivable from raw launch evidence'
    }
}
$expectedGpuIdentity = Get-OptionalProperty `
    -Value (Get-OptionalProperty -Value $controlManifest -Name 'cold_state') `
    -Name 'memory'
$expectedGpuIdentity = Get-OptionalProperty `
    -Value $expectedGpuIdentity -Name 'gpu'
$preflightForMemory = Get-RetainedJson -Name 'preflight.json' -Label 'preflight'
$memoryBeforeLaunch = Get-RetainedJson `
    -Name 'memory-before-launch.json' -Label 'memory before launch'
$memoryBeforeTeardown = Get-RetainedJson `
    -Name 'memory-before-teardown.json' -Label 'memory before teardown'
[void](Test-MemorySnapshot `
    -Snapshot (Get-OptionalProperty -Value $preflightForMemory -Name 'memory') `
    -ExpectedGpu $expectedGpuIdentity -Label 'preflight')
[void](Test-MemorySnapshot -Snapshot $memoryBeforeLaunch `
    -ExpectedGpu $expectedGpuIdentity -Label 'before launch')
[void](Test-MemorySnapshot -Snapshot $memoryBeforeTeardown `
    -ExpectedGpu $expectedGpuIdentity -Label 'before teardown')
if ($null -ne $preflightForMemory -and
    -not (Test-JsonEquivalent -Left $memoryBeforeLaunch `
        -Right (Get-OptionalProperty -Value $preflightForMemory -Name 'memory'))) {
    Add-Error 'preflight memory differs from memory-before-launch.json'
}
$teardownEvidence = Get-RetainedJson -Name 'teardown.json' -Label 'teardown'
$derivedTeardownPassed = $false
if ($null -ne $teardownEvidence) {
    if (-not (Test-JsonEquivalent -Left $teardownEvidence `
        -Right $attempt.teardown)) {
        Add-Error 'teardown.json differs from embedded teardown evidence'
    }
    $postHealthBound = Test-ExchangeFile `
        -Exchange $teardownEvidence.health_after_teardown `
        -Label 'health after teardown' -ExpectedMethod 'GET' `
        -ExpectedUri "http://127.0.0.1:$($plan.port)/health" `
        -ExpectedResponseFile 'health-after-teardown.body' `
        -ExpectedTimeoutSeconds 2
    $downEvidencePassed = $true
    $expectedLiveWrapperRecords = [System.Collections.Generic.List[object]]::new()
    function Add-ExpectedLiveWrapperRecord {
        param(
            [AllowNull()][Parameter(Mandatory = $true)]$ProcessRecord,
            [Parameter(Mandatory = $true)][string]$Source
        )

        if ($null -ne $ProcessRecord -and
            [bool](Get-OptionalProperty -Value $ProcessRecord `
                -Name 'post_process_alive')) {
            $expectedLiveWrapperRecords.Add([ordered]@{
                source = $Source
                pid = [UInt32]$ProcessRecord.pid
                file = [string]$ProcessRecord.file
                started_at_utc = [string]$ProcessRecord.started_at_utc
            })
        }
    }
    Add-ExpectedLiveWrapperRecord -ProcessRecord $attempt.startup.launch_process `
        -Source 'ferric server up'
    Add-ExpectedLiveWrapperRecord `
        -ProcessRecord (Get-OptionalProperty -Value $attempt.smoke -Name 'process') `
        -Source 'ferric query nonce smoke'
    Add-ExpectedLiveWrapperRecord `
        -ProcessRecord (Get-OptionalProperty -Value $attempt.smoke `
            -Name 'trace_verify') `
        -Source 'ferric trace verify'
    $ordinaryDownCount = 0
    $cleanupDownObserved = $false
    foreach ($downAttempt in @($teardownEvidence.down_attempts)) {
        $downLabel = [string](Get-OptionalProperty -Value $downAttempt `
            -Name 'teardown_label')
        $expectedStdout = $null
        $expectedStderr = $null
        $downSource = $null
        if ($downLabel -match '^down-([1-9][0-9]*)$' -and
            -not $cleanupDownObserved -and
            [int]$Matches[1] -eq ($ordinaryDownCount + 1)) {
            $ordinaryDownCount++
            $expectedStdout = "$downLabel.stdout.log"
            $expectedStderr = "$downLabel.stderr.log"
            $downSource = "ferric server down $ordinaryDownCount"
        }
        elseif ($downLabel -eq 'down-cleanup' -and -not $cleanupDownObserved) {
            $cleanupDownObserved = $true
            $expectedStdout = 'down-cleanup.stdout.log'
            $expectedStderr = 'down-cleanup.stderr.log'
            $downSource = 'ferric server down cleanup'
        }
        else {
            Add-Error 'teardown down-attempt labels are not an exact ordered sequence'
            $downEvidencePassed = $false
        }
        if ($null -ne $expectedStdout) {
            $downEvidencePassed =
                (Test-ProcessRecord -Record $downAttempt `
                    -ExpectedFile (Join-Path $repoRoot $plan.ferric.relative_path) `
                    -ExpectedArguments @('server', 'down') `
                    -ExpectedStdoutFile $expectedStdout `
                    -ExpectedStderrFile $expectedStderr `
                    -Label $downSource) -and $downEvidencePassed
            Add-ExpectedLiveWrapperRecord -ProcessRecord $downAttempt `
                -Source $downSource
        }
        foreach ($streamProperty in @('stdout_file', 'stderr_file')) {
            $streamName = Get-OptionalProperty -Value $downAttempt `
                -Name $streamProperty
            if ([string]::IsNullOrWhiteSpace([string]$streamName) -or
                -not (Test-Path -LiteralPath (
                    Join-Path $resolvedAttempt ([string]$streamName)
                ) -PathType Leaf)) {
                $downEvidencePassed = $false
            }
        }
    }
    if (-not (Test-JsonEquivalent -Left @($expectedLiveWrapperRecords) `
            -Right @($teardownEvidence.live_wrapper_process_records))) {
        $downEvidencePassed = $false
    }
    $wrapperCleanup = @($teardownEvidence.wrapper_process_cleanup)
    if ($wrapperCleanup.Count -ne $expectedLiveWrapperRecords.Count) {
        $downEvidencePassed = $false
    }
    else {
        for ($wrapperIndex = 0; $wrapperIndex -lt $wrapperCleanup.Count;
            $wrapperIndex++) {
            $expectedWrapper = $expectedLiveWrapperRecords[$wrapperIndex]
            $cleanup = $wrapperCleanup[$wrapperIndex]
            $cleanupIdentityPassed =
                [string]$cleanup.source -eq [string]$expectedWrapper.source -and
                [UInt32]$cleanup.pid -eq [UInt32]$expectedWrapper.pid -and
                [string]$cleanup.expected_file -eq
                    [string]$expectedWrapper.file -and
                [string]$cleanup.expected_started_at_utc -eq
                    [string]$expectedWrapper.started_at_utc -and
                -not $cleanup.owned_process_alive_after -and
                (
                    (-not $cleanup.observed_alive_before -and
                        -not $cleanup.identity_matched -and
                        -not $cleanup.pid_reused -and
                        -not $cleanup.kill_attempted -and
                        -not $cleanup.kill_succeeded) -or
                    ($cleanup.observed_alive_before -and
                        $cleanup.identity_matched -and
                        -not $cleanup.pid_reused -and
                        $cleanup.kill_attempted -and
                        $cleanup.kill_succeeded) -or
                    ($cleanup.observed_alive_before -and
                        -not $cleanup.identity_matched -and
                        $cleanup.pid_reused -and
                        -not $cleanup.kill_attempted -and
                        -not $cleanup.kill_succeeded)
                )
            if (-not $cleanupIdentityPassed) {
                $downEvidencePassed = $false
            }
        }
    }
    if ($attempt.startup.healthy -and
        @($teardownEvidence.down_attempts).Count -lt 1) {
        $downEvidencePassed = $false
    }
    if ($attempt.startup.healthy) {
        $retainedRunfilePath = Join-Path $resolvedAttempt 'runfile.local.json'
        if (-not (Test-Path -LiteralPath $retainedRunfilePath -PathType Leaf)) {
            $downEvidencePassed = $false
        }
        else {
            $retainedRunfile = Get-Content -Raw -LiteralPath $retainedRunfilePath |
                ConvertFrom-Json
            if ($null -eq $teardownEvidence.saved_pid -or
                [UInt32]$teardownEvidence.saved_pid -ne
                    [UInt32]$retainedRunfile.pid -or
                [UInt32]$teardownEvidence.saved_pid -ne
                    [UInt32]$attempt.attestation.process.pid) {
                $downEvidencePassed = $false
            }
        }
    }
    elseif ($null -ne $teardownEvidence.saved_pid) {
        $downEvidencePassed = $false
    }
    $derivedTeardownPassed =
        $downEvidencePassed -and
        [double]::IsFinite([double]$teardownEvidence.cleanup_duration_ms) -and
        [double]$teardownEvidence.cleanup_duration_ms -ge 0 -and
        [double]$teardownEvidence.cleanup_duration_ms -le
            ([double]$plan.teardown_safety_grace_seconds * 1000.0 + 1000.0) -and
        [int]$teardownEvidence.cleanup_grace_seconds -eq
            [int]$plan.teardown_safety_grace_seconds -and
        @($teardownEvidence.errors).Count -eq 0 -and
        -not $teardownEvidence.saved_pid_alive -and
        @($teardownEvidence.listener_records).Count -eq 0 -and
        $teardownEvidence.local_runfile_absent -and
        $teardownEvidence.global_runfile_absent -and
        @($teardownEvidence.matching_model_processes).Count -eq 0 -and
        @($teardownEvidence.wrapper_processes_alive).Count -eq 0 -and
        $postHealthBound -and
        [int]$teardownEvidence.health_after_teardown.status_code -ne 200 -and
        $null -ne $teardownEvidence.memory_after_teardown
    [void](Test-MemorySnapshot `
        -Snapshot (Get-OptionalProperty -Value $teardownEvidence `
            -Name 'memory_after_teardown') `
        -ExpectedGpu $expectedGpuIdentity -Label 'after teardown')
    if (-not (Test-JsonEquivalent -Left $memoryBeforeTeardown `
        -Right (Get-OptionalProperty -Value $teardownEvidence `
            -Name 'memory_before_teardown'))) {
        Add-Error 'teardown memory differs from memory-before-teardown.json'
    }
    if ([bool]$attempt.teardown.passed -ne $derivedTeardownPassed) {
        Add-Error 'teardown pass flag is not derivable from retained lifecycle evidence'
    }
}

$commonPreflight = Get-RetainedJson -Name 'preflight.json' -Label 'preflight'
$commonLaunch = Get-RetainedJson `
    -Name 'launch-command.json' -Label 'launch declaration'
$commonModelSpec = if ($coordinate.Count -eq 1) {
    $plan.models.($coordinate.quant)
}
else {
    $null
}
$commonModelPath = if ($null -ne $commonModelSpec) {
    Join-Path $repoRoot $commonModelSpec.relative_path
}
else {
    $null
}
$liveModelFilePresent =
    $null -ne $commonModelPath -and
    (Test-Path -LiteralPath $commonModelPath -PathType Leaf)
$liveModelSha256 = if ($liveModelFilePresent -and
    -not $deferLiveModelHash) {
    Get-Sha256Lower -Path $commonModelPath
}
else {
    $null
}
$commonFerricPath = Join-Path $repoRoot $plan.ferric.relative_path
$commonLlamaBin = Join-Path $repoRoot `
    $plan.llama_cpp.ignored_runtime_relative_path
if ($null -ne $commonModelSpec) {
    $commonLaunchArguments = @(
        'server', 'up', '--engine', 'llama-server',
        '--model', $commonModelPath,
        '--ctx', [string]$attempt.context,
        '--threads', [string]$plan.server.threads,
        '--gpu-layers', [string]$commonModelSpec.requested_gpu_layers,
        '--batch-size', [string]$plan.server.batch_size,
        '--seed', [string]$plan.server.seed,
        '--parallel', [string]$plan.server.parallel_slots,
        '--port', [string]$plan.port
    )
    $commonLlamaArguments = @(
        'llama-server', '-m', $commonModelPath,
        '-c', [string]$attempt.context,
        '-t', [string]$plan.server.threads,
        '-ngl', [string]$commonModelSpec.requested_gpu_layers,
        '-b', [string]$plan.server.batch_size,
        '--seed', [string]$plan.server.seed,
        '--parallel', [string]$plan.server.parallel_slots,
        '--host', '127.0.0.1', '--port', [string]$plan.port
    )
    $commonProcessPassed = Test-ProcessRecord `
        -Record $launchProcessEvidence -ExpectedFile $commonFerricPath `
        -ExpectedArguments $commonLaunchArguments `
        -ExpectedStdoutFile 'launch.stdout.log' `
        -ExpectedStderrFile 'launch.stderr.log' `
        -Label 'server launch' -RequireArgumentLine
    $expectedEnvironmentNames = @(
        @('Path', 'LLAMA_ARG_LOG_FILE') +
        @($plan.server.environment.PSObject.Properties.Name) +
        @($plan.server.logging_environment.PSObject.Properties.Name) |
            Sort-Object
    )
    $actualEnvironmentNames = if ($null -ne $commonLaunch) {
        @($commonLaunch.environment.PSObject.Properties.Name | Sort-Object)
    }
    else {
        @()
    }
    $declaredParentEnvironmentNames = if ($null -ne $commonLaunch -and
        $null -ne $commonLaunch.declared_parent_environment) {
        @($commonLaunch.declared_parent_environment.PSObject.Properties.Name |
            Sort-Object)
    }
    else {
        @()
    }
    $declaredParentPath = if ($null -ne $commonLaunch) {
        Get-OptionalProperty -Value $commonLaunch.declared_parent_environment `
            -Name 'Path'
    }
    else {
        $null
    }
    $commonLaunchPassed =
        $null -ne $commonLaunch -and
        $commonLaunch.schema -eq 'animus-ferric-runtime-launch-v3' -and
        [int]$commonLaunch.control_epoch -eq 3 -and
        $commonLaunch.attestation_protocol -eq
            [string]$plan.template_attestation.protocol -and
        $commonLaunch.process_command_protocol -ceq
            [string]$plan.process_command_attestation.protocol -and
        $commonLaunch.coordinate -eq $attempt.coordinate -and
        [System.IO.Path]::GetFullPath(
            [string]$commonLaunch.executable
        ).Equals(
            [System.IO.Path]::GetFullPath($commonFerricPath),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        [System.IO.Path]::GetFullPath(
            [string]$commonLaunch.working_directory
        ).Equals(
            [System.IO.Path]::GetFullPath($repoRoot),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        (@($commonLaunch.arguments) -join "`n") -eq
            ($commonLaunchArguments -join "`n") -and
        (@($commonLaunch.expected_llama_argv) -join "`n") -eq
            ($commonLlamaArguments -join "`n") -and
        [System.IO.Path]::GetFullPath(
            [string]$commonLaunch.child_path_prepend
        ).Equals(
            [System.IO.Path]::GetFullPath($commonLlamaBin),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        ($actualEnvironmentNames -join "`n") -eq
            ($expectedEnvironmentNames -join "`n") -and
        ($declaredParentEnvironmentNames -join "`n") -ceq 'Path' -and
        -not [string]::IsNullOrWhiteSpace([string]$declaredParentPath) -and
        [string]$commonLaunch.environment.Path -ceq
            "$commonLlamaBin;$declaredParentPath" -and
        [System.IO.Path]::GetFullPath(
            [string]$commonLaunch.environment.LLAMA_ARG_LOG_FILE
        ).Equals(
            [System.IO.Path]::GetFullPath(
                (Join-Path $repoRoot `
                    (Join-Path ([string]$plan.raw_attempt_root) `
                        "$($attempt.coordinate)/server.log"))
            ),
            [System.StringComparison]::OrdinalIgnoreCase
        )
    foreach ($entry in $plan.server.environment.PSObject.Properties) {
        if ((Get-OptionalProperty -Value $commonLaunch.environment `
                -Name $entry.Name) -ne [string]$entry.Value) {
            $commonLaunchPassed = $false
        }
    }
    foreach ($entry in $plan.server.logging_environment.PSObject.Properties) {
        if ((Get-OptionalProperty -Value $commonLaunch.environment `
                -Name $entry.Name) -ne [string]$entry.Value) {
            $commonLaunchPassed = $false
        }
    }
    if (-not $commonLaunchPassed -or -not $commonProcessPassed) {
        Add-Error 'launch evidence does not prove the frozen coordinate'
    }

    $commonRuntimeIdentity = Test-FileIdentityManifest -Root $commonLlamaBin `
        -Expected @($controlManifest.binaries.llama_runtime.files)
    $expectedControlHash = if (
        Test-Path -LiteralPath $controlManifestPath -PathType Leaf
    ) {
        Get-Sha256Lower -Path $controlManifestPath
    }
    else {
        $null
    }
    $commonDeviceObservation = $null
    if ($null -ne $commonPreflight) {
        try {
            $commonDeviceObservation = Get-LlamaDeviceObservation `
                -Output @($commonPreflight.llama_server.devices)
        }
        catch {
            Add-Error "preflight llama device output is malformed: $($_.Exception.Message)"
        }
    }
    $commonPreflightPassed =
        $null -ne $commonPreflight -and
        $commonPreflight.schema -eq 'animus-ferric-runtime-preflight-v3' -and
        $commonPreflight.task -ceq 'T-11409' -and
        [int]$commonPreflight.control_epoch -eq 3 -and
        $commonPreflight.attestation_protocol -ceq
            [string]$plan.template_attestation.protocol -and
        $commonPreflight.process_command_protocol -ceq
            [string]$plan.process_command_attestation.protocol -and
        $commonPreflight.coordinate -eq $attempt.coordinate -and
        $commonPreflight.repository_commit -ceq
            $controlManifest.repository.head_at_freeze -and
        $commonPreflight.repository_status_semantics -ceq
            'descriptive_snapshot_not_a_cleanliness_claim' -and
        $commonPreflight.runtime_plan_sha256 -eq
            (Get-Sha256Lower -Path $planPath) -and
        $commonPreflight.control_inputs_sha256 -eq $expectedControlHash -and
        $commonPreflight.model.display_path -eq
            $commonModelSpec.relative_path -and
        [UInt64]$commonPreflight.model.bytes -eq
            [UInt64]$commonModelSpec.bytes -and
        $commonPreflight.model.sha256 -eq $commonModelSpec.sha256 -and
        [System.IO.Path]::GetFullPath(
            [string]$commonPreflight.ferric.path
        ).Equals(
            [System.IO.Path]::GetFullPath($commonFerricPath),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        [UInt64]$commonPreflight.ferric.bytes -eq
            [UInt64]$controlManifest.binaries.ferric.bytes -and
        $commonPreflight.ferric.sha256 -eq
            $controlManifest.binaries.ferric.sha256 -and
        (@($commonPreflight.ferric.version) -join "`n") -eq
            (@($controlManifest.binaries.ferric.version_output) -join "`n") -and
        [System.IO.Path]::GetFullPath(
            [string]$commonPreflight.llama_server.path
        ).Equals(
            [System.IO.Path]::GetFullPath(
                (Join-Path $commonLlamaBin 'llama-server.exe')
            ),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        [UInt64]$commonPreflight.llama_server.bytes -eq
            [UInt64]$controlManifest.binaries.llama_server.bytes -and
        $commonPreflight.llama_server.sha256 -eq
            $controlManifest.binaries.llama_server.sha256 -and
        (@($commonPreflight.llama_server.version) -join "`n") -eq
            (@($controlManifest.binaries.llama_server.version_output) -join "`n") -and
        $null -ne $commonDeviceObservation -and
        (Test-JsonEquivalent `
            -Left $commonPreflight.llama_server.device_identity `
            -Right $commonDeviceObservation.identity) -and
        (Test-JsonEquivalent -Left $commonDeviceObservation.identity `
            -Right $controlManifest.binaries.llama_server.device_identity) -and
        (Test-JsonEquivalent -Left $commonDeviceObservation.identity `
            -Right $plan.llama_cpp.expected_device) -and
        [UInt64]$commonPreflight.llama_server.device_free_mib -eq
            [UInt64]$commonDeviceObservation.free_mib -and
        [UInt64]$commonDeviceObservation.free_mib -ge
            [UInt64]$plan.minimum_gpu_free_mib_before_launch -and
        $commonPreflight.llama_server.help_output_sha256 -eq
            $controlManifest.binaries.llama_server.help_output_sha256 -and
        @($commonPreflight.inherited_runtime_environment).Count -eq 0 -and
        $commonPreflight.local_runfile_absent -and
        $commonPreflight.global_runfile_absent -and
        @($commonPreflight.listener_records).Count -eq 0 -and
        @($commonPreflight.llama_server_process_records).Count -eq 0 -and
        $commonPreflight.listener_absent -and
        $commonPreflight.any_llama_server_process_absent -and
        [UInt64]$commonPreflight.memory.gpu.free_mib -ge
            [UInt64]$plan.minimum_gpu_free_mib_before_launch -and
        $commonRuntimeIdentity.passed -and
        (Test-Path -LiteralPath $commonModelPath -PathType Leaf) -and
        [UInt64](Get-Item -LiteralPath $commonModelPath).Length -eq
            [UInt64]$commonModelSpec.bytes -and
        ($deferLiveModelHash -or
            $liveModelSha256 -ceq [string]$commonModelSpec.sha256) -and
        (Get-Sha256Lower -Path $commonFerricPath) -eq
            $controlManifest.binaries.ferric.sha256
    if (-not $commonPreflightPassed) {
        Add-Error 'preflight does not bind the frozen launch/runtime coordinate'
    }
}

$derivedAttestationPassed = $false
if ($attempt.startup.healthy) {
    $startupEvidence = Get-RetainedJson -Name 'startup.json' -Label 'startup'
    $attestationEvidence = Get-RetainedJson `
        -Name 'attestation.json' -Label 'attestation'
    $preflightEvidence = Get-RetainedJson -Name 'preflight.json' -Label 'preflight'
    $launchEvidence = Get-RetainedJson `
        -Name 'launch-command.json' -Label 'launch declaration'
    $localRunfileEvidence = Get-RetainedJson `
        -Name 'runfile.local.json' -Label 'local runfile'
    $globalRunfileEvidence = Get-RetainedJson `
        -Name 'runfile.global.json' -Label 'global runfile'
    $healthBodyOk = Test-ExchangeFile `
        -Exchange $attempt.attestation.endpoints.health -Label 'health' `
        -ExpectedMethod 'GET' `
        -ExpectedUri "http://127.0.0.1:$($plan.port)/health" `
        -ExpectedResponseFile 'health.body' -ExpectedTimeoutSeconds 30 `
        -RequireSuccess
    $modelsBodyOk = Test-ExchangeFile `
        -Exchange $attempt.attestation.endpoints.models -Label 'models' `
        -ExpectedMethod 'GET' `
        -ExpectedUri "http://127.0.0.1:$($plan.port)/v1/models" `
        -ExpectedResponseFile 'models.body.json' -ExpectedTimeoutSeconds 30 `
        -RequireSuccess
    $propsBodyOk = Test-ExchangeFile `
        -Exchange $attempt.attestation.endpoints.props -Label 'props' `
        -ExpectedMethod 'GET' `
        -ExpectedUri "http://127.0.0.1:$($plan.port)/props" `
        -ExpectedResponseFile 'props.body.json' -ExpectedTimeoutSeconds 30 `
        -RequireSuccess
    $startupLogPath = Join-Path $resolvedAttempt 'startup.log'
    $serverLogPath = Join-Path $resolvedAttempt 'server.log'
    $startupLogPresent = Test-Path -LiteralPath $startupLogPath -PathType Leaf
    $serverLogPresent = Test-Path -LiteralPath $serverLogPath -PathType Leaf
    if (-not $startupLogPresent) {
        Add-Error 'healthy startup lacks startup.log'
    }
    if (-not $serverLogPresent) {
        Add-Error 'healthy startup lacks the complete server.log'
    }

    if ($null -ne $startupEvidence -and
        -not (Test-JsonEquivalent -Left $startupEvidence `
            -Right $attempt.startup)) {
        Add-Error 'startup.json differs from embedded startup evidence'
    }
    if ($null -ne $attestationEvidence -and
        -not (Test-JsonEquivalent -Left $attestationEvidence `
            -Right $attempt.attestation)) {
        Add-Error 'attestation.json differs from embedded attestation evidence'
    }
    $attestationSchemaPassed =
        $null -ne $attestationEvidence -and
        $attempt.attestation.schema -eq
            'animus-ferric-managed-server-attestation-v3' -and
        [int]$attempt.attestation.control_epoch -eq 3 -and
        $attempt.attestation.attestation_protocol -eq
            [string]$plan.template_attestation.protocol -and
        $attempt.attestation.process_command_protocol -ceq
            [string]$plan.process_command_attestation.protocol
    if (-not $attestationSchemaPassed) {
        Add-Error 'managed-server attestation does not declare epoch-3 semantics'
    }

    $modelSpecForEvidence = $plan.models.($attempt.quant)
    $modelPathForEvidence = Join-Path $repoRoot `
        $modelSpecForEvidence.relative_path
    $ferricPathForEvidence = Join-Path $repoRoot $plan.ferric.relative_path
    $llamaBinForEvidence = Join-Path $repoRoot `
        $plan.llama_cpp.ignored_runtime_relative_path
    $expectedLaunchArguments = @(
        'server', 'up',
        '--engine', 'llama-server',
        '--model', $modelPathForEvidence,
        '--ctx', [string]$attempt.context,
        '--threads', [string]$plan.server.threads,
        '--gpu-layers', [string]$modelSpecForEvidence.requested_gpu_layers,
        '--batch-size', [string]$plan.server.batch_size,
        '--seed', [string]$plan.server.seed,
        '--parallel', [string]$plan.server.parallel_slots,
        '--port', [string]$plan.port
    )
    $expectedLlamaArguments = @(
        'llama-server', '-m', $modelPathForEvidence,
        '-c', [string]$attempt.context,
        '-t', [string]$plan.server.threads,
        '-ngl', [string]$modelSpecForEvidence.requested_gpu_layers,
        '-b', [string]$plan.server.batch_size,
        '--seed', [string]$plan.server.seed,
        '--parallel', [string]$plan.server.parallel_slots,
        '--host', '127.0.0.1', '--port', [string]$plan.port
    )
    $launchProcessRecordPassed = Test-ProcessRecord `
        -Record $launchProcessEvidence -ExpectedFile $ferricPathForEvidence `
        -ExpectedArguments $expectedLaunchArguments `
        -ExpectedStdoutFile 'launch.stdout.log' `
        -ExpectedStderrFile 'launch.stderr.log' `
        -Label 'server launch' -RequireArgumentLine
    $launchFieldsPassed = $false
    if ($null -ne $launchEvidence) {
        $launchFieldsPassed =
            $launchEvidence.schema -eq 'animus-ferric-runtime-launch-v3' -and
            [int]$launchEvidence.control_epoch -eq 3 -and
            $launchEvidence.attestation_protocol -eq
                [string]$plan.template_attestation.protocol -and
            $launchEvidence.process_command_protocol -ceq
                [string]$plan.process_command_attestation.protocol -and
            $launchEvidence.coordinate -eq $attempt.coordinate -and
            $launchEvidence.executable -eq $ferricPathForEvidence -and
            (@($launchEvidence.arguments) -join "`n") -eq
                ($expectedLaunchArguments -join "`n") -and
            (@($launchEvidence.expected_llama_argv) -join "`n") -eq
                ($expectedLlamaArguments -join "`n") -and
            $launchEvidence.child_path_prepend -eq $llamaBinForEvidence
        foreach ($entry in $plan.server.environment.PSObject.Properties) {
            if ((Get-OptionalProperty -Value $launchEvidence.environment `
                    -Name $entry.Name) -ne [string]$entry.Value) {
                $launchFieldsPassed = $false
            }
        }
        foreach ($entry in $plan.server.logging_environment.PSObject.Properties) {
            if ((Get-OptionalProperty -Value $launchEvidence.environment `
                    -Name $entry.Name) -ne [string]$entry.Value) {
                $launchFieldsPassed = $false
            }
        }
        $launchPath = Get-OptionalProperty -Value $launchEvidence.environment `
            -Name 'Path'
        $launchLog = Get-OptionalProperty -Value $launchEvidence.environment `
            -Name 'LLAMA_ARG_LOG_FILE'
        $expectedEnvironmentNames = @(
            @('Path', 'LLAMA_ARG_LOG_FILE') +
            @($plan.server.environment.PSObject.Properties.Name) +
            @($plan.server.logging_environment.PSObject.Properties.Name) |
                Sort-Object
        )
        $actualEnvironmentNames = @(
            $launchEvidence.environment.PSObject.Properties.Name | Sort-Object
        )
        $declaredParentEnvironmentNames = @(
            $launchEvidence.declared_parent_environment.PSObject.Properties.Name |
                Sort-Object
        )
        $declaredParentPath = Get-OptionalProperty `
            -Value $launchEvidence.declared_parent_environment -Name 'Path'
        $expectedRawAttemptRoot = Join-Path $repoRoot `
            (Join-Path ([string]$plan.raw_attempt_root) $attempt.coordinate)
        if (-not [string]$launchPath.Equals(
                "$llamaBinForEvidence;$declaredParentPath",
                [System.StringComparison]::Ordinal
            ) -or
            ($declaredParentEnvironmentNames -join "`n") -cne 'Path' -or
            [string]::IsNullOrWhiteSpace([string]$declaredParentPath) -or
            -not [System.IO.Path]::GetFullPath([string]$launchLog).Equals(
                [System.IO.Path]::GetFullPath(
                    (Join-Path $expectedRawAttemptRoot 'server.log')
                ),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -or
            ($actualEnvironmentNames -join "`n") -ne
                ($expectedEnvironmentNames -join "`n")) {
            $launchFieldsPassed = $false
        }
    }
    if (-not $launchFieldsPassed) {
        Add-Error 'launch declaration is not the frozen coordinate'
    }

    $runfilesPassed = $false
    if ($null -ne $localRunfileEvidence -and $null -ne $globalRunfileEvidence) {
        $localRunfilePath = Join-Path $resolvedAttempt 'runfile.local.json'
        $globalRunfilePath = Join-Path $resolvedAttempt 'runfile.global.json'
        $runfilesPassed =
            (Get-Sha256Lower -Path $localRunfilePath) -eq
                (Get-Sha256Lower -Path $globalRunfilePath) -and
            (Test-JsonEquivalent -Left $localRunfileEvidence `
                -Right $globalRunfileEvidence) -and
            (Test-JsonEquivalent -Left $localRunfileEvidence `
                -Right $attempt.attestation.runfiles.value) -and
            [string]$localRunfileEvidence.engine -eq 'llama-server' -and
            [int]$localRunfileEvidence.port -eq [int]$plan.port -and
            [string]$localRunfileEvidence.base_url -eq
                "http://127.0.0.1:$($plan.port)/v1" -and
            [bool]$localRunfileEvidence.tailscale -eq $false -and
            [System.IO.Path]::GetFullPath(
                [string]$localRunfileEvidence.model
            ).Equals(
                [System.IO.Path]::GetFullPath($modelPathForEvidence),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            [int]$localRunfileEvidence.context_size -eq [int]$attempt.context -and
            [int]$localRunfileEvidence.sampling_seed -eq [int]$plan.server.seed -and
            [int]$localRunfileEvidence.parallel_slots -eq
                [int]$plan.server.parallel_slots -and
            [UInt32]$localRunfileEvidence.pid -eq
                [UInt32]$attempt.attestation.process.pid
    }
    if (-not $runfilesPassed) {
        Add-Error 'managed-server runfiles are not internally consistent'
    }

    $liveModelIdentityPassed =
        (Test-Path -LiteralPath $modelPathForEvidence -PathType Leaf) -and
        [UInt64](Get-Item -LiteralPath $modelPathForEvidence).Length -eq
            [UInt64]$modelSpecForEvidence.bytes -and
        ($deferLiveModelHash -or
            $liveModelSha256 -ceq [string]$modelSpecForEvidence.sha256)
    if (-not $liveModelIdentityPassed) {
        Add-Error 'live GGUF identity differs from the frozen coordinate'
    }
    $liveFerricIdentityPassed =
        (Test-Path -LiteralPath $ferricPathForEvidence -PathType Leaf) -and
        (Get-Sha256Lower -Path $ferricPathForEvidence) -eq
            $controlManifest.binaries.ferric.sha256
    if (-not $liveFerricIdentityPassed) {
        Add-Error 'live Ferric executable differs from the frozen control'
    }

    $modelsJson = if ($modelsBodyOk) {
        try {
            Get-Content -Raw -LiteralPath (
                Join-Path $resolvedAttempt 'models.body.json'
            ) | ConvertFrom-Json
        }
        catch {
            $null
        }
    }
    else {
        $null
    }
    $propsJson = if ($propsBodyOk) {
        try {
            Get-Content -Raw -LiteralPath (
                Join-Path $resolvedAttempt 'props.body.json'
            ) | ConvertFrom-Json
        }
        catch {
            $null
        }
    }
    else {
        $null
    }
    $modelEntries = @(
        if ($null -ne $modelsJson) {
            Get-OptionalProperty -Value $modelsJson -Name 'data'
        }
    )
    $servedId = if ($modelEntries.Count -eq 1) {
        Get-OptionalProperty -Value $modelEntries[0] -Name 'id'
    }
    else {
        $null
    }
    $defaultSettings = Get-OptionalProperty -Value $propsJson `
        -Name 'default_generation_settings'
    $servedContext = Get-OptionalProperty -Value $defaultSettings -Name 'n_ctx'
    $chatCaps = Get-OptionalProperty -Value $propsJson -Name 'chat_template_caps'
    $supportsPreserve = Get-OptionalProperty -Value $chatCaps `
        -Name 'supports_preserve_reasoning'
    $templateProbeFacts = Get-TemplateProbeFacts -Plan $plan `
        -ArtifactDirectory $artifactDir -EvidenceDirectory $resolvedAttempt
    $templateProbeExchangeRecords = @(
        $attempt.attestation.endpoints.template_probe_exchanges
    )
    $templateProbeHttpPassed =
        $templateProbeExchangeRecords.Count -eq 4 -and
        (@($templateProbeExchangeRecords | ForEach-Object {
            [string]$_.name
        }) -join "`n") -eq "defaults`nalias-false`nall-false`nall-true"
    foreach ($probeRecord in $templateProbeExchangeRecords) {
        $probeName = [string]$probeRecord.name
        $probePassed = Test-ExchangeFile `
            -Exchange $probeRecord.exchange `
            -Label "template probe $probeName" -ExpectedMethod 'POST' `
            -ExpectedUri "http://127.0.0.1:$($plan.port)$($plan.template_attestation.endpoint)" `
            -ExpectedResponseFile "template-probe.$probeName.response.json" `
            -ExpectedTimeoutSeconds 30 -RequireSuccess
        if (-not $probePassed) {
            $templateProbeHttpPassed = $false
        }
    }
    $totalSlots = Get-OptionalProperty -Value $propsJson -Name 'total_slots'

    $startupText = if ($startupLogPresent) {
        Get-Content -Raw -LiteralPath $startupLogPath
    }
    else {
        ''
    }
    $offloadMatches = [regex]::Matches(
        $startupText,
        'offloaded\s+(\d+)\s*/\s*(\d+)\s+layers',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    $effectiveGpuLayers = $null
    $totalLayers = $null
    if ($offloadMatches.Count -gt 0) {
        $offloadMatch = $offloadMatches[$offloadMatches.Count - 1]
        $effectiveGpuLayers = [int]$offloadMatch.Groups[1].Value
        $totalLayers = [int]$offloadMatch.Groups[2].Value
    }
    $kvMatches = [regex]::Matches(
        $startupText,
        'K\s*\(q8_0\)\s*:[^\r\n]*V\s*\(q8_0\)\s*:',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    $flashMatches = [regex]::Matches(
        $startupText,
        '(?:flash_attn\s*=\s*(?:1|on|enabled)\b|flash attention is enabled)',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    $reasoningMatches = [regex]::Matches(
        $startupText,
        'chat template,\s*thinking\s*=\s*1\b',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )
    $preserveDisabledWarnings = [regex]::Matches(
        $startupText,
        'supports preserving reasoning,\s*consider enabling|does not support[^\r\n]*reasoning[- ]preserve',
        [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
    )

    $runtimeIdentityPassed = $false
    if ($null -ne $controlManifest) {
        $runtimeIdentity = Test-FileIdentityManifest -Root $llamaBinForEvidence `
            -Expected @($controlManifest.binaries.llama_runtime.files)
        $runtimeIdentityPassed =
            $runtimeIdentity.passed -and
            $attempt.attestation.effective.llama_runtime_identity.passed -and
            (Test-JsonEquivalent `
                -Left $attempt.attestation.effective.llama_runtime_identity.actual `
                -Right $runtimeIdentity.actual)
    }
    if (-not $runtimeIdentityPassed) {
        Add-Error 'attestation does not bind the exact frozen llama.cpp runtime tree'
    }

    $preflightPassed = $false
    if ($null -ne $preflightEvidence) {
        $preflightPassed =
            $preflightEvidence.schema -eq 'animus-ferric-runtime-preflight-v3' -and
            $preflightEvidence.task -ceq 'T-11409' -and
            [int]$preflightEvidence.control_epoch -eq 3 -and
            $preflightEvidence.attestation_protocol -ceq
                [string]$plan.template_attestation.protocol -and
            $preflightEvidence.process_command_protocol -ceq
                [string]$plan.process_command_attestation.protocol -and
            $preflightEvidence.coordinate -eq $attempt.coordinate -and
            $preflightEvidence.repository_commit -ceq
                $controlManifest.repository.head_at_freeze -and
            $preflightEvidence.repository_status_semantics -ceq
                'descriptive_snapshot_not_a_cleanliness_claim' -and
            [System.IO.Path]::GetFullPath(
                [string]$preflightEvidence.ferric.path
            ).Equals(
                [System.IO.Path]::GetFullPath($ferricPathForEvidence),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            [UInt64]$preflightEvidence.ferric.bytes -eq
                [UInt64]$controlManifest.binaries.ferric.bytes -and
            $preflightEvidence.model.bytes -eq $modelSpecForEvidence.bytes -and
            $preflightEvidence.model.sha256 -eq $modelSpecForEvidence.sha256 -and
            $preflightEvidence.ferric.sha256 -eq
                $controlManifest.binaries.ferric.sha256 -and
            (@($preflightEvidence.ferric.version) -join "`n") -eq
                (@($controlManifest.binaries.ferric.version_output) -join "`n") -and
            [System.IO.Path]::GetFullPath(
                [string]$preflightEvidence.llama_server.path
            ).Equals(
                [System.IO.Path]::GetFullPath(
                    (Join-Path $llamaBinForEvidence 'llama-server.exe')
                ),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            [UInt64]$preflightEvidence.llama_server.bytes -eq
                [UInt64]$controlManifest.binaries.llama_server.bytes -and
            $preflightEvidence.llama_server.sha256 -eq
                $plan.llama_cpp.expected_server_sha256 -and
            (@($preflightEvidence.llama_server.version) -join "`n") -eq
                (@($controlManifest.binaries.llama_server.version_output) -join "`n") -and
            $null -ne $commonDeviceObservation -and
            (Test-JsonEquivalent `
                -Left $preflightEvidence.llama_server.device_identity `
                -Right $commonDeviceObservation.identity) -and
            (Test-JsonEquivalent -Left $commonDeviceObservation.identity `
                -Right $controlManifest.binaries.llama_server.device_identity) -and
            [UInt64]$preflightEvidence.llama_server.device_free_mib -eq
                [UInt64]$commonDeviceObservation.free_mib -and
            [UInt64]$commonDeviceObservation.free_mib -ge
                [UInt64]$plan.minimum_gpu_free_mib_before_launch -and
            $preflightEvidence.llama_server.help_output_sha256 -eq
                $controlManifest.binaries.llama_server.help_output_sha256 -and
            @($preflightEvidence.inherited_runtime_environment).Count -eq 0 -and
            $preflightEvidence.local_runfile_absent -and
            $preflightEvidence.global_runfile_absent -and
            @($preflightEvidence.listener_records).Count -eq 0 -and
            @($preflightEvidence.llama_server_process_records).Count -eq 0 -and
            $preflightEvidence.listener_absent -and
            $preflightEvidence.any_llama_server_process_absent -and
            [UInt64]$preflightEvidence.memory.gpu.free_mib -ge
                [UInt64]$plan.minimum_gpu_free_mib_before_launch
    }
    if (-not $preflightPassed) {
        Add-Error 'preflight does not establish a frozen uncontended coordinate'
    }
    [void](Test-MemorySnapshot `
        -Snapshot (Get-OptionalProperty -Value $attempt.attestation `
            -Name 'memory_after_load') `
        -ExpectedGpu $expectedGpuIdentity -Label 'after model load')

    $frozenProcessPath = Join-Path $llamaBinForEvidence 'llama-server.exe'
    $processCommandBinding = Test-BoundWindowsProcessCommand `
        -ExecutablePath ([string]$attempt.attestation.process.executable_path) `
        -ExecutableSha256 `
            ([string]$attempt.attestation.process.executable_sha256) `
        -CommandLine ([string]$attempt.attestation.process.command_line) `
        -FrozenExecutablePath $frozenProcessPath `
        -FrozenExecutableSha256 `
            ([string]$plan.llama_cpp.expected_server_sha256) `
        -ExpectedArgv @($expectedLlamaArguments)
    $recordedCommandBinding = Get-OptionalProperty `
        -Value $attempt.attestation.process -Name 'command_binding'
    $recordedCommandBindingPassed =
        $null -ne $recordedCommandBinding -and
        (Test-JsonEquivalent -Left $recordedCommandBinding `
            -Right $processCommandBinding)
    if (-not $recordedCommandBindingPassed) {
        Add-Error 'recorded process-command binding is not derivable from evidence'
    }
    $processCreationBinding = Test-ProcessCreationWindow `
        -CreationDateUtc $attempt.attestation.process.creation_date_utc `
        -PreflightCapturedUtc $preflightEvidence.captured_at_utc `
        -AttestationCapturedUtc $attempt.attestation.captured_at_utc `
        -ToleranceSeconds `
            ([int]$plan.process_command_attestation.creation_time_tolerance_seconds)
    $recordedCreationBinding = Get-OptionalProperty `
        -Value $attempt.attestation.process -Name 'creation_binding'
    $normalizedRecordedCreationBinding = if ($null -ne $recordedCreationBinding) {
        [ordered]@{
            schema = [string]$recordedCreationBinding.schema
            passed = [bool]$recordedCreationBinding.passed
            tolerance_seconds = [int]$recordedCreationBinding.tolerance_seconds
            creation_date_utc = ConvertTo-UtcIso8601 `
                -Value $recordedCreationBinding.creation_date_utc
            preflight_captured_utc = ConvertTo-UtcIso8601 `
                -Value $recordedCreationBinding.preflight_captured_utc
            attestation_captured_utc = ConvertTo-UtcIso8601 `
                -Value $recordedCreationBinding.attestation_captured_utc
            errors = @($recordedCreationBinding.errors)
        }
    }
    else {
        $null
    }
    $recordedCreationBindingPassed =
        $null -ne $normalizedRecordedCreationBinding -and
        (Test-JsonEquivalent -Left $normalizedRecordedCreationBinding `
            -Right $processCreationBinding)
    if (-not $recordedCreationBindingPassed) {
        Add-Error 'recorded process-creation binding is not derivable from evidence'
    }
    $preflightCaptured = [DateTimeOffset]::MinValue
    $attestationCaptured = [DateTimeOffset]::MinValue
    $processEvidenceChronologyPassed =
        [DateTimeOffset]::TryParseExact(
            [string]$processCreationBinding.preflight_captured_utc,
            'o',
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::RoundtripKind,
            [ref]$preflightCaptured
        ) -and
        [DateTimeOffset]::TryParseExact(
            [string]$processCreationBinding.attestation_captured_utc,
            'o',
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::RoundtripKind,
            [ref]$attestationCaptured
        ) -and
        $attemptChronologyPassed -and
        $preflightCaptured -ge $attemptStarted -and
        $attestationCaptured -ge $preflightCaptured -and
        $attestationCaptured -le $attemptCompleted
    if (-not $processEvidenceChronologyPassed) {
        Add-Error 'process attestation timestamps are outside the attempt chronology'
    }
    $listenerRecords = @($attempt.attestation.listener.records)
    $listenerRecordsPassed =
        $listenerRecords.Count -ge 1 -and
        @($listenerRecords | Where-Object {
            [int]$_.LocalPort -ne [int]$plan.port -or
            [UInt32]$_.OwningProcess -ne
                [UInt32]$attempt.attestation.process.pid -or
            [string]$_.State -notin @('Listen', '2') -or
            [string]$_.LocalAddress -ne '127.0.0.1'
        }).Count -eq 0
    $processAndListenerPassed =
        $processCommandBinding.passed -and
        $recordedCommandBindingPassed -and
        $processCreationBinding.passed -and
        $recordedCreationBindingPassed -and
        $processEvidenceChronologyPassed -and
        @($attempt.attestation.listener.owners).Count -eq 1 -and
        [UInt32]$attempt.attestation.listener.owners[0] -eq
            [UInt32]$attempt.attestation.process.pid -and
        $listenerRecordsPassed
    $endpointValuesPassed =
        $healthBodyOk -and $modelsBodyOk -and $propsBodyOk -and
        [int]$attempt.attestation.endpoints.health.status_code -eq 200 -and
        [int]$attempt.attestation.endpoints.models.status_code -eq 200 -and
        [int]$attempt.attestation.endpoints.props.status_code -eq 200 -and
        $modelEntries.Count -eq 1 -and
        [System.IO.Path]::GetFileName([string]$servedId) -eq
            $modelSpecForEvidence.file -and
        $attempt.attestation.endpoints.served_model_id -eq $servedId -and
        [int64]$attempt.attestation.endpoints.served_n_ctx -eq
            [int64]$servedContext -and
        $attempt.attestation.endpoints.served_n_ctx_source -eq
            'props.default_generation_settings.n_ctx' -and
        [int]$attempt.attestation.endpoints.total_slots -eq [int]$totalSlots -and
        [int64]$servedContext -eq [int64]$attempt.context -and
        [int]$totalSlots -eq [int]$plan.server.parallel_slots -and
        $supportsPreserve -eq $true -and
        $templateProbeHttpPassed -and
        $templateProbeFacts.passed -and
        (Test-JsonEquivalent -Left $templateProbeFacts `
            -Right $attempt.attestation.effective.template_attestation)
    $effectiveValuesPassed =
        $startupLogPresent -and
        (Get-Sha256Lower -Path $startupLogPath) -eq
            $attempt.attestation.effective.startup_log_sha256 -and
        $null -ne $effectiveGpuLayers -and
        [int]$effectiveGpuLayers -gt 0 -and
        [int]$attempt.attestation.effective.gpu_layers -eq
            [int]$effectiveGpuLayers -and
        [int]$attempt.attestation.effective.total_layers_reported -eq
            [int]$totalLayers -and
        [int64]$attempt.attestation.effective.context -eq
            [int64]$servedContext -and
        $attempt.attestation.effective.cache_type_k -eq 'q8_0' -and
        $attempt.attestation.effective.cache_type_v -eq 'q8_0' -and
        $attempt.attestation.effective.flash_attention -eq 'enabled' -and
        $attempt.attestation.effective.reasoning_enabled -and
        $attempt.attestation.effective.preserve_reasoning_supported -and
        $attempt.attestation.effective.preserve_reasoning_enabled -and
        $attempt.attestation.effective.preserve_reasoning_evidence_source -eq
            'apply-template-differential-v1' -and
        $attempt.attestation.effective.thinking_generation_prefix_enabled -and
        $templateProbeFacts.differential.preserve_thinking_default_effective -and
        $templateProbeFacts.differential.thinking_generation_prefix_effective -and
        [int]$attempt.attestation.effective.reasoning_budget -eq 1024 -and
        $attempt.attestation.effective.reasoning_budget_evidence_source -eq
            'frozen_llama_help_env_mapping_and_launch_environment' -and
        [int]$attempt.attestation.effective.request_timeout_seconds -eq 720 -and
        $attempt.attestation.effective.request_timeout_evidence_source -eq
            'frozen_llama_help_env_mapping_and_launch_environment' -and
        $attempt.attestation.effective.llama_help_sha256 -eq
            $controlManifest.binaries.llama_server.help_output_sha256 -and
        [int]$attempt.attestation.effective.preserve_disabled_warning_count -eq 0 -and
        $kvMatches.Count -ge 1 -and
        $flashMatches.Count -ge 1 -and
        $reasoningMatches.Count -ge 1 -and
        $preserveDisabledWarnings.Count -eq 0
    $requestedValuesPassed =
        [int]$attempt.attestation.requested.context -eq [int]$attempt.context -and
        [int]$attempt.attestation.requested.gpu_layers -eq
            [int]$modelSpecForEvidence.requested_gpu_layers -and
        $attempt.attestation.requested.cache_type_k -eq 'q8_0' -and
        $attempt.attestation.requested.cache_type_v -eq 'q8_0' -and
        $attempt.attestation.requested.flash_attention -eq 'on' -and
        [int]$attempt.attestation.requested.fit_target_mib -eq 1024 -and
        $attempt.attestation.requested.reasoning -eq 'on' -and
        [int]$attempt.attestation.requested.reasoning_budget -eq 1024 -and
        $attempt.attestation.requested.reasoning_preserve -eq 'true' -and
        [int]$attempt.attestation.requested.timeout_seconds -eq 720 -and
        [int]$attempt.attestation.requested.threads -eq [int]$plan.server.threads -and
        [int]$attempt.attestation.requested.batch_size -eq
            [int]$plan.server.batch_size -and
        [int]$attempt.attestation.requested.parallel_slots -eq
            [int]$plan.server.parallel_slots -and
        [int]$attempt.attestation.requested.seed -eq [int]$plan.server.seed

    $derivedAttestationPassed =
        $null -ne $startupEvidence -and
        $null -ne $attestationEvidence -and
        $attestationSchemaPassed -and
        $preflightPassed -and
        $launchFieldsPassed -and
        $launchProcessRecordPassed -and
        $runfilesPassed -and
        $liveModelIdentityPassed -and
        $liveFerricIdentityPassed -and
        $runtimeIdentityPassed -and
        $processAndListenerPassed -and
        $endpointValuesPassed -and
        $effectiveValuesPassed -and
        $requestedValuesPassed
    if ([bool]$attempt.attestation.passed -ne $derivedAttestationPassed) {
        Add-Error 'managed-server attestation pass flag is not derivable from raw evidence'
    }
}
elseif ($attempt.attestation.passed) {
    Add-Error 'failed startup cannot claim a passing attestation'
}

$derivedSmokePassed = $false
if ($attempt.attestation.passed) {
    $smokeEvidence = Get-RetainedJson -Name 'smoke.json' -Label 'smoke'
    $smokeCommand = Get-RetainedJson `
        -Name 'smoke-command.json' -Label 'smoke command'
    $beforeManifest = Get-RetainedJson `
        -Name 'smoke-workspace.before.json' -Label 'smoke before-manifest'
    $afterManifest = Get-RetainedJson `
        -Name 'smoke-workspace.after.json' -Label 'smoke after-manifest'
    $tracePath = Join-Path $resolvedAttempt 'smoke.trace.jsonl'
    $smokeStdoutPath = Join-Path $resolvedAttempt 'smoke.stdout.log'
    $smokeStderrPath = Join-Path $resolvedAttempt 'smoke.stderr.log'
    $traceVerifyStdoutPath = Join-Path $resolvedAttempt `
        'smoke-trace-verify.stdout.log'
    $traceVerifyStderrPath = Join-Path $resolvedAttempt `
        'smoke-trace-verify.stderr.log'
    $smokeProcessLogsPresent =
        (Test-Path -LiteralPath $smokeStdoutPath -PathType Leaf) -and
        (Test-Path -LiteralPath $smokeStderrPath -PathType Leaf)
    if (-not $smokeProcessLogsPresent) {
        Add-Error 'smoke process logs are missing'
    }
    if ($null -ne $smokeEvidence -and
        -not (Test-JsonEquivalent -Left $smokeEvidence -Right $attempt.smoke)) {
        Add-Error 'smoke.json differs from embedded smoke evidence'
    }

    $smokeCommandPassed = $false
    $smokeProcessRecordPassed = $false
    if ($null -ne $smokeCommand) {
        $smokeArgs = @($smokeCommand.arguments)
        $smokePromptPath = Join-Path $artifactDir $plan.smoke.prompt_file
        $nonceControlPath = Join-Path $artifactDir $plan.smoke.nonce_file
        $smokePrompt = (Get-Content -Raw -LiteralPath $smokePromptPath).TrimEnd(
            "`r", "`n"
        )
        $rawSmokeRoot = Join-Path $repoRoot `
            (Join-Path ([string]$plan.raw_attempt_root) $attempt.coordinate)
        $expectedSmokeArgs = @(
            'query',
            '--workspace', (Join-Path $rawSmokeRoot 'smoke-workspace'),
            '--model', [string]$attempt.attestation.endpoints.served_model_id,
            '--api-base', "http://127.0.0.1:$($plan.port)/v1",
            '--params-b', '27',
            '--quant', [string]$attempt.quant,
            '--family', 'qwen3.8',
            '--ctx', [string]$attempt.context,
            '--temperature', [string]$plan.smoke.temperature,
            '--protocol', [string]$plan.smoke.protocol,
            '--harness-policy', [string]$plan.smoke.harness_policy,
            '--tier', [string]$plan.smoke.tier,
            '--max-ring', [string]$plan.smoke.max_ring,
            '--max-turns', [string]$plan.smoke.max_turns,
            '--profile-dir', (Join-Path $rawSmokeRoot 'empty-profile'),
            '--no-config',
            '--no-stream',
            $smokePrompt
        )
        $smokeProcessRecordPassed = Test-ProcessRecord `
            -Record (Get-OptionalProperty -Value $attempt.smoke -Name 'process') `
            -ExpectedFile (Join-Path $repoRoot $plan.ferric.relative_path) `
            -ExpectedArguments $expectedSmokeArgs `
            -ExpectedStdoutFile 'smoke.stdout.log' `
            -ExpectedStderrFile 'smoke.stderr.log' `
            -Label 'nonce smoke'
        $smokeCommandPassed =
            $smokeCommand.executable -eq
                (Join-Path $repoRoot $plan.ferric.relative_path) -and
            $smokeCommand.working_directory -eq $repoRoot -and
            ($smokeArgs -join "`n") -eq ($expectedSmokeArgs -join "`n") -and
            $smokeCommand.prompt_sha256 -eq
                (Get-Sha256Lower -Path $smokePromptPath) -and
            $smokeCommand.nonce_sha256 -eq
                (Get-Sha256Lower -Path $nonceControlPath)
    }
    if (-not $smokeCommandPassed) {
        Add-Error 'smoke command is not the frozen nonce coordinate'
    }

    $workspaceEvidenceIntegrityPassed = $false
    $workspaceUnchangedDerived = $false
    $nonceStillExact = $false
    $workspaceTraceFiles = @()
    if ($null -ne $beforeManifest -and $null -ne $afterManifest) {
        $workspaceRoot = Join-Path $resolvedAttempt 'smoke-workspace'
        $noncePath = Join-Path $workspaceRoot 'nonce.txt'
        $currentWorkspaceManifest = if (Test-Path -LiteralPath $workspaceRoot) {
            Get-TreeManifest -Root $workspaceRoot -ExcludedPrefixes @('.ferric')
        }
        else {
            @()
        }
        $workspaceTraceFiles = @(
            Get-ChildItem -LiteralPath (Join-Path $workspaceRoot '.ferric/trace') `
                -File -Filter '*.jsonl' -ErrorAction SilentlyContinue
        )
        $nonceControlPath = Join-Path $artifactDir $plan.smoke.nonce_file
        $nonceControlItem = Get-Item -LiteralPath $nonceControlPath
        $expectedBeforeManifest = @([ordered]@{
            path = 'nonce.txt'
            bytes = [UInt64]$nonceControlItem.Length
            sha256 = Get-Sha256Lower -Path $nonceControlPath
        })
        $workspaceUnchangedDerived = Test-ManifestEqual `
            -Before $beforeManifest -After $afterManifest
        $nonceStillExact =
            (Test-Path -LiteralPath $noncePath -PathType Leaf) -and
            ((Get-Content -Raw -LiteralPath $noncePath).TrimEnd("`r", "`n") -eq
                $plan.smoke.require_exact_summary)
        $workspaceEvidenceIntegrityPassed =
            (Test-JsonEquivalent -Left $beforeManifest `
                -Right $expectedBeforeManifest) -and
            (Test-JsonEquivalent -Left $afterManifest `
                -Right $currentWorkspaceManifest) -and
            [int]$attempt.smoke.trace_count -eq $workspaceTraceFiles.Count -and
            [bool]$attempt.smoke.workspace_unchanged -eq
                $workspaceUnchangedDerived -and
            (Test-JsonEquivalent -Left $attempt.smoke.before_manifest `
                -Right $beforeManifest) -and
            (Test-JsonEquivalent -Left $attempt.smoke.after_manifest `
                -Right $afterManifest)
    }
    if (-not $workspaceEvidenceIntegrityPassed) {
        Add-Error 'smoke workspace evidence is internally inconsistent'
    }

    $traceCount = $workspaceTraceFiles.Count
    $traceFileIntegrityPassed = $false
    $traceVerifyEvidencePassed = $false
    $traceFacts = $null
    $freshTraceParseError = $null
    $ferricTraceVerified = $false
    $traceFactsPassed = $false
    $recordedTraceVerify = Get-OptionalProperty -Value $attempt.smoke `
        -Name 'trace_verify'
    $traceVerifyNotRunReason = [string](Get-OptionalProperty `
        -Value $attempt.smoke -Name 'trace_verify_not_run_reason')
    $recordedTraceParseError = [string](Get-OptionalProperty `
        -Value $attempt.smoke -Name 'trace_parse_error')
    if ($traceCount -eq 1) {
        $traceFileIntegrityPassed =
            (Test-Path -LiteralPath $tracePath -PathType Leaf) -and
            $null -ne $attempt.smoke.trace_sha256 -and
            $attempt.smoke.trace_sha256 -eq (Get-Sha256Lower -Path $tracePath) -and
            (Get-Sha256Lower -Path $workspaceTraceFiles[0].FullName) -eq
                (Get-Sha256Lower -Path $tracePath)
        if (-not $traceFileIntegrityPassed) {
            Add-Error 'single smoke trace is not retained and hash-bound'
        }
        try {
            $traceFacts = Get-TraceFacts -TracePath $tracePath `
                -ExpectedNonce $plan.smoke.require_exact_summary `
                -ForbiddenTools @($plan.smoke.forbidden_tools)
        }
        catch {
            $freshTraceParseError = $_.Exception.Message
        }
        $parseEvidencePassed =
            (
                $null -ne $traceFacts -and
                [string]::IsNullOrWhiteSpace($freshTraceParseError) -and
                [string]::IsNullOrWhiteSpace($recordedTraceParseError) -and
                (Test-JsonEquivalent -Left $attempt.smoke.trace_facts `
                    -Right $traceFacts)
            ) -or
            (
                $null -eq $traceFacts -and
                -not [string]::IsNullOrWhiteSpace($freshTraceParseError) -and
                -not [string]::IsNullOrWhiteSpace($recordedTraceParseError) -and
                $null -eq $attempt.smoke.trace_facts
            )
        if (-not $parseEvidencePassed) {
            Add-Error 'smoke trace parse result differs from retained summary'
        }
        if ($null -ne $recordedTraceVerify) {
            $traceVerifyLogsPresent =
                (Test-Path -LiteralPath $traceVerifyStdoutPath -PathType Leaf) -and
                (Test-Path -LiteralPath $traceVerifyStderrPath -PathType Leaf)
            $traceVerifyProcessRecordPassed = Test-ProcessRecord `
                -Record $recordedTraceVerify `
                -ExpectedFile (Join-Path $repoRoot $plan.ferric.relative_path) `
                -ExpectedArguments @(
                    'trace',
                    'verify',
                    (Join-Path $rawSmokeRoot 'smoke.trace.jsonl')
                ) `
                -ExpectedStdoutFile 'smoke-trace-verify.stdout.log' `
                -ExpectedStderrFile 'smoke-trace-verify.stderr.log' `
                -Label 'trace verifier'
            $freshTraceVerify = Invoke-BoundedProcessResult `
                -FilePath (Join-Path $repoRoot $plan.ferric.relative_path) `
                -Arguments @('trace', 'verify', $tracePath) `
                -TimeoutMilliseconds 60000
            if ($freshTraceVerify.timed_out) {
                Add-Error 'fresh Ferric trace verification timed out'
            }
            $ferricTraceVerified =
                -not $freshTraceVerify.timed_out -and
                [int]$freshTraceVerify.exit_code -eq 0
            $traceVerifyEvidencePassed =
                $traceVerifyLogsPresent -and
                $traceVerifyProcessRecordPassed -and
                [string]::IsNullOrWhiteSpace($traceVerifyNotRunReason)
        }
        else {
            $traceVerifyEvidencePassed =
                $traceVerifyNotRunReason -eq
                    'quant_wall_cap_expired_before_trace_verify' -and
                $null -eq $attempt.smoke.trace_verify -and
                (([bool]$attempt.smoke.process.execution_timed_out) -or
                    [bool]$attempt.wall_cap_breached) -and
                (Test-Path -LiteralPath $traceVerifyStdoutPath -PathType Leaf) -and
                (Test-Path -LiteralPath $traceVerifyStderrPath -PathType Leaf)
            if (-not $traceVerifyEvidencePassed) {
                Add-Error 'missing trace-verifier process lacks a valid wall-cap cause'
            }
        }
        $traceFactsPassed =
            $null -ne $traceFacts -and
            $traceFacts.protocol -eq 'constrained_json' -and
            $traceFacts.all_turns_json_schema_constrained -and
            $traceFacts.read_file_before_task_complete -and
            [int]$traceFacts.exact_nonce_read_result_count -ge 1 -and
            $traceFacts.exact_task_complete_summary -and
            @($traceFacts.forbidden_tools_observed).Count -eq 0 -and
            $traceFacts.session_end_reason -eq 'task_complete'
    }
    else {
        $traceFileIntegrityPassed =
            -not (Test-Path -LiteralPath $tracePath -PathType Leaf) -and
            $null -eq $attempt.smoke.trace_sha256 -and
            $null -eq $recordedTraceVerify -and
            $null -eq $attempt.smoke.trace_facts -and
            [string]::IsNullOrWhiteSpace($recordedTraceParseError) -and
            $traceVerifyNotRunReason -eq 'trace_count_not_one' -and
            -not (Test-Path -LiteralPath $traceVerifyStdoutPath) -and
            -not (Test-Path -LiteralPath $traceVerifyStderrPath)
        $traceVerifyEvidencePassed = $traceFileIntegrityPassed
        if (-not $traceFileIntegrityPassed) {
            Add-Error 'non-singleton smoke trace outcome is internally inconsistent'
        }
    }

    $smokeProcessPassed =
        $smokeProcessLogsPresent -and
        -not $attempt.smoke.process.timed_out -and
        [int]$attempt.smoke.process.exit_code -eq 0
    $traceVerifyProcessPassed =
        $null -ne $recordedTraceVerify -and
        -not $recordedTraceVerify.timed_out -and
        [int]$recordedTraceVerify.exit_code -eq 0
    $derivedSmokePassed =
        $null -ne $smokeEvidence -and
        $smokeCommandPassed -and
        $smokeProcessRecordPassed -and
        $smokeProcessLogsPresent -and
        $workspaceEvidenceIntegrityPassed -and
        $workspaceUnchangedDerived -and
        $nonceStillExact -and
        $traceFileIntegrityPassed -and
        $traceVerifyEvidencePassed -and
        $ferricTraceVerified -and
        $traceFactsPassed -and
        $smokeProcessPassed -and
        $traceVerifyProcessPassed
    if ([bool]$attempt.smoke.passed -ne $derivedSmokePassed) {
        Add-Error 'smoke pass flag is not derivable from retained trace evidence'
    }
}
elseif ($attempt.smoke.passed) {
    Add-Error 'failed attestation cannot claim a passing smoke'
}

$throughputRows = @()
$throughputPath = Join-Path $resolvedAttempt 'throughput.jsonl'
if ($attempt.smoke.passed) {
    if (-not (Test-Path -LiteralPath $throughputPath -PathType Leaf)) {
        Add-Error 'smoke passed but throughput.jsonl is absent'
    }
    else {
        $throughputRows = Read-JsonLines -Path $throughputPath
        [void](Test-MemorySnapshot `
            -Snapshot (Get-OptionalProperty -Value $attempt.throughput `
                -Name 'memory_after_measurement') `
            -ExpectedGpu $expectedGpuIdentity -Label 'after measurement')
        $expectedLabels = @($plan.throughput.sequence)
        $observedLabels = @($throughputRows | ForEach-Object { $_.label })
        if ($throughputRows.Count -ne 4 -or
            ($observedLabels -join "`n") -ne ($expectedLabels -join "`n")) {
            Add-Error 'throughput sequence is not exactly warmup plus three fixed trials'
        }
        $requestPath = Join-Path $resolvedAttempt 'throughput-request.json'
        if (-not (Test-Path -LiteralPath $requestPath -PathType Leaf)) {
            Add-Error 'throughput request body is absent'
        }
        else {
            $requestHash = Get-Sha256Lower -Path $requestPath
            $templatePath = Join-Path $artifactDir $plan.throughput.request_template
            $templateText = Get-Content -Raw -LiteralPath $templatePath
            $escapedServedModel =
                $attempt.attestation.endpoints.served_model_id |
                    ConvertTo-Json -Compress
            $expectedRequestText = $templateText.Replace(
                '"__SERVED_MODEL_ID__"',
                $escapedServedModel
            ).Replace("`r`n", "`n").Replace("`r", "`n")
            $actualRequestText = [System.Text.Encoding]::UTF8.GetString(
                [System.IO.File]::ReadAllBytes($requestPath)
            )
            if ($actualRequestText -ne $expectedRequestText) {
                Add-Error 'throughput request bytes do not derive from the frozen template'
            }
            if ($attempt.throughput.request_sha256 -ne $requestHash) {
                Add-Error 'throughput summary request hash mismatch'
            }
            for ($rowIndex = 0; $rowIndex -lt $throughputRows.Count; $rowIndex++) {
                $row = $throughputRows[$rowIndex]
                $expectedScored = $rowIndex -gt 0
                if ([int]$row.ordinal -ne ($rowIndex + 1) -or
                    [bool]$row.scored -ne $expectedScored) {
                    Add-Error "ordinal/scoring drift in sample $($row.label)"
                }
                if ($row.request_sha256 -ne $requestHash) {
                    Add-Error "request drift in sample $($row.label)"
                }
                $expectedResponseFile =
                    "throughput-$($expectedLabels[$rowIndex]).response.json"
                $rowQuantElapsed = [double]$row.quant_elapsed_before_request_seconds
                $rowRemainingMs = [int64]$row.remaining_wall_ms_before_request
                $derivedRemainingMs = [Math]::Max(
                    0,
                    ([int64]$plan.quant_wall_cap_seconds * 1000) -
                        [int64][Math]::Round($rowQuantElapsed * 1000.0)
                )
                if (-not [double]::IsFinite($rowQuantElapsed) -or
                    $rowQuantElapsed -lt
                        [double]$attempt.prior_quant_elapsed_seconds -or
                    $rowRemainingMs -lt 0 -or
                    [Math]::Abs($rowRemainingMs - $derivedRemainingMs) -gt 2) {
                    Add-Error "wall-budget evidence drift in sample $($row.label)"
                }
                $expectedExchangeTimeout = if ($rowRemainingMs -le 0) {
                    0
                }
                else {
                    [Math]::Max(
                        1,
                        [Math]::Min(
                            [int]$plan.server_request_timeout_seconds,
                            [int][Math]::Ceiling($rowRemainingMs / 1000.0)
                        )
                    )
                }
                if ($row.exchange.method -ne 'POST' -or
                    $row.exchange.uri -ne
                        "http://127.0.0.1:$($plan.port)/v1/chat/completions" -or
                    $row.exchange.response_file -ne $expectedResponseFile -or
                    [UInt64]$row.request_bytes -ne
                        [UInt64](Get-Item -LiteralPath $requestPath).Length -or
                    [int]$row.exchange.timeout_seconds -ne
                        [int]$expectedExchangeTimeout) {
                    Add-Error "exchange metadata drift in sample $($row.label)"
                }
                [void](Test-ExchangeFile -Exchange $row.exchange `
                    -Label "throughput $($row.label)")
                $responsePath = Join-Path $resolvedAttempt `
                    $row.exchange.response_file
                if (-not (Test-Path -LiteralPath $responsePath -PathType Leaf)) {
                    Add-Error "missing response file for $($row.label)"
                    continue
                }
                if ((Get-Sha256Lower -Path $responsePath) -ne
                    $row.exchange.response_sha256) {
                    Add-Error "response hash mismatch for $($row.label)"
                }
                $rawResponse = [System.Text.Encoding]::UTF8.GetString(
                    [System.IO.File]::ReadAllBytes($responsePath)
                )
                if ($rawResponse -ne [string]$row.raw_response) {
                    Add-Error "embedded response bytes drifted for $($row.label)"
                }
                $responseObject = $null
                try {
                    $responseObject = $rawResponse | ConvertFrom-Json
                }
                catch {
                    $responseObject = $null
                }
                $rawUsage = Get-OptionalProperty -Value $responseObject -Name 'usage'
                $rawTimings = Get-OptionalProperty -Value $responseObject -Name 'timings'
                $rawCompletionTokens = Get-OptionalProperty `
                    -Value $rawUsage -Name 'completion_tokens'
                $rawPredictedTokens = Get-OptionalProperty `
                    -Value $rawTimings -Name 'predicted_n'
                $rawPredictedMilliseconds = Get-OptionalProperty `
                    -Value $rawTimings -Name 'predicted_ms'
                $rawReportedRate = Get-OptionalProperty `
                    -Value $rawTimings -Name 'predicted_per_second'
                if ($row.usage_completion_tokens -ne $rawCompletionTokens -or
                    $row.timings_predicted_n -ne $rawPredictedTokens -or
                    $row.timings_predicted_ms -ne $rawPredictedMilliseconds -or
                    $row.timings_reported_per_second -ne $rawReportedRate) {
                    Add-Error "sample counters are not derived from raw response $($row.label)"
                }
                $computedRate = $null
                if ($null -ne $rawPredictedTokens -and
                    $null -ne $rawPredictedMilliseconds -and
                    [double]$rawPredictedMilliseconds -gt 0) {
                    $computedRate = [double]$rawPredictedTokens /
                        ([double]$rawPredictedMilliseconds / 1000.0)
                }
                if ($null -ne $computedRate -and
                    [Math]::Abs($computedRate -
                        [double]$row.computed_decoded_tokens_per_second) -gt 0.000001) {
                    Add-Error "decoded rate was not derived from counters for $($row.label)"
                }
                $expectedCounterConsistency =
                    $null -ne $rawCompletionTokens -and
                    $null -ne $rawPredictedTokens -and
                    ([int]$rawCompletionTokens -eq [int]$rawPredictedTokens)
                if ([bool]$row.counter_consistent -ne $expectedCounterConsistency) {
                    Add-Error "counter consistency is misreported for $($row.label)"
                }
                $expectedRateConsistency =
                    $null -ne $computedRate -and
                    $null -ne $rawReportedRate -and
                    ([Math]::Abs(
                        [double]$computedRate - [double]$rawReportedRate
                    ) -le [Math]::Max(0.01, [double]$computedRate * 0.01))
                if ([bool]$row.rate_consistent -ne $expectedRateConsistency) {
                    Add-Error "rate consistency is misreported for $($row.label)"
                }
                $expectedFailureCause = if ($null -ne $row.exchange.error) {
                    if ([string]$row.exchange.error -match
                        '(?i)timed?\s*out|timeout|taskcanceled|cancelled|canceled') {
                        'timeout'
                    }
                    else {
                        'request_error'
                    }
                }
                elseif ([int]$row.exchange.status_code -lt 200 -or
                    [int]$row.exchange.status_code -ge 300) {
                    'request_error'
                }
                elseif ($null -eq $responseObject) {
                    'malformed_response'
                }
                elseif (-not $expectedCounterConsistency) {
                    'counter_inconsistency'
                }
                elseif (-not $expectedRateConsistency) {
                    'rate_inconsistency'
                }
                elseif ([int]$rawPredictedTokens -lt
                    [int]$plan.throughput.minimum_decoded_tokens) {
                    'decoded_length_below_minimum'
                }
                elseif ([int]$rawPredictedTokens -gt
                    [int]$plan.throughput.max_tokens) {
                    'decoded_length_above_limit'
                }
                else {
                    $null
                }
                if ($row.failure_cause -ne $expectedFailureCause) {
                    Add-Error "failure cause is misreported for $($row.label)"
                }
                $expectedValid = $null -eq $expectedFailureCause
                if ([bool]$row.valid -ne $expectedValid) {
                    Add-Error "sample validity is misreported for $($row.label)"
                }
            }
            $validRequests = @($throughputRows | Where-Object { $_.valid })
            $trials = @($throughputRows | Where-Object { $_.label -ne 'warmup' })
            $validTrials = @($trials | Where-Object { $_.valid })
            $median = $null
            if ($trials.Count -eq 3 -and $validTrials.Count -eq 3) {
                $median = Get-Median -Values ([double[]]@(
                    $validTrials | ForEach-Object {
                        $_.computed_decoded_tokens_per_second
                    }
                ))
            }
            if ($validTrials.Count -ne [int]$attempt.throughput.valid_trial_count) {
                Add-Error 'valid trial count is not derivable from raw rows'
            }
            if ($validRequests.Count -ne
                [int]$attempt.throughput.valid_request_count) {
                Add-Error 'valid request count is not derivable from raw rows'
            }
            if ($null -eq $median) {
                if ($null -ne $attempt.throughput.median_decoded_tokens_per_second) {
                    Add-Error 'invalid sample set must have a null median'
                }
            }
            elseif ([Math]::Abs(
                [double]$median -
                [double]$attempt.throughput.median_decoded_tokens_per_second
            ) -gt 0.000001) {
                Add-Error 'throughput median is not derivable from raw rows'
            }
            $expectedThroughputPass =
                $throughputRows.Count -eq 4 -and
                $validRequests.Count -eq 4 -and
                $validTrials.Count -eq 3 -and
                $null -ne $median -and
                [double]$median -ge
                    [double]$plan.throughput.minimum_median_decoded_tokens_per_second
            if ([bool]$attempt.throughput.passed -ne $expectedThroughputPass) {
                Add-Error 'throughput pass flag is not derivable from raw rows'
            }
            $throughputSummaryEvidence = Get-RetainedJson `
                -Name 'throughput-summary.json' -Label 'throughput summary'
            if ($null -eq $throughputSummaryEvidence -or
                -not (Test-JsonEquivalent -Left $throughputSummaryEvidence `
                    -Right $attempt.throughput) -or
                $attempt.throughput.template_sha256 -ne
                    (Get-Sha256Lower -Path $templatePath) -or
                (@($attempt.throughput.scheduled_samples) -join "`n") -ne
                    ($expectedLabels -join "`n") -or
                [int]$attempt.throughput.observed_samples -ne
                    $throughputRows.Count -or
                -not (Test-JsonEquivalent -Left $attempt.throughput.samples `
                    -Right $throughputRows)) {
                Add-Error 'throughput summary is not an exact derivation of raw samples'
            }
        }
    }
}
elseif (Test-Path -LiteralPath $throughputPath -PathType Leaf) {
    Add-Error 'throughput ran despite a failed functional smoke'
}

if ($attempt.startup.healthy) {
    if (-not (Test-Path -LiteralPath (Join-Path $resolvedAttempt 'server.log'))) {
        Add-Error 'healthy startup lacks the complete server log'
    }
}
elseif ($attempt.startup.classification -eq 'startup_memory_pressure' -and
    -not $attempt.startup.memory_match.matched) {
    Add-Error 'memory-pressure classification lacks a retained matching diagnostic'
}

$expectedViable =
    $attempt.startup.healthy -and
    $attempt.attestation.passed -and
    $attempt.smoke.passed -and
    $attempt.throughput.passed -and
    $attempt.teardown.passed -and
    (-not $attempt.wall_cap_breached) -and
    ($null -eq $attempt.fatal_error)
$expectedInfrastructureBlocked =
    $attempt.wall_cap_breached -or
    (-not $attempt.teardown.passed) -or
    ($null -ne $attempt.fatal_error) -or
    ($attempt.startup.healthy -and -not $attempt.attestation.passed) -or
    (-not $attempt.startup.healthy)
$expectedVerdict = if ($expectedViable) {
    'viable'
}
elseif ($expectedInfrastructureBlocked) {
    'infrastructure_blocked'
}
else {
    'non_viable'
}
if ($attempt.verdict -ne $expectedVerdict) {
    Add-Error 'attempt verdict is not derivable from component results'
}
$expectedEvidenceComplete = -not $expectedInfrastructureBlocked
if ([bool]$attempt.evidence_complete -ne $expectedEvidenceComplete) {
    Add-Error 'evidence-completeness flag is not derivable from component results'
}
$expectedReasonCodes = [System.Collections.Generic.List[string]]::new()
if (-not $attempt.startup.healthy) {
    $expectedReasonCodes.Add([string]$attempt.startup.classification)
}
if ($attempt.startup.healthy -and -not $attempt.attestation.passed) {
    $expectedReasonCodes.Add('managed_server_attestation_failed')
}
if ($attempt.attestation.passed -and -not $attempt.smoke.passed) {
    $expectedReasonCodes.Add('functional_smoke_failed')
}
if ($attempt.smoke.passed -and -not $attempt.throughput.passed) {
    if ([int]$attempt.throughput.valid_request_count -ne 4 -or
        [int]$attempt.throughput.valid_trial_count -ne 3) {
        $expectedReasonCodes.Add('invalid_throughput_sample_set')
    }
    elseif ($null -ne $attempt.throughput.median_decoded_tokens_per_second -and
        [double]$attempt.throughput.median_decoded_tokens_per_second -lt
            [double]$plan.throughput.minimum_median_decoded_tokens_per_second) {
        $expectedReasonCodes.Add('throughput_median_below_floor')
    }
    else {
        $expectedReasonCodes.Add('throughput_failed')
    }
}
if ($attempt.wall_cap_breached) {
    $expectedReasonCodes.Add('quant_wall_cap_breached')
}
if (-not $attempt.teardown.passed) {
    $expectedReasonCodes.Add('teardown_incomplete')
}
if ($null -ne $attempt.fatal_error) {
    $expectedReasonCodes.Add('orchestration_error')
}
if ((@($attempt.reason_codes) -join "`n") -ne
    (@($expectedReasonCodes) -join "`n")) {
    Add-Error 'reason-code sequence is not derivable from component results'
}
$expectedFailureClassification = if (-not $attempt.startup.healthy) {
    [string]$attempt.startup.classification
}
elseif (-not $attempt.attestation.passed) {
    'managed_server_attestation_failed'
}
elseif (-not $attempt.smoke.passed) {
    'functional_smoke_failed'
}
elseif (-not $attempt.throughput.passed) {
    'throughput_non_viable'
}
else {
    $null
}
if ($attempt.failure_classification -ne $expectedFailureClassification) {
    Add-Error 'failure classification is not derivable from component results'
}

$report = [ordered]@{
    schema = 'animus-ferric-runtime-verification-v3'
    task = 'T-11409'
    control_epoch = 3
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
    live_model_identity = [ordered]@{
        checked = -not $deferLiveModelHash
        mode = if ($deferLiveModelHash) {
            'deferred_to_freeze'
        }
        else {
            'checked_in_verifier'
        }
        sha256 = $liveModelSha256
    }
    attempt_path = $resolvedAttempt
    coordinate = $attempt.coordinate
    verdict = $attempt.verdict
    passed = ($errors.Count -eq 0)
    manifest = $manifestCheck
    control_anchor_mode = $controlAnchorMode
    throughput_rows = $throughputRows.Count
    errors = @($errors)
}
$report | ConvertTo-Json -Depth 32
if ($errors.Count -gt 0) {
    exit 1
}
exit 0
}
catch {
    $failurePath = try {
        [System.IO.Path]::GetFullPath($AttemptPath)
    }
    catch {
        $AttemptPath
    }
    [ordered]@{
        schema = 'animus-ferric-runtime-verification-v3'
        task = 'T-11409'
        control_epoch = 3
        attestation_protocol = 'apply-template-differential-v1'
        process_command_protocol = 'windows-bound-process-command-v1'
        attempt_path = $failurePath
        passed = $false
        errors = @(
            "validator exception: $($_.Exception.Message)"
            "validator location: $($_.InvocationInfo.PositionMessage)"
            "validator stack: $($_.ScriptStackTrace)"
        )
    } | ConvertTo-Json -Depth 16
    exit 1
}
