[CmdletBinding()]
param(
    [ValidatePattern('^(latest|[0-9]{3})$')]
    [string]$Attempt = 'latest',
    [switch]$CheckLive
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'runtime-common.ps1')

function Invoke-S115RuntimeVerification {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$AttemptId,
        [switch]$Live
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    function Add-Error { param([string]$Message) $errors.Add($Message) }
    function Read-Json {
        param([Parameter(Mandatory = $true)][string]$Name)
        $path = Join-Path $resolved.path $Name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            Add-Error "missing retained file: $Name"
            return $null
        }
        try { Read-S115EvidenceJson -Path $path }
        catch { Add-Error "malformed retained JSON: $Name"; $null }
    }
    function Assert-Hash {
        param([string]$Name, [AllowNull()][string]$Expected)
        $path = Join-Path $resolved.path $Name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            [string]::IsNullOrWhiteSpace($Expected) -or
            (Get-Sha256Lower -Path $path) -cne $Expected) {
            Add-Error "retained hash cross-link failed: $Name"
        }
    }

    $context = Get-S115Context
    try { $control = Assert-S115ControlInputs -Context $context }
    catch { Add-Error $_.Exception.Message; $control = $null }
    $resolved = Resolve-S115AttemptDirectory -Context $context `
        -Attempt $AttemptId
    $manifestPath = Join-Path $resolved.path `
        ([string]$context.plan.evidence.manifest)
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        Add-Error 'files.sha256 is absent'
        $manifest = $null
    }
    else {
        $manifest = Test-HashManifest -Root $resolved.path `
            -ManifestPath $manifestPath -RejectUnlistedFiles
        if (-not $manifest.passed) {
            Add-Error "attempt file manifest failed: $($manifest.errors -join '; ')"
        }
    }

    $attemptSourceManifestSha256 = $null
    $attemptRuntimePlanSha256 = $null
    $attemptControlCompatibility = $null
    $provenance = Read-Json -Name 'control-provenance.json'
    if ($null -ne $provenance -and $null -ne $control) {
        $attemptSourceManifestSha256 =
            [string]$provenance.source_manifest_sha256
        $attemptControlCompatibility =
            Test-S115VerifierControlManifestCompatibility `
                -AttemptId $resolved.id `
                -AttemptSourceManifestSha256 $attemptSourceManifestSha256 `
                -CurrentManifestSha256 ([string]$control.manifest_sha256)
        if ($provenance.schema -cne
                'animus-ferric-s115-attempt-control-provenance-v1' -or
            -not $attemptControlCompatibility.passed) {
            Add-Error 'attempt source control manifest is neither current nor the exact allowed predecessor'
        }
        if ([string]$provenance.frozen_manifest_sha256 -cne
            $attemptSourceManifestSha256) {
            Add-Error 'attempt source and frozen control manifest identities differ'
        }
        $frozenManifest = Join-Path $resolved.path 'control/control-inputs.sha256'
        if (-not (Test-Path -LiteralPath $frozenManifest -PathType Leaf) -or
            (Get-Sha256Lower -Path $frozenManifest) -cne
                $attemptSourceManifestSha256) {
            Add-Error 'attempt did not freeze its own exact source control manifest'
        }
        $frozenEntries = @{}
        if (Test-Path -LiteralPath $frozenManifest -PathType Leaf) {
            foreach ($line in Get-Content -LiteralPath $frozenManifest) {
                if ([string]::IsNullOrWhiteSpace($line)) { continue }
                if ($line -notmatch '^([0-9a-f]{64})  ([^/\\]+)$' -or
                    $frozenEntries.ContainsKey($Matches[2])) {
                    Add-Error 'frozen control manifest is malformed or duplicated'
                    continue
                }
                $frozenEntries[$Matches[2]] = $Matches[1]
            }
        }
        $recordNames = [System.Collections.Generic.HashSet[string]]::new(
            [System.StringComparer]::OrdinalIgnoreCase
        )
        foreach ($record in @($provenance.files)) {
            if (-not $recordNames.Add([string]$record.name) -or
                -not $frozenEntries.ContainsKey([string]$record.name) -or
                [string]$record.sha256 -cne
                    [string]$frozenEntries[[string]$record.name]) {
                Add-Error "control provenance mapping changed: $($record.name)"
            }
            $path = Join-Path $resolved.path "control/$($record.name)"
            if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
                (Get-Sha256Lower -Path $path) -cne [string]$record.sha256 -or
                [UInt64](Get-Item -LiteralPath $path).Length -ne
                    [UInt64]$record.bytes) {
                Add-Error "frozen control copy changed: $($record.name)"
            }
        }
        if ($frozenEntries.Count -ne 15 -or $recordNames.Count -ne 15 -or
            @($frozenEntries.Keys | Where-Object {
                -not $recordNames.Contains([string]$_)
            }).Count -ne 0) {
            Add-Error 'frozen control provenance is not the exact fifteen-name set'
        }
        $frozenPlanPath = Join-Path $resolved.path 'control/runtime-plan.json'
        if (-not (Test-Path -LiteralPath $frozenPlanPath -PathType Leaf)) {
            Add-Error 'attempt frozen runtime plan is absent'
        }
        else {
            $attemptRuntimePlanSha256 = Get-Sha256Lower -Path $frozenPlanPath
            if (-not $frozenEntries.ContainsKey('runtime-plan.json') -or
                [string]$frozenEntries['runtime-plan.json'] -cne
                    $attemptRuntimePlanSha256) {
                Add-Error 'attempt runtime plan is not bound by its source manifest'
            }
            try {
                $frozenPlan = Read-S115EvidenceJson -Path $frozenPlanPath
                if ($frozenPlan.schema -cne 'animus-ferric-s115-runtime-plan-v1' -or
                    $frozenPlan.task -cne 'T-11503' -or
                    -not (Test-JsonEquivalent -Left $frozenPlan `
                        -Right $context.plan)) {
                    Add-Error 'attempt predecessor plan differs from the current compatible plan'
                }
            }
            catch { Add-Error 'attempt frozen runtime plan is malformed' }
        }
    }

    $start = Read-Json -Name 'attempt-start.json'
    $preflight = Read-Json -Name 'preflight.json'
    if ($null -ne $start) {
        if ($null -eq $control -or
            $null -eq $attemptSourceManifestSha256 -or
            $null -eq $attemptRuntimePlanSha256 -or
            [string]$start.attempt -cne $resolved.id -or
            [string]$start.control_manifest_sha256 -cne
                $attemptSourceManifestSha256 -or
            [string]$start.runtime_plan_sha256 -cne
                $attemptRuntimePlanSha256 -or
            [int]$start.policy.attempt_wall_seconds -ne 5400 -or
            -not [bool]$start.policy.no_qualification_attempt_retry -or
            [string]$start.policy.provider_retry_policy -cne
                [string]$context.plan.policy.provider_retry_policy -or
            -not [bool]$start.policy.no_fallback -or
            -not [bool]$start.policy.no_download) {
            Add-Error 'attempt-start contract differs from the frozen plan'
        }
    }
    if ($null -ne $preflight) {
        $runfilePaths = Get-S115RunfilePaths -Context $context
        $requiredRunfileFields = @(
            'path', 'present', 'bytes', 'sha256', 'content', 'parse_error', 'value'
        )
        $localRunfileFields = @($preflight.host.runfiles.local.PSObject.Properties.Name)
        $globalRunfileFields = @($preflight.host.runfiles.global.PSObject.Properties.Name)
        $processFields = @(
            'ProcessId', 'ParentProcessId', 'Name', 'ExecutablePath',
            'CommandLine', 'CreationDate'
        )
        $listenerFields = @('LocalAddress', 'LocalPort', 'State', 'OwningProcess')
        $derivedBubblewrapVersion = Get-S115BubblewrapVersionFacts `
            -Output ([string]$preflight.isolation.result.stdout) `
            -ExpectedVersion ([string]$context.plan.wsl.bubblewrap_version)
        if ($preflight.schema -cne 'animus-ferric-s115-e17-a-v1' -or
            -not [bool]$preflight.passed -or
            -not [bool]$preflight.enforced_after_complete_capture -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.captured_at_utc) -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.boot_time_utc) -or
            [UInt64]$preflight.host.memory.total_physical_bytes -le 0 -or
            [UInt64]$preflight.host.memory.available_physical_bytes -le 0 -or
            [UInt64]$preflight.host.memory.commit_limit_bytes -le 0 -or
            [UInt64]$preflight.host.memory.committed_bytes -le 0 -or
            [UInt64]$preflight.host.memory.commit_available_bytes -le 0 -or
            [UInt64]$preflight.host.memory.committed_bytes +
                [UInt64]$preflight.host.memory.commit_available_bytes -ne
                [UInt64]$preflight.host.memory.commit_limit_bytes -or
            [UInt64]$preflight.host.disk.total_bytes -le 0 -or
            [UInt64]$preflight.host.disk.free_bytes -le 0 -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.gpu.name) -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.gpu.uuid) -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.gpu.driver_version) -or
            [UInt64]$preflight.host.gpu.total_mib -le 0 -or
            [UInt64]$preflight.host.gpu.free_mib -le 0 -or
            [UInt64]$preflight.host.gpu.used_mib -gt
                [UInt64]$preflight.host.gpu.total_mib -or
            [UInt32]$preflight.host.gpu.utilization_percent -gt 100 -or
            [UInt32]$preflight.host.gpu.temperature_c -le 0 -or
            [string]::IsNullOrWhiteSpace([string]$preflight.host.gpu.power_state) -or
            [double]$preflight.host.gpu.power_draw_watts -le 0 -or
            $preflight.host.runfiles.local.present -or
            $preflight.host.runfiles.global.present -or
            [string]$preflight.host.runfiles.local.path -cne $runfilePaths.local -or
            [string]$preflight.host.runfiles.global.path -cne $runfilePaths.global -or
            (@($localRunfileFields | Sort-Object) -join "`n") -cne
                (@($requiredRunfileFields | Sort-Object) -join "`n") -or
            (@($globalRunfileFields | Sort-Object) -join "`n") -cne
                (@($requiredRunfileFields | Sort-Object) -join "`n") -or
            @($preflight.host.qualified_or_runfile_owned_processes).Count -ne 0 -or
            @($preflight.host.relevant_listeners).Count -ne 0 -or
            (@($preflight.host.qualified_process_record_fields) -join "`n") -cne
                ($processFields -join "`n") -or
            (@($preflight.host.listener_record_fields) -join "`n") -cne
                ($listenerFields -join "`n") -or
            @($preflight.inherited_forbidden_environment_names).Count -ne 0 -or
            $preflight.host.wsl.status.timed_out -or
            [int]$preflight.host.wsl.status.exit_code -ne 0 -or
            $preflight.host.wsl.distributions.timed_out -or
            [int]$preflight.host.wsl.distributions.exit_code -ne 0 -or
            -not [bool]$preflight.isolation.passed -or
            [string]$preflight.isolation.distribution -cne
                [string]$context.plan.wsl.distribution -or
            [int]$preflight.isolation.observed_version -ne 2 -or
            [string]::IsNullOrWhiteSpace([string]$preflight.isolation.observed_state) -or
            $preflight.isolation.distribution_list.timed_out -or
            [int]$preflight.isolation.distribution_list.exit_code -ne 0 -or
            $preflight.isolation.result.timed_out -or
            [int]$preflight.isolation.result.exit_code -ne 0 -or
            -not $derivedBubblewrapVersion.passed -or
            -not (Test-JsonEquivalent `
                -Left $preflight.isolation.bubblewrap_version `
                -Right $derivedBubblewrapVersion) -or
            -not ([string]$preflight.isolation.result.stdout).Contains(
                'S115_NETWORK_NAMESPACE_ONLY_LOOPBACK=1') -or
            -not (Test-JsonEquivalent -Left $preflight.frozen_targets.model `
                -Right $context.plan.model) -or
            -not (Test-JsonEquivalent -Left $preflight.frozen_targets.engine `
                -Right $context.plan.engine) -or
            [string]$preflight.frozen_targets.release_result.sha256 -cne
                [string]$context.plan.qualified_release.result_sha256 -or
            [string]$preflight.frozen_targets.ferric.sha256 -cne
                [string]$context.plan.qualified_release.binary_sha256 -or
            [string]$preflight.frozen_targets.model.sha256 -cne
                [string]$context.plan.model.sha256) {
            Add-Error 'complete E17-A preflight is not derivably satisfied'
        }
    }

    $identity = Read-Json -Name 'runtime-identity.json'
    $finalIdentity = Read-Json -Name 'runtime-identity.final.json'
    if ($null -ne $identity -and $null -ne $finalIdentity) {
        $sourceManifest = Get-Content -Raw -LiteralPath `
            $context.source_manifest_path | ConvertFrom-Json
        $expectedRuntimeFiles = @($sourceManifest.binaries.llama_runtime.files)
        $retainedRuntimeByPath = @{}
        foreach ($entry in @($identity.files.runtime_tree.actual)) {
            if ($retainedRuntimeByPath.ContainsKey([string]$entry.path)) {
                Add-Error "duplicate retained runtime file: $($entry.path)"
            }
            $retainedRuntimeByPath[[string]$entry.path] = $entry
        }
        $runtimeManifestEquivalent = $retainedRuntimeByPath.Count -eq
            $expectedRuntimeFiles.Count
        foreach ($expectedEntry in $expectedRuntimeFiles) {
            if (-not $retainedRuntimeByPath.ContainsKey([string]$expectedEntry.path)) {
                $runtimeManifestEquivalent = $false
                continue
            }
            $actualEntry = $retainedRuntimeByPath[[string]$expectedEntry.path]
            if ([UInt64]$actualEntry.bytes -ne [UInt64]$expectedEntry.bytes -or
                [string]$actualEntry.sha256 -cne [string]$expectedEntry.sha256) {
                $runtimeManifestEquivalent = $false
            }
        }
        $deviceLines = @(([string]$identity.engine_devices.stdout).Replace("`r", '').Split("`n") |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
        try { $derivedDevice = Get-LlamaDeviceObservation -Output $deviceLines }
        catch { $derivedDevice = $null; Add-Error 'retained engine device output is not parseable' }
        $engineVersionText = "$([string]$identity.engine_version.stdout)`n$([string]$identity.engine_version.stderr)"
        if (-not [bool]$identity.passed -or
            (Get-Sha256Lower -Path $context.source_manifest_path) -cne
                [string]$context.plan.engine.source_manifest_sha256 -or
            (Get-Sha256Lower -Path $context.release_result_path) -cne
                [string]$context.plan.qualified_release.result_sha256 -or
            [string]$identity.files.release_result_sha256 -cne
                [string]$context.plan.qualified_release.result_sha256 -or
            -not [bool]$identity.files.runtime_tree.passed -or
            @($identity.files.runtime_tree.actual).Count -ne 55 -or
            [int]$identity.files.runtime_tree.actual_file_count -ne 55 -or
            -not $runtimeManifestEquivalent -or
            [string]$identity.files.ferric.path -cne $context.ferric_path -or
            [UInt64]$identity.files.ferric.bytes -ne
                [UInt64]$context.plan.qualified_release.binary_bytes -or
            [string]$identity.files.ferric.sha256 -cne
                [string]$context.plan.qualified_release.binary_sha256 -or
            [string]$identity.files.model.path -cne $context.model_path -or
            [UInt64]$identity.files.model.bytes -ne [UInt64]$context.plan.model.bytes -or
            [string]$identity.files.model.sha256 -cne
                [string]$context.plan.model.sha256 -or
            [string]$identity.files.engine.path -cne $context.engine_path -or
            [UInt64]$identity.files.engine.bytes -ne
                [UInt64]$context.plan.engine.binary_bytes -or
            [string]$identity.files.engine.sha256 -cne
                [string]$context.plan.engine.binary_sha256 -or
            [string]$identity.files.cuda_backend.path -cne $context.cuda_path -or
            [UInt64]$identity.files.cuda_backend.bytes -ne
                [UInt64]$context.plan.engine.cuda_backend_bytes -or
            [string]$identity.files.cuda_backend.sha256 -cne
                [string]$context.plan.engine.cuda_backend_sha256 -or
            $identity.ferric_version.timed_out -or
            [int]$identity.ferric_version.exit_code -ne 0 -or
            ([string]$identity.ferric_version.stdout).Trim() -cne
                [string]$context.plan.qualified_release.version -or
            $identity.engine_version.timed_out -or
            [int]$identity.engine_version.exit_code -ne 0 -or
            -not $engineVersionText.Contains(
                [string]$context.plan.engine.commit) -or
            -not $engineVersionText.Contains('10516') -or
            $identity.engine_devices.timed_out -or
            [int]$identity.engine_devices.exit_code -ne 0 -or
            $null -eq $derivedDevice -or
            -not (Test-JsonEquivalent -Left $identity.parsed_device -Right $derivedDevice) -or
            [UInt64]$identity.parsed_device.identity.total_mib -le 0 -or
            [UInt64]$identity.parsed_device.free_mib -lt
                [UInt64]$context.plan.policy.minimum_gpu_free_mib -or
            -not (Test-JsonEquivalent -Left $identity.files -Right $finalIdentity)) {
            Add-Error 'runtime/model identity is not exact and stable'
        }
    }
    $inventoryBefore = Read-Json -Name 'model-inventory.before.json'
    $inventoryAfter = Read-Json -Name 'model-inventory.after.json'
    if ($null -ne $inventoryBefore -and $null -ne $inventoryAfter -and
        -not (Test-JsonEquivalent -Left $inventoryBefore -Right $inventoryAfter)) {
        Add-Error 'models inventory changed during the no-download attempt'
    }

    $launch = Read-Json -Name 'launch-command.json'
    $launchResult = Read-Json -Name 'launch-result.json'
    $binding = Read-Json -Name 'post-launch-binding.json'
    $launchProvenance = Read-Json -Name 'launch-provenance.json'
    $expectedLaunch = @(
        'server', 'up', '--engine', 'llama-server', '--model', $context.model_path,
        '--ctx', '32768', '--threads', '12', '--gpu-layers', '24',
        '--batch-size', '512', '--seed', '42', '--parallel', '1', '--port', '8080'
    )
    if ($null -ne $launch) {
        $expectedEnvironmentNames = @('Path', 'LLAMA_ARG_LOG_FILE') +
            @($context.plan.launch_environment.PSObject.Properties.Name)
        $environmentPassed =
            ([string]$launch.environment.Path).StartsWith(
                "$($context.engine_root);",
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            [string]$launch.environment.LLAMA_ARG_LOG_FILE -ceq (Join-Path `
                (Join-Path $context.raw_attempt_root $resolved.id) `
                ([string]$context.plan.evidence.raw_server_log))
        foreach ($property in $context.plan.launch_environment.PSObject.Properties) {
            if ([string]$launch.environment.($property.Name) -cne
                [string]$property.Value) { $environmentPassed = $false }
        }
        if ($launch.schema -cne 'animus-ferric-s115-single-launch-v1' -or
            [int]$launch.launch_ordinal -ne 1 -or
            [string]$launch.executable -cne $context.ferric_path -or
            [string]$launch.executable_sha256 -cne
                [string]$context.plan.qualified_release.binary_sha256 -or
            [string]$launch.working_directory -cne $context.repository_root -or
            (@($launch.arguments) -join "`n") -cne ($expectedLaunch -join "`n") -or
            (@($launch.expected_child_argv) -join "`n") -cne
                (@(Get-S115ExpectedChildArgv -Context $context) -join "`n") -or
            (@($launch.environment.PSObject.Properties.Name | Sort-Object) -join "`n") -cne
                (@($expectedEnvironmentNames | Sort-Object) -join "`n") -or
            -not $environmentPassed) {
            Add-Error 'single frozen launch declaration changed'
        }
    }
    $engineResolution = Read-Json -Name 'engine-resolution.json'
    if ($null -ne $engineResolution -and $null -ne $launch) {
        $firstResolution = $engineResolution.proof.first_path_match
        $shadowEntries = @($engineResolution.proof.higher_priority_candidates |
            Where-Object { $_.present -and -not $_.is_pinned })
        if ($engineResolution.schema -cne
                'animus-ferric-s115-start-process-engine-resolution-v1' -or
            -not [bool]$engineResolution.passed -or
            [string]$engineResolution.path_injection_strategy -cne
                'parent-process-scoped-inheritance-restored-no-Start-Process-Path-override' -or
            [string]$engineResolution.effective_path -cne
                [string]$launch.environment.Path -or
            -not [bool]$engineResolution.proof.passed -or
            [string]$engineResolution.proof.pinned_path -cne $context.engine_path -or
            [string]$engineResolution.proof.pinned_sha256 -cne
                [string]$context.plan.engine.binary_sha256 -or
            $shadowEntries.Count -ne 0 -or $null -eq $firstResolution -or
            [string]$firstResolution.path -cne $context.engine_path -or
            -not [bool]$firstResolution.regular_file -or
            [bool]$firstResolution.reparse_point -or
            [string]$firstResolution.sha256 -cne
                [string]$context.plan.engine.binary_sha256) {
            Add-Error 'bare engine resolution proof is not exact or shadow-free'
        }
    }
    if ($null -ne $launchResult -and
        ($launchResult.timed_out -or $launchResult.exit_code -ne 0)) {
        Add-Error 'single Ferric launch did not complete successfully'
    }
    if ($null -ne $binding -and -not [bool]$binding.passed) {
        Add-Error 'post-launch process/runfile/listener binding failed'
    }
    if ($null -ne $launchProvenance -and
        (-not [bool]$launchProvenance.passed -or
        [UInt32]$launchProvenance.wrapper_pid -ne [UInt32]$launchResult.pid -or
        [UInt32]$launchProvenance.child_pid -ne [UInt32]$binding.process.pid -or
        [UInt32]$launchProvenance.child_parent_pid -ne
            [UInt32]$launchResult.pid -or
        [UInt32]$launchProvenance.ready_pid -ne [UInt32]$binding.process.pid -or
        -not [bool]$launchProvenance.creation_binding.passed -or
        [string]$launchProvenance.ready_line -notmatch
            '^server ready:\s+http://127\.0\.0\.1:8080/v1\s+\(pid\s+\d+\)$' -or
        [UInt64]$launchProvenance.ready_stdout_prefix.bytes -le 0 -or
        [string]$launchProvenance.ready_stdout_prefix.sha256 -notmatch
            '^[0-9a-f]{64}$')) {
        Add-Error 'bound child is not proven to descend from the sole Ferric launch'
    }
    $launchStreams = Read-Json -Name 'launch-stream-prefixes.json'
    $rawAttempt = Join-Path $context.raw_attempt_root $resolved.id
    $readySnapshotPath = Join-Path $resolved.path 'launch-ready.stdout.prefix.bin'
    $readySnapshotPassed = Test-Path -LiteralPath $readySnapshotPath -PathType Leaf
    if ($readySnapshotPassed) {
        $readySnapshotPassed =
            [string]$launchProvenance.ready_stdout_prefix_file -ceq
                'launch-ready.stdout.prefix.bin' -and
            [UInt64](Get-Item -LiteralPath $readySnapshotPath).Length -eq
                [UInt64]$launchProvenance.ready_stdout_prefix.bytes -and
            (Get-Sha256Lower -Path $readySnapshotPath) -ceq
                [string]$launchProvenance.ready_stdout_prefix.sha256
    }
    if (-not $readySnapshotPassed) {
        Add-Error 'tracked launch-ready stdout prefix bytes are absent or detached'
    }
    if ($null -ne $launchStreams -and
        ([string]$launchStreams.stdout.raw_relative_path -cne
            (Get-RelativeSlashPath -Root $context.repository_root -Path (
                Join-Path $rawAttempt 'launch-live.stdout.log')) -or
        [string]$launchStreams.stderr.raw_relative_path -cne
            (Get-RelativeSlashPath -Root $context.repository_root -Path (
                Join-Path $rawAttempt 'launch-live.stderr.log')) -or
        [string]$launchStreams.stdout.sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$launchStreams.stderr.sha256 -notmatch '^[0-9a-f]{64}$' -or
        [UInt64]$launchStreams.stdout.bytes -lt
            [UInt64]$launchProvenance.ready_stdout_prefix.bytes -or
        -not [bool]$launchStreams.ready_stdout_prefix.passed -or
        [UInt64]$launchStreams.ready_stdout_prefix.expected_bytes -ne
            [UInt64]$launchProvenance.ready_stdout_prefix.bytes -or
        [string]$launchStreams.ready_stdout_prefix.expected_sha256 -cne
            [string]$launchProvenance.ready_stdout_prefix.sha256)) {
        Add-Error 'launch stream prefix evidence is semantically detached'
    }

    $health = Read-Json -Name 'health.body.json'
    $models = Read-Json -Name 'models.body.json'
    $props = Read-Json -Name 'props.body.json'
    $endpointExchanges = Read-Json -Name 'endpoint-exchanges.json'
    $propertyDigest = $null
    if ($null -ne $health -and $null -ne $models -and $null -ne $props) {
        $entries = @($models.data)
        if ($entries.Count -ne 1) { Add-Error 'models body does not contain one model' }
        else {
            $propertyDigest = Get-S115StablePropertyDigest -Props $props `
                -ModelEntry $entries[0]
            if ([string]$health.status -cne 'ok' -or
                [string]$entries[0].id -cne $context.model_path -or
                [UInt64]$propertyDigest.value.served_n_params -ne
                    [UInt64]$context.plan.model.parameters -or
                [string]$propertyDigest.value.served_ftype -cne
                    [string]$context.plan.model.expected_served_ftype -or
                [int64]$propertyDigest.value.context -ne 32768 -or
                [int]$propertyDigest.value.seed -ne 42 -or
                [int]$propertyDigest.value.total_slots -ne 1 -or
                [string]$propertyDigest.value.chat_template_sha256 -cne
                    [string]$context.plan.template_attestation.expected_chat_template_sha256 -or
                $propertyDigest.value.supports_preserve_reasoning -ne $true -or
                [string]$props.build_info -cne 'b10516-b95502ba9') {
                Add-Error 'retained endpoint bodies fail model/property derivation'
            }
        }
    }
    if ($null -ne $endpointExchanges) {
        foreach ($spec in @(
            @('health', 'GET', 'http://127.0.0.1:8080/health', 'health.body.json'),
            @('models', 'GET', 'http://127.0.0.1:8080/v1/models', 'models.body.json'),
            @('props', 'GET', 'http://127.0.0.1:8080/props', 'props.body.json')
        )) {
            $exchange = $endpointExchanges.($spec[0])
            $bodyPath = Join-Path $resolved.path $spec[3]
            if ([string]$exchange.method -cne $spec[1] -or
                [string]$exchange.uri -cne $spec[2] -or
                [int]$exchange.status_code -ne 200 -or
                -not [string]::IsNullOrWhiteSpace([string]$exchange.error) -or
                [string]$exchange.response_file -cne $spec[3] -or
                [string]$exchange.response_sha256 -cne
                    (Get-Sha256Lower -Path $bodyPath) -or
                [UInt64]$exchange.response_bytes -ne
                    [UInt64](Get-Item -LiteralPath $bodyPath).Length) {
                Add-Error "endpoint exchange is not bound: $($spec[0])"
            }
        }
    }

    $template = Read-Json -Name 'template-attestation.json'
    if ($null -ne $template) {
        try {
            $derivedTemplate = Get-TemplateProbeFacts -Plan $context.plan `
                -ArtifactDirectory (Join-Path $resolved.path 'control') `
                -EvidenceDirectory $resolved.path
            $templateExchangePassed = @($template.exchanges).Count -eq 4
            for ($index = 0; $index -lt [Math]::Min(
                @($template.exchanges).Count, 4); $index++) {
                $record = @($template.exchanges)[$index]
                $arm = @($context.plan.template_attestation.arms)[$index]
                $responseName = "template-probe.$($arm.name).response.json"
                $responsePath = Join-Path $resolved.path $responseName
                if ([string]$record.name -cne [string]$arm.name -or
                    [string]$record.exchange.method -cne 'POST' -or
                    [string]$record.exchange.uri -cne
                        'http://127.0.0.1:8080/apply-template' -or
                    [int]$record.exchange.status_code -ne 200 -or
                    -not [string]::IsNullOrWhiteSpace([string]$record.exchange.error) -or
                    [int]$record.exchange.timeout_seconds -ne 30 -or
                    [string]$record.exchange.response_file -cne $responseName -or
                    [string]$record.exchange.response_sha256 -cne
                        (Get-Sha256Lower -Path $responsePath) -or
                    [UInt64]$record.exchange.response_bytes -ne
                        [UInt64](Get-Item -LiteralPath $responsePath).Length) {
                    $templateExchangePassed = $false
                }
            }
            if (-not [bool]$template.passed -or -not $derivedTemplate.passed -or
                -not (Test-JsonEquivalent -Left $template.facts `
                    -Right $derivedTemplate) -or -not $templateExchangePassed -or
                -not [bool]$derivedTemplate.differential.preserve_thinking_default_effective) {
                Add-Error 'four-arm template attestation failed independent derivation'
            }
        }
        catch { Add-Error "template attestation derivation failed: $($_.Exception.Message)" }
    }
    $log = Read-Json -Name 'server-log-attestation.json'
    $expectedRawLogRelative = "target/s115-runtime-qualification/attempts/$($resolved.id)/server-live.log"
    $retainedLogValue = if ($null -ne $log) {
        [ordered]@{
            effective_gpu_layers = $log.effective_gpu_layers
            total_layers = $log.total_layers
            offload_line = $log.offload_line
            kv_cache_lines = @($log.kv_cache_lines)
            flash_attention_lines = @($log.flash_attention_lines)
            thinking_lines = @($log.thinking_lines)
            preserve_warning_count = $log.preserve_warning_count
        }
    } else { $null }
    if ($null -ne $log -and
        (-not [bool]$log.passed -or [int]$log.effective_gpu_layers -ne 24 -or
        -not [bool]$log.facts.passed -or
        [string]$log.facts.schema -cne
            'animus-ferric-s115-server-log-facts-v1' -or
        [string]$log.facts.sha256 -notmatch '^[0-9a-f]{64}$' -or
        -not (Test-JsonEquivalent -Left $log.facts.value -Right $retainedLogValue) -or
        @($log.kv_cache_lines).Count -lt 1 -or
        @($log.flash_attention_lines).Count -lt 1 -or
        @($log.thinking_lines).Count -lt 1 -or
        [int]$log.preserve_warning_count -ne 0 -or
        [string]$log.raw_relative_path -cne $expectedRawLogRelative -or
        [UInt64]$log.prefix.bytes -le 0 -or
        [string]$log.prefix.sha256 -notmatch '^[0-9a-f]{64}$')) {
        Add-Error 'compact server-log attestation fails effective-property proof'
    }
    $postLoad = Read-Json -Name 'post-load-host.json'
    $postLoadProcesses = if ($null -ne $postLoad) {
        @($postLoad.qualified_or_runfile_owned_processes)
    } else { @() }
    $postLoadListeners = if ($null -ne $postLoad) {
        @($postLoad.relevant_listeners)
    } else { @() }
    if ($null -ne $postLoad -and
        ([UInt64]$postLoad.memory.total_physical_bytes -le 0 -or
        [UInt64]$postLoad.memory.available_physical_bytes -le 0 -or
        [UInt64]$postLoad.memory.committed_bytes -le 0 -or
        [UInt64]$postLoad.memory.commit_limit_bytes -le 0 -or
        [UInt64]$postLoad.memory.commit_available_bytes -le 0 -or
        [string]::IsNullOrWhiteSpace([string]$postLoad.gpu.name) -or
        [string]::IsNullOrWhiteSpace([string]$postLoad.gpu.driver_version) -or
        [UInt64]$postLoad.gpu.total_mib -le 0 -or
        [UInt64]$postLoad.gpu.used_mib -le 0 -or
        [UInt64]$postLoad.gpu.free_mib -le 0 -or
        [UInt32]$postLoad.gpu.utilization_percent -gt 100 -or
        [UInt32]$postLoad.gpu.temperature_c -le 0 -or
        [string]::IsNullOrWhiteSpace([string]$postLoad.gpu.power_state) -or
        [double]$postLoad.gpu.power_draw_watts -le 0 -or
        -not [bool]$postLoad.runfiles.local.present -or
        -not [bool]$postLoad.runfiles.global.present -or
        $postLoadProcesses.Count -ne 1 -or
        [string]::IsNullOrWhiteSpace(
            [string]$postLoadProcesses[0].ExecutablePath) -or
        [string]::IsNullOrWhiteSpace(
            [string]$postLoadProcesses[0].CommandLine) -or
        $postLoadListeners.Count -ne 1 -or
        [string]$postLoadListeners[0].LocalAddress -cne '127.0.0.1' -or
        [int]$postLoadListeners[0].LocalPort -ne 8080)) {
        Add-Error 'post-load startup allocation snapshot is incomplete'
    }
    $attestation = Read-Json -Name 'server-attestation.json'
    if ($null -ne $attestation -and
        (-not [bool]$attestation.passed -or
        [string]$attestation.served_model_id -cne $context.model_path -or
        [UInt64]$attestation.served_n_params -ne
            [UInt64]$context.plan.model.parameters -or
        -not (Test-JsonEquivalent -Left $attestation.stable_properties `
            -Right $propertyDigest) -or
        [string]$attestation.build_info -cne
            "$($context.plan.engine.release)-$($context.plan.engine.commit)" -or
        [int]$attestation.effective.context -ne 32768 -or
        [int]$attestation.effective.gpu_layers -ne 24 -or
        [int]$attestation.effective.seed -ne 42 -or
        [string]$attestation.effective.quant -cne 'Q4_K - Medium' -or
        [string]$attestation.effective.cache_type_k -cne 'q8_0' -or
        [string]$attestation.effective.cache_type_v -cne 'q8_0' -or
        [string]$attestation.effective.flash_attention -cne 'enabled' -or
        [string]$attestation.effective.reasoning -cne 'enabled' -or
        -not [bool]$attestation.effective.preserve_reasoning -or
        [string]$attestation.post_load_host_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path 'post-load-host.json')) -or
        [string]$attestation.template_attestation_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path `
                'template-attestation.json')) -or
        [string]$attestation.server_log_attestation_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path `
                'server-log-attestation.json')) -or
        [string]$attestation.server_log_facts_sha256 -cne
            [string]$log.facts.sha256 -or
        -not (Test-JsonEquivalent -Left $attestation.endpoint_exchanges `
            -Right $endpointExchanges))) {
        Add-Error 'managed-server attestation is not the exact effective coordinate'
    }

    $smoke = Read-Json -Name 'smoke.json'
    $smokeCommand = Read-Json -Name 'smoke-command.json'
    $workspaceBefore = Read-Json -Name 'smoke-workspace.before.json'
    $workspaceAfter = Read-Json -Name 'smoke-workspace.after.json'
    $tracePath = Join-Path $resolved.path 'smoke.trace.jsonl'
    $smokeWorkspace = Join-Path $rawAttempt 'smoke-workspace'
    $traceRoot = Join-Path $rawAttempt 'external-trace'
    $profileRoot = Join-Path $rawAttempt 'empty-profile'
    $frozenControlRoot = Join-Path $resolved.path 'control'
    $promptControlPath = Join-Path $frozenControlRoot `
        ([string]$context.plan.smoke.prompt_file)
    $nonceControlPath = Join-Path $frozenControlRoot `
        ([string]$context.plan.smoke.nonce_file)
    $prompt = (Get-Content -Raw -LiteralPath (Join-Path `
        $frozenControlRoot $context.plan.smoke.prompt_file
    )).TrimEnd("`r", "`n")
    $expectedSmokeArguments = @(
        'query', '--workspace', $smokeWorkspace, '--trace-dir', $traceRoot,
        '--model', $context.model_path,
        '--api-base', 'http://127.0.0.1:8080/v1', '--params-b', '27',
        '--quant', [string]$context.plan.coordinate.quant,
        '--family', 'qwen3.8', '--ctx', '32768',
        '--temperature', [string]$context.plan.smoke.temperature,
        '--protocol', [string]$context.plan.smoke.protocol,
        '--harness-policy', [string]$context.plan.smoke.harness_policy,
        '--tier', [string]$context.plan.smoke.tier,
        '--max-ring', [string]$context.plan.smoke.max_ring,
        '--max-turns', [string]$context.plan.smoke.max_turns,
        '--profile-dir', $profileRoot, '--no-config', '--no-stream', $prompt
    )
    if ($null -ne $smokeCommand -and
        ([string]$smokeCommand.executable -cne $context.ferric_path -or
        (@($smokeCommand.arguments) -join "`n") -cne
            ($expectedSmokeArguments -join "`n") -or
        [string]$smokeCommand.workspace -cne
            (Get-RelativeSlashPath -Root $context.repository_root `
                -Path $smokeWorkspace) -or
        [string]$smokeCommand.external_trace_root -cne
            (Get-RelativeSlashPath -Root $context.repository_root `
                -Path $traceRoot) -or
        [string]$smokeCommand.provider_retry_policy -cne
            [string]$context.plan.policy.provider_retry_policy -or
        [bool]$smokeCommand.zero_underlying_http_retries_claimed -or
        [string]$smokeCommand.prompt_sha256 -cne
            (Get-Sha256Lower -Path $promptControlPath) -or
        [string]$smokeCommand.nonce_sha256 -cne
            (Get-Sha256Lower -Path $nonceControlPath))) {
        Add-Error 'smoke command is not the exact external-trace invocation'
    }
    if ($null -ne $workspaceBefore -and $null -ne $workspaceAfter -and
        (-not (Test-ManifestEqual -Before $workspaceBefore -After $workspaceAfter) -or
        [string]$smoke.before_manifest_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path `
                'smoke-workspace.before.json')) -or
        [string]$smoke.after_manifest_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path `
                'smoke-workspace.after.json')))) {
        Add-Error 'smoke workspace immutability is not hash-bound'
    }
    if ($null -ne $smoke -and (Test-Path -LiteralPath $tracePath)) {
        try {
            $traceFacts = Get-TraceFacts -TracePath $tracePath `
                -ExpectedNonce ([string]$context.plan.smoke.expected_summary) `
                -ForbiddenTools @($context.plan.smoke.forbidden_tools)
            $expectedTraceVerifyArguments = @('trace', 'verify', $tracePath)
            if (-not [bool]$smoke.passed -or $smoke.trace_count -ne 1 -or
                [string]$smoke.process.file -cne $context.ferric_path -or
                (@($smoke.process.arguments) -join "`n") -cne
                    ($expectedSmokeArguments -join "`n") -or
                [bool]$smoke.process.timed_out -or
                [bool]$smoke.process.post_process_alive -or
                [int]$smoke.process.exit_code -ne 0 -or
                [string]$smoke.process.stdout_file -cne 'smoke.stdout.log' -or
                [string]$smoke.process.stderr_file -cne 'smoke.stderr.log' -or
                [string]$smoke.trace_verify.file -cne $context.ferric_path -or
                (@($smoke.trace_verify.arguments) -join "`n") -cne
                    ($expectedTraceVerifyArguments -join "`n") -or
                [bool]$smoke.trace_verify.timed_out -or
                [int]$smoke.trace_verify.exit_code -ne 0 -or
                -not (Test-JsonEquivalent -Left $smoke.trace_facts -Right $traceFacts) -or
                -not [bool]$smoke.workspace_unchanged -or
                -not [bool]$smoke.workspace_ferric_absent -or
                $traceFacts.protocol -cne 'constrained_json' -or
                -not $traceFacts.all_turns_json_schema_constrained -or
                -not $traceFacts.read_file_before_task_complete -or
                [int]$traceFacts.exact_nonce_read_result_count -lt 1 -or
                -not $traceFacts.exact_task_complete_summary -or
                @($traceFacts.forbidden_tools_observed).Count -ne 0 -or
                $traceFacts.session_end_reason -cne 'task_complete') {
                Add-Error 'external-trace grammar nonce smoke is not derivably valid'
            }
        }
        catch { Add-Error "retained trace failed parsing: $($_.Exception.Message)" }
    }

    $throughput = Read-Json -Name 'throughput-summary.json'
    $requestPath = Join-Path $resolved.path 'throughput-request.json'
    if (-not (Test-Path -LiteralPath $requestPath -PathType Leaf)) {
        Add-Error 'throughput-request.json is absent'
    }
    if ($null -ne $throughput -and (Test-Path -LiteralPath $requestPath)) {
        $templateText = Get-Content -Raw -LiteralPath (Join-Path `
            $frozenControlRoot $context.plan.throughput.request_template)
        $escaped = $context.model_path | ConvertTo-Json -Compress
        $expectedRequest = $templateText.Replace('"__SERVED_MODEL_ID__"', $escaped)
        $actualRequest = Get-Content -Raw -LiteralPath $requestPath
        if ($actualRequest -cne $expectedRequest -or
            [string]$throughput.request_sha256 -cne
                (Get-Sha256Lower -Path $requestPath) -or
            (@($throughput.scheduled_samples) -join "`n") -cne
                (@($context.plan.throughput.sequence) -join "`n") -or
            [int]$throughput.replacement_samples -ne 0 -or
            @($throughput.samples).Count -ne 4) {
            Add-Error 'throughput request/sequence is not the frozen no-replacement protocol'
        }
        $rates = [System.Collections.Generic.List[double]]::new()
        for ($index = 0; $index -lt @($throughput.samples).Count; $index++) {
            $sample = @($throughput.samples)[$index]
            $expectedLabel = @($context.plan.throughput.sequence)[$index]
            $responseName = "throughput-$expectedLabel.response.json"
            $response = Read-Json -Name $responseName
            if ($null -eq $response) { continue }
            $completion = [int]$response.usage.completion_tokens
            $predicted = [int]$response.timings.predicted_n
            $milliseconds = [double]$response.timings.predicted_ms
            $rate = if ($milliseconds -gt 0) {
                [double]$predicted / ($milliseconds / 1000.0)
            } else { [double]::NaN }
            $reportedRate = [double]$response.timings.predicted_per_second
            $responsePath = Join-Path $resolved.path $responseName
            if ([string]$sample.label -cne $expectedLabel -or
                [int]$sample.ordinal -ne $index + 1 -or
                [bool]$sample.scored -ne ($expectedLabel -cne 'warmup') -or
                [string]$sample.request_sha256 -cne
                    (Get-Sha256Lower -Path $requestPath) -or
                [string]$sample.exchange.method -cne 'POST' -or
                [string]$sample.exchange.uri -cne
                    'http://127.0.0.1:8080/v1/chat/completions' -or
                [int]$sample.exchange.status_code -ne 200 -or
                -not [string]::IsNullOrWhiteSpace([string]$sample.exchange.error) -or
                [int]$sample.exchange.timeout_seconds -le 0 -or
                [int]$sample.exchange.timeout_seconds -gt
                    [int]$context.plan.policy.request_timeout_seconds -or
                [string]$sample.exchange.response_file -cne $responseName -or
                [string]$sample.exchange.response_sha256 -cne
                    (Get-Sha256Lower -Path $responsePath) -or
                [UInt64]$sample.exchange.response_bytes -ne
                    [UInt64](Get-Item -LiteralPath $responsePath).Length -or
                [int]$sample.usage_completion_tokens -ne $completion -or
                [int]$sample.timings_predicted_n -ne $predicted -or
                [double]$sample.timings_predicted_ms -ne $milliseconds -or
                [Math]::Abs([double]$sample.timings_reported_per_second -
                    $reportedRate) -gt 0.000001 -or
                $milliseconds -le 0 -or
                [Math]::Abs($reportedRate - $rate) -gt
                    [Math]::Max(0.01, $rate * 0.01) -or
                $completion -ne $predicted -or
                $predicted -lt [int]$context.plan.throughput.minimum_decoded_tokens -or
                $predicted -gt [int]$context.plan.throughput.max_tokens -or
                [Math]::Abs($rate -
                    [double]$sample.computed_decoded_tokens_per_second) -gt 0.000001 -or
                -not [bool]$sample.counter_consistent -or
                -not [bool]$sample.rate_consistent -or
                $null -ne $sample.failure_cause -or
                -not [bool]$sample.valid) {
                Add-Error "throughput sample is not derivable: $expectedLabel"
            }
            if ($expectedLabel -cne 'warmup') { $rates.Add($rate) }
        }
        $median = if ($rates.Count -eq 3) {
            Get-Median -Values ([double[]]@($rates))
        } else { $null }
        if (-not [bool]$throughput.passed -or
            [int]$throughput.valid_request_count -ne 4 -or
            [int]$throughput.valid_trial_count -ne 3 -or
            $null -eq $median -or
            [Math]::Abs([double]$throughput.median_decoded_tokens_per_second -
                [double]$median) -gt 0.000001 -or
            [double]$median -lt
                [double]$context.plan.throughput.minimum_median_decoded_tokens_per_second) {
            Add-Error 'throughput summary is not a valid three-trial median'
        }
    }

    $handoff = Read-Json -Name 'handoff.json'
    $finalBinding = Read-Json -Name 'final-binding.json'
    if ($null -ne $handoff) {
        $handoffBindingCreationEquivalent =
            Test-S115UtcInstantEquivalent -Left $handoff.process.creation_utc `
                -Right $binding.process.creation_utc
        $handoffFinalCreationEquivalent = if ($null -ne $finalBinding) {
            Test-S115UtcInstantEquivalent -Left $handoff.process.creation_utc `
                -Right $finalBinding.process.creation_utc
        }
        else { [pscustomobject]@{ passed = $false } }
        $coordinateEquivalent = Test-JsonEquivalent -Left $handoff.coordinate `
            -Right $context.plan.coordinate
        $listenersEquivalent = if ($null -ne $finalBinding) {
            Test-JsonEquivalent -Left @($handoff.listeners) `
                -Right @($finalBinding.listeners)
        }
        else { $false }
        $handoffEnvironmentEquivalent = if ($null -ne $launch) {
            Test-JsonEquivalent -Left $handoff.frozen_launch.environment `
                -Right $launch.environment
        }
        else { $false }
        if ($handoff.schema -cne 'animus-ferric-s115-runtime-handoff-v1' -or
            $handoff.state -cne 'qualified_running' -or
            $handoff.attempt -cne $resolved.id -or
            $handoff.endpoint -cne 'http://127.0.0.1:8080/v1' -or
            -not $coordinateEquivalent -or
            $handoff.served_model_id -cne $context.model_path -or
            [UInt64]$handoff.served_n_params -ne
                [UInt64]$context.plan.model.parameters -or
            [int]$handoff.counts.launch -ne 1 -or
            [int]$handoff.counts.fallback -ne 0 -or
            [int]$handoff.counts.download -ne 0 -or
            [int]$handoff.counts.restart -ne 0 -or
            [int]$handoff.counts.throughput -ne 4 -or
            [int]$handoff.counts.throughput_replacement -ne 0 -or
            $handoff.disposition -cne 'leave_same_bound_process_running' -or
            -not $handoffEnvironmentEquivalent -or
            [UInt32]$handoff.process.pid -ne [UInt32]$binding.process.pid -or
            -not $handoffBindingCreationEquivalent.passed -or
            [string]$handoff.property_digest_sha256 -cne
                [string]$propertyDigest.sha256 -or
            [string]$handoff.server_log_facts_sha256 -cne
                [string]$log.facts.sha256 -or
            [string]$handoff.server_log_prefix.sha256 -cne
                [string]$log.prefix.sha256 -or
            [UInt64]$handoff.server_log_prefix.bytes -ne
                [UInt64]$log.prefix.bytes -or
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
            $null -eq $control -or
            [string]$handoff.identities.control_manifest_sha256 -cne
                $attemptSourceManifestSha256 -or
            [string]$handoff.identities.runtime_plan_sha256 -cne
                $attemptRuntimePlanSha256) {
            Add-Error 'handoff does not bind the exact qualified running process'
        }
        if ($null -ne $finalBinding -and
            (-not [bool]$finalBinding.passed -or
            [UInt32]$handoff.process.pid -ne
                [UInt32]$finalBinding.process.pid -or
            -not $handoffFinalCreationEquivalent.passed -or
            [string]$handoff.process.executable_path -cne
                [string]$finalBinding.process.executable_path -or
            [string]$handoff.process.command_line -cne
                [string]$finalBinding.process.command_line -or
            [string]$handoff.runfiles.local_path -cne
                [string]$finalBinding.runfiles.local.path -or
            [string]$handoff.runfiles.local_sha256 -cne
                [string]$finalBinding.runfiles.local.sha256 -or
            [string]$handoff.runfiles.global_path -cne
                [string]$finalBinding.runfiles.global.path -or
            [string]$handoff.runfiles.global_sha256 -cne
                [string]$finalBinding.runfiles.global.sha256 -or
            -not $listenersEquivalent)) {
            Add-Error 'handoff differs from retained final binding evidence'
        }
        Assert-Hash -Name 'control-provenance.json' `
            -Expected $handoff.evidence.control_provenance_sha256
        Assert-Hash -Name 'runtime-identity.final.json' `
            -Expected $handoff.evidence.runtime_identity_sha256
        Assert-Hash -Name 'server-attestation.json' `
            -Expected $handoff.evidence.server_attestation_sha256
        Assert-Hash -Name 'template-attestation.json' `
            -Expected $handoff.evidence.template_attestation_sha256
        Assert-Hash -Name 'smoke.json' -Expected $handoff.evidence.smoke_sha256
        Assert-Hash -Name 'smoke.trace.jsonl' -Expected $handoff.evidence.trace_sha256
        Assert-Hash -Name 'throughput-summary.json' `
            -Expected $handoff.evidence.throughput_sha256
        Assert-Hash -Name 'final-binding.json' `
            -Expected $handoff.evidence.final_binding_sha256
        Assert-Hash -Name 'model-inventory.before.json' `
            -Expected $handoff.evidence.model_inventory_before_sha256
        Assert-Hash -Name 'model-inventory.after.json' `
            -Expected $handoff.evidence.model_inventory_after_sha256
        Assert-Hash -Name 'launch-stream-prefixes.json' `
            -Expected $handoff.evidence.launch_stream_prefixes_sha256
        Assert-Hash -Name 'launch-provenance.json' `
            -Expected $handoff.evidence.launch_provenance_sha256
        Assert-Hash -Name 'launch-ready.stdout.prefix.bin' `
            -Expected $handoff.evidence.launch_ready_stdout_prefix_sha256
        Assert-Hash -Name 'engine-resolution.json' `
            -Expected $handoff.evidence.engine_resolution_sha256
    }
    $journalPath = Join-Path $resolved.path 'journal.jsonl'
    if (-not (Test-Path -LiteralPath $journalPath -PathType Leaf)) {
        Add-Error 'journal.jsonl is absent'
    }
    else {
        try {
            $journal = @(Get-Content -LiteralPath $journalPath |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
                ForEach-Object { $_ | ConvertFrom-S115EvidenceJson })
            $expectedThroughputNames = @($context.plan.throughput.sequence |
                ForEach-Object { "throughput:$_" })
            $expectedJournalSequence = [System.Collections.Generic.List[string]]::new()
            foreach ($entry in @(
                'state:attempt_allocated',
                'observation:complete_e17_a',
                'command:single_ferric_server_up',
                'result:single_ferric_server_up',
                'command:single_external_trace_smoke',
                'result:single_external_trace_smoke'
            )) { $expectedJournalSequence.Add($entry) }
            foreach ($name in $expectedThroughputNames) {
                $expectedJournalSequence.Add("http_request:$name")
                $expectedJournalSequence.Add("http_result:$name")
            }
            $expectedJournalSequence.Add('terminal:qualified_running')
            $actualJournalSequence = @($journal | ForEach-Object {
                "$($_.kind):$($_.name)"
            })
            $journalPayloadPassed =
                [string]$journal[0].details.attempt -ceq $resolved.id -and
                (Test-JsonEquivalent -Left $journal[1].details -Right $preflight) -and
                (Test-JsonEquivalent -Left $journal[2].details -Right $launch) -and
                [string]$journal[3].details.result_sha256 -ceq
                    (Get-Sha256Lower -Path (Join-Path $resolved.path 'launch-result.json')) -and
                [int]$journal[3].details.exit_code -eq 0 -and
                -not [bool]$journal[3].details.timed_out -and
                [string]$journal[4].details.command_sha256 -ceq
                    (Get-Sha256Lower -Path (Join-Path $resolved.path 'smoke-command.json')) -and
                [string]$journal[5].details.smoke_sha256 -ceq
                    (Get-Sha256Lower -Path (Join-Path $resolved.path 'smoke.json')) -and
                [bool]$journal[5].details.passed
            for ($index = 0; $index -lt 4; $index++) {
                $requestJournal = $journal[6 + ($index * 2)]
                $resultJournal = $journal[7 + ($index * 2)]
                $sample = @($throughput.samples)[$index]
                if ([int]$requestJournal.details.ordinal -ne $index + 1 -or
                    [string]$requestJournal.details.request_sha256 -cne
                        (Get-Sha256Lower -Path $requestPath) -or
                    [int]$requestJournal.details.timeout_cap_seconds -ne
                        [int]$context.plan.policy.request_timeout_seconds -or
                    [int]$resultJournal.details.ordinal -ne $index + 1 -or
                    [string]$resultJournal.details.request_sha256 -cne
                        (Get-Sha256Lower -Path $requestPath) -or
                    [string]$resultJournal.details.response_sha256 -cne
                        [string]$sample.exchange.response_sha256 -or
                    -not [bool]$resultJournal.details.valid) {
                    $journalPayloadPassed = $false
                }
            }
            $terminalJournal = $journal[$journal.Count - 1]
            if ([UInt32]$terminalJournal.details.pid -ne [UInt32]$handoff.process.pid -or
                -not (Test-S115UtcInstantEquivalent `
                    -Left $terminalJournal.details.creation_utc `
                    -Right $handoff.process.creation_utc).passed) {
                $journalPayloadPassed = $false
            }
            if (($actualJournalSequence -join "`n") -cne
                    (@($expectedJournalSequence) -join "`n") -or
                -not $journalPayloadPassed) {
                Add-Error 'journal does not derive exact launch/smoke/throughput/no-qualification-retry history'
            }
        }
        catch { Add-Error "journal.jsonl is malformed: $($_.Exception.Message)" }
    }
    $retainedLiveGate = Read-Json -Name 'live-handoff-verification.json'
    if ($null -ne $retainedLiveGate) {
        $liveGateModelEntries = @($retainedLiveGate.endpoints.models.json.data)
        $liveGatePropertyDigest = if ($liveGateModelEntries.Count -eq 1) {
            Get-S115StablePropertyDigest `
                -Props $retainedLiveGate.endpoints.props.json `
                -ModelEntry $liveGateModelEntries[0]
        } else { $null }
        $firstGateListenersEquivalent = Test-JsonEquivalent `
            -Left @($retainedLiveGate.binding.listeners) `
            -Right @($finalBinding.listeners)
        $finalGateListenersEquivalent = Test-JsonEquivalent `
            -Left @($retainedLiveGate.final_binding.listeners) `
            -Right @($finalBinding.listeners)
        $firstGateCreationEquivalent = Test-S115UtcInstantEquivalent `
            -Left $retainedLiveGate.binding.process.creation_utc `
            -Right $handoff.process.creation_utc
        $finalGateCreationEquivalent = Test-S115UtcInstantEquivalent `
            -Left $retainedLiveGate.final_binding.process.creation_utc `
            -Right $handoff.process.creation_utc
        if ($retainedLiveGate.schema -cne
                'animus-ferric-s115-live-handoff-verification-v1' -or
            -not [bool]$retainedLiveGate.passed -or
            @($retainedLiveGate.errors).Count -ne 0 -or
            -not [bool]$retainedLiveGate.binding.passed -or
            -not [bool]$retainedLiveGate.final_binding.passed -or
            [UInt32]$retainedLiveGate.binding.process.pid -ne
                [UInt32]$handoff.process.pid -or
            [UInt32]$retainedLiveGate.final_binding.process.pid -ne
                [UInt32]$handoff.process.pid -or
            -not $firstGateCreationEquivalent.passed -or
            -not $finalGateCreationEquivalent.passed -or
            [string]$retainedLiveGate.binding.runfiles.local.sha256 -cne
                [string]$handoff.runfiles.local_sha256 -or
            [string]$retainedLiveGate.binding.runfiles.global.sha256 -cne
                [string]$handoff.runfiles.global_sha256 -or
            [string]$retainedLiveGate.final_binding.runfiles.local.sha256 -cne
                [string]$handoff.runfiles.local_sha256 -or
            [string]$retainedLiveGate.final_binding.runfiles.global.sha256 -cne
                [string]$handoff.runfiles.global_sha256 -or
            -not $firstGateListenersEquivalent -or -not $finalGateListenersEquivalent -or
            -not (Test-JsonEquivalent -Left $retainedLiveGate.runtime_identity `
                -Right $finalIdentity) -or
            [int]$retainedLiveGate.endpoints.health.status_code -ne 200 -or
            [string]$retainedLiveGate.endpoints.health.json.status -cne 'ok' -or
            [int]$retainedLiveGate.endpoints.models.status_code -ne 200 -or
            [int]$retainedLiveGate.endpoints.props.status_code -ne 200 -or
            $liveGateModelEntries.Count -ne 1 -or
            [string]$liveGateModelEntries[0].id -cne $context.model_path -or
            $null -eq $liveGatePropertyDigest -or
            [string]$liveGatePropertyDigest.sha256 -cne
                [string]$handoff.property_digest_sha256 -or
            -not [bool]$retainedLiveGate.server_log_facts.passed -or
            [string]$retainedLiveGate.server_log_facts.sha256 -cne
                [string]$handoff.server_log_facts_sha256 -or
            -not [bool]$retainedLiveGate.server_log_prefix.passed -or
            [UInt64]$retainedLiveGate.server_log_prefix.expected_bytes -ne
                [UInt64]$handoff.server_log_prefix.bytes -or
            [string]$retainedLiveGate.server_log_prefix.expected_sha256 -cne
                [string]$handoff.server_log_prefix.sha256) {
            Add-Error 'qualification-time live handoff gate is not fully cross-linked'
        }
    }
    $result = Read-Json -Name 'result.json'
    if ($null -ne $result -and
        (-not [bool]$result.passed -or $result.state -cne 'qualified_running' -or
        $result.attempt -cne $resolved.id -or [int]$result.launch_count -ne 1 -or
        [double]$result.wall_seconds -le 0 -or
        [double]$result.wall_seconds -gt
            [double]$context.plan.policy.attempt_wall_seconds -or
        [int]$result.smoke_invocation_count -ne 1 -or
        [int]$result.throughput_request_count -ne 4 -or
        [int]$result.replacement_request_count -ne 0 -or
        [string]$result.handoff_sha256 -cne
            (Get-Sha256Lower -Path (Join-Path $resolved.path 'handoff.json')))) {
        Add-Error 'terminal result is not the successful one-launch handoff result'
    }

    $liveResult = $null
    if ($Live -and $errors.Count -eq 0) {
        $liveResult = Test-S115LiveHandoff -Context $context -Handoff $handoff
        if (-not $liveResult.passed) {
            Add-Error "live verification failed: $($liveResult.errors -join '; ')"
        }
    }
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-runtime-verification-v1'
        passed = $errors.Count -eq 0
        attempt = $resolved.id
        mode = if ($Live) { 'offline-plus-live' } else { 'offline' }
        manifest = $manifest
        control_binding = [ordered]@{
            attempt_source_manifest_sha256 = $attemptSourceManifestSha256
            attempt_runtime_plan_sha256 = $attemptRuntimePlanSha256
            compatibility = $attemptControlCompatibility
        }
        live = $liveResult
        errors = @($errors)
    }
}

if ($MyInvocation.InvocationName -cne '.') {
    try {
        $verification = Invoke-S115RuntimeVerification `
            -AttemptId $Attempt -Live:$CheckLive
        $verification | ConvertTo-Json -Depth 64
        if (-not $verification.passed) { exit 1 }
    }
    catch {
        [pscustomobject]@{
            schema = 'animus-ferric-s115-runtime-verification-v1'
            passed = $false
            attempt = $Attempt
            errors = @($_.Exception.ToString())
        } | ConvertTo-Json -Depth 16
        exit 1
    }
}
