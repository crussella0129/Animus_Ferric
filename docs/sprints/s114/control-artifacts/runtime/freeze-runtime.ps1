[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
. (Join-Path $artifactDir 'runtime-common.ps1')
$repoRoot = Get-RepositoryRoot -ArtifactDirectory $artifactDir
$planPath = Join-Path $artifactDir 'runtime-plan.json'
$plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
$outputPath = Join-Path $artifactDir 'control-inputs.json'
$digestPath = Join-Path $artifactDir 'control-inputs.sha256'

if (Test-Path -LiteralPath $outputPath) {
    throw 'control-inputs.json already exists; frozen runtime inputs are immutable'
}
if (Test-Path -LiteralPath $digestPath) {
    throw 'control-inputs.sha256 already exists without its manifest'
}

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
$controlNames = @($selfTestInputNames) + @('runtime-self-test.json')
$controls = @(
    foreach ($name in $controlNames) {
        $path = Join-Path $artifactDir $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "missing runtime control input: $name"
        }
        $item = Get-Item -LiteralPath $path
        [ordered]@{
            path = $name
            bytes = [UInt64]$item.Length
            sha256 = Get-Sha256Lower -Path $path
        }
    }
)
$selfTestPath = Join-Path $artifactDir 'runtime-self-test.json'
$selfTest = Get-Content -Raw -LiteralPath $selfTestPath | ConvertFrom-Json
$currentSelfTestInputs = @(
    $controls | Where-Object { $_.path -ne 'runtime-self-test.json' }
)
$requiredSelfTests = @(
    'valid_full_fixture_passes',
    'missing_trace_is_valid_functional_non_viability',
    'malformed_trace_is_valid_functional_non_viability',
    'workspace_mutation_is_valid_functional_non_viability',
    'request_body_tamper_rejected',
    'non_derivable_median_rejected',
    'unlisted_extra_artifact_rejected',
    'memory_classifier_is_specific',
    'exclusive_calibration_lock_rejects_contention',
    'early_failure_releases_calibration_lock'
)
$observedSelfTestNames = @($selfTest.tests | ForEach-Object { $_.name })
$selfTestPassed =
    $selfTest.schema -eq 'animus-ferric-runtime-self-test-v1' -and
    [bool]$selfTest.passed -and
    @($selfTest.tests).Count -gt 0 -and
    @($selfTest.tests | Where-Object { -not $_.passed }).Count -eq 0 -and
    @($observedSelfTestNames | Select-Object -Unique).Count -eq
        $observedSelfTestNames.Count -and
    @($requiredSelfTests | Where-Object {
        $_ -notin $observedSelfTestNames
    }).Count -eq 0 -and
    (Test-JsonEquivalent -Left @($selfTest.inputs) `
        -Right $currentSelfTestInputs)
if (-not $selfTestPassed) {
    throw 'runtime self-test is absent, failed, incomplete, or not bound to the current control bytes'
}

$ferricPath = Join-Path $repoRoot $plan.ferric.relative_path
$llamaBin = Join-Path $repoRoot $plan.llama_cpp.ignored_runtime_relative_path
$llamaPath = Join-Path $llamaBin 'llama-server.exe'
$cudaBackendPath = Join-Path $llamaBin 'ggml-cuda.dll'
$mainArchivePath = Join-Path $repoRoot `
    $plan.llama_cpp.main_asset.ignored_archive_relative_path
$cudaArchivePath = Join-Path $repoRoot `
    $plan.llama_cpp.cuda_runtime_asset.ignored_archive_relative_path
$q4Path = Join-Path $repoRoot $plan.models.Q4_K_M.relative_path
$q3Path = Join-Path $repoRoot $plan.models.Q3_K_XL.relative_path
$cargoLockPath = Join-Path $repoRoot 'Cargo.lock'
$llamaRuntimeFiles = @(Get-FileIdentityManifest -Root $llamaBin)
if ($llamaRuntimeFiles.Count -eq 0) {
    throw 'the extracted llama.cpp runtime tree is empty'
}

$requiredFiles = @(
    $ferricPath,
    $llamaPath,
    $cudaBackendPath,
    $mainArchivePath,
    $cudaArchivePath,
    $q4Path,
    $cargoLockPath
)
foreach ($path in $requiredFiles) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "required frozen runtime file is absent: $path"
    }
}
if (Test-Path -LiteralPath $q3Path -PathType Leaf) {
    throw 'Q3 must remain absent until verified Q4 non-viability authorizes acquisition'
}

$checks = [ordered]@{
    ferric_sha256 = Get-Sha256Lower -Path $ferricPath
    cargo_lock_sha256 = Get-Sha256Lower -Path $cargoLockPath
    llama_server_sha256 = Get-Sha256Lower -Path $llamaPath
    cuda_backend_sha256 = Get-Sha256Lower -Path $cudaBackendPath
    main_archive_bytes = [UInt64](Get-Item -LiteralPath $mainArchivePath).Length
    main_archive_sha256 = Get-Sha256Lower -Path $mainArchivePath
    cuda_archive_bytes = [UInt64](Get-Item -LiteralPath $cudaArchivePath).Length
    cuda_archive_sha256 = Get-Sha256Lower -Path $cudaArchivePath
    q4_bytes = [UInt64](Get-Item -LiteralPath $q4Path).Length
    q4_sha256 = Get-Sha256Lower -Path $q4Path
}

$expectedMatches =
    ($checks.ferric_sha256 -eq $plan.ferric.expected_sha256_at_freeze) -and
    ($checks.cargo_lock_sha256 -eq $plan.ferric.cargo_lock_sha256) -and
    ($checks.llama_server_sha256 -eq $plan.llama_cpp.expected_server_sha256) -and
    ($checks.cuda_backend_sha256 -eq $plan.llama_cpp.expected_cuda_backend_sha256) -and
    ($checks.main_archive_bytes -eq [UInt64]$plan.llama_cpp.main_asset.bytes) -and
    ($checks.main_archive_sha256 -eq $plan.llama_cpp.main_asset.sha256) -and
    ($checks.cuda_archive_bytes -eq [UInt64]$plan.llama_cpp.cuda_runtime_asset.bytes) -and
    ($checks.cuda_archive_sha256 -eq $plan.llama_cpp.cuda_runtime_asset.sha256) -and
    ($checks.q4_bytes -eq [UInt64]$plan.models.Q4_K_M.bytes) -and
    ($checks.q4_sha256 -eq $plan.models.Q4_K_M.sha256)
if (-not $expectedMatches) {
    throw "frozen runtime identity mismatch: $($checks | ConvertTo-Json -Compress)"
}

$ferricVersion = @(Invoke-BoundedTextProcess -FilePath $ferricPath `
    -Arguments @('--version'))
$llamaVersion = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--version'))
$llamaDevices = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--list-devices'))
if (@($llamaDevices | Where-Object { $_ -match 'CUDA0:.*RTX 2080 Ti' }).Count -ne 1) {
    throw "verified CUDA runtime did not expose the expected GPU: $llamaDevices"
}
$llamaHelp = @(Invoke-BoundedTextProcess -FilePath $llamaPath `
    -Arguments @('--help'))
$llamaHelpText = ($llamaHelp -join "`n") + "`n"
$requiredOptionMappings = [ordered]@{
    reasoning = '(?s)--reasoning \[on\|off\|auto\].*?LLAMA_ARG_REASONING'
    reasoning_budget = '(?s)--reasoning-budget N.*?LLAMA_ARG_THINK_BUDGET'
    reasoning_preserve = '(?s)--reasoning-preserve.*?LLAMA_ARG_REASONING_PRESERVE'
    chat_template_kwargs = '(?s)--chat-template-kwargs STRING.*?LLAMA_ARG_CHAT_TEMPLATE_KWARGS'
    timeout = '(?s)--timeout N.*?LLAMA_ARG_TIMEOUT'
}
foreach ($mapping in $requiredOptionMappings.GetEnumerator()) {
    if ($llamaHelpText -notmatch $mapping.Value) {
        throw "llama-server help lacks the required option mapping: $($mapping.Key)"
    }
}

$localRunfile = Join-Path $repoRoot '.ferric/server.json'
$globalRunfile = Join-Path (Join-Path $env:APPDATA 'ferric') 'server.json'
$listener = @(Get-NetTCPConnection -State Listen -LocalPort $plan.port `
    -ErrorAction SilentlyContinue)
if ((Test-Path -LiteralPath $localRunfile) -or
    (Test-Path -LiteralPath $globalRunfile) -or
    $listener.Count -gt 0) {
    throw 'runtime controls may only freeze from a cold managed-server state'
}
$freezeMemory = Get-MemorySnapshot
if ($null -eq $freezeMemory.gpu -or
    [UInt64]$freezeMemory.gpu.free_mib -lt
        [UInt64]$plan.minimum_gpu_free_mib_before_launch) {
    throw 'runtime controls require the declared uncontended GPU-memory floor'
}

$manifest = [ordered]@{
    schema = 'animus-ferric-runtime-control-inputs-v1'
    frozen_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    controls = $controls
    binaries = [ordered]@{
        ferric = [ordered]@{
            display_path = $plan.ferric.relative_path
            bytes = [UInt64](Get-Item -LiteralPath $ferricPath).Length
            sha256 = $checks.ferric_sha256
            version_output = $ferricVersion
        }
        llama_server = [ordered]@{
            display_path = "$($plan.llama_cpp.ignored_runtime_relative_path)/llama-server.exe"
            bytes = [UInt64](Get-Item -LiteralPath $llamaPath).Length
            sha256 = $checks.llama_server_sha256
            version_output = $llamaVersion
            device_output = $llamaDevices
            help_output_sha256 = Get-Sha256Text -Text $llamaHelpText
            option_environment_mappings = $requiredOptionMappings
        }
        cuda_backend = [ordered]@{
            display_path = "$($plan.llama_cpp.ignored_runtime_relative_path)/ggml-cuda.dll"
            bytes = [UInt64](Get-Item -LiteralPath $cudaBackendPath).Length
            sha256 = $checks.cuda_backend_sha256
        }
        llama_runtime = [ordered]@{
            display_root = $plan.llama_cpp.ignored_runtime_relative_path
            file_count = $llamaRuntimeFiles.Count
            files = $llamaRuntimeFiles
        }
    }
    archives = [ordered]@{
        main = [ordered]@{
            display_path = $plan.llama_cpp.main_asset.ignored_archive_relative_path
            bytes = $checks.main_archive_bytes
            sha256 = $checks.main_archive_sha256
            url = $plan.llama_cpp.main_asset.url
        }
        cuda_runtime = [ordered]@{
            display_path = $plan.llama_cpp.cuda_runtime_asset.ignored_archive_relative_path
            bytes = $checks.cuda_archive_bytes
            sha256 = $checks.cuda_archive_sha256
            url = $plan.llama_cpp.cuda_runtime_asset.url
        }
    }
    models = [ordered]@{
        q4 = [ordered]@{
            display_path = $plan.models.Q4_K_M.relative_path
            bytes = $checks.q4_bytes
            sha256 = $checks.q4_sha256
        }
        q3_present_at_freeze = Test-Path -LiteralPath $q3Path -PathType Leaf
    }
    cold_state = [ordered]@{
        local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        global_runfile_absent = -not (Test-Path -LiteralPath $globalRunfile)
        listener_absent = ($listener.Count -eq 0)
        memory = $freezeMemory
        minimum_gpu_free_mib = [UInt64]$plan.minimum_gpu_free_mib_before_launch
    }
}
Write-JsonLf -Path $outputPath -Value $manifest
$manifestHash = Get-Sha256Lower -Path $outputPath
Write-Utf8Lf -Path $digestPath -Text "$manifestHash  control-inputs.json`n"

$manifest
