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
if (-not (Test-RuntimePlanIdentity -Plan $plan)) {
    throw 'runtime plan does not declare the epoch-3 recovery protocol'
}
$repositoryHead = [string](@(Invoke-BoundedTextProcess -FilePath 'git' `
    -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD'))[0])
$repositoryHead = $repositoryHead.Trim()
if ($repositoryHead -cne
    [string]$plan.repository_commit_before_epoch_3_runtime_controls) {
    throw 'repository HEAD differs from the declared epoch-3 pre-control base'
}
$epochOneBaseline = [string](@(Invoke-BoundedTextProcess -FilePath 'git' `
    -Arguments @(
        '-C',
        $repoRoot,
        'rev-parse',
        "$($plan.repository_commit_before_epoch_1_runtime_controls)^{commit}"
    ))[0])
if ($epochOneBaseline.Trim() -cne
    [string]$plan.repository_commit_before_epoch_1_runtime_controls) {
    throw 'declared epoch-1 pre-control baseline is not a repository commit'
}
$epochTwoBaseline = [string](@(Invoke-BoundedTextProcess -FilePath 'git' `
    -Arguments @(
        '-C',
        $repoRoot,
        'rev-parse',
        "$($plan.repository_commit_before_epoch_2_runtime_controls)^{commit}"
    ))[0])
if ($epochTwoBaseline.Trim() -cne
    [string]$plan.repository_commit_before_epoch_2_runtime_controls) {
    throw 'declared epoch-2 pre-control baseline is not a repository commit'
}
[void](Invoke-BoundedTextProcess -FilePath 'git' -Arguments @(
    '-C',
    $repoRoot,
    'merge-base',
    '--is-ancestor',
    [string]$plan.repository_commit_before_epoch_1_runtime_controls,
    [string]$plan.repository_commit_before_epoch_2_runtime_controls
))
[void](Invoke-BoundedTextProcess -FilePath 'git' -Arguments @(
    '-C',
    $repoRoot,
    'merge-base',
    '--is-ancestor',
    [string]$plan.repository_commit_before_epoch_2_runtime_controls,
    [string]$plan.repository_commit_before_epoch_3_runtime_controls
))
$recoveryAnchors = Test-RecoveryAnchors -Plan $plan -RepositoryRoot $repoRoot
if (-not $recoveryAnchors.passed) {
    throw ($recoveryAnchors.errors -join '; ')
}
$measurementContinuity = Test-EpochThreeMeasurementContinuity `
    -Plan $plan -RepositoryRoot $repoRoot
if (-not $measurementContinuity.passed) {
    throw ($measurementContinuity.errors -join '; ')
}

$selfTestInputNames = @(Get-EpochThreeStaticControlNames)
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
    'live_q4_identity_matches_plan',
    'model_hash_deferral_scope_is_restricted',
    'missing_trace_is_valid_functional_non_viability',
    'malformed_trace_is_valid_functional_non_viability',
    'workspace_mutation_is_valid_functional_non_viability',
    'request_body_tamper_rejected',
    'non_derivable_median_rejected',
    'unlisted_extra_artifact_rejected',
    'manifest_path_escape_rejected',
    'memory_classifier_is_specific',
    'exclusive_calibration_lock_rejects_contention',
    'early_failure_releases_calibration_lock',
    'omitted_props_defaults_with_valid_template_probe_passes',
    'live_basename_argv0_with_frozen_image_passes',
    'frozen_absolute_argv0_passes',
    'unauthorized_argv0_spellings_rejected',
    'executable_path_and_hash_tamper_rejected',
    'non_argv0_command_mutations_rejected',
    'windows_quoting_and_argument_boundaries_are_exact',
    'malformed_windows_command_lines_rejected',
    'process_tail_tamper_rejected_by_verifier',
    'process_creation_window_tamper_rejected',
    'preflight_absence_claim_requires_empty_snapshots',
    'missing_reasoning_sentinel_rejected',
    'closed_generation_think_prefix_rejected',
    'template_probe_request_tamper_rejected',
    'chat_template_source_tamper_rejected',
    'template_probe_http_failure_rejected',
    'verifier_path_independence_passes',
    'declared_parent_path_tamper_rejected',
    'epoch_prefixed_paths_do_not_collide',
    'final_manifest_covers_all_runtime_epochs',
    'final_manifest_rejects_external_stage',
    'prior_epoch_anchor_mismatch_rejected',
    'prior_epoch_control_digest_mismatch_rejected',
    'prior_epoch_attempt_tree_mismatch_rejected',
    'prior_epoch_git_protection_mismatch_rejected',
    'epoch_2_terminal_identity_tamper_rejected',
    'measurement_contract_drift_rejected',
    'wrong_task_runtime_plan_rejected',
    'llama_device_free_memory_drift_passes',
    'llama_device_identity_tamper_rejected'
)
$observedSelfTestNames = @($selfTest.tests | ForEach-Object { $_.name })
$selfTestPassed =
    $selfTest.schema -eq 'animus-ferric-runtime-self-test-v3' -and
    $selfTest.task -ceq 'T-11409' -and
    [int]$selfTest.control_epoch -eq 3 -and
    $selfTest.attestation_protocol -ceq
        [string]$plan.template_attestation.protocol -and
    $selfTest.process_command_protocol -ceq
        [string]$plan.process_command_attestation.protocol -and
    [bool]$selfTest.live_q4_identity.passed -and
    [UInt64]$selfTest.live_q4_identity.bytes -eq
        [UInt64]$plan.models.Q4_K_M.bytes -and
    $selfTest.live_q4_identity.sha256 -ceq
        [string]$plan.models.Q4_K_M.sha256 -and
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
$llamaDeviceObservation = Get-LlamaDeviceObservation -Output $llamaDevices
if (-not (Test-JsonEquivalent -Left $llamaDeviceObservation.identity `
        -Right $plan.llama_cpp.expected_device) -or
    [UInt64]$llamaDeviceObservation.free_mib -lt
        [UInt64]$plan.minimum_gpu_free_mib_before_launch) {
    throw "verified CUDA runtime did not expose the expected cold GPU: $($llamaDeviceObservation | ConvertTo-Json -Compress)"
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
    schema = 'animus-ferric-runtime-control-inputs-v3'
    task = 'T-11409'
    control_epoch = 3
    attestation_protocol = [string]$plan.template_attestation.protocol
    process_command_protocol =
        [string]$plan.process_command_attestation.protocol
    frozen_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    runtime_plan_sha256 = Get-Sha256Lower -Path $planPath
    prior_epochs = @($plan.recovery.prior_epochs)
    recovery_anchors = $recoveryAnchors
    measurement_continuity = $measurementContinuity
    repository = [ordered]@{
        head_at_freeze = $repositoryHead
        epoch_1_pre_control_baseline =
            [string]$plan.repository_commit_before_epoch_1_runtime_controls
        epoch_2_pre_control_baseline =
            [string]$plan.repository_commit_before_epoch_2_runtime_controls
        epoch_3_pre_control_base =
            [string]$plan.repository_commit_before_epoch_3_runtime_controls
        prior_evidence_checkpoint =
            [string]$plan.recovery.prior_evidence_checkpoint
    }
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
            device_identity = $llamaDeviceObservation.identity
            device_output_at_freeze = $llamaDevices
            device_free_mib_at_freeze = [UInt64]$llamaDeviceObservation.free_mib
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
