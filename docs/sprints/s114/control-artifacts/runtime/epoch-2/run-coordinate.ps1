[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet(
        'e02-01-q4-32768',
        'e02-02-q4-16384',
        'e02-03-q3-32768',
        'e02-04-q3-16384'
    )]
    [string]$Coordinate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    throw 'runtime plan is not the frozen epoch-2 recovery protocol'
}
$coordinatePlan = @($plan.coordinates | Where-Object { $_.id -eq $Coordinate })
if ($coordinatePlan.Count -ne 1) {
    throw "coordinate is not uniquely declared: $Coordinate"
}
$coordinatePlan = $coordinatePlan[0]
$modelSpec = $plan.models.($coordinatePlan.quant)

$rawParent = Join-Path $repoRoot ([string]$plan.raw_attempt_root)
$rawDir = Join-Path $rawParent $Coordinate
$archiveParent = Join-Path $artifactDir 'attempts'
$archiveDir = Join-Path $archiveParent $Coordinate
$controlManifestPath = Join-Path $artifactDir 'control-inputs.json'
$controlDigestPath = Join-Path $artifactDir 'control-inputs.sha256'
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$gateVerifierPath = Join-Path $artifactDir 'verify-q4-gate.ps1'
$localRunfile = Join-Path $repoRoot '.ferric/server.json'
$globalRunfile = Join-Path (Join-Path $env:APPDATA 'ferric') 'server.json'
$ferricPath = Join-Path $repoRoot $plan.ferric.relative_path
$llamaBin = Join-Path $repoRoot $plan.llama_cpp.ignored_runtime_relative_path
$llamaPath = Join-Path $llamaBin 'llama-server.exe'
$cudaBackendPath = Join-Path $llamaBin 'ggml-cuda.dll'
$modelPath = Join-Path $repoRoot $modelSpec.relative_path
$journalPath = Join-Path $rawDir 'command-journal.jsonl'
$calibrationLockDirectory = Join-Path $repoRoot `
    'target/s114-experiment/runtime-lock'
[System.IO.Directory]::CreateDirectory($calibrationLockDirectory) | Out-Null
$calibrationLockPath = Join-Path $calibrationLockDirectory 'calibration.lock'
try {
    $calibrationLock = [System.IO.FileStream]::new(
        $calibrationLockPath,
        [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
}
catch [System.IO.IOException] {
    throw 'another Sprint 114 calibration process holds the exclusive runtime lock'
}
try {
$lockRecord = [ordered]@{
    schema = 'animus-ferric-runtime-lock-v1'
    pid = $PID
    coordinate = $Coordinate
    acquired_at_utc = (Get-Date).ToUniversalTime().ToString('o')
}
$lockBytes = [System.Text.UTF8Encoding]::new($false).GetBytes(
    (($lockRecord | ConvertTo-Json -Compress) + "`n")
)
$calibrationLock.SetLength(0)
$calibrationLock.Write($lockBytes, 0, $lockBytes.Length)
$calibrationLock.Flush($true)
$attemptStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$attemptStartedAt = (Get-Date).ToUniversalTime().ToString('o')
$inheritedQuantSeconds = 0.0
$frozenLlamaHelpSha256 = $null

function Add-JournalEntry {
    param(
        [Parameter(Mandatory = $true)][string]$Kind,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)]$Details
    )

    $row = [ordered]@{
        schema = 'animus-ferric-runtime-journal-row-v1'
        at_utc = (Get-Date).ToUniversalTime().ToString('o')
        elapsed_ms = [Math]::Round($attemptStopwatch.Elapsed.TotalMilliseconds, 3)
        kind = $Kind
        name = $Name
        details = $Details
    }
    $json = $row | ConvertTo-Json -Depth 64 -Compress
    [System.IO.File]::AppendAllText(
        $journalPath,
        ($json + "`n"),
        [System.Text.UTF8Encoding]::new($false)
    )
}

function Get-OptionalProperty {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Name
    )

    if ($null -eq $Value) {
        return $null
    }
    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    $property.Value
}

function Register-LiveWrapperProcess {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][string]$Source
    )

    if ($null -eq $Record -or
        -not [bool](Get-OptionalProperty -Value $Record `
            -Name 'post_process_alive')) {
        return
    }
    $script:liveWrapperProcessRecords.Add([ordered]@{
        source = $Source
        pid = [UInt32]$Record.pid
        file = [string]$Record.file
        started_at_utc = [string]$Record.started_at_utc
    })
}

function Get-RemainingMilliseconds {
    $remaining = ([int64]$plan.quant_wall_cap_seconds * 1000) -
        [int64][Math]::Ceiling($inheritedQuantSeconds * 1000.0) -
        [int64]$attemptStopwatch.ElapsedMilliseconds
    if ($remaining -le 0) {
        return 0
    }
    [int][Math]::Min($remaining, [int]::MaxValue)
}

function Get-OperationTimeoutMilliseconds {
    param(
        [Parameter(Mandatory = $true)][int]$MaximumMilliseconds,
        [int]$RequiredTailMilliseconds = 120000,
        [Parameter(Mandatory = $true)][string]$Operation
    )

    $available = (Get-RemainingMilliseconds) - $RequiredTailMilliseconds
    if ($available -le 0) {
        throw "quant wall cap lacks the reserved tail before $Operation"
    }
    [int][Math]::Min($MaximumMilliseconds, $available)
}

function Assert-ControlInputs {
    if (-not (Test-Path -LiteralPath $controlManifestPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $controlDigestPath -PathType Leaf)) {
        throw 'runtime controls have not been frozen'
    }
    $digestLine = (Get-Content -Raw -LiteralPath $controlDigestPath).Trim()
    if ($digestLine -notmatch '^([0-9a-f]{64})  control-inputs\.json$') {
        throw 'malformed control-inputs.sha256'
    }
    if ((Get-Sha256Lower -Path $controlManifestPath) -ne $Matches[1]) {
        throw 'control-inputs.json digest mismatch'
    }
    $controlManifest = Get-Content -Raw -LiteralPath $controlManifestPath |
        ConvertFrom-Json
    if ($controlManifest.schema -cne
            'animus-ferric-runtime-control-inputs-v2' -or
        $controlManifest.task -cne 'T-11409' -or
        [int]$controlManifest.control_epoch -ne 2 -or
        $controlManifest.attestation_protocol -cne
            [string]$plan.template_attestation.protocol -or
        -not (Test-JsonEquivalent -Left @($controlManifest.epoch_1_anchors) `
            -Right @($plan.recovery.epoch_1_anchors))) {
        throw 'runtime control manifest is not the frozen epoch-2 recovery'
    }
    if ($controlManifest.runtime_plan_sha256 -ne
        (Get-Sha256Lower -Path $planPath)) {
        throw 'runtime plan changed after freeze'
    }
    $recoveryAnchors = Test-RecoveryAnchors -Plan $plan `
        -RepositoryRoot $repoRoot
    if (-not $recoveryAnchors.passed) {
        throw ($recoveryAnchors.errors -join '; ')
    }
    foreach ($control in $controlManifest.controls) {
        $path = Join-Path $artifactDir $control.path
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-Sha256Lower -Path $path) -ne $control.sha256) {
            throw "runtime control changed after freeze: $($control.path)"
        }
    }
    if ((Get-Sha256Lower -Path $ferricPath) -ne
        $controlManifest.binaries.ferric.sha256) {
        throw 'Ferric binary changed after runtime freeze'
    }
    if ((Get-Sha256Lower -Path $llamaPath) -ne
        $controlManifest.binaries.llama_server.sha256) {
        throw 'llama-server binary changed after runtime freeze'
    }
    if ((Get-Sha256Lower -Path $cudaBackendPath) -ne
        $controlManifest.binaries.cuda_backend.sha256) {
        throw 'CUDA backend changed after runtime freeze'
    }
    $runtimeIdentity = Test-FileIdentityManifest -Root $llamaBin `
        -Expected @($controlManifest.binaries.llama_runtime.files)
    if (-not $runtimeIdentity.passed) {
        throw "llama.cpp runtime tree changed after freeze: $($runtimeIdentity.errors -join '; ')"
    }
    $helpTimeout = Get-OperationTimeoutMilliseconds `
        -MaximumMilliseconds 30000 -Operation 'llama-server help verification'
    $llamaHelp = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
        -Arguments @('--help') -TimeoutMilliseconds $helpTimeout)
    $llamaHelpText = ($llamaHelp -join "`n") + "`n"
    $script:frozenLlamaHelpSha256 = Get-Sha256Text -Text $llamaHelpText
    if ($frozenLlamaHelpSha256 -ne
        $controlManifest.binaries.llama_server.help_output_sha256) {
        throw 'llama-server option/environment mapping help changed after freeze'
    }
    foreach ($mapping in
        $controlManifest.binaries.llama_server.option_environment_mappings.PSObject.Properties) {
        if ($llamaHelpText -notmatch [string]$mapping.Value) {
            throw "llama-server option/environment mapping is absent: $($mapping.Name)"
        }
    }
    $controlManifest
}

function Assert-EnvironmentClean {
    $observed = @(
        Get-ChildItem Env: |
            Where-Object {
                $_.Name -like 'LLAMA_ARG_*' -or
                $_.Name -like 'FERRIC_*' -or
                $_.Name -like 'GGML_*' -or
                $_.Name -like 'CUDA_*' -or
                $_.Name -like 'OMP_*' -or
                $_.Name -like 'MKL_*' -or
                $_.Name -eq 'OPENAI_API_KEY' -or
                $_.Name -in @('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY')
            } |
            Select-Object -ExpandProperty Name |
            Sort-Object
    )
    if ($observed.Count -gt 0) {
        throw "undeclared inherited runtime environment is forbidden: $($observed -join ', ')"
    }
    $observed
}

function Assert-ColdCapacity {
    if ((Test-Path -LiteralPath $localRunfile) -or
        (Test-Path -LiteralPath $globalRunfile)) {
        throw 'a local or global managed-server runfile already exists'
    }
    $listeners = @(Get-NetTCPConnection -State Listen `
        -LocalPort $plan.port -ErrorAction SilentlyContinue)
    if ($listeners.Count -gt 0) {
        throw "port $($plan.port) is already listening"
    }
    $llamaProcesses = @(
        Get-CimInstance Win32_Process -OperationTimeoutSec 5 |
            Where-Object { $_.Name -like 'llama-server*' } |
            Select-Object ProcessId, Name, ExecutablePath, CommandLine
    )
    if ($llamaProcesses.Count -gt 0) {
        throw 'a pre-existing llama-server process would contaminate calibration'
    }
    $memory = Get-MemorySnapshot
    if ($null -eq $memory.gpu -or
        [UInt64]$memory.gpu.free_mib -lt
            [UInt64]$plan.minimum_gpu_free_mib_before_launch) {
        throw 'GPU free memory is below the frozen uncontended calibration floor'
    }
    $memory
}

function Assert-CoordinateAuthorization {
    if ($null -ne $coordinatePlan.predecessor) {
        $predecessorDir = Join-Path $archiveParent $coordinatePlan.predecessor
        $predecessorManifest = Join-Path $predecessorDir 'files.sha256'
        $predecessorAttemptPath = Join-Path $predecessorDir 'attempt.json'
        if (-not (Test-Path -LiteralPath $predecessorAttemptPath -PathType Leaf) -or
            -not (Test-Path -LiteralPath $predecessorManifest -PathType Leaf)) {
            throw "context retry lacks archived predecessor: $($coordinatePlan.predecessor)"
        }
        $predecessor = Get-Content -Raw -LiteralPath $predecessorAttemptPath |
            ConvertFrom-Json
        $predecessorStarted = [DateTimeOffset]::MinValue
        $predecessorCompleted = [DateTimeOffset]::MinValue
        if ($null -eq $predecessor.quant_elapsed_seconds -or
            -not [double]::IsFinite(
                [double]$predecessor.quant_elapsed_seconds
            ) -or
            -not [DateTimeOffset]::TryParse(
                [string]$predecessor.started_at_utc,
                [ref]$predecessorStarted
            ) -or
            -not [DateTimeOffset]::TryParse(
                [string]$predecessor.completed_at_utc,
                [ref]$predecessorCompleted
            )) {
            throw 'predecessor lacks finite elapsed-time evidence'
        }
        $script:inheritedQuantSeconds = [Math]::Max(
            [double]$predecessor.quant_elapsed_seconds,
            ($predecessorCompleted - $predecessorStarted).TotalSeconds
        )
        $predecessorTimeout = Get-OperationTimeoutMilliseconds `
            -MaximumMilliseconds 300000 `
            -Operation 'predecessor semantic verification'
        $manifestCheck = Test-HashManifest -Root $predecessorDir `
            -ManifestPath $predecessorManifest `
            -RejectUnlistedFiles
        $predecessorVerificationProcess = Invoke-PowerShellFileBounded `
            -ScriptPath $validatorPath `
            -Arguments @('-AttemptPath', $predecessorDir) `
            -TimeoutMilliseconds $predecessorTimeout
        $predecessorVerificationCode =
            $predecessorVerificationProcess.exit_code
        $predecessorVerification =
            $predecessorVerificationProcess.stdout | ConvertFrom-Json
        if (-not $manifestCheck.passed -or
            $predecessorVerificationCode -ne 0 -or
            -not $predecessorVerification.passed -or
            $predecessor.schema -ne 'animus-ferric-runtime-attempt-v2' -or
            [int]$predecessor.control_epoch -ne 2 -or
            $predecessor.coordinate -ne $coordinatePlan.predecessor -or
            $predecessor.quant -ne $coordinatePlan.quant -or
            $predecessor.failure_classification -ne 'startup_memory_pressure' -or
            $predecessor.startup.healthy -or
            -not $predecessor.teardown.passed -or
            $predecessor.wall_cap_breached -or
            $null -eq $predecessor.quant_elapsed_seconds -or
            [double]$predecessor.quant_elapsed_seconds -le 0 -or
            [double]$predecessor.quant_elapsed_seconds -ge
                [double]$plan.quant_wall_cap_seconds) {
            throw '16384 retry is not authorized by a verified startup-memory failure'
        }
        $script:inheritedQuantSeconds =
            [double]$predecessor.quant_elapsed_seconds
    }

    if ($coordinatePlan.quant -eq 'Q3_K_XL') {
        $gateTimeout = Get-OperationTimeoutMilliseconds `
            -MaximumMilliseconds 300000 `
            -Operation 'Q4 fallback-gate verification'
        $gateVerificationProcess = Invoke-PowerShellFileBounded `
            -ScriptPath $gateVerifierPath `
            -TimeoutMilliseconds $gateTimeout
        $gateVerificationCode = $gateVerificationProcess.exit_code
        $gateVerification = $gateVerificationProcess.stdout | ConvertFrom-Json
        if ($gateVerificationCode -ne 0 -or
            $gateVerification.schema -cne
                'animus-ferric-q4-gate-verification-v2' -or
            $gateVerification.task -cne 'T-11409' -or
            [int]$gateVerification.control_epoch -ne 2 -or
            $gateVerification.attestation_protocol -cne
                [string]$plan.template_attestation.protocol -or
            -not $gateVerification.passed -or
            $gateVerification.derivation.q4_verdict -ne 'non_viable' -or
            -not $gateVerification.derivation.q3_fallback_authorized) {
            throw 'Q3 gate failed fresh Q4 attempt-chain verification'
        }
    }
}

$controlManifest = $null
$preflight = $null
$launchProcess = $null
$startup = [ordered]@{
    healthy = $false
    classification = 'not_started'
    memory_match = [ordered]@{ matched = $false; matches = @() }
}
$attestation = [ordered]@{ passed = $false; reason = 'not_run' }
$smoke = [ordered]@{ passed = $false; reason = 'not_run' }
$throughput = [ordered]@{
    passed = $false
    reason = 'not_run'
    request_sha256 = $null
    scheduled_samples = @($plan.throughput.sequence)
    observed_samples = 0
    valid_request_count = 0
    valid_trial_count = 0
    median_decoded_tokens_per_second = $null
}
$teardown = [ordered]@{ passed = $false; reason = 'not_run' }
$serverStarted = $false
$savedServerPid = $null
$fatalError = $null
$wallCapBreached = $false
$servedModelId = $null
$liveWrapperProcessRecords = [System.Collections.Generic.List[object]]::new()

$controlManifest = Assert-ControlInputs
Assert-CoordinateAuthorization
$inheritedRuntimeEnvironment = @(Assert-EnvironmentClean)
$preAttemptMemory = Assert-ColdCapacity
if (Test-Path -LiteralPath $rawDir) {
    throw "raw attempt already exists and will not be overwritten: $rawDir"
}
if (Test-Path -LiteralPath $archiveDir) {
    throw "archived attempt already exists and will not be overwritten: $archiveDir"
}
[System.IO.Directory]::CreateDirectory($rawDir) | Out-Null

try {
    if (-not (Test-Path -LiteralPath $modelPath -PathType Leaf)) {
        throw "declared model is absent: $($modelSpec.relative_path)"
    }
    [void](Get-OperationTimeoutMilliseconds -MaximumMilliseconds 300000 `
        -RequiredTailMilliseconds 300000 -Operation 'GGUF identity hashing')
    $modelItem = Get-Item -LiteralPath $modelPath
    $modelHash = Get-Sha256Lower -Path $modelPath
    if ([UInt64]$modelItem.Length -ne [UInt64]$modelSpec.bytes -or
        $modelHash -ne $modelSpec.sha256) {
        throw 'declared model identity mismatch'
    }
    $deviceTimeout = Get-OperationTimeoutMilliseconds `
        -MaximumMilliseconds 30000 -Operation 'CUDA device verification'
    $deviceOutput = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
        -Arguments @('--list-devices') -TimeoutMilliseconds $deviceTimeout)
    $deviceObservation = Get-LlamaDeviceObservation -Output $deviceOutput
    if (-not (Test-JsonEquivalent -Left $deviceObservation.identity `
            -Right $plan.llama_cpp.expected_device) -or
        -not (Test-JsonEquivalent -Left $deviceObservation.identity `
            -Right $controlManifest.binaries.llama_server.device_identity) -or
        [UInt64]$deviceObservation.free_mib -lt
            [UInt64]$plan.minimum_gpu_free_mib_before_launch) {
        throw 'the frozen CUDA runtime no longer exposes the declared cold GPU'
    }

    $identityTimeout = Get-OperationTimeoutMilliseconds `
        -MaximumMilliseconds 30000 -Operation 'binary version capture'
    $repositoryCommit = @(Invoke-BoundedTextProcess -FilePath 'git' `
        -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD') `
        -TimeoutMilliseconds $identityTimeout)[0]
    $repositoryStatus = @(Invoke-BoundedTextProcess -FilePath 'git' `
        -Arguments @('-C', $repoRoot, 'status', '--short', '--branch') `
        -TimeoutMilliseconds $identityTimeout)
    if (([string]$repositoryCommit).Trim() -cne
        [string]$controlManifest.repository.head_at_freeze) {
        throw 'repository HEAD changed after the epoch-2 runtime freeze'
    }
    $ferricVersion = @(Invoke-BoundedTextProcess -FilePath $ferricPath `
        -Arguments @('--version') -TimeoutMilliseconds $identityTimeout)
    $llamaVersion = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
        -Arguments @('--version') -TimeoutMilliseconds $identityTimeout)

    $preflight = [ordered]@{
        schema = 'animus-ferric-runtime-preflight-v2'
        task = 'T-11409'
        control_epoch = 2
        attestation_protocol = [string]$plan.template_attestation.protocol
        coordinate = $Coordinate
        captured_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        repository_commit = ([string]$repositoryCommit).Trim()
        repository_status = $repositoryStatus
        repository_status_semantics =
            'descriptive_snapshot_not_a_cleanliness_claim'
        runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
        control_inputs_sha256 = Get-Sha256Lower -Path $controlManifestPath
        model = [ordered]@{
            display_path = $modelSpec.relative_path
            bytes = [UInt64]$modelItem.Length
            sha256 = $modelHash
        }
        ferric = [ordered]@{
            path = $ferricPath
            bytes = [UInt64](Get-Item -LiteralPath $ferricPath).Length
            sha256 = Get-Sha256Lower -Path $ferricPath
            version = $ferricVersion
        }
        llama_server = [ordered]@{
            path = $llamaPath
            bytes = [UInt64](Get-Item -LiteralPath $llamaPath).Length
            sha256 = Get-Sha256Lower -Path $llamaPath
            version = $llamaVersion
            devices = $deviceOutput
            device_identity = $deviceObservation.identity
            device_free_mib = [UInt64]$deviceObservation.free_mib
            help_output_sha256 = $frozenLlamaHelpSha256
        }
        inherited_runtime_environment = $inheritedRuntimeEnvironment
        local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        global_runfile_absent = -not (Test-Path -LiteralPath $globalRunfile)
        listener_absent = $true
        any_llama_server_process_absent = $true
        minimum_gpu_free_mib = [UInt64]$plan.minimum_gpu_free_mib_before_launch
        memory = $preAttemptMemory
    }
    Write-JsonLf -Path (Join-Path $rawDir 'preflight.json') -Value $preflight
    Add-JournalEntry -Kind 'observation' -Name 'preflight' -Details $preflight

    $serverLog = Join-Path $rawDir 'server.log'
    $launchStdout = Join-Path $rawDir 'launch.stdout.log'
    $launchStderr = Join-Path $rawDir 'launch.stderr.log'
    $parentPath = [string]$env:Path
    $launchEnvironment = @{
        Path = "$llamaBin;$parentPath"
        LLAMA_ARG_CACHE_TYPE_K = [string]$plan.server.environment.LLAMA_ARG_CACHE_TYPE_K
        LLAMA_ARG_CACHE_TYPE_V = [string]$plan.server.environment.LLAMA_ARG_CACHE_TYPE_V
        LLAMA_ARG_FLASH_ATTN = [string]$plan.server.environment.LLAMA_ARG_FLASH_ATTN
        LLAMA_ARG_FIT = [string]$plan.server.environment.LLAMA_ARG_FIT
        LLAMA_ARG_FIT_TARGET = [string]$plan.server.environment.LLAMA_ARG_FIT_TARGET
        LLAMA_ARG_REASONING = [string]$plan.server.environment.LLAMA_ARG_REASONING
        LLAMA_ARG_THINK_BUDGET = [string]$plan.server.environment.LLAMA_ARG_THINK_BUDGET
        LLAMA_ARG_REASONING_PRESERVE = [string]$plan.server.environment.LLAMA_ARG_REASONING_PRESERVE
        LLAMA_ARG_CHAT_TEMPLATE_KWARGS = [string]$plan.server.environment.LLAMA_ARG_CHAT_TEMPLATE_KWARGS
        LLAMA_ARG_TIMEOUT = [string]$plan.server.environment.LLAMA_ARG_TIMEOUT
        LLAMA_ARG_LOG_COLORS = [string]$plan.server.logging_environment.LLAMA_ARG_LOG_COLORS
        LLAMA_ARG_LOG_TIMESTAMPS = [string]$plan.server.logging_environment.LLAMA_ARG_LOG_TIMESTAMPS
        LLAMA_ARG_LOG_VERBOSITY = [string]$plan.server.logging_environment.LLAMA_ARG_LOG_VERBOSITY
        LLAMA_ARG_LOG_FILE = $serverLog
    }
    $launchArguments = @(
        'server', 'up',
        '--engine', 'llama-server',
        '--model', $modelPath,
        '--ctx', [string]$coordinatePlan.context,
        '--threads', [string]$plan.server.threads,
        '--gpu-layers', [string]$modelSpec.requested_gpu_layers,
        '--batch-size', [string]$plan.server.batch_size,
        '--seed', [string]$plan.server.seed,
        '--parallel', [string]$plan.server.parallel_slots,
        '--port', [string]$plan.port
    )
    $launchDeclaration = [ordered]@{
        schema = 'animus-ferric-runtime-launch-v2'
        control_epoch = 2
        attestation_protocol = [string]$plan.template_attestation.protocol
        coordinate = $Coordinate
        executable = $ferricPath
        arguments = $launchArguments
        working_directory = $repoRoot
        child_path_prepend = $llamaBin
        declared_parent_environment = [ordered]@{
            Path = $parentPath
        }
        environment = $launchEnvironment
        expected_llama_argv = @(
            'llama-server', '-m', $modelPath, '-c', [string]$coordinatePlan.context,
            '-t', [string]$plan.server.threads, '-ngl',
            [string]$modelSpec.requested_gpu_layers, '-b',
            [string]$plan.server.batch_size, '--seed', [string]$plan.server.seed,
            '--parallel', [string]$plan.server.parallel_slots,
            '--host', '127.0.0.1', '--port', [string]$plan.port
        )
    }
    Write-JsonLf -Path (Join-Path $rawDir 'launch-command.json') `
        -Value $launchDeclaration
    Add-JournalEntry -Kind 'command' -Name 'ferric server up' `
        -Details $launchDeclaration
    Write-JsonLf -Path (Join-Path $rawDir 'memory-before-launch.json') `
        -Value $preAttemptMemory

    $launchTimeout = [Math]::Min(
        [int]$plan.startup_wait_seconds * 1000,
        (Get-RemainingMilliseconds)
    )
    if ($launchTimeout -le 0) {
        throw 'quant wall cap expired before launch'
    }
    $launchProcess = Invoke-FileRedirectedProcess -FilePath $ferricPath `
        -Arguments $launchArguments -WorkingDirectory $repoRoot `
        -StdoutPath $launchStdout -StderrPath $launchStderr `
        -TimeoutMilliseconds $launchTimeout -Environment $launchEnvironment
    Register-LiveWrapperProcess -Record $launchProcess `
        -Source 'ferric server up'
    Write-JsonLf -Path (Join-Path $rawDir 'launch-process.json') `
        -Value $launchProcess
    Add-JournalEntry -Kind 'result' -Name 'ferric server up' `
        -Details $launchProcess
    Start-Sleep -Milliseconds 750

    $launchText = ''
    foreach ($path in @($launchStdout, $launchStderr, $serverLog)) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $launchText += "`n" + (Get-Content -Raw -LiteralPath $path)
        }
    }
    $startupClassificationPath = Join-Path $rawDir 'startup-classification.log'
    Write-Utf8Lf -Path $startupClassificationPath -Text $launchText
    $memoryMatch = Test-StartupMemoryPressure -Text $launchText `
        -Patterns @($plan.startup_memory_patterns)
    $startupHealthy =
        (-not $launchProcess.timed_out) -and
        ($launchProcess.exit_code -eq 0) -and
        (Test-Path -LiteralPath $localRunfile -PathType Leaf) -and
        (Test-Path -LiteralPath $globalRunfile -PathType Leaf)
    $startupClass = if ($startupHealthy) {
        'healthy'
    }
    elseif ($memoryMatch.matched) {
        'startup_memory_pressure'
    }
    elseif ($launchProcess.timed_out) {
        'startup_timeout'
    }
    else {
        'startup_other_failure'
    }
    $startup = [ordered]@{
        healthy = $startupHealthy
        classification = $startupClass
        memory_match = $memoryMatch
        classification_input_file = 'startup-classification.log'
        classification_input_bytes = [UInt64](
            Get-Item -LiteralPath $startupClassificationPath
        ).Length
        classification_input_sha256 = Get-Sha256Lower `
            -Path $startupClassificationPath
        launch_process = $launchProcess
    }
    Write-JsonLf -Path (Join-Path $rawDir 'startup.json') -Value $startup

    if ($startupHealthy) {
        $serverStarted = $true
        Copy-Item -LiteralPath $localRunfile `
            -Destination (Join-Path $rawDir 'runfile.local.json')
        Copy-Item -LiteralPath $globalRunfile `
            -Destination (Join-Path $rawDir 'runfile.global.json')
        $runfile = Get-Content -Raw -LiteralPath $localRunfile | ConvertFrom-Json
        $savedServerPid = [UInt32]$runfile.pid
        $runfileHashesEqual =
            (Get-Sha256Lower -Path $localRunfile) -eq
            (Get-Sha256Lower -Path $globalRunfile)
        $process = Get-CimInstance Win32_Process `
            -Filter "ProcessId = $savedServerPid" -OperationTimeoutSec 5
        $listener = @(Get-NetTCPConnection -State Listen -LocalPort $plan.port `
            -ErrorAction SilentlyContinue)

        if ((Get-RemainingMilliseconds) -lt 30000) {
            throw 'quant wall cap cannot accommodate GET /health'
        }
        $health = Invoke-HttpExchange -Method GET `
            -Uri "http://127.0.0.1:$($plan.port)/health" `
            -ResponsePath (Join-Path $rawDir 'health.body') `
            -TimeoutSeconds 30
        Add-JournalEntry -Kind 'http' -Name 'GET /health' -Details $health
        if ((Get-RemainingMilliseconds) -lt 30000) {
            throw 'quant wall cap cannot accommodate GET /v1/models'
        }
        $models = Invoke-HttpExchange -Method GET `
            -Uri "http://127.0.0.1:$($plan.port)/v1/models" `
            -ResponsePath (Join-Path $rawDir 'models.body.json') `
            -TimeoutSeconds 30
        Add-JournalEntry -Kind 'http' -Name 'GET /v1/models' -Details $models
        if ((Get-RemainingMilliseconds) -lt 30000) {
            throw 'quant wall cap cannot accommodate GET /props'
        }
        $props = Invoke-HttpExchange -Method GET `
            -Uri "http://127.0.0.1:$($plan.port)/props" `
            -ResponsePath (Join-Path $rawDir 'props.body.json') `
            -TimeoutSeconds 30
        Add-JournalEntry -Kind 'http' -Name 'GET /props' -Details $props

        $templateProbeExchanges = [System.Collections.Generic.List[object]]::new()
        foreach ($arm in @($plan.template_attestation.arms)) {
            if ((Get-RemainingMilliseconds) -lt 30000) {
                throw "quant wall cap cannot accommodate template probe $($arm.name)"
            }
            $retainedRequest = Join-Path $rawDir `
                "template-probe.$($arm.name).request.json"
            $retainedResponse = Join-Path $rawDir `
                "template-probe.$($arm.name).response.json"
            Copy-Item -LiteralPath (Join-Path $artifactDir $arm.request_file) `
                -Destination $retainedRequest
            $exchange = Invoke-HttpExchange -Method POST `
                -Uri "http://127.0.0.1:$($plan.port)$($plan.template_attestation.endpoint)" `
                -RequestBodyPath $retainedRequest `
                -ResponsePath $retainedResponse -TimeoutSeconds 30
            $templateProbeExchanges.Add([ordered]@{
                name = [string]$arm.name
                exchange = $exchange
            })
            Add-JournalEntry -Kind 'http' `
                -Name "POST $($plan.template_attestation.endpoint) [$($arm.name)]" `
                -Details $exchange
        }

        $modelsJson = $null
        $propsJson = $null
        try {
            $modelsJson = [System.Text.Encoding]::UTF8.GetString(
                [System.IO.File]::ReadAllBytes((Join-Path $rawDir 'models.body.json'))
            ) | ConvertFrom-Json
        }
        catch {
            $modelsJson = $null
        }
        try {
            $propsJson = [System.Text.Encoding]::UTF8.GetString(
                [System.IO.File]::ReadAllBytes((Join-Path $rawDir 'props.body.json'))
            ) | ConvertFrom-Json
        }
        catch {
            $propsJson = $null
        }
        $modelEntries = @(
            if ($null -ne $modelsJson) {
                Get-OptionalProperty -Value $modelsJson -Name 'data'
            }
        )
        if ($modelEntries.Count -eq 1 -and $null -ne $modelEntries[0]) {
            $servedModelId = [string](Get-OptionalProperty `
                -Value $modelEntries[0] -Name 'id')
        }
        $modelMeta = if ($modelEntries.Count -eq 1) {
            Get-OptionalProperty -Value $modelEntries[0] -Name 'meta'
        }
        else {
            $null
        }
        $defaultGenerationSettings = Get-OptionalProperty `
            -Value $propsJson -Name 'default_generation_settings'
        $effectiveContext = Get-OptionalProperty `
            -Value $defaultGenerationSettings -Name 'n_ctx'
        $servedParams = Get-OptionalProperty -Value $modelMeta -Name 'n_params'
        if ($null -eq $servedParams -and $modelEntries.Count -eq 1) {
            $servedParams = Get-OptionalProperty -Value $modelEntries[0] `
                -Name 'n_params'
        }

        $serverText = if (Test-Path -LiteralPath $serverLog -PathType Leaf) {
            Get-Content -Raw -LiteralPath $serverLog
        }
        else {
            $launchText
        }
        $offloadMatches = [regex]::Matches(
            $serverText,
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
        $kvCacheMatches = [regex]::Matches(
            $serverText,
            'K\s*\(q8_0\)\s*:[^\r\n]*V\s*\(q8_0\)\s*:',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        $flashEnabledMatches = [regex]::Matches(
            $serverText,
            '(?:flash_attn\s*=\s*(?:1|on|enabled)\b|flash attention is enabled)',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        $thinkingEnabledMatches = [regex]::Matches(
            $serverText,
            'chat template,\s*thinking\s*=\s*1\b',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        $chatTemplateCaps = Get-OptionalProperty `
            -Value $propsJson -Name 'chat_template_caps'
        $supportsPreserveReasoning = Get-OptionalProperty `
            -Value $chatTemplateCaps -Name 'supports_preserve_reasoning'
        $templateProbeFacts = Get-TemplateProbeFacts -Plan $plan `
            -ArtifactDirectory $artifactDir -EvidenceDirectory $rawDir
        $templateProbeHttpPassed =
            @($templateProbeExchanges | Where-Object {
                $_.exchange.status_code -ne 200 -or
                -not [string]::IsNullOrWhiteSpace([string]$_.exchange.error)
            }).Count -eq 0
        $preserveDisabledWarnings = [regex]::Matches(
            $serverText,
            'supports preserving reasoning,\s*consider enabling|does not support[^\r\n]*reasoning[- ]preserve',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        $totalSlots = Get-OptionalProperty -Value $propsJson -Name 'total_slots'
        $startupLogPath = Join-Path $rawDir 'startup.log'
        Write-Utf8Lf -Path $startupLogPath -Text $serverText
        $processHash = if ($null -ne $process -and
            -not [string]::IsNullOrWhiteSpace([string]$process.ExecutablePath) -and
            (Test-Path -LiteralPath $process.ExecutablePath -PathType Leaf)) {
            Get-Sha256Lower -Path $process.ExecutablePath
        }
        else {
            $null
        }
        $runtimeIdentity = Test-FileIdentityManifest -Root $llamaBin `
            -Expected @($controlManifest.binaries.llama_runtime.files)
        $listenerOwners = @($listener | Select-Object -ExpandProperty OwningProcess -Unique)
        $processArgv = if ($null -ne $process) {
            Split-SimpleWindowsCommandLine -CommandLine ([string]$process.CommandLine)
        }
        else {
            @()
        }
        $expectedProcessArgv = if ($null -ne $process) {
            @([string]$process.ExecutablePath) +
                @($launchDeclaration.expected_llama_argv | Select-Object -Skip 1)
        }
        else {
            @()
        }
        $listenerRecordsPassed =
            $listener.Count -ge 1 -and
            @($listener | Where-Object {
                [int]$_.LocalPort -ne [int]$plan.port -or
                [UInt32]$_.OwningProcess -ne $savedServerPid -or
                [string]$_.State -notin @('Listen', '2') -or
                [string]$_.LocalAddress -ne '127.0.0.1'
            }).Count -eq 0
        $servedFilename = if ([string]::IsNullOrWhiteSpace($servedModelId)) {
            $null
        }
        else {
            [System.IO.Path]::GetFileName($servedModelId)
        }
        $coreAttestationPassed =
            $runfileHashesEqual -and
            ([string]$runfile.engine -eq 'llama-server') -and
            ([int]$runfile.port -eq [int]$plan.port) -and
            ([string]$runfile.base_url -eq
                "http://127.0.0.1:$($plan.port)/v1") -and
            ([bool]$runfile.tailscale -eq $false) -and
            ($runfile.model -eq $modelPath) -and
            ([int]$runfile.context_size -eq [int]$coordinatePlan.context) -and
            ([int]$runfile.sampling_seed -eq [int]$plan.server.seed) -and
            ([int]$runfile.parallel_slots -eq [int]$plan.server.parallel_slots) -and
            ($null -ne $process) -and
            [System.IO.Path]::GetFullPath(
                [string]$process.ExecutablePath
            ).Equals(
                [System.IO.Path]::GetFullPath($llamaPath),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            ($processHash -eq $plan.llama_cpp.expected_server_sha256) -and
            (($processArgv -join "`n") -eq
                ($expectedProcessArgv -join "`n")) -and
            $runtimeIdentity.passed -and
            ($listenerOwners.Count -eq 1) -and
            ([UInt32]$listenerOwners[0] -eq $savedServerPid) -and
            $listenerRecordsPassed -and
            ($health.status_code -eq 200) -and
            ($models.status_code -eq 200) -and
            ($props.status_code -eq 200) -and
            ([int]$totalSlots -eq [int]$plan.server.parallel_slots) -and
            ($modelEntries.Count -eq 1) -and
            ($servedFilename -eq $modelSpec.file) -and
            ([int64]$effectiveContext -eq [int64]$coordinatePlan.context) -and
            ($null -ne $effectiveGpuLayers) -and
            ([int]$effectiveGpuLayers -gt 0) -and
            ($kvCacheMatches.Count -ge 1) -and
            ($flashEnabledMatches.Count -ge 1) -and
            ($thinkingEnabledMatches.Count -ge 1) -and
            ($supportsPreserveReasoning -eq $true) -and
            $templateProbeHttpPassed -and
            $templateProbeFacts.passed -and
            ($preserveDisabledWarnings.Count -eq 0)
        $attestation = [ordered]@{
            schema = 'animus-ferric-managed-server-attestation-v2'
            control_epoch = 2
            attestation_protocol = [string]$plan.template_attestation.protocol
            passed = $coreAttestationPassed
            coordinate = $Coordinate
            captured_at_utc = (Get-Date).ToUniversalTime().ToString('o')
            runfiles = [ordered]@{
                local_path = $localRunfile
                global_path = $globalRunfile
                byte_identical = $runfileHashesEqual
                value = $runfile
            }
            process = if ($null -ne $process) {
                [ordered]@{
                    pid = [UInt32]$process.ProcessId
                    executable_path = [string]$process.ExecutablePath
                    executable_sha256 = $processHash
                    command_line = [string]$process.CommandLine
                    creation_date = [string]$process.CreationDate
                }
            }
            else {
                $null
            }
            listener = [ordered]@{
                owners = $listenerOwners
                records = @($listener | Select-Object LocalAddress, LocalPort,
                    State, OwningProcess)
            }
            endpoints = [ordered]@{
                health = $health
                models = $models
                props = $props
                served_model_id = $servedModelId
                served_n_ctx = $effectiveContext
                served_n_ctx_source = 'props.default_generation_settings.n_ctx'
                served_n_params = $servedParams
                total_slots = $totalSlots
                chat_template_caps = $chatTemplateCaps
                template_probe_exchanges = @($templateProbeExchanges)
            }
            requested = [ordered]@{
                context = [int]$coordinatePlan.context
                gpu_layers = [int]$modelSpec.requested_gpu_layers
                cache_type_k = [string]$plan.server.environment.LLAMA_ARG_CACHE_TYPE_K
                cache_type_v = [string]$plan.server.environment.LLAMA_ARG_CACHE_TYPE_V
                flash_attention = [string]$plan.server.environment.LLAMA_ARG_FLASH_ATTN
                fit = [string]$plan.server.environment.LLAMA_ARG_FIT
                fit_target_mib = [int]$plan.server.environment.LLAMA_ARG_FIT_TARGET
                reasoning = [string]$plan.server.environment.LLAMA_ARG_REASONING
                reasoning_budget = [int]$plan.server.environment.LLAMA_ARG_THINK_BUDGET
                reasoning_preserve = [string]$plan.server.environment.LLAMA_ARG_REASONING_PRESERVE
                timeout_seconds = [int]$plan.server.environment.LLAMA_ARG_TIMEOUT
                threads = [int]$plan.server.threads
                batch_size = [int]$plan.server.batch_size
                parallel_slots = [int]$plan.server.parallel_slots
                seed = [int]$plan.server.seed
            }
            effective = [ordered]@{
                context = $effectiveContext
                gpu_layers = $effectiveGpuLayers
                total_layers_reported = $totalLayers
                cache_type_k = if ($kvCacheMatches.Count -ge 1) { 'q8_0' } else { $null }
                cache_type_v = if ($kvCacheMatches.Count -ge 1) { 'q8_0' } else { $null }
                kv_cache_attestation_lines = @($kvCacheMatches | ForEach-Object { $_.Value })
                flash_attention = if ($flashEnabledMatches.Count -ge 1) {
                    'enabled'
                }
                else {
                    $null
                }
                flash_attention_attestation_lines = @(
                    $flashEnabledMatches | ForEach-Object { $_.Value }
                )
                reasoning_enabled = ($thinkingEnabledMatches.Count -ge 1)
                reasoning_attestation_lines = @(
                    $thinkingEnabledMatches | ForEach-Object { $_.Value }
                )
                preserve_reasoning_supported = $supportsPreserveReasoning
                preserve_reasoning_enabled =
                    $templateProbeFacts.differential.preserve_thinking_default_effective
                preserve_reasoning_evidence_source =
                    'apply-template-differential-v1'
                thinking_generation_prefix_enabled =
                    $templateProbeFacts.differential.thinking_generation_prefix_effective
                template_attestation = $templateProbeFacts
                reasoning_budget = [int]$plan.server.environment.LLAMA_ARG_THINK_BUDGET
                reasoning_budget_evidence_source =
                    'frozen_llama_help_env_mapping_and_launch_environment'
                request_timeout_seconds =
                    [int]$plan.server.environment.LLAMA_ARG_TIMEOUT
                request_timeout_evidence_source =
                    'frozen_llama_help_env_mapping_and_launch_environment'
                llama_help_sha256 = $frozenLlamaHelpSha256
                preserve_disabled_warning_count = $preserveDisabledWarnings.Count
                llama_runtime_identity = $runtimeIdentity
                startup_log_sha256 = Get-Sha256Lower -Path $startupLogPath
            }
            memory_after_load = Get-MemorySnapshot
        }
        Write-JsonLf -Path (Join-Path $rawDir 'attestation.json') `
            -Value $attestation

        if ($attestation.passed) {
            $smokeWorkspace = Join-Path $rawDir 'smoke-workspace'
            $profileDir = Join-Path $rawDir 'empty-profile'
            [System.IO.Directory]::CreateDirectory($smokeWorkspace) | Out-Null
            [System.IO.Directory]::CreateDirectory($profileDir) | Out-Null
            Copy-Item -LiteralPath (Join-Path $artifactDir $plan.smoke.nonce_file) `
                -Destination (Join-Path $smokeWorkspace 'nonce.txt')
            $beforeManifest = Get-TreeManifest -Root $smokeWorkspace `
                -ExcludedPrefixes @('.ferric')
            Write-JsonLf -Path (Join-Path $rawDir 'smoke-workspace.before.json') `
                -Value $beforeManifest
            $smokePrompt = (Get-Content -Raw -LiteralPath (
                Join-Path $artifactDir $plan.smoke.prompt_file
            )).TrimEnd("`r", "`n")
            $smokeStdout = Join-Path $rawDir 'smoke.stdout.log'
            $smokeStderr = Join-Path $rawDir 'smoke.stderr.log'
            $smokeArguments = @(
                'query',
                '--workspace', $smokeWorkspace,
                '--model', $servedModelId,
                '--api-base', "http://127.0.0.1:$($plan.port)/v1",
                '--params-b', '27',
                '--quant', [string]$coordinatePlan.quant,
                '--family', 'qwen3.8',
                '--ctx', [string]$coordinatePlan.context,
                '--temperature', [string]$plan.smoke.temperature,
                '--protocol', [string]$plan.smoke.protocol,
                '--harness-policy', [string]$plan.smoke.harness_policy,
                '--tier', [string]$plan.smoke.tier,
                '--max-ring', [string]$plan.smoke.max_ring,
                '--max-turns', [string]$plan.smoke.max_turns,
                '--profile-dir', $profileDir,
                '--no-config',
                '--no-stream',
                $smokePrompt
            )
            $smokeDeclaration = [ordered]@{
                executable = $ferricPath
                arguments = $smokeArguments
                working_directory = $repoRoot
                prompt_sha256 = Get-Sha256Lower -Path (
                    Join-Path $artifactDir $plan.smoke.prompt_file
                )
                nonce_sha256 = Get-Sha256Lower -Path (
                    Join-Path $artifactDir $plan.smoke.nonce_file
                )
            }
            Write-JsonLf -Path (Join-Path $rawDir 'smoke-command.json') `
                -Value $smokeDeclaration
            Add-JournalEntry -Kind 'command' -Name 'ferric query nonce smoke' `
                -Details $smokeDeclaration
            $smokeTimeout = Get-RemainingMilliseconds
            if ($smokeTimeout -le 0) {
                throw 'quant wall cap expired before smoke'
            }
            $smokeProcess = Invoke-CapturedProcess -FilePath $ferricPath `
                -Arguments $smokeArguments -WorkingDirectory $repoRoot `
                -StdoutPath $smokeStdout -StderrPath $smokeStderr `
                -TimeoutMilliseconds $smokeTimeout
            Register-LiveWrapperProcess -Record $smokeProcess `
                -Source 'ferric query nonce smoke'
            Add-JournalEntry -Kind 'result' -Name 'ferric query nonce smoke' `
                -Details $smokeProcess

            $traceFiles = @(
                Get-ChildItem -LiteralPath (Join-Path $smokeWorkspace '.ferric/trace') `
                    -File -Filter '*.jsonl' -ErrorAction SilentlyContinue
            )
            $traceFacts = $null
            $traceParseError = $null
            $traceVerifyProcess = $null
            $traceVerifyNotRunReason = $null
            if ($traceFiles.Count -eq 1) {
                $retainedTrace = Join-Path $rawDir 'smoke.trace.jsonl'
                Copy-Item -LiteralPath $traceFiles[0].FullName `
                    -Destination $retainedTrace
                $verifyStdout = Join-Path $rawDir 'smoke-trace-verify.stdout.log'
                $verifyStderr = Join-Path $rawDir 'smoke-trace-verify.stderr.log'
                $verifyTimeout = [Math]::Min(60000, (Get-RemainingMilliseconds))
                if ($verifyTimeout -gt 0) {
                    $traceVerifyProcess = Invoke-CapturedProcess `
                        -FilePath $ferricPath `
                        -Arguments @('trace', 'verify', $retainedTrace) `
                        -WorkingDirectory $repoRoot -StdoutPath $verifyStdout `
                        -StderrPath $verifyStderr `
                        -TimeoutMilliseconds $verifyTimeout
                    Register-LiveWrapperProcess -Record $traceVerifyProcess `
                        -Source 'ferric trace verify'
                    Add-JournalEntry -Kind 'command_result' `
                        -Name 'ferric trace verify' -Details $traceVerifyProcess
                }
                else {
                    $traceVerifyNotRunReason =
                        'quant_wall_cap_expired_before_trace_verify'
                    Write-Utf8Lf -Path $verifyStdout -Text ''
                    Write-Utf8Lf -Path $verifyStderr -Text ''
                }
                try {
                    $traceFacts = Get-TraceFacts -TracePath $retainedTrace `
                        -ExpectedNonce $plan.smoke.require_exact_summary `
                        -ForbiddenTools @($plan.smoke.forbidden_tools)
                }
                catch {
                    $traceParseError = $_.Exception.Message
                }
            }
            else {
                $traceVerifyNotRunReason = 'trace_count_not_one'
            }
            $afterManifest = Get-TreeManifest -Root $smokeWorkspace `
                -ExcludedPrefixes @('.ferric')
            Write-JsonLf -Path (Join-Path $rawDir 'smoke-workspace.after.json') `
                -Value $afterManifest
            $workspaceUnchanged = Test-ManifestEqual -Before $beforeManifest `
                -After $afterManifest
            $smokePassed =
                (-not $smokeProcess.timed_out) -and
                ($smokeProcess.exit_code -eq 0) -and
                ($traceFiles.Count -eq 1) -and
                ($null -ne $traceVerifyProcess) -and
                (-not $traceVerifyProcess.timed_out) -and
                ($traceVerifyProcess.exit_code -eq 0) -and
                ($null -ne $traceFacts) -and
                ($traceFacts.protocol -eq 'constrained_json') -and
                $traceFacts.all_turns_json_schema_constrained -and
                $traceFacts.read_file_before_task_complete -and
                ($traceFacts.exact_nonce_read_result_count -ge 1) -and
                $traceFacts.exact_task_complete_summary -and
                ($traceFacts.forbidden_tools_observed.Count -eq 0) -and
                ($traceFacts.session_end_reason -eq 'task_complete') -and
                $workspaceUnchanged
            $smoke = [ordered]@{
                schema = 'animus-ferric-qwen38-smoke-v1'
                passed = $smokePassed
                process = $smokeProcess
                trace_count = $traceFiles.Count
                trace_sha256 = if ($traceFiles.Count -eq 1) {
                    Get-Sha256Lower -Path (Join-Path $rawDir 'smoke.trace.jsonl')
                }
                else {
                    $null
                }
                trace_verify = $traceVerifyProcess
                trace_verify_not_run_reason = $traceVerifyNotRunReason
                trace_facts = $traceFacts
                trace_parse_error = $traceParseError
                workspace_unchanged = $workspaceUnchanged
                before_manifest = $beforeManifest
                after_manifest = $afterManifest
            }
            Write-JsonLf -Path (Join-Path $rawDir 'smoke.json') -Value $smoke

            if ($smoke.passed) {
                $templatePath = Join-Path $artifactDir `
                    $plan.throughput.request_template
                $templateText = Get-Content -Raw -LiteralPath $templatePath
                $escapedModelId = $servedModelId | ConvertTo-Json -Compress
                $requestText = $templateText.Replace(
                    '"__SERVED_MODEL_ID__"',
                    $escapedModelId
                )
                if ($requestText -eq $templateText -or
                    $requestText.Contains('__SERVED_MODEL_ID__')) {
                    throw 'throughput model placeholder substitution failed'
                }
                $requestPath = Join-Path $rawDir 'throughput-request.json'
                Write-Utf8Lf -Path $requestPath -Text $requestText
                $requestHash = Get-Sha256Lower -Path $requestPath
                $requestObject = Get-Content -Raw -LiteralPath $requestPath |
                    ConvertFrom-Json
                if ($requestObject.max_tokens -ne 256 -or
                    $requestObject.temperature -ne 1.0 -or
                    $requestObject.seed -ne 42 -or
                    $requestObject.stream -ne $false) {
                    throw 'frozen throughput request fields drifted'
                }

                $throughputPath = Join-Path $rawDir 'throughput.jsonl'
                $sampleRows = [System.Collections.Generic.List[object]]::new()
                $ordinal = 0
                foreach ($label in @($plan.throughput.sequence)) {
                    $ordinal++
                    $responseName = "throughput-$label.response.json"
                    $responsePath = Join-Path $rawDir $responseName
                    $quantElapsedBeforeRequest = $inheritedQuantSeconds +
                        $attemptStopwatch.Elapsed.TotalSeconds
                    $remainingMs = Get-RemainingMilliseconds
                    if ($remainingMs -le 0) {
                        $wallCapBreached = $true
                        [System.IO.File]::WriteAllBytes(
                            $responsePath,
                            [byte[]]::new(0)
                        )
                        $exchange = [ordered]@{
                            method = 'POST'
                            uri = "http://127.0.0.1:$($plan.port)/v1/chat/completions"
                            started_at_utc = (Get-Date).ToUniversalTime().ToString('o')
                            completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
                            wall_ms = 0
                            timeout_seconds = 0
                            status_code = $null
                            reason = $null
                            headers = @{}
                            error = 'quant_wall_cap_before_request'
                            response_file = $responseName
                            response_bytes = 0
                            response_sha256 = Get-Sha256Lower -Path $responsePath
                        }
                    }
                    else {
                        $timeoutSeconds = [Math]::Max(
                            1,
                            [Math]::Min(
                                [int]$plan.server_request_timeout_seconds,
                                [int][Math]::Ceiling($remainingMs / 1000.0)
                            )
                        )
                        Add-JournalEntry -Kind 'http_request' -Name $label `
                            -Details ([ordered]@{
                                request_sha256 = $requestHash
                                timeout_seconds = $timeoutSeconds
                            })
                        $exchange = Invoke-HttpExchange -Method POST `
                            -Uri "http://127.0.0.1:$($plan.port)/v1/chat/completions" `
                            -RequestBodyPath $requestPath `
                            -ResponsePath $responsePath `
                            -TimeoutSeconds $timeoutSeconds
                    }
                    $responseObject = $null
                    if ($exchange.status_code -ge 200 -and
                        $exchange.status_code -lt 300 -and
                        $exchange.response_bytes -gt 0) {
                        try {
                            $responseObject = [System.Text.Encoding]::UTF8.GetString(
                                [System.IO.File]::ReadAllBytes($responsePath)
                            ) | ConvertFrom-Json
                        }
                        catch {
                            $responseObject = $null
                        }
                    }
                    $usage = Get-OptionalProperty -Value $responseObject -Name 'usage'
                    $timings = Get-OptionalProperty -Value $responseObject -Name 'timings'
                    $completionTokens = Get-OptionalProperty -Value $usage `
                        -Name 'completion_tokens'
                    $predictedTokens = Get-OptionalProperty -Value $timings `
                        -Name 'predicted_n'
                    $predictedMilliseconds = Get-OptionalProperty -Value $timings `
                        -Name 'predicted_ms'
                    $reportedRate = Get-OptionalProperty -Value $timings `
                        -Name 'predicted_per_second'
                    $computedRate = $null
                    if ($null -ne $predictedTokens -and
                        $null -ne $predictedMilliseconds -and
                        [double]$predictedMilliseconds -gt 0) {
                        $computedRate = [double]$predictedTokens /
                            ([double]$predictedMilliseconds / 1000.0)
                    }
                    $counterConsistent =
                        $null -ne $completionTokens -and
                        $null -ne $predictedTokens -and
                        ([int]$completionTokens -eq [int]$predictedTokens)
                    $rateConsistent =
                        $null -ne $computedRate -and
                        $null -ne $reportedRate -and
                        ([Math]::Abs([double]$computedRate - [double]$reportedRate) `
                            -le [Math]::Max(0.01, [double]$computedRate * 0.01))
                    $failureCause = if ($null -ne $exchange.error) {
                        if ([string]$exchange.error -match
                            '(?i)timed?\s*out|timeout|taskcanceled|cancelled|canceled') {
                            'timeout'
                        }
                        else {
                            'request_error'
                        }
                    }
                    elseif ($exchange.status_code -lt 200 -or
                        $exchange.status_code -ge 300) {
                        'request_error'
                    }
                    elseif ($null -eq $responseObject) {
                        'malformed_response'
                    }
                    elseif (-not $counterConsistent) {
                        'counter_inconsistency'
                    }
                    elseif (-not $rateConsistent) {
                        'rate_inconsistency'
                    }
                    elseif ([int]$predictedTokens -lt
                        [int]$plan.throughput.minimum_decoded_tokens) {
                        'decoded_length_below_minimum'
                    }
                    elseif ([int]$predictedTokens -gt
                        [int]$plan.throughput.max_tokens) {
                        'decoded_length_above_limit'
                    }
                    else {
                        $null
                    }
                    $valid =
                        $null -eq $failureCause
                    $rawResponse = if ($exchange.response_bytes -gt 0) {
                        [System.Text.Encoding]::UTF8.GetString(
                            [System.IO.File]::ReadAllBytes($responsePath)
                        )
                    }
                    else {
                        ''
                    }
                    $row = [ordered]@{
                        schema = 'animus-ferric-throughput-sample-v1'
                        ordinal = $ordinal
                        label = $label
                        scored = $label -ne 'warmup'
                        request_sha256 = $requestHash
                        request_bytes = [UInt64](Get-Item -LiteralPath $requestPath).Length
                        quant_elapsed_before_request_seconds = [Math]::Round(
                            $quantElapsedBeforeRequest,
                            6
                        )
                        remaining_wall_ms_before_request = [int64]$remainingMs
                        exchange = $exchange
                        usage_completion_tokens = $completionTokens
                        timings_predicted_n = $predictedTokens
                        timings_predicted_ms = $predictedMilliseconds
                        timings_reported_per_second = $reportedRate
                        computed_decoded_tokens_per_second = $computedRate
                        counter_consistent = $counterConsistent
                        rate_consistent = $rateConsistent
                        failure_cause = $failureCause
                        valid = $valid
                        raw_response = $rawResponse
                    }
                    $sampleRows.Add($row)
                    [System.IO.File]::AppendAllText(
                        $throughputPath,
                        (($row | ConvertTo-Json -Depth 64 -Compress) + "`n"),
                        [System.Text.UTF8Encoding]::new($false)
                    )
                    Add-JournalEntry -Kind 'http_result' -Name $label -Details $row
                }
                $trialRows = @($sampleRows | Where-Object { $_.scored })
                $validTrials = @($trialRows | Where-Object { $_.valid })
                $validRequests = @($sampleRows | Where-Object { $_.valid })
                $median = $null
                if ($trialRows.Count -eq 3 -and $validTrials.Count -eq 3) {
                    $rates = [double[]]@(
                        $validTrials |
                            ForEach-Object { $_.computed_decoded_tokens_per_second }
                    )
                    $median = Get-Median -Values $rates
                }
                $throughputPassed =
                    ($sampleRows.Count -eq 4) -and
                    ($validRequests.Count -eq 4) -and
                    ($trialRows.Count -eq 3) -and
                    ($validTrials.Count -eq 3) -and
                    ($null -ne $median) -and
                    ([double]$median -ge
                        [double]$plan.throughput.minimum_median_decoded_tokens_per_second)
                $throughput = [ordered]@{
                    schema = 'animus-ferric-throughput-summary-v1'
                    passed = $throughputPassed
                    reason = if ($throughputPassed) { 'viable' } else { 'non_viable' }
                    request_sha256 = $requestHash
                    template_sha256 = Get-Sha256Lower -Path $templatePath
                    scheduled_samples = @($plan.throughput.sequence)
                    observed_samples = $sampleRows.Count
                    valid_request_count = $validRequests.Count
                    valid_trial_count = $validTrials.Count
                    median_decoded_tokens_per_second = $median
                    minimum_required = [double]$plan.throughput.minimum_median_decoded_tokens_per_second
                    samples = @($sampleRows)
                    memory_after_measurement = Get-MemorySnapshot
                }
                Write-JsonLf -Path (Join-Path $rawDir 'throughput-summary.json') `
                    -Value $throughput
            }
        }
    }
}
catch {
    $fatalError = $_.Exception.ToString()
    Add-JournalEntry -Kind 'error' -Name 'coordinate orchestration' `
        -Details ([ordered]@{ error = $fatalError })
}
finally {
    $downResults = [System.Collections.Generic.List[object]]::new()
    $teardownErrors = [System.Collections.Generic.List[string]]::new()
    $memoryBeforeTeardown = $null
    $memoryAfterTeardown = $null
    $postHealth = $null
    $listenersAfter = @()
    $matchingProcessesAfter = @()
    $wrapperProcessesAlive = @()
    $wrapperProcessCleanup = [System.Collections.Generic.List[object]]::new()
    $savedPidAlive = $false
    $cleanupWatch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        Write-JsonLf -Path (Join-Path $rawDir 'memory-before-teardown.json') `
            -Value ($memoryBeforeTeardown = Get-MemorySnapshot)
    }
    catch {
        $teardownErrors.Add("memory-before-teardown: $($_.Exception.Message)")
    }
    try {
        if ((Test-Path -LiteralPath $localRunfile) -or
            (Test-Path -LiteralPath $globalRunfile)) {
            for ($attemptNumber = 1; $attemptNumber -le 2; $attemptNumber++) {
                $cleanupRemaining =
                    ([int]$plan.teardown_safety_grace_seconds * 1000) -
                    [int]$cleanupWatch.ElapsedMilliseconds - 30000
                if ($cleanupRemaining -le 0) {
                    $teardownErrors.Add('managed shutdown exhausted teardown safety grace')
                    break
                }
                $downStdout = Join-Path $rawDir "down-$attemptNumber.stdout.log"
                $downStderr = Join-Path $rawDir "down-$attemptNumber.stderr.log"
                try {
                    $downResult = Invoke-CapturedProcess -FilePath $ferricPath `
                        -Arguments @('server', 'down') -WorkingDirectory $repoRoot `
                        -StdoutPath $downStdout -StderrPath $downStderr `
                        -TimeoutMilliseconds ([Math]::Min(30000, $cleanupRemaining))
                    $downResult['teardown_label'] = "down-$attemptNumber"
                    $downResults.Add($downResult)
                    Register-LiveWrapperProcess -Record $downResult `
                        -Source "ferric server down $attemptNumber"
                    Add-JournalEntry -Kind 'command_result' `
                        -Name 'ferric server down' -Details ([ordered]@{
                            attempt = $attemptNumber
                            result = $downResult
                        })
                    if ($downResult.exit_code -eq 0 -and
                        -not (Test-Path -LiteralPath $localRunfile) -and
                        -not (Test-Path -LiteralPath $globalRunfile)) {
                        break
                    }
                }
                catch {
                    $teardownErrors.Add(
                        "managed shutdown $attemptNumber`: $($_.Exception.Message)"
                    )
                }
                Start-Sleep -Seconds 2
            }
        }

        $ownedProcesses = @(
            Get-CimInstance Win32_Process -OperationTimeoutSec 5 |
                Where-Object {
                    $_.Name -like 'llama-server*' -and
                    $null -ne $_.ExecutablePath -and
                    [System.IO.Path]::GetFullPath(
                        [string]$_.ExecutablePath
                    ).Equals(
                        [System.IO.Path]::GetFullPath($llamaPath),
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -and
                    $null -ne $_.CommandLine -and
                    $_.CommandLine.Contains($modelPath)
                }
        )
        foreach ($ownedProcess in $ownedProcesses) {
            try {
                Stop-Process -Id ([UInt32]$ownedProcess.ProcessId) `
                    -Force -ErrorAction Stop
            }
            catch {
                $teardownErrors.Add(
                    "owned process $($ownedProcess.ProcessId) kill: $($_.Exception.Message)"
                )
            }
        }
        if ($ownedProcesses.Count -gt 0 -or
            (Test-Path -LiteralPath $localRunfile) -or
            (Test-Path -LiteralPath $globalRunfile)) {
            Start-Sleep -Milliseconds 750
            $cleanupRemaining =
                ([int]$plan.teardown_safety_grace_seconds * 1000) -
                [int]$cleanupWatch.ElapsedMilliseconds - 30000
            if ($cleanupRemaining -gt 0) {
                $cleanupStdout = Join-Path $rawDir 'down-cleanup.stdout.log'
                $cleanupStderr = Join-Path $rawDir 'down-cleanup.stderr.log'
                try {
                    $cleanupResult = Invoke-CapturedProcess `
                        -FilePath $ferricPath -Arguments @('server', 'down') `
                        -WorkingDirectory $repoRoot -StdoutPath $cleanupStdout `
                        -StderrPath $cleanupStderr `
                        -TimeoutMilliseconds ([Math]::Min(15000, $cleanupRemaining))
                    $cleanupResult['teardown_label'] = 'down-cleanup'
                    $downResults.Add($cleanupResult)
                    Register-LiveWrapperProcess -Record $cleanupResult `
                        -Source 'ferric server down cleanup'
                }
                catch {
                    $teardownErrors.Add(
                        "post-kill runfile cleanup: $($_.Exception.Message)"
                    )
                }
            }
        }
    }
    catch {
        $teardownErrors.Add("cleanup orchestration: $($_.Exception.Message)")
    }

    foreach ($wrapperRecord in @($liveWrapperProcessRecords)) {
        $cleanupRecord = [ordered]@{
            source = $wrapperRecord.source
            pid = [UInt32]$wrapperRecord.pid
            expected_file = [string]$wrapperRecord.file
            expected_started_at_utc = [string]$wrapperRecord.started_at_utc
            observed_alive_before = $false
            observed_file = $null
            observed_started_at_utc = $null
            identity_matched = $false
            pid_reused = $false
            kill_attempted = $false
            kill_succeeded = $false
            owned_process_alive_after = $false
        }
        try {
            $wrapperProcess = Get-Process -Id ([UInt32]$wrapperRecord.pid) `
                -ErrorAction SilentlyContinue
            if ($null -ne $wrapperProcess) {
                $cleanupRecord.observed_alive_before = $true
                $cleanupRecord.observed_file = [string]$wrapperProcess.Path
                $cleanupRecord.observed_started_at_utc =
                    $wrapperProcess.StartTime.ToUniversalTime().ToString('o')
                $expectedStart = [DateTimeOffset]::Parse(
                    [string]$wrapperRecord.started_at_utc
                )
                $observedStart = [DateTimeOffset]::Parse(
                    [string]$cleanupRecord.observed_started_at_utc
                )
                $cleanupRecord.identity_matched =
                    -not [string]::IsNullOrWhiteSpace(
                        [string]$cleanupRecord.observed_file
                    ) -and
                    [System.IO.Path]::GetFullPath(
                        [string]$cleanupRecord.observed_file
                    ).Equals(
                        [System.IO.Path]::GetFullPath(
                            [string]$wrapperRecord.file
                        ),
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -and
                    [Math]::Abs(
                        ($observedStart - $expectedStart).TotalSeconds
                    ) -le 5.0
                $cleanupRecord.pid_reused = -not $cleanupRecord.identity_matched
                if ($cleanupRecord.identity_matched) {
                    $cleanupRecord.kill_attempted = $true
                    Stop-Process -Id ([UInt32]$wrapperRecord.pid) -Force `
                        -ErrorAction Stop
                    $cleanupRecord.kill_succeeded = $true
                    try {
                        Wait-Process -Id ([UInt32]$wrapperRecord.pid) `
                            -Timeout 5 -ErrorAction SilentlyContinue
                    }
                    catch { }
                    $afterProcess = Get-Process `
                        -Id ([UInt32]$wrapperRecord.pid) `
                        -ErrorAction SilentlyContinue
                    if ($null -ne $afterProcess) {
                        $afterPath = [string]$afterProcess.Path
                        $afterStart = $afterProcess.StartTime.ToUniversalTime()
                        $cleanupRecord.owned_process_alive_after =
                            -not [string]::IsNullOrWhiteSpace($afterPath) -and
                            [System.IO.Path]::GetFullPath($afterPath).Equals(
                                [System.IO.Path]::GetFullPath(
                                    [string]$wrapperRecord.file
                                ),
                                [System.StringComparison]::OrdinalIgnoreCase
                            ) -and
                            [Math]::Abs(
                                ($afterStart - $expectedStart.UtcDateTime).
                                    TotalSeconds
                            ) -le 5.0
                    }
                }
            }
        }
        catch {
            $teardownErrors.Add(
                "wrapper cleanup $($wrapperRecord.source): $($_.Exception.Message)"
            )
            $cleanupRecord.owned_process_alive_after = $true
        }
        if ($cleanupRecord.owned_process_alive_after) {
            $wrapperProcessesAlive += $cleanupRecord
        }
        $wrapperProcessCleanup.Add($cleanupRecord)
    }

    try {
        Start-Sleep -Milliseconds 750
        $listenersAfter = @(Get-NetTCPConnection -State Listen `
            -LocalPort $plan.port -ErrorAction SilentlyContinue)
        if ($null -ne $savedServerPid) {
            $savedPidAlive = $null -ne (Get-Process -Id $savedServerPid `
                -ErrorAction SilentlyContinue)
        }
        $matchingProcessesAfter = @(
            Get-CimInstance Win32_Process -OperationTimeoutSec 5 |
                Where-Object {
                    $_.Name -like 'llama-server*' -and
                    $null -ne $_.CommandLine -and
                    $_.CommandLine.Contains($modelPath)
                } |
                Select-Object ProcessId, Name, ExecutablePath, CommandLine
        )
        $postHealth = Invoke-HttpExchange -Method GET `
            -Uri "http://127.0.0.1:$($plan.port)/health" `
            -ResponsePath (Join-Path $rawDir 'health-after-teardown.body') `
            -TimeoutSeconds 2
        $memoryAfterTeardown = Get-MemorySnapshot
        if ($cleanupWatch.Elapsed.TotalSeconds -gt
            [double]$plan.teardown_safety_grace_seconds) {
            $teardownErrors.Add('teardown safety grace exceeded')
        }
        $teardownPassed =
            (-not $savedPidAlive) -and
            ($listenersAfter.Count -eq 0) -and
            (-not (Test-Path -LiteralPath $localRunfile)) -and
            (-not (Test-Path -LiteralPath $globalRunfile)) -and
            ($matchingProcessesAfter.Count -eq 0) -and
            ($wrapperProcessesAlive.Count -eq 0) -and
            ($postHealth.status_code -ne 200) -and
            ($null -ne $memoryBeforeTeardown) -and
            ($null -ne $memoryAfterTeardown) -and
            ($teardownErrors.Count -eq 0)
    }
    catch {
        $teardownErrors.Add("cold-state verification: $($_.Exception.Message)")
        $teardownPassed = $false
    }
    $cleanupWatch.Stop()
    $teardown = [ordered]@{
        schema = 'animus-ferric-runtime-teardown-v1'
        passed = $teardownPassed
        cleanup_duration_ms = [Math]::Round(
            $cleanupWatch.Elapsed.TotalMilliseconds,
            3
        )
        cleanup_grace_seconds = [int]$plan.teardown_safety_grace_seconds
        down_attempts = @($downResults)
        saved_pid = $savedServerPid
        saved_pid_alive = $savedPidAlive
        listener_records = @($listenersAfter | Select-Object LocalAddress,
            LocalPort, State, OwningProcess)
        local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        global_runfile_absent = -not (Test-Path -LiteralPath $globalRunfile)
        matching_model_processes = $matchingProcessesAfter
        live_wrapper_process_records = @($liveWrapperProcessRecords)
        wrapper_process_cleanup = @($wrapperProcessCleanup)
        wrapper_processes_alive = @($wrapperProcessesAlive)
        memory_before_teardown = $memoryBeforeTeardown
        health_after_teardown = $postHealth
        memory_after_teardown = $memoryAfterTeardown
        errors = @($teardownErrors)
    }
    Write-JsonLf -Path (Join-Path $rawDir 'teardown.json') -Value $teardown
    if (-not $teardownPassed -and $null -eq $fatalError) {
        $fatalError = "teardown failure: $($teardownErrors -join '; ')"
    }
}

$attemptStopwatch.Stop()
$quantElapsedSeconds = $inheritedQuantSeconds +
    $attemptStopwatch.Elapsed.TotalSeconds
if ($quantElapsedSeconds -gt [double]$plan.quant_wall_cap_seconds) {
    $wallCapBreached = $true
}
$reasonCodes = [System.Collections.Generic.List[string]]::new()
if (-not $startup.healthy) {
    $reasonCodes.Add([string]$startup.classification)
}
if ($startup.healthy -and -not $attestation.passed) {
    $reasonCodes.Add('managed_server_attestation_failed')
}
if ($attestation.passed -and -not $smoke.passed) {
    $reasonCodes.Add('functional_smoke_failed')
}
if ($smoke.passed -and -not $throughput.passed) {
    if ($throughput.valid_request_count -ne 4 -or
        $throughput.valid_trial_count -ne 3) {
        $reasonCodes.Add('invalid_throughput_sample_set')
    }
    elseif ($null -ne $throughput.median_decoded_tokens_per_second -and
        [double]$throughput.median_decoded_tokens_per_second -lt
        [double]$plan.throughput.minimum_median_decoded_tokens_per_second) {
        $reasonCodes.Add('throughput_median_below_floor')
    }
    else {
        $reasonCodes.Add('throughput_failed')
    }
}
if ($wallCapBreached) {
    $reasonCodes.Add('quant_wall_cap_breached')
}
if (-not $teardown.passed) {
    $reasonCodes.Add('teardown_incomplete')
}
if ($null -ne $fatalError) {
    $reasonCodes.Add('orchestration_error')
}
$coordinateViable =
    $startup.healthy -and
    $attestation.passed -and
    $smoke.passed -and
    $throughput.passed -and
    (-not $wallCapBreached) -and
    $teardown.passed -and
    ($null -eq $fatalError)
$infrastructureBlocked =
    $wallCapBreached -or
    (-not $teardown.passed) -or
    ($null -ne $fatalError) -or
    ($startup.healthy -and -not $attestation.passed) -or
    (-not $startup.healthy)
$verdict = if ($coordinateViable) {
    'viable'
}
elseif ($infrastructureBlocked) {
    'infrastructure_blocked'
}
else {
    'non_viable'
}
$attempt = [ordered]@{
    schema = 'animus-ferric-runtime-attempt-v2'
    control_epoch = 2
    attestation_protocol = [string]$plan.template_attestation.protocol
    task = 'T-11409'
    coordinate = $Coordinate
    quant = [string]$coordinatePlan.quant
    context = [int]$coordinatePlan.context
    requested_gpu_layers = [int]$modelSpec.requested_gpu_layers
    started_at_utc = $attemptStartedAt
    completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    duration_seconds = [Math]::Round($attemptStopwatch.Elapsed.TotalSeconds, 3)
    prior_quant_elapsed_seconds = [Math]::Round($inheritedQuantSeconds, 3)
    quant_elapsed_seconds = [Math]::Round($quantElapsedSeconds, 3)
    wall_cap_seconds = [int]$plan.quant_wall_cap_seconds
    wall_cap_breached = $wallCapBreached
    startup = $startup
    attestation = $attestation
    smoke = $smoke
    throughput = $throughput
    teardown = $teardown
    failure_classification = if ($startup.classification -ne 'healthy') {
        [string]$startup.classification
    }
    elseif (-not $attestation.passed) {
        'managed_server_attestation_failed'
    }
    elseif (-not $smoke.passed) {
        'functional_smoke_failed'
    }
    elseif (-not $throughput.passed) {
        'throughput_non_viable'
    }
    else {
        $null
    }
    reason_codes = @($reasonCodes)
    verdict = $verdict
    evidence_complete = (-not $infrastructureBlocked)
    fatal_error = $fatalError
}
Write-JsonLf -Path (Join-Path $rawDir 'attempt.json') -Value $attempt
Write-HashManifest -Root $rawDir `
    -OutputPath (Join-Path $rawDir 'files.sha256')
$rawManifestCheck = Test-HashManifest -Root $rawDir `
    -ManifestPath (Join-Path $rawDir 'files.sha256') `
    -RejectUnlistedFiles
if (-not $rawManifestCheck.passed) {
    throw "raw attempt manifest failed: $($rawManifestCheck.errors -join '; ')"
}
if (-not $teardown.passed) {
    throw "cold teardown was not proven; raw incident evidence remains at $rawDir and no official attempt was published"
}
[System.IO.Directory]::CreateDirectory($archiveParent) | Out-Null
$archiveStageParent = Join-Path $repoRoot `
    'target/s114-experiment/archive-stage'
[System.IO.Directory]::CreateDirectory($archiveStageParent) | Out-Null
$archiveStageOwner = Join-Path $archiveStageParent `
    ([guid]::NewGuid().ToString('N'))
$archiveStage = Join-Path $archiveStageOwner $Coordinate
[System.IO.Directory]::CreateDirectory($archiveStageOwner) | Out-Null
$archivePublished = $false
try {
    Copy-Item -LiteralPath $rawDir -Destination $archiveStage -Recurse
    if ($null -ne $coordinatePlan.predecessor) {
        Copy-Item -LiteralPath (
            Join-Path $archiveParent $coordinatePlan.predecessor
        ) -Destination (
            Join-Path $archiveStageOwner $coordinatePlan.predecessor
        ) -Recurse
    }
    $archiveManifestCheck = Test-HashManifest -Root $archiveStage `
        -ManifestPath (Join-Path $archiveStage 'files.sha256') `
        -RejectUnlistedFiles
    if (-not $archiveManifestCheck.passed) {
        throw "staged attempt manifest failed: $($archiveManifestCheck.errors -join '; ')"
    }
    $stageVerificationProcess = Invoke-PowerShellFileBounded `
        -ScriptPath $validatorPath `
        -Arguments @('-AttemptPath', $archiveStage)
    $stageVerification = try {
        $stageVerificationProcess.stdout | ConvertFrom-Json
    }
    catch {
        $null
    }
    if ($stageVerificationProcess.exit_code -ne 0 -or
        $null -eq $stageVerification -or
        -not $stageVerification.passed) {
        $stageErrors = if ($null -ne $stageVerification) {
            @($stageVerification.errors) -join '; '
        }
        else {
            $stageVerificationProcess.stderr
        }
        throw "staged attempt semantic verification failed: $stageErrors"
    }
    if ($null -ne $coordinatePlan.predecessor) {
        [System.IO.Directory]::Delete(
            (Join-Path $archiveStageOwner $coordinatePlan.predecessor),
            $true
        )
    }
    [System.IO.Directory]::Move($archiveStage, $archiveDir)
    $archivePublished = $true
}
finally {
    $resolvedStageParent = [System.IO.Path]::GetFullPath($archiveStageParent)
    $resolvedStageOwner = [System.IO.Path]::GetFullPath($archiveStageOwner)
    try {
        if ($resolvedStageOwner.StartsWith(
                "$resolvedStageParent$([System.IO.Path]::DirectorySeparatorChar)",
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            (Test-Path -LiteralPath $resolvedStageOwner -PathType Container)) {
            [System.IO.Directory]::Delete($resolvedStageOwner, $true)
        }
    }
    catch {
        if (-not $archivePublished) {
            Write-Warning "could not clean unpublished owned stage: $($_.Exception.Message)"
        }
        else {
            Write-Warning "archive published; empty stage-owner cleanup failed: $($_.Exception.Message)"
        }
    }
}

$result = [ordered]@{
    coordinate = $Coordinate
    verdict = $verdict
    reason_codes = @($reasonCodes)
    duration_seconds = $attempt.duration_seconds
    archived_path = "attempts/$Coordinate"
    archived_manifest_sha256 = Get-Sha256Lower -Path (
        Join-Path $archiveDir 'files.sha256'
    )
    archive_verification = $archiveManifestCheck
}
$result | ConvertTo-Json -Depth 16

$runnerExitCode = if ($infrastructureBlocked) { 2 } else { 0 }
}
finally {
    $calibrationLock.Dispose()
}
exit $runnerExitCode
