[CmdletBinding()]
param([switch]$BaseOnly)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$plan = Get-Content -Raw -LiteralPath (Join-Path $artifactDir 'runtime-plan.json') |
    ConvertFrom-Json
$validatorPath = Join-Path $artifactDir 'verify-runtime.ps1'
$runnerPath = Join-Path $artifactDir 'run-coordinate.ps1'
$resultPath = Join-Path $artifactDir 'runtime-self-test.json'
$selfTestInputNames = @(
    '.gitattributes',
    'README.md',
    'runtime-plan.json',
    'nonce.txt',
    'smoke-prompt.txt',
    'trace-selftest-fixture.jsonl',
    'throughput-request.template.json',
    'runtime-common.ps1',
    'freeze-runtime.ps1',
    'run-coordinate.ps1',
    'verify-runtime.ps1',
    'verify-q4-gate.ps1',
    'test-runtime.ps1',
    'record-q4-verdict.ps1',
    'finalize-selection.ps1'
)
if (Test-Path -LiteralPath $resultPath) {
    throw 'runtime-self-test.json already exists and will not be overwritten'
}
if (Test-Path -LiteralPath (Join-Path $artifactDir 'control-inputs.json')) {
    throw 'runtime self-test must run before immutable runtime controls are frozen'
}

$stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffffffZ')
$testRoot = Join-Path $repoRoot `
    "target/s114-experiment/runtime-selftest/$PID-$stamp"
$base = Join-Path $testRoot 'base/01-q4-32768'
[System.IO.Directory]::CreateDirectory($base) | Out-Null

function Write-ThroughputRows {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][object[]]$Rows
    )
    $text = @($Rows | ForEach-Object {
        $_ | ConvertTo-Json -Depth 64 -Compress
    }) -join "`n"
    Write-Utf8Lf -Path (Join-Path $Root 'throughput.jsonl') `
        -Text ($text + "`n")
}

function Update-CaseManifest {
    param([Parameter(Mandatory = $true)][string]$Path)
    Write-HashManifest -Root $Path -OutputPath (Join-Path $Path 'files.sha256')
}

function Invoke-Validator {
    param([Parameter(Mandatory = $true)][string]$Path)
    $process = Invoke-PowerShellFileBounded -ScriptPath $validatorPath `
        -Arguments @('-AttemptPath', $Path) -TimeoutMilliseconds 600000
    $report = try { $process.stdout | ConvertFrom-Json } catch { $null }
    [ordered]@{
        exit_code = $process.exit_code
        parseable = $null -ne $report
        report = $report
        stderr = $process.stderr
    }
}

function Copy-Case {
    param([Parameter(Mandatory = $true)][string]$Name)
    $parent = Join-Path $testRoot $Name
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
    $destination = Join-Path $parent '01-q4-32768'
    Copy-Item -LiteralPath $base -Destination $destination -Recurse
    $destination
}

function Read-CaseAttempt {
    param([Parameter(Mandatory = $true)][string]$Path)
    Get-Content -Raw -LiteralPath (Join-Path $Path 'attempt.json') |
        ConvertFrom-Json
}

function Write-CaseAttempt {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Attempt
    )
    Write-JsonLf -Path (Join-Path $Path 'attempt.json') -Value $Attempt
}

function Add-RejectionResult {
    param(
        [Parameter(Mandatory = $true)]$Results,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $validation = Invoke-Validator -Path $Path
    $Results.Add([ordered]@{
        name = $Name
        passed = ($validation.exit_code -ne 0 -and
            $validation.parseable -and -not $validation.report.passed)
        report = $validation.report
        stderr = $validation.stderr
    })
}

function Convert-ToFunctionalFailureFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Smoke
    )

    foreach ($name in @(
        'throughput-summary.json',
        'throughput.jsonl',
        'throughput-request.json'
    ) + @($plan.throughput.sequence | ForEach-Object {
        "throughput-$($_).response.json"
    })) {
        $candidate = Join-Path $Path $name
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            [System.IO.File]::Delete($candidate)
        }
    }
    $attempt = Read-CaseAttempt -Path $Path
    $attempt.smoke = $Smoke
    $attempt.throughput = [ordered]@{
        passed = $false
        reason = 'not_run'
        request_sha256 = $null
        scheduled_samples = @($plan.throughput.sequence)
        observed_samples = 0
        valid_request_count = 0
        valid_trial_count = 0
        median_decoded_tokens_per_second = $null
    }
    $attempt.failure_classification = 'functional_smoke_failed'
    $attempt.reason_codes = @('functional_smoke_failed')
    $attempt.verdict = 'non_viable'
    $attempt.evidence_complete = $true
    $attempt.fatal_error = $null
    Write-JsonLf -Path (Join-Path $Path 'smoke.json') -Value $Smoke
    Write-CaseAttempt -Path $Path -Attempt $attempt
    Update-CaseManifest -Path $Path
}

function Add-AcceptedNonViableResult {
    param(
        [Parameter(Mandatory = $true)]$Results,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $validation = Invoke-Validator -Path $Path
    $Results.Add([ordered]@{
        name = $Name
        passed = ($validation.exit_code -eq 0 -and
            $validation.parseable -and $validation.report.passed -and
            $validation.report.verdict -eq 'non_viable')
        report = $validation.report
        stderr = $validation.stderr
    })
}

function New-Exchange {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$ResponseFile,
        [AllowEmptyString()][Parameter(Mandatory = $true)][string]$Body,
        [Parameter(Mandatory = $true)][DateTimeOffset]$Started,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds,
        [Nullable[int]]$StatusCode,
        [AllowNull()][string]$ErrorMessage
    )
    $responsePath = Join-Path $Root $ResponseFile
    Write-Utf8Lf -Path $responsePath -Text $Body
    [ordered]@{
        method = $Method
        uri = $Uri
        started_at_utc = $Started.ToString('o')
        completed_at_utc = $Started.AddMilliseconds(100).ToString('o')
        wall_ms = 100.0
        timeout_seconds = $TimeoutSeconds
        status_code = $StatusCode
        reason = if ($null -ne $StatusCode) { 'OK' } else { $null }
        headers = [ordered]@{}
        error = if ([string]::IsNullOrEmpty($ErrorMessage)) {
            $null
        }
        else {
            $ErrorMessage
        }
        response_file = $ResponseFile
        response_bytes = [UInt64](Get-Item -LiteralPath $responsePath).Length
        response_sha256 = Get-Sha256Lower -Path $responsePath
    }
}

$coordinate = '01-q4-32768'
$canonicalRawRoot = Join-Path $repoRoot `
    "target/s114-experiment/smoke/$coordinate"
$modelSpec = $plan.models.Q4_K_M
$modelPath = Join-Path $repoRoot $modelSpec.relative_path
$ferricPath = Join-Path $repoRoot $plan.ferric.relative_path
$llamaBin = Join-Path $repoRoot $plan.llama_cpp.ignored_runtime_relative_path
$llamaPath = Join-Path $llamaBin 'llama-server.exe'
$startedAt = [DateTimeOffset]::UtcNow
$completedAt = $startedAt.AddSeconds(100)
$memory = Get-MemorySnapshot
$ferricVersion = @(Invoke-BoundedTextProcess -FilePath $ferricPath `
    -Arguments @('--version'))
$llamaVersion = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--version'))
$llamaDevices = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--list-devices'))
$llamaHelpText = (@(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--help')) -join "`n") + "`n"
$runtimeIdentity = Test-FileIdentityManifest -Root $llamaBin `
    -Expected @(Get-FileIdentityManifest -Root $llamaBin)

$preflight = [ordered]@{
    schema = 'animus-ferric-runtime-preflight-v1'
    coordinate = $coordinate
    captured_at_utc = $startedAt.AddSeconds(1).ToString('o')
    repository_commit = 'self-test'
    repository_status = @()
    runtime_plan_sha256 = Get-Sha256Lower -Path (
        Join-Path $artifactDir 'runtime-plan.json'
    )
    control_inputs_sha256 = $null
    model = [ordered]@{
        display_path = $modelSpec.relative_path
        bytes = [UInt64]$modelSpec.bytes
        sha256 = [string]$modelSpec.sha256
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
        devices = $llamaDevices
        help_output_sha256 = Get-Sha256Text -Text $llamaHelpText
    }
    inherited_runtime_environment = @()
    local_runfile_absent = $true
    global_runfile_absent = $true
    listener_absent = $true
    any_llama_server_process_absent = $true
    minimum_gpu_free_mib = [UInt64]$plan.minimum_gpu_free_mib_before_launch
    memory = $memory
}
Write-JsonLf -Path (Join-Path $base 'preflight.json') -Value $preflight
Write-JsonLf -Path (Join-Path $base 'memory-before-launch.json') -Value $memory

$launchEnvironment = [ordered]@{ Path = "$llamaBin;$env:Path" }
foreach ($property in $plan.server.environment.PSObject.Properties) {
    $launchEnvironment[$property.Name] = [string]$property.Value
}
foreach ($property in $plan.server.logging_environment.PSObject.Properties) {
    $launchEnvironment[$property.Name] = [string]$property.Value
}
$launchEnvironment['LLAMA_ARG_LOG_FILE'] = Join-Path $canonicalRawRoot 'server.log'
$launchArguments = @(
    'server', 'up', '--engine', 'llama-server', '--model', $modelPath,
    '--ctx', '32768', '--threads', [string]$plan.server.threads,
    '--gpu-layers', [string]$modelSpec.requested_gpu_layers,
    '--batch-size', [string]$plan.server.batch_size,
    '--seed', [string]$plan.server.seed,
    '--parallel', [string]$plan.server.parallel_slots,
    '--port', [string]$plan.port
)
$llamaArguments = @(
    'llama-server', '-m', $modelPath, '-c', '32768',
    '-t', [string]$plan.server.threads,
    '-ngl', [string]$modelSpec.requested_gpu_layers,
    '-b', [string]$plan.server.batch_size,
    '--seed', [string]$plan.server.seed,
    '--parallel', [string]$plan.server.parallel_slots,
    '--host', '127.0.0.1', '--port', [string]$plan.port
)
$launch = [ordered]@{
    schema = 'animus-ferric-runtime-launch-v1'
    coordinate = $coordinate
    executable = $ferricPath
    arguments = $launchArguments
    working_directory = $repoRoot
    child_path_prepend = $llamaBin
    environment = $launchEnvironment
    expected_llama_argv = $llamaArguments
}
Write-JsonLf -Path (Join-Path $base 'launch-command.json') -Value $launch
Write-Utf8Lf -Path (Join-Path $base 'launch.stdout.log') -Text 'server ready'
Write-Utf8Lf -Path (Join-Path $base 'launch.stderr.log') -Text ''
$launchProcess = [ordered]@{
    file = $ferricPath
    arguments = $launchArguments
    pid = 41001
    argument_line = (@($launchArguments | ForEach-Object {
        ConvertTo-WindowsCommandLineArgument -Argument ([string]$_)
    }) -join ' ')
    started_at_utc = $startedAt.AddSeconds(2).ToString('o')
    completed_at_utc = $startedAt.AddSeconds(3).ToString('o')
    duration_ms = 1000.0
    timed_out = $false
    execution_timed_out = $false
    post_kill_wait_timed_out = $false
    kill_attempted = $false
    kill_succeeded = $false
    post_process_alive = $false
    exit_code = 0
    stdout_file = 'launch.stdout.log'
    stderr_file = 'launch.stderr.log'
}
Write-JsonLf -Path (Join-Path $base 'launch-process.json') -Value $launchProcess
$startupText = @'
offloaded 24/65 layers
K (q8_0): 64 MiB V (q8_0): 64 MiB
llama_context: flash_attn = 1
chat template, thinking = 1
'@
Write-Utf8Lf -Path (Join-Path $base 'server.log') -Text $startupText
Write-Utf8Lf -Path (Join-Path $base 'startup.log') -Text $startupText
$classificationText = "`nserver ready`n`n$startupText"
Write-Utf8Lf -Path (Join-Path $base 'startup-classification.log') `
    -Text $classificationText
$startup = [ordered]@{
    healthy = $true
    classification = 'healthy'
    memory_match = Test-StartupMemoryPressure -Text $classificationText `
        -Patterns @($plan.startup_memory_patterns)
    classification_input_file = 'startup-classification.log'
    classification_input_bytes = [UInt64](
        Get-Item -LiteralPath (Join-Path $base 'startup-classification.log')
    ).Length
    classification_input_sha256 = Get-Sha256Lower -Path (
        Join-Path $base 'startup-classification.log'
    )
    launch_process = $launchProcess
}
Write-JsonLf -Path (Join-Path $base 'startup.json') -Value $startup

$serverPid = [UInt32]42420
$servedModelId = $modelPath
$runfile = [ordered]@{
    engine = 'llama-server'
    pid = $serverPid
    port = [int]$plan.port
    base_url = "http://127.0.0.1:$($plan.port)/v1"
    tailscale = $false
    model = $modelPath
    context_size = 32768
    sampling_seed = [int]$plan.server.seed
    parallel_slots = [int]$plan.server.parallel_slots
}
Write-JsonLf -Path (Join-Path $base 'runfile.local.json') -Value $runfile
Copy-Item -LiteralPath (Join-Path $base 'runfile.local.json') `
    -Destination (Join-Path $base 'runfile.global.json')
$health = New-Exchange -Root $base -Method 'GET' `
    -Uri "http://127.0.0.1:$($plan.port)/health" `
    -ResponseFile 'health.body' -Body '{"status":"ok"}' `
    -Started $startedAt.AddSeconds(5) -TimeoutSeconds 30 -StatusCode 200
$modelsBody = [ordered]@{
    data = @([ordered]@{ id = $servedModelId; object = 'model' })
} | ConvertTo-Json -Depth 8 -Compress
$models = New-Exchange -Root $base -Method 'GET' `
    -Uri "http://127.0.0.1:$($plan.port)/v1/models" `
    -ResponseFile 'models.body.json' -Body $modelsBody `
    -Started $startedAt.AddSeconds(6) -TimeoutSeconds 30 -StatusCode 200
$propsBody = [ordered]@{
    default_generation_settings = [ordered]@{ n_ctx = 32768 }
    chat_template_caps = [ordered]@{ supports_preserve_reasoning = $true }
    default_template_kwargs = [ordered]@{
        enable_thinking = $true
        preserve_thinking = $true
    }
    total_slots = 1
} | ConvertTo-Json -Depth 8 -Compress
$props = New-Exchange -Root $base -Method 'GET' `
    -Uri "http://127.0.0.1:$($plan.port)/props" `
    -ResponseFile 'props.body.json' -Body $propsBody `
    -Started $startedAt.AddSeconds(7) -TimeoutSeconds 30 -StatusCode 200
$processCommandLine = @(
    $llamaPath
) + @($llamaArguments | Select-Object -Skip 1) | ForEach-Object {
    ConvertTo-WindowsCommandLineArgument -Argument ([string]$_)
}
$attestation = [ordered]@{
    schema = 'animus-ferric-managed-server-attestation-v1'
    passed = $true
    coordinate = $coordinate
    captured_at_utc = $startedAt.AddSeconds(8).ToString('o')
    runfiles = [ordered]@{
        local_path = Join-Path $repoRoot '.ferric/server.json'
        global_path = Join-Path (Join-Path $env:APPDATA 'ferric') 'server.json'
        byte_identical = $true
        value = $runfile
    }
    process = [ordered]@{
        pid = $serverPid
        executable_path = $llamaPath
        executable_sha256 = [string]$plan.llama_cpp.expected_server_sha256
        command_line = ($processCommandLine -join ' ')
        creation_date = $startedAt.AddSeconds(3).ToString('o')
    }
    listener = [ordered]@{
        owners = @($serverPid)
        records = @([ordered]@{
            LocalAddress = '127.0.0.1'
            LocalPort = [int]$plan.port
            State = 'Listen'
            OwningProcess = $serverPid
        })
    }
    endpoints = [ordered]@{
        health = $health
        models = $models
        props = $props
        served_model_id = $servedModelId
        served_n_ctx = 32768
        served_n_ctx_source = 'props.default_generation_settings.n_ctx'
        served_n_params = 27000000000
        total_slots = 1
        chat_template_caps = [ordered]@{ supports_preserve_reasoning = $true }
        default_template_kwargs = [ordered]@{
            enable_thinking = $true
            preserve_thinking = $true
        }
    }
    requested = [ordered]@{
        context = 32768
        gpu_layers = 24
        cache_type_k = 'q8_0'
        cache_type_v = 'q8_0'
        flash_attention = 'on'
        fit = 'on'
        fit_target_mib = 1024
        reasoning = 'on'
        reasoning_budget = 1024
        reasoning_preserve = 'true'
        timeout_seconds = 720
        threads = 12
        batch_size = 512
        parallel_slots = 1
        seed = 42
    }
    effective = [ordered]@{
        context = 32768
        gpu_layers = 24
        total_layers_reported = 65
        cache_type_k = 'q8_0'
        cache_type_v = 'q8_0'
        kv_cache_attestation_lines = @('K (q8_0): 64 MiB V (q8_0): 64 MiB')
        flash_attention = 'enabled'
        flash_attention_attestation_lines = @('flash_attn = 1')
        reasoning_enabled = $true
        reasoning_attestation_lines = @('chat template, thinking = 1')
        preserve_reasoning_supported = $true
        preserve_reasoning_enabled = $true
        preserve_reasoning_evidence_source = 'props.default_template_kwargs'
        reasoning_budget = 1024
        reasoning_budget_evidence_source =
            'frozen_llama_help_env_mapping_and_launch_environment'
        request_timeout_seconds = 720
        request_timeout_evidence_source =
            'frozen_llama_help_env_mapping_and_launch_environment'
        llama_help_sha256 = Get-Sha256Text -Text $llamaHelpText
        preserve_disabled_warning_count = 0
        llama_runtime_identity = $runtimeIdentity
        startup_log_sha256 = Get-Sha256Lower -Path (
            Join-Path $base 'startup.log'
        )
    }
    memory_after_load = $memory
}
Write-JsonLf -Path (Join-Path $base 'attestation.json') -Value $attestation

$smokeWorkspace = Join-Path $base 'smoke-workspace'
$traceDirectory = Join-Path $smokeWorkspace '.ferric/trace'
[System.IO.Directory]::CreateDirectory($traceDirectory) | Out-Null
Copy-Item -LiteralPath (Join-Path $artifactDir 'nonce.txt') `
    -Destination (Join-Path $smokeWorkspace 'nonce.txt')
$beforeManifest = Get-TreeManifest -Root $smokeWorkspace `
    -ExcludedPrefixes @('.ferric')
Write-JsonLf -Path (Join-Path $base 'smoke-workspace.before.json') `
    -Value $beforeManifest
$tracePath = Join-Path $base 'smoke.trace.jsonl'
Copy-Item -LiteralPath (Join-Path $artifactDir 'trace-selftest-fixture.jsonl') `
    -Destination $tracePath
Copy-Item -LiteralPath $tracePath `
    -Destination (Join-Path $traceDirectory 'fixture.jsonl')
$afterManifest = Get-TreeManifest -Root $smokeWorkspace `
    -ExcludedPrefixes @('.ferric')
Write-JsonLf -Path (Join-Path $base 'smoke-workspace.after.json') `
    -Value $afterManifest
Write-Utf8Lf -Path (Join-Path $base 'smoke.stdout.log') -Text 'fixture smoke'
Write-Utf8Lf -Path (Join-Path $base 'smoke.stderr.log') -Text ''
Write-Utf8Lf -Path (Join-Path $base 'smoke-trace-verify.stdout.log') `
    -Text 'fixture trace verified'
Write-Utf8Lf -Path (Join-Path $base 'smoke-trace-verify.stderr.log') -Text ''
$smokePromptPath = Join-Path $artifactDir $plan.smoke.prompt_file
$smokePrompt = (Get-Content -Raw -LiteralPath $smokePromptPath).TrimEnd(
    "`r", "`n"
)
$rawSmokeRoot = $canonicalRawRoot
$smokeArguments = @(
    'query', '--workspace', (Join-Path $rawSmokeRoot 'smoke-workspace'),
    '--model', $servedModelId,
    '--api-base', "http://127.0.0.1:$($plan.port)/v1",
    '--params-b', '27', '--quant', 'Q4_K_M', '--family', 'qwen3.8',
    '--ctx', '32768', '--temperature', [string]$plan.smoke.temperature,
    '--protocol', [string]$plan.smoke.protocol,
    '--harness-policy', [string]$plan.smoke.harness_policy,
    '--tier', [string]$plan.smoke.tier,
    '--max-ring', [string]$plan.smoke.max_ring,
    '--max-turns', [string]$plan.smoke.max_turns,
    '--profile-dir', (Join-Path $rawSmokeRoot 'empty-profile'),
    '--no-config', '--no-stream', $smokePrompt
)
$smokeCommand = [ordered]@{
    executable = $ferricPath
    arguments = $smokeArguments
    working_directory = $repoRoot
    prompt_sha256 = Get-Sha256Lower -Path $smokePromptPath
    nonce_sha256 = Get-Sha256Lower -Path (
        Join-Path $artifactDir $plan.smoke.nonce_file
    )
}
Write-JsonLf -Path (Join-Path $base 'smoke-command.json') -Value $smokeCommand
$smokeProcess = [ordered]@{
    file = $ferricPath
    arguments = $smokeArguments
    pid = 41002
    started_at_utc = $startedAt.AddSeconds(10).ToString('o')
    completed_at_utc = $startedAt.AddSeconds(11).ToString('o')
    duration_ms = 1000.0
    timed_out = $false
    execution_timed_out = $false
    output_drain_timed_out = $false
    kill_attempted = $false
    kill_succeeded = $false
    post_process_alive = $false
    exit_code = 0
    stdout_file = 'smoke.stdout.log'
    stderr_file = 'smoke.stderr.log'
}
$traceVerifyProcess = [ordered]@{
    file = $ferricPath
    arguments = @(
        'trace',
        'verify',
        (Join-Path $rawSmokeRoot 'smoke.trace.jsonl')
    )
    pid = 41003
    started_at_utc = $startedAt.AddSeconds(12).ToString('o')
    completed_at_utc = $startedAt.AddSeconds(13).ToString('o')
    duration_ms = 1000.0
    timed_out = $false
    execution_timed_out = $false
    output_drain_timed_out = $false
    kill_attempted = $false
    kill_succeeded = $false
    post_process_alive = $false
    exit_code = 0
    stdout_file = 'smoke-trace-verify.stdout.log'
    stderr_file = 'smoke-trace-verify.stderr.log'
}
$traceFacts = Get-TraceFacts -TracePath $tracePath `
    -ExpectedNonce $plan.smoke.require_exact_summary `
    -ForbiddenTools @($plan.smoke.forbidden_tools)
$smoke = [ordered]@{
    schema = 'animus-ferric-qwen38-smoke-v1'
    passed = $true
    process = $smokeProcess
    trace_count = 1
    trace_sha256 = Get-Sha256Lower -Path $tracePath
    trace_verify = $traceVerifyProcess
    trace_verify_not_run_reason = $null
    trace_facts = $traceFacts
    trace_parse_error = $null
    workspace_unchanged = $true
    before_manifest = $beforeManifest
    after_manifest = $afterManifest
}
Write-JsonLf -Path (Join-Path $base 'smoke.json') -Value $smoke

$templatePath = Join-Path $artifactDir $plan.throughput.request_template
$templateText = Get-Content -Raw -LiteralPath $templatePath
$requestText = $templateText.Replace(
    '"__SERVED_MODEL_ID__"',
    ($servedModelId | ConvertTo-Json -Compress)
)
$requestPath = Join-Path $base 'throughput-request.json'
Write-Utf8Lf -Path $requestPath -Text $requestText
$requestHash = Get-Sha256Lower -Path $requestPath
$rows = [System.Collections.Generic.List[object]]::new()
$labels = @($plan.throughput.sequence)
for ($index = 0; $index -lt $labels.Count; $index++) {
    $label = [string]$labels[$index]
    $responseName = "throughput-$label.response.json"
    $responseObject = [ordered]@{
        usage = [ordered]@{ completion_tokens = 256 }
        timings = [ordered]@{
            predicted_n = 256
            predicted_ms = 100000
            predicted_per_second = 2.56
        }
    }
    $responseText = $responseObject | ConvertTo-Json -Depth 8 -Compress
    $elapsedSeconds = 30 + ($index * 10)
    $exchange = New-Exchange -Root $base -Method 'POST' `
        -Uri "http://127.0.0.1:$($plan.port)/v1/chat/completions" `
        -ResponseFile $responseName -Body $responseText `
        -Started $startedAt.AddSeconds($elapsedSeconds) `
        -TimeoutSeconds 720 -StatusCode 200
    $rows.Add([ordered]@{
        schema = 'animus-ferric-throughput-sample-v1'
        ordinal = $index + 1
        label = $label
        scored = $index -gt 0
        request_sha256 = $requestHash
        request_bytes = [UInt64](Get-Item -LiteralPath $requestPath).Length
        quant_elapsed_before_request_seconds = [double]$elapsedSeconds
        remaining_wall_ms_before_request = [int64](
            ([int64]$plan.quant_wall_cap_seconds * 1000) -
            ($elapsedSeconds * 1000)
        )
        exchange = $exchange
        usage_completion_tokens = 256
        timings_predicted_n = 256
        timings_predicted_ms = 100000
        timings_reported_per_second = 2.56
        computed_decoded_tokens_per_second = 2.56
        counter_consistent = $true
        rate_consistent = $true
        failure_cause = $null
        valid = $true
        raw_response = $responseText
    })
}
Write-ThroughputRows -Root $base -Rows @($rows)
$throughput = [ordered]@{
    schema = 'animus-ferric-throughput-summary-v1'
    passed = $true
    reason = 'viable'
    request_sha256 = $requestHash
    template_sha256 = Get-Sha256Lower -Path $templatePath
    scheduled_samples = $labels
    observed_samples = 4
    valid_request_count = 4
    valid_trial_count = 3
    median_decoded_tokens_per_second = 2.56
    minimum_required = 2.0
    samples = @($rows)
    memory_after_measurement = $memory
}
Write-JsonLf -Path (Join-Path $base 'throughput-summary.json') `
    -Value $throughput

Write-Utf8Lf -Path (Join-Path $base 'down-1.stdout.log') `
    -Text 'stopped fixture server'
Write-Utf8Lf -Path (Join-Path $base 'down-1.stderr.log') -Text ''
Write-JsonLf -Path (Join-Path $base 'memory-before-teardown.json') `
    -Value $memory
$postHealth = New-Exchange -Root $base -Method 'GET' `
    -Uri "http://127.0.0.1:$($plan.port)/health" `
    -ResponseFile 'health-after-teardown.body' -Body '' `
    -Started $startedAt.AddSeconds(90) -TimeoutSeconds 2 `
    -ErrorMessage 'connection refused'
$teardown = [ordered]@{
    schema = 'animus-ferric-runtime-teardown-v1'
    passed = $true
    cleanup_duration_ms = 1000.0
    cleanup_grace_seconds = [int]$plan.teardown_safety_grace_seconds
    down_attempts = @([ordered]@{
        file = $ferricPath
        arguments = @('server', 'down')
        pid = 41004
        started_at_utc = $startedAt.AddSeconds(88).ToString('o')
        completed_at_utc = $startedAt.AddSeconds(89).ToString('o')
        duration_ms = 1000.0
        timed_out = $false
        execution_timed_out = $false
        output_drain_timed_out = $false
        kill_attempted = $false
        kill_succeeded = $false
        post_process_alive = $false
        exit_code = 0
        stdout_file = 'down-1.stdout.log'
        stderr_file = 'down-1.stderr.log'
        teardown_label = 'down-1'
    })
    saved_pid = $serverPid
    saved_pid_alive = $false
    listener_records = @()
    local_runfile_absent = $true
    global_runfile_absent = $true
    matching_model_processes = @()
    live_wrapper_process_records = @()
    wrapper_process_cleanup = @()
    wrapper_processes_alive = @()
    memory_before_teardown = $memory
    health_after_teardown = $postHealth
    memory_after_teardown = $memory
    errors = @()
}
Write-JsonLf -Path (Join-Path $base 'teardown.json') -Value $teardown

$journalRows = @(
    [ordered]@{
        schema = 'animus-ferric-runtime-journal-row-v1'
        at_utc = $startedAt.AddSeconds(1).ToString('o')
        elapsed_ms = 1000.0
        kind = 'observation'
        name = 'preflight'
        details = [ordered]@{ fixture = $true }
    },
    [ordered]@{
        schema = 'animus-ferric-runtime-journal-row-v1'
        at_utc = $startedAt.AddSeconds(30).ToString('o')
        elapsed_ms = 30000.0
        kind = 'http_result'
        name = 'warmup'
        details = [ordered]@{ fixture = $true }
    },
    [ordered]@{
        schema = 'animus-ferric-runtime-journal-row-v1'
        at_utc = $startedAt.AddSeconds(90).ToString('o')
        elapsed_ms = 90000.0
        kind = 'observation'
        name = 'teardown'
        details = [ordered]@{ fixture = $true }
    }
)
Write-Utf8Lf -Path (Join-Path $base 'command-journal.jsonl') -Text (
    (@($journalRows | ForEach-Object {
        $_ | ConvertTo-Json -Depth 16 -Compress
    }) -join "`n") + "`n"
)

$attempt = [ordered]@{
    schema = 'animus-ferric-runtime-attempt-v1'
    task = 'T-11409'
    coordinate = $coordinate
    quant = 'Q4_K_M'
    context = 32768
    requested_gpu_layers = 24
    started_at_utc = $startedAt.ToString('o')
    completed_at_utc = $completedAt.ToString('o')
    duration_seconds = 100.0
    prior_quant_elapsed_seconds = 0.0
    quant_elapsed_seconds = 100.0
    wall_cap_seconds = 5400
    wall_cap_breached = $false
    startup = $startup
    attestation = $attestation
    smoke = $smoke
    throughput = $throughput
    teardown = $teardown
    failure_classification = $null
    reason_codes = @()
    verdict = 'viable'
    evidence_complete = $true
    fatal_error = $null
}
Write-CaseAttempt -Path $base -Attempt $attempt
Update-CaseManifest -Path $base

$results = [System.Collections.Generic.List[object]]::new()
$valid = Invoke-Validator -Path $base
$results.Add([ordered]@{
    name = 'valid_full_fixture_passes'
    passed = ($valid.exit_code -eq 0 -and
        $valid.parseable -and $valid.report.passed)
    report = $valid.report
    stderr = $valid.stderr
})
if ($BaseOnly) {
    $results[0] | ConvertTo-Json -Depth 100
    if (-not $results[0].passed) { exit 1 }
    exit 0
}

$missingTracePath = Copy-Case -Name 'functional-missing-trace'
$missingTraceSmoke = (Read-CaseAttempt -Path $missingTracePath).smoke
foreach ($missingTraceFile in @(
    (Join-Path $missingTracePath 'smoke.trace.jsonl'),
    (Join-Path $missingTracePath 'smoke-trace-verify.stdout.log'),
    (Join-Path $missingTracePath 'smoke-trace-verify.stderr.log'),
    (Join-Path $missingTracePath 'smoke-workspace/.ferric/trace/fixture.jsonl')
)) {
    if (Test-Path -LiteralPath $missingTraceFile -PathType Leaf) {
        [System.IO.File]::Delete($missingTraceFile)
    }
}
$missingTraceSmoke.passed = $false
$missingTraceSmoke.trace_count = 0
$missingTraceSmoke.trace_sha256 = $null
$missingTraceSmoke.trace_verify = $null
$missingTraceSmoke.trace_verify_not_run_reason = 'trace_count_not_one'
$missingTraceSmoke.trace_facts = $null
$missingTraceSmoke.trace_parse_error = $null
Convert-ToFunctionalFailureFixture -Path $missingTracePath `
    -Smoke $missingTraceSmoke
Add-AcceptedNonViableResult -Results $results `
    -Name 'missing_trace_is_valid_functional_non_viability' `
    -Path $missingTracePath

$malformedTracePath = Copy-Case -Name 'functional-malformed-trace'
$malformedTraceAttempt = Read-CaseAttempt -Path $malformedTracePath
$malformedTraceSmoke = $malformedTraceAttempt.smoke
$malformedTraceText = "{not-json`n"
Write-Utf8Lf -Path (Join-Path $malformedTracePath 'smoke.trace.jsonl') `
    -Text $malformedTraceText
Write-Utf8Lf -Path (
    Join-Path $malformedTracePath 'smoke-workspace/.ferric/trace/fixture.jsonl'
) -Text $malformedTraceText
$malformedTraceSmoke.passed = $false
$malformedTraceSmoke.trace_sha256 = Get-Sha256Lower -Path (
    Join-Path $malformedTracePath 'smoke.trace.jsonl'
)
$malformedTraceSmoke.trace_verify.exit_code = 1
$malformedTraceSmoke.trace_facts = $null
$malformedTraceSmoke.trace_parse_error = 'synthetic malformed JSON trace'
Write-Utf8Lf -Path (
    Join-Path $malformedTracePath 'smoke-trace-verify.stdout.log'
) -Text ''
Write-Utf8Lf -Path (
    Join-Path $malformedTracePath 'smoke-trace-verify.stderr.log'
) -Text 'fixture malformed trace'
Convert-ToFunctionalFailureFixture -Path $malformedTracePath `
    -Smoke $malformedTraceSmoke
Add-AcceptedNonViableResult -Results $results `
    -Name 'malformed_trace_is_valid_functional_non_viability' `
    -Path $malformedTracePath

$mutatedWorkspacePath = Copy-Case -Name 'functional-workspace-mutation'
$mutatedWorkspaceAttempt = Read-CaseAttempt -Path $mutatedWorkspacePath
$mutatedWorkspaceSmoke = $mutatedWorkspaceAttempt.smoke
Write-Utf8Lf -Path (Join-Path $mutatedWorkspacePath 'smoke-workspace/nonce.txt') `
    -Text 'mutated-by-fixture'
$mutatedAfterManifest = Get-TreeManifest `
    -Root (Join-Path $mutatedWorkspacePath 'smoke-workspace') `
    -ExcludedPrefixes @('.ferric')
$mutatedWorkspaceSmoke.passed = $false
$mutatedWorkspaceSmoke.workspace_unchanged = $false
$mutatedWorkspaceSmoke.after_manifest = $mutatedAfterManifest
Write-JsonLf -Path (
    Join-Path $mutatedWorkspacePath 'smoke-workspace.after.json'
) -Value $mutatedAfterManifest
Convert-ToFunctionalFailureFixture -Path $mutatedWorkspacePath `
    -Smoke $mutatedWorkspaceSmoke
Add-AcceptedNonViableResult -Results $results `
    -Name 'workspace_mutation_is_valid_functional_non_viability' `
    -Path $mutatedWorkspacePath

$requestDriftPath = Copy-Case -Name 'request-drift'
[System.IO.File]::AppendAllText(
    (Join-Path $requestDriftPath 'throughput-request.json'), 'x',
    [System.Text.UTF8Encoding]::new($false)
)
Update-CaseManifest -Path $requestDriftPath
Add-RejectionResult -Results $results -Name 'request_body_tamper_rejected' `
    -Path $requestDriftPath

$missingRowPath = Copy-Case -Name 'missing-row'
Write-ThroughputRows -Root $missingRowPath -Rows @($rows | Select-Object -First 3)
Update-CaseManifest -Path $missingRowPath
Add-RejectionResult -Results $results `
    -Name 'missing_or_replaced_sample_rejected' -Path $missingRowPath

$medianDriftPath = Copy-Case -Name 'median-drift'
$medianAttempt = Read-CaseAttempt -Path $medianDriftPath
$medianAttempt.throughput.median_decoded_tokens_per_second = 9.99
Write-CaseAttempt -Path $medianDriftPath -Attempt $medianAttempt
Write-JsonLf -Path (Join-Path $medianDriftPath 'throughput-summary.json') `
    -Value $medianAttempt.throughput
Update-CaseManifest -Path $medianDriftPath
Add-RejectionResult -Results $results -Name 'non_derivable_median_rejected' `
    -Path $medianDriftPath

$extraFilePath = Copy-Case -Name 'extra-file'
Write-Utf8Lf -Path (Join-Path $extraFilePath 'unlisted.txt') -Text 'extra'
Add-RejectionResult -Results $results `
    -Name 'unlisted_extra_artifact_rejected' -Path $extraFilePath

$counterDriftPath = Copy-Case -Name 'counter-drift'
$counterAttempt = Read-CaseAttempt -Path $counterDriftPath
$counterAttempt.throughput.samples[1].usage_completion_tokens = 255
Write-ThroughputRows -Root $counterDriftPath `
    -Rows @($counterAttempt.throughput.samples)
Write-JsonLf -Path (Join-Path $counterDriftPath 'throughput-summary.json') `
    -Value $counterAttempt.throughput
Write-CaseAttempt -Path $counterDriftPath -Attempt $counterAttempt
Update-CaseManifest -Path $counterDriftPath
Add-RejectionResult -Results $results `
    -Name 'raw_counter_drift_rejected' -Path $counterDriftPath

$scoringDriftPath = Copy-Case -Name 'scoring-drift'
$scoringAttempt = Read-CaseAttempt -Path $scoringDriftPath
$scoringAttempt.throughput.samples[0].scored = $true
Write-ThroughputRows -Root $scoringDriftPath `
    -Rows @($scoringAttempt.throughput.samples)
Write-JsonLf -Path (Join-Path $scoringDriftPath 'throughput-summary.json') `
    -Value $scoringAttempt.throughput
Write-CaseAttempt -Path $scoringDriftPath -Attempt $scoringAttempt
Update-CaseManifest -Path $scoringDriftPath
Add-RejectionResult -Results $results `
    -Name 'scoring_drift_rejected' -Path $scoringDriftPath

$warmupPath = Copy-Case -Name 'warmup-invalid-pass-claim'
$warmupAttempt = Read-CaseAttempt -Path $warmupPath
$warmupAttempt.throughput.samples[0].exchange.status_code = $null
$warmupAttempt.throughput.samples[0].exchange.error = 'request failed'
$warmupAttempt.throughput.samples[0].failure_cause = 'request_error'
$warmupAttempt.throughput.samples[0].valid = $false
$warmupAttempt.throughput.valid_request_count = 3
Write-ThroughputRows -Root $warmupPath `
    -Rows @($warmupAttempt.throughput.samples)
Write-JsonLf -Path (Join-Path $warmupPath 'throughput-summary.json') `
    -Value $warmupAttempt.throughput
Write-CaseAttempt -Path $warmupPath -Attempt $warmupAttempt
Update-CaseManifest -Path $warmupPath
Add-RejectionResult -Results $results `
    -Name 'invalid_warmup_cannot_claim_pass' -Path $warmupPath

$timeoutPath = Copy-Case -Name 'timeout-drift'
$timeoutAttempt = Read-CaseAttempt -Path $timeoutPath
$timeoutAttempt.throughput.samples[1].exchange.timeout_seconds = 1
Write-ThroughputRows -Root $timeoutPath `
    -Rows @($timeoutAttempt.throughput.samples)
Write-JsonLf -Path (Join-Path $timeoutPath 'throughput-summary.json') `
    -Value $timeoutAttempt.throughput
Write-CaseAttempt -Path $timeoutPath -Attempt $timeoutAttempt
Update-CaseManifest -Path $timeoutPath
Add-RejectionResult -Results $results `
    -Name 'shortened_trial_timeout_rejected' -Path $timeoutPath

$versionPath = Copy-Case -Name 'version-drift'
$versionPreflightPath = Join-Path $versionPath 'preflight.json'
$versionPreflight = Get-Content -Raw -LiteralPath $versionPreflightPath |
    ConvertFrom-Json
$versionPreflight.llama_server.version = @('forged-version')
Write-JsonLf -Path $versionPreflightPath -Value $versionPreflight
Update-CaseManifest -Path $versionPath
Add-RejectionResult -Results $results `
    -Name 'runtime_version_tamper_rejected' -Path $versionPath

$durationPath = Copy-Case -Name 'duration-drift'
$durationAttempt = Read-CaseAttempt -Path $durationPath
$durationAttempt.duration_seconds = 1.0
$durationAttempt.quant_elapsed_seconds = 1.0
Write-CaseAttempt -Path $durationPath -Attempt $durationAttempt
Update-CaseManifest -Path $durationPath
Add-RejectionResult -Results $results `
    -Name 'wall_clock_duration_drift_rejected' -Path $durationPath

$strictCases = @(
    [ordered]@{ name = 'prelaunch_shape'; target = 'startup'; property = 'launch_process' },
    [ordered]@{ name = 'null_process'; target = 'attestation'; property = 'process'; null = $true },
    [ordered]@{ name = 'attestation_exception'; target = 'attestation'; property = 'endpoints' },
    [ordered]@{ name = 'smoke_exception'; target = 'smoke'; property = 'trace_verify' },
    [ordered]@{ name = 'teardown_exception'; target = 'teardown'; property = 'health_after_teardown' }
)
foreach ($strictCase in $strictCases) {
    $casePath = Copy-Case -Name $strictCase.name
    $caseAttempt = Read-CaseAttempt -Path $casePath
    $target = $caseAttempt.($strictCase.target)
    if ($strictCase.PSObject.Properties['null']) {
        $target.($strictCase.property) = $null
    }
    else {
        $target.PSObject.Properties.Remove([string]$strictCase.property)
    }
    Write-JsonLf -Path (Join-Path $casePath "$($strictCase.target).json") `
        -Value $target
    Write-CaseAttempt -Path $casePath -Attempt $caseAttempt
    Update-CaseManifest -Path $casePath
    Add-RejectionResult -Results $results `
        -Name "$($strictCase.name)_returns_failed_json" -Path $casePath
}

$memoryPositive = Test-StartupMemoryPressure `
    -Text 'CUDA error: out of memory while allocating weights' `
    -Patterns @($plan.startup_memory_patterns)
$memoryNegative = Test-StartupMemoryPressure `
    -Text 'cudaMalloc: invalid argument' `
    -Patterns @($plan.startup_memory_patterns)
$results.Add([ordered]@{
    name = 'memory_classifier_is_specific'
    passed = ($memoryPositive.matched -and -not $memoryNegative.matched)
    evidence = [ordered]@{
        positive = $memoryPositive
        negative = $memoryNegative
    }
})

$lockDirectory = Join-Path $repoRoot 'target/s114-experiment/runtime-lock'
[System.IO.Directory]::CreateDirectory($lockDirectory) | Out-Null
$lockPath = Join-Path $lockDirectory 'calibration.lock'
$lockProbe = [System.IO.FileStream]::new(
    $lockPath,
    [System.IO.FileMode]::OpenOrCreate,
    [System.IO.FileAccess]::ReadWrite,
    [System.IO.FileShare]::None
)
try {
    $contender = Invoke-PowerShellFileBounded -ScriptPath $runnerPath `
        -Arguments @('-Coordinate', '01-q4-32768') `
        -TimeoutMilliseconds 30000
}
finally {
    $lockProbe.Dispose()
}
$results.Add([ordered]@{
    name = 'exclusive_calibration_lock_rejects_contention'
    passed = ($contender.exit_code -ne 0 -and
        $contender.stderr -match 'exclusive runtime lock')
    evidence = $contender
})

$earlyFailure = Invoke-PowerShellFileBounded -ScriptPath $runnerPath `
    -Arguments @('-Coordinate', '01-q4-32768') `
    -TimeoutMilliseconds 30000
$lockReleasedAfterEarlyFailure = $false
$lockReacquire = $null
try {
    $lockReacquire = [System.IO.FileStream]::new(
        $lockPath,
        [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
    $lockReleasedAfterEarlyFailure = $true
}
finally {
    if ($null -ne $lockReacquire) {
        $lockReacquire.Dispose()
    }
}
$results.Add([ordered]@{
    name = 'early_failure_releases_calibration_lock'
    passed = ($earlyFailure.exit_code -ne 0 -and
        $earlyFailure.stderr -match 'runtime controls have not been frozen' -and
        $lockReleasedAfterEarlyFailure)
    evidence = [ordered]@{
        process = $earlyFailure
        lock_reacquired = $lockReleasedAfterEarlyFailure
    }
})

$allPassed = @($results | Where-Object { -not $_.passed }).Count -eq 0
$selfTestInputs = @(
    foreach ($name in $selfTestInputNames) {
        $path = Join-Path $artifactDir $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "runtime self-test input disappeared: $name"
        }
        $item = Get-Item -LiteralPath $path
        [ordered]@{
            path = $name
            bytes = [UInt64]$item.Length
            sha256 = Get-Sha256Lower -Path $path
        }
    }
)
$report = [ordered]@{
    schema = 'animus-ferric-runtime-self-test-v1'
    completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    passed = $allPassed
    test_root = $testRoot
    inputs = $selfTestInputs
    tests = @($results)
}
Write-JsonLf -Path $resultPath -Value $report
$report | ConvertTo-Json -Depth 100
if (-not $allPassed) { exit 1 }
exit 0
