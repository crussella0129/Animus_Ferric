[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'runtime-common.ps1')

function Invoke-S115RuntimeQualification {
    [CmdletBinding()]
    param()

    $context = Get-S115Context
    $control = Assert-S115ControlInputs -Context $context
    $lock = $null
    $tracked = $null
    $raw = $null
    $attemptId = $null
    $ownedCreationUtc = $null
    $phase = 'control'
    $started = [DateTimeOffset]::UtcNow
    $wall = [System.Diagnostics.Stopwatch]::StartNew()

    function Get-RemainingMilliseconds {
        [int][Math]::Max(0, [Math]::Floor(
            ([double]$context.plan.policy.attempt_wall_seconds -
                $wall.Elapsed.TotalSeconds) * 1000.0
        ))
    }
    function Add-AttemptJournal {
        param(
            [Parameter(Mandatory = $true)][string]$Kind,
            [Parameter(Mandatory = $true)][string]$Name,
            [AllowNull()]$Details
        )
        if ($null -eq $tracked) { return }
        $entry = [ordered]@{
            at_utc = [DateTimeOffset]::UtcNow.ToString('o')
            elapsed_seconds = [Math]::Round($wall.Elapsed.TotalSeconds, 6)
            kind = $Kind
            name = $Name
            details = $Details
        }
        [System.IO.File]::AppendAllText(
            (Join-Path $tracked 'journal.jsonl'),
            (($entry | ConvertTo-Json -Depth 64 -Compress) + "`n"),
            [System.Text.UTF8Encoding]::new($false)
        )
    }
    function Assert-ExchangeSucceeded {
        param(
            [Parameter(Mandatory = $true)]$Exchange,
            [Parameter(Mandatory = $true)][string]$Label
        )
        if ($null -ne $Exchange.error -or $Exchange.status_code -ne 200) {
            throw "$Label did not return one successful HTTP 200 response"
        }
    }
    function Get-ModelInventory {
        $root = Join-Path $context.repository_root 'models'
        @(
            Get-ChildItem -LiteralPath $root -Recurse -File -Force |
                ForEach-Object {
                    [ordered]@{
                        path = Get-RelativeSlashPath -Root $context.repository_root `
                            -Path $_.FullName
                        bytes = [UInt64]$_.Length
                        last_write_utc = $_.LastWriteTimeUtc.ToString('o')
                    }
                } | Sort-Object { $_.path }
        )
    }
    function Get-StreamPrefixObservation {
        param([Parameter(Mandatory = $true)][string]$Path)
        $item = Get-Item -LiteralPath $Path
        $snapshot = if ([UInt64]$item.Length -eq 0) {
            [ordered]@{
                bytes = [UInt64]0
                sha256 = Get-Sha256Text -Text ''
            }
        }
        else { Get-S115StableFilePrefixSnapshot -Path $Path }
        [ordered]@{
            raw_relative_path = Get-RelativeSlashPath `
                -Root $context.repository_root -Path $Path
            bytes = [UInt64]$snapshot.bytes
            sha256 = [string]$snapshot.sha256
        }
    }

    try {
        $phase = 'allocation'
        $lockParent = Split-Path -Parent $context.lock_path
        $null = New-S115SafeDirectory -Root $context.repository_root `
            -Path $lockParent
        $lock = Open-S115RuntimeLock -Path $context.lock_path
        $null = New-S115SafeDirectory -Root $context.repository_root `
            -Path $context.tracked_attempt_root
        $null = New-S115SafeDirectory -Root $context.repository_root `
            -Path $context.raw_attempt_root
        $attemptId = Get-S115NextAttemptId `
            -TrackedRoot $context.tracked_attempt_root `
            -RawRoot $context.raw_attempt_root
        $tracked = Join-Path $context.tracked_attempt_root $attemptId
        $raw = Join-Path $context.raw_attempt_root $attemptId
        $null = New-S115SafeDirectory -Root $context.repository_root -Path $tracked
        $null = New-S115SafeDirectory -Root $context.repository_root -Path $raw
        $frozenControl = Join-Path $tracked 'control'
        $null = New-S115SafeDirectory -Root $context.repository_root `
            -Path $frozenControl
        $controlRecords = [System.Collections.Generic.List[object]]::new()
        foreach ($line in Get-Content -LiteralPath (Join-Path `
            $context.artifact_directory 'control-inputs.sha256')) {
            if ([string]::IsNullOrWhiteSpace($line)) { continue }
            if ($line -notmatch '^([0-9a-f]{64})  ([^/\\]+)$') {
                throw 'control manifest changed after verification'
            }
            $expectedHash = [string]$Matches[1]
            $controlName = [string]$Matches[2]
            $source = Join-Path $context.artifact_directory $controlName
            $destination = Join-Path $frozenControl $controlName
            Copy-Item -LiteralPath $source -Destination $destination
            $copiedHash = Get-Sha256Lower -Path $destination
            if ($copiedHash -cne $expectedHash) {
                throw "control changed during attempt freeze: $controlName"
            }
            $controlRecords.Add([ordered]@{
                name = $controlName
                bytes = [UInt64](Get-Item -LiteralPath $destination).Length
                sha256 = $copiedHash
            })
        }
        Copy-Item -LiteralPath (Join-Path $context.artifact_directory `
            'control-inputs.sha256') -Destination (Join-Path $frozenControl `
            'control-inputs.sha256')
        $frozenManifestHash = Get-Sha256Lower -Path (Join-Path `
            $frozenControl 'control-inputs.sha256')
        if ($frozenManifestHash -cne $control.manifest_sha256) {
            throw 'control manifest changed during attempt freeze'
        }
        $controlProvenance = [ordered]@{
            schema = 'animus-ferric-s115-attempt-control-provenance-v1'
            source_manifest_sha256 = $control.manifest_sha256
            frozen_manifest_sha256 = $frozenManifestHash
            files = @($controlRecords)
        }
        Write-JsonLf -Path (Join-Path $tracked 'control-provenance.json') `
            -Value $controlProvenance
        Write-JsonLf -Path (Join-Path $tracked 'attempt-start.json') -Value ([ordered]@{
            schema = 'animus-ferric-s115-runtime-attempt-start-v1'
            attempt = $attemptId
            started_at_utc = $started.ToString('o')
            control_manifest_sha256 = $control.manifest_sha256
            runtime_plan_sha256 = Get-Sha256Lower -Path $context.plan_path
            policy = [ordered]@{
                attempt_wall_seconds = [int]$context.plan.policy.attempt_wall_seconds
                no_qualification_attempt_retry =
                    [bool]$context.plan.policy.no_qualification_attempt_retry
                provider_retry_policy = [string]$context.plan.policy.provider_retry_policy
                no_fallback = [bool]$context.plan.policy.no_fallback
                no_download = [bool]$context.plan.policy.no_download
            }
        })
        Add-AttemptJournal -Kind 'state' -Name 'attempt_allocated' `
            -Details ([ordered]@{ attempt = $attemptId })

        $phase = 'preflight'
        $hostSnapshot = Get-S115HostSnapshot -Context $context
        $isolation = Invoke-S115WslIsolationProbe -Context $context
        $inheritedRuntimeEnvironment = @(
            Get-ChildItem Env: | Where-Object {
                $_.Name -like 'LLAMA_ARG_*' -or
                $_.Name -like 'FERRIC_*' -or $_.Name -like 'GGML_*' -or
                $_.Name -like 'CUDA_*' -or $_.Name -like 'OMP_*' -or
                $_.Name -like 'MKL_*' -or $_.Name -ieq 'OPENAI_API_KEY' -or
                $_.Name -iin @('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY')
            } | Select-Object -ExpandProperty Name | Sort-Object
        )
        $preflight = [ordered]@{
            schema = 'animus-ferric-s115-e17-a-v1'
            passed = $false
            host = $hostSnapshot
            isolation = $isolation
            inherited_forbidden_environment_names = $inheritedRuntimeEnvironment
            frozen_targets = [ordered]@{
                release_result = [ordered]@{
                    path = [string]$context.plan.qualified_release.result_path
                    sha256 = [string]$context.plan.qualified_release.result_sha256
                }
                ferric = [ordered]@{
                    path = [string]$context.plan.qualified_release.binary_path
                    bytes = [UInt64]$context.plan.qualified_release.binary_bytes
                    sha256 = [string]$context.plan.qualified_release.binary_sha256
                }
                model = $context.plan.model
                engine = $context.plan.engine
            }
            enforced_after_complete_capture = $true
            errors = @()
        }
        $preflightErrors = [System.Collections.Generic.List[string]]::new()
        if ($hostSnapshot.runfiles.local.present -or
            $hostSnapshot.runfiles.global.present) {
            $preflightErrors.Add('cold start requires both Ferric runfiles absent')
        }
        if (@($hostSnapshot.qualified_or_runfile_owned_processes).Count -ne 0) {
            $preflightErrors.Add('cold start requires no qualified-image/runfile PID process')
        }
        if (@($hostSnapshot.relevant_listeners).Count -ne 0) {
            $preflightErrors.Add('cold start requires no relevant listener')
        }
        if ([UInt64]$hostSnapshot.gpu.free_mib -lt
            [UInt64]$context.plan.policy.minimum_gpu_free_mib) {
            $preflightErrors.Add('GPU free memory is below the frozen launch floor')
        }
        if (-not $isolation.passed) {
            $preflightErrors.Add('Ubuntu WSL2 Bubblewrap network isolation probe failed')
        }
        if ($inheritedRuntimeEnvironment.Count -ne 0) {
            $preflightErrors.Add("forbidden inherited runtime environment: $($inheritedRuntimeEnvironment -join ', ')")
        }
        $preflight.passed = $preflightErrors.Count -eq 0
        $preflight.errors = @($preflightErrors)
        Write-JsonLf -Path (Join-Path $tracked 'preflight.json') -Value $preflight
        Add-AttemptJournal -Kind 'observation' -Name 'complete_e17_a' `
            -Details $preflight
        if (-not $preflight.passed) {
            throw "preflight rejected the attempt: $($preflight.errors -join '; ')"
        }

        $phase = 'identity'
        $identity = Get-S115RuntimeIdentity -Context $context
        $ferricVersion = Invoke-BoundedProcessResult -FilePath $context.ferric_path `
            -Arguments @('--version') -TimeoutMilliseconds 30000
        $engineVersion = Invoke-BoundedProcessResult -FilePath $context.engine_path `
            -Arguments @('--version') -TimeoutMilliseconds 30000
        $engineDevices = Invoke-BoundedProcessResult -FilePath $context.engine_path `
            -Arguments @('--list-devices') -TimeoutMilliseconds 30000
        $deviceLines = @(
            ([string]$engineDevices.stdout).Replace("`r", '').Split("`n") |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
        )
        $device = if (-not $engineDevices.timed_out -and
            $engineDevices.exit_code -eq 0) {
            Get-LlamaDeviceObservation -Output $deviceLines
        } else { $null }
        $toolIdentityPassed =
            -not $ferricVersion.timed_out -and $ferricVersion.exit_code -eq 0 -and
            ([string]$ferricVersion.stdout).Trim() -ceq
                [string]$context.plan.qualified_release.version -and
            -not $engineVersion.timed_out -and $engineVersion.exit_code -eq 0 -and
            -not $engineDevices.timed_out -and $engineDevices.exit_code -eq 0 -and
            $null -ne $device -and
            [UInt64]$device.free_mib -ge
                [UInt64]$context.plan.policy.minimum_gpu_free_mib
        $identityEvidence = [ordered]@{
            schema = 'animus-ferric-s115-runtime-identity-v1'
            passed = $toolIdentityPassed
            files = $identity
            ferric_version = $ferricVersion
            engine_version = $engineVersion
            engine_devices = $engineDevices
            parsed_device = $device
        }
        Write-JsonLf -Path (Join-Path $tracked 'runtime-identity.json') `
            -Value $identityEvidence
        if (-not $toolIdentityPassed) {
            throw 'runtime tool version/device identity did not match the frozen contract'
        }
        $modelInventoryBefore = @(Get-ModelInventory)
        Write-JsonLf -Path (Join-Path $tracked 'model-inventory.before.json') `
            -Value $modelInventoryBefore

        $phase = 'launch'
        $serverLog = Join-Path $raw ([string]$context.plan.evidence.raw_server_log)
        $launchStdout = Join-Path $raw 'launch-live.stdout.log'
        $launchStderr = Join-Path $raw 'launch-live.stderr.log'
        $rawSafety = Test-S115SafeDirectoryTraversal -Root $context.repository_root `
            -Path $raw -RequireTarget
        $existingLiveLeaves = @(@($serverLog, $launchStdout, $launchStderr) |
            Where-Object {
                $null -ne (Get-Item -LiteralPath $_ -Force -ErrorAction SilentlyContinue)
            })
        if (-not $rawSafety.passed -or $existingLiveLeaves.Count -ne 0) {
            throw 'raw launch directory is unsafe or a live-output leaf already exists'
        }
        $launchEnvironment = @{
            Path = "$($context.engine_root);$([string]$env:Path)"
            LLAMA_ARG_LOG_FILE = $serverLog
        }
        foreach ($property in $context.plan.launch_environment.PSObject.Properties) {
            $launchEnvironment[$property.Name] = [string]$property.Value
        }
        $engineResolutionProof = Get-S115BareEngineResolutionProof `
            -Context $context -LaunchPath ([string]$launchEnvironment.Path)
        $engineResolution = [ordered]@{
            schema = 'animus-ferric-s115-start-process-engine-resolution-v1'
            passed = [bool]$engineResolutionProof.passed
            path_injection_strategy =
                'parent-process-scoped-inheritance-restored-no-Start-Process-Path-override'
            effective_path = [string]$launchEnvironment.Path
            proof = $engineResolutionProof
        }
        Write-JsonLf -Path (Join-Path $tracked 'engine-resolution.json') `
            -Value $engineResolution
        if (-not $engineResolution.passed) {
            throw "bare llama-server resolution is unsafe: $($engineResolution.proof.errors -join '; ')"
        }
        $launchArguments = @(
            'server', 'up',
            '--engine', 'llama-server',
            '--model', $context.model_path,
            '--ctx', [string]$context.plan.coordinate.context,
            '--threads', [string]$context.plan.coordinate.threads,
            '--gpu-layers', [string]$context.plan.coordinate.gpu_layers,
            '--batch-size', [string]$context.plan.coordinate.batch_size,
            '--seed', [string]$context.plan.coordinate.seed,
            '--parallel', [string]$context.plan.coordinate.parallel_slots,
            '--port', [string]$context.plan.coordinate.port
        )
        $launchDeclaration = [ordered]@{
            schema = 'animus-ferric-s115-single-launch-v1'
            launch_ordinal = 1
            executable = $context.ferric_path
            executable_sha256 = [string]$context.plan.qualified_release.binary_sha256
            arguments = $launchArguments
            working_directory = $context.repository_root
            environment = $launchEnvironment
            expected_child_argv = @(Get-S115ExpectedChildArgv -Context $context)
        }
        Write-JsonLf -Path (Join-Path $tracked 'launch-command.json') `
            -Value $launchDeclaration
        Add-AttemptJournal -Kind 'command' -Name 'single_ferric_server_up' `
            -Details $launchDeclaration
        $startProcessEnvironment = [hashtable]$launchEnvironment.Clone()
        $null = $startProcessEnvironment.Remove('Path')
        $originalProcessPath = [string]$env:Path
        try {
            $env:Path = [string]$launchEnvironment.Path
            $launchResult = Invoke-FileRedirectedProcess `
                -FilePath $context.ferric_path -Arguments $launchArguments `
                -WorkingDirectory $context.repository_root `
                -StdoutPath $launchStdout -StderrPath $launchStderr `
                -TimeoutMilliseconds ([Math]::Min(
                    [int]$context.plan.policy.startup_timeout_seconds * 1000,
                    (Get-RemainingMilliseconds)
                )) -Environment $startProcessEnvironment
        }
        finally {
            $env:Path = $originalProcessPath
        }
        Write-JsonLf -Path (Join-Path $tracked 'launch-result.json') `
            -Value $launchResult
        Add-AttemptJournal -Kind 'result' -Name 'single_ferric_server_up' `
            -Details ([ordered]@{
                result_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'launch-result.json')
                exit_code = $launchResult.exit_code
                timed_out = [bool]$launchResult.timed_out
            })
        Start-Sleep -Milliseconds 750
        $binding = Get-S115LiveBinding -Context $context `
            -PreflightCapturedUtc ([string]$launchResult.started_at_utc)
        Write-JsonLf -Path (Join-Path $tracked 'post-launch-binding.json') `
            -Value $binding
        if ($binding.passed -and $null -ne $binding.process) {
            $ownedCreationUtc = [string]$binding.process.creation_utc
        }
        $launchCreationBinding = if ($null -ne $binding.process) {
            Test-ProcessCreationWindow `
                -CreationDateUtc ([string]$binding.process.creation_utc) `
                -PreflightCapturedUtc ([string]$launchResult.started_at_utc) `
                -AttestationCapturedUtc ([string]$launchResult.completed_at_utc) `
                -ToleranceSeconds ([int]$context.plan.policy.creation_time_tolerance_seconds)
        } else { $null }
        $readyPrefix = if ((Test-Path -LiteralPath $launchStdout -PathType Leaf) -and
            [UInt64](Get-Item -LiteralPath $launchStdout).Length -gt 0) {
            Get-S115StableFilePrefixSnapshot -Path $launchStdout
        } else { [ordered]@{ bytes = [UInt64]0; sha256 = Get-Sha256Text -Text '' } }
        $readyText = if ([UInt64]$readyPrefix.bytes -gt 0) {
            Read-S115Utf8FilePrefix -Path $launchStdout `
                -Bytes ([UInt64]$readyPrefix.bytes)
        } else { '' }
        $readySnapshotName = 'launch-ready.stdout.prefix.bin'
        $readySnapshotPath = Join-Path $tracked $readySnapshotName
        [System.IO.File]::WriteAllBytes(
            $readySnapshotPath,
            $script:S115Utf8NoBom.GetBytes($readyText)
        )
        if ([UInt64](Get-Item -LiteralPath $readySnapshotPath).Length -ne
                [UInt64]$readyPrefix.bytes -or
            (Get-Sha256Lower -Path $readySnapshotPath) -cne
                [string]$readyPrefix.sha256) {
            throw 'tracked ready stdout prefix is not byte-identical to the live stream'
        }
        $readyMatches = [regex]::Matches(
            $readyText,
            'server ready:\s+http://127\.0\.0\.1:8080/v1\s+\(pid\s+(\d+)\)',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        $readyPid = if ($readyMatches.Count -eq 1) {
            [UInt32]$readyMatches[0].Groups[1].Value
        } else { $null }
        $launchProvenancePassed = $binding.passed -and
            $null -ne $binding.process -and $null -ne $launchCreationBinding -and
            $launchCreationBinding.passed -and
            [UInt32]$binding.process.parent_pid -eq [UInt32]$launchResult.pid -and
            $readyMatches.Count -eq 1 -and
            [UInt32]$readyPid -eq [UInt32]$binding.process.pid
        $launchProvenance = [ordered]@{
            schema = 'animus-ferric-s115-launch-provenance-v1'
            passed = $launchProvenancePassed
            wrapper_pid = [UInt32]$launchResult.pid
            child_pid = if ($null -ne $binding.process) {
                [UInt32]$binding.process.pid
            } else { $null }
            child_parent_pid = if ($null -ne $binding.process) {
                [UInt32]$binding.process.parent_pid
            } else { $null }
            creation_binding = $launchCreationBinding
            ready_stdout_prefix = $readyPrefix
            ready_stdout_prefix_file = $readySnapshotName
            ready_line = if ($readyMatches.Count -eq 1) {
                $readyMatches[0].Value
            } else { $null }
            ready_pid = $readyPid
        }
        Write-JsonLf -Path (Join-Path $tracked 'launch-provenance.json') `
            -Value $launchProvenance
        if ($launchResult.timed_out -or $launchResult.exit_code -ne 0 -or
            -not $launchProvenancePassed) {
            throw 'the single Ferric launch did not produce a strongly bound server'
        }
        $serverLogRelative = Get-RelativeSlashPath `
            -Root $context.repository_root -Path $serverLog
        $serverLogSafety = Test-S115PathComponentsAreNotReparsePoints `
            -Root $context.repository_root -RelativePath $serverLogRelative
        if (-not $serverLogSafety.passed -or
            [string]$serverLogSafety.full_path -cne
                [System.IO.Path]::GetFullPath($serverLog)) {
            throw "live server log is unsafe: $($serverLogSafety.errors -join '; ')"
        }

        $phase = 'attestation'
        $base = "http://127.0.0.1:$($context.plan.coordinate.port)"
        $health = Invoke-HttpExchange -Method GET -Uri "$base/health" `
            -ResponsePath (Join-Path $tracked 'health.body.json') -TimeoutSeconds 30
        $models = Invoke-HttpExchange -Method GET -Uri "$base/v1/models" `
            -ResponsePath (Join-Path $tracked 'models.body.json') -TimeoutSeconds 30
        $props = Invoke-HttpExchange -Method GET -Uri "$base/props" `
            -ResponsePath (Join-Path $tracked 'props.body.json') -TimeoutSeconds 30
        foreach ($pair in @(
            @($health, 'GET /health'), @($models, 'GET /v1/models'),
            @($props, 'GET /props')
        )) { Assert-ExchangeSucceeded -Exchange $pair[0] -Label $pair[1] }
        $endpointExchanges = [ordered]@{
            health = $health
            models = $models
            props = $props
        }
        Write-JsonLf -Path (Join-Path $tracked 'endpoint-exchanges.json') `
            -Value $endpointExchanges
        $healthJson = Get-Content -Raw -LiteralPath (
            Join-Path $tracked 'health.body.json') | ConvertFrom-Json
        $modelsJson = Get-Content -Raw -LiteralPath (
            Join-Path $tracked 'models.body.json') | ConvertFrom-Json
        $propsJson = Get-Content -Raw -LiteralPath (
            Join-Path $tracked 'props.body.json') | ConvertFrom-Json
        $entries = @($modelsJson.data)
        if ($entries.Count -ne 1) { throw 'models endpoint did not return exactly one model' }
        $servedModelId = [string]$entries[0].id
        $servedModelPath = [System.IO.Path]::GetFullPath($servedModelId)
        if (-not $servedModelPath.Equals(
                [System.IO.Path]::GetFullPath($context.model_path),
                [System.StringComparison]::OrdinalIgnoreCase
            )) { throw 'served model id is not the frozen model path' }
        $propertyDigest = Get-S115StablePropertyDigest -Props $propsJson `
            -ModelEntry $entries[0]
        $servedNParams = [UInt64]$propertyDigest.value.served_n_params
        if ([string]$healthJson.status -cne 'ok' -or
            $servedNParams -ne [UInt64]$context.plan.model.parameters -or
            [string]$propertyDigest.value.served_ftype -cne
                [string]$context.plan.model.expected_served_ftype -or
            [int64]$propertyDigest.value.context -ne
                [int64]$context.plan.coordinate.context -or
            [int]$propertyDigest.value.seed -ne
                [int]$context.plan.coordinate.seed -or
            [int]$propertyDigest.value.total_slots -ne
                [int]$context.plan.coordinate.parallel_slots -or
            [string]$propertyDigest.value.chat_template_sha256 -cne
                [string]$context.plan.template_attestation.expected_chat_template_sha256 -or
            $propertyDigest.value.supports_preserve_reasoning -ne $true -or
            [string]$propsJson.build_info -cne
                "$($context.plan.engine.release)-$($context.plan.engine.commit)") {
            throw 'health/model/property values differ from the frozen contract'
        }

        $templateExchanges = [System.Collections.Generic.List[object]]::new()
        foreach ($arm in @($context.plan.template_attestation.arms)) {
            $requestName = "template-probe.$($arm.name).request.json"
            $responseName = "template-probe.$($arm.name).response.json"
            $requestPath = Join-Path $tracked $requestName
            Copy-Item -LiteralPath (Join-Path $frozenControl `
                ([string]$arm.request_file)) -Destination $requestPath
            $exchange = Invoke-HttpExchange -Method POST `
                -Uri "$base$($context.plan.template_attestation.endpoint)" `
                -RequestBodyPath $requestPath `
                -ResponsePath (Join-Path $tracked $responseName) -TimeoutSeconds 30
            Assert-ExchangeSucceeded -Exchange $exchange `
                -Label "template probe $($arm.name)"
            $templateExchanges.Add([ordered]@{
                name = [string]$arm.name
                exchange = $exchange
            })
        }
        $templateFacts = Get-TemplateProbeFacts -Plan $context.plan `
            -ArtifactDirectory $frozenControl `
            -EvidenceDirectory $tracked
        Write-JsonLf -Path (Join-Path $tracked 'template-attestation.json') `
            -Value ([ordered]@{
                passed = $templateFacts.passed
                exchanges = @($templateExchanges)
                facts = $templateFacts
            })
        if (-not $templateFacts.passed) {
            throw 'four-arm template differential failed'
        }

        $logPrefix = Get-S115StableFilePrefixSnapshot -Path $serverLog
        $serverText = Read-S115Utf8FilePrefix -Path $serverLog `
            -Bytes ([UInt64]$logPrefix.bytes)
        $logFacts = Get-S115ServerLogFacts -Text $serverText -Context $context
        $logPassed = [bool]$logFacts.passed
        $logAttestation = [ordered]@{
            schema = 'animus-ferric-s115-server-log-attestation-v1'
            passed = $logPassed
            raw_relative_path = Get-RelativeSlashPath `
                -Root $context.repository_root -Path $serverLog
            prefix = $logPrefix
            facts = $logFacts
            effective_gpu_layers = $logFacts.value.effective_gpu_layers
            total_layers = $logFacts.value.total_layers
            offload_line = $logFacts.value.offload_line
            kv_cache_lines = @($logFacts.value.kv_cache_lines)
            flash_attention_lines = @($logFacts.value.flash_attention_lines)
            thinking_lines = @($logFacts.value.thinking_lines)
            preserve_warning_count = $logFacts.value.preserve_warning_count
        }
        Write-JsonLf -Path (Join-Path $tracked 'server-log-attestation.json') `
            -Value $logAttestation
        if (-not $logPassed) {
            throw 'effective GPU/KV/flash/reasoning log attestation failed'
        }

        $postLoadHost = Get-S115HostSnapshot -Context $context
        Write-JsonLf -Path (Join-Path $tracked 'post-load-host.json') `
            -Value $postLoadHost

        $attestation = [ordered]@{
            schema = 'animus-ferric-s115-managed-server-attestation-v1'
            passed = $true
            binding = $binding
            endpoint_exchanges = $endpointExchanges
            served_model_id = $servedModelId
            served_n_params = $servedNParams
            stable_properties = $propertyDigest
            build_info = [string]$propsJson.build_info
            post_load_host_sha256 = Get-Sha256Lower -Path (
                Join-Path $tracked 'post-load-host.json')
            effective = [ordered]@{
                context = [int64]$propertyDigest.value.context
                seed = [int]$propertyDigest.value.seed
                quant = [string]$propertyDigest.value.served_ftype
                parallel_slots = [int]$propertyDigest.value.total_slots
                gpu_layers = [int]$logAttestation.effective_gpu_layers
                cache_type_k = 'q8_0'
                cache_type_v = 'q8_0'
                flash_attention = 'enabled'
                reasoning = 'enabled'
                preserve_reasoning = [bool]$templateFacts.differential.preserve_thinking_default_effective
            }
            template_attestation_sha256 = Get-Sha256Lower -Path (
                Join-Path $tracked 'template-attestation.json')
            server_log_attestation_sha256 = Get-Sha256Lower -Path (
                Join-Path $tracked 'server-log-attestation.json')
            server_log_facts_sha256 = [string]$logFacts.sha256
        }
        Write-JsonLf -Path (Join-Path $tracked 'server-attestation.json') `
            -Value $attestation

        $phase = 'smoke'
        $workspace = Join-Path $raw 'smoke-workspace'
        $traceRoot = Join-Path $raw 'external-trace'
        $profileRoot = Join-Path $raw 'empty-profile'
        foreach ($directory in @($workspace, $profileRoot)) {
            $null = New-S115SafeDirectory -Root $context.repository_root `
                -Path $directory
        }
        if ($null -ne (Get-Item -LiteralPath $traceRoot -Force `
            -ErrorAction SilentlyContinue)) {
            throw 'external trace root must be absent before Ferric creates it'
        }
        Copy-Item -LiteralPath (Join-Path $frozenControl `
            ([string]$context.plan.smoke.nonce_file)) `
            -Destination (Join-Path $workspace 'nonce.txt')
        $before = Get-TreeManifest -Root $workspace
        Write-JsonLf -Path (Join-Path $tracked 'smoke-workspace.before.json') `
            -Value $before
        $prompt = (Get-Content -Raw -LiteralPath (Join-Path `
            $frozenControl ([string]$context.plan.smoke.prompt_file)
        )).TrimEnd("`r", "`n")
        $smokeArguments = @(
            'query', '--workspace', $workspace, '--trace-dir', $traceRoot,
            '--model', $servedModelId, '--api-base', "$base/v1",
            '--params-b', '27', '--quant', [string]$context.plan.coordinate.quant,
            '--family', 'qwen3.8', '--ctx', [string]$context.plan.coordinate.context,
            '--temperature', [string]$context.plan.smoke.temperature,
            '--protocol', [string]$context.plan.smoke.protocol,
            '--harness-policy', [string]$context.plan.smoke.harness_policy,
            '--tier', [string]$context.plan.smoke.tier,
            '--max-ring', [string]$context.plan.smoke.max_ring,
            '--max-turns', [string]$context.plan.smoke.max_turns,
            '--profile-dir', $profileRoot, '--no-config', '--no-stream', $prompt
        )
        Write-JsonLf -Path (Join-Path $tracked 'smoke-command.json') -Value ([ordered]@{
            schema = 'animus-ferric-s115-external-trace-smoke-command-v1'
            executable = $context.ferric_path
            arguments = $smokeArguments
            workspace = Get-RelativeSlashPath -Root $context.repository_root `
                -Path $workspace
            external_trace_root = Get-RelativeSlashPath `
                -Root $context.repository_root -Path $traceRoot
            provider_retry_policy = [string]$context.plan.policy.provider_retry_policy
            zero_underlying_http_retries_claimed = $false
            prompt_sha256 = Get-Sha256Lower -Path (Join-Path `
                $frozenControl ([string]$context.plan.smoke.prompt_file))
            nonce_sha256 = Get-Sha256Lower -Path (Join-Path `
                $frozenControl ([string]$context.plan.smoke.nonce_file))
        })
        Add-AttemptJournal -Kind 'command' -Name 'single_external_trace_smoke' `
            -Details ([ordered]@{
                command_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'smoke-command.json')
            })
        $smokeResult = Invoke-CapturedProcess -FilePath $context.ferric_path `
            -Arguments $smokeArguments -WorkingDirectory $context.repository_root `
            -StdoutPath (Join-Path $raw 'smoke.stdout.log') `
            -StderrPath (Join-Path $raw 'smoke.stderr.log') `
            -TimeoutMilliseconds ([Math]::Min(
                [int]$context.plan.policy.smoke_timeout_seconds * 1000,
                (Get-RemainingMilliseconds)
            ))
        $traceFiles = @(Get-ChildItem -LiteralPath $traceRoot -File `
            -Filter '*.jsonl' -Force -ErrorAction Stop)
        $traceFacts = $null
        $traceVerify = $null
        if ($traceFiles.Count -eq 1) {
            $retainedTrace = Join-Path $tracked 'smoke.trace.jsonl'
            Copy-Item -LiteralPath $traceFiles[0].FullName -Destination $retainedTrace
            $traceVerify = Invoke-CapturedProcess -FilePath $context.ferric_path `
                -Arguments @('trace', 'verify', $retainedTrace) `
                -WorkingDirectory $context.repository_root `
                -StdoutPath (Join-Path $tracked 'trace-verify.stdout.log') `
                -StderrPath (Join-Path $tracked 'trace-verify.stderr.log') `
                -TimeoutMilliseconds ([Math]::Min(60000,
                    (Get-RemainingMilliseconds)))
            $traceFacts = Get-TraceFacts -TracePath $retainedTrace `
                -ExpectedNonce ([string]$context.plan.smoke.expected_summary) `
                -ForbiddenTools @($context.plan.smoke.forbidden_tools)
        }
        $after = Get-TreeManifest -Root $workspace
        Write-JsonLf -Path (Join-Path $tracked 'smoke-workspace.after.json') `
            -Value $after
        $workspaceUnchanged = Test-ManifestEqual -Before $before -After $after
        $workspaceFerricAbsent = -not (Test-Path -LiteralPath `
            (Join-Path $workspace '.ferric'))
        $smokePassed = -not $smokeResult.timed_out -and
            $smokeResult.exit_code -eq 0 -and $traceFiles.Count -eq 1 -and
            $null -ne $traceVerify -and -not $traceVerify.timed_out -and
            $traceVerify.exit_code -eq 0 -and $null -ne $traceFacts -and
            $traceFacts.protocol -ceq 'constrained_json' -and
            $traceFacts.all_turns_json_schema_constrained -and
            $traceFacts.read_file_before_task_complete -and
            $traceFacts.exact_nonce_read_result_count -ge 1 -and
            $traceFacts.exact_task_complete_summary -and
            @($traceFacts.forbidden_tools_observed).Count -eq 0 -and
            $traceFacts.session_end_reason -ceq 'task_complete' -and
            $workspaceUnchanged -and $workspaceFerricAbsent
        $smoke = [ordered]@{
            schema = 'animus-ferric-s115-external-trace-smoke-v1'
            passed = $smokePassed
            process = $smokeResult
            trace_count = $traceFiles.Count
            trace_verify = $traceVerify
            trace_facts = $traceFacts
            workspace_unchanged = $workspaceUnchanged
            workspace_ferric_absent = $workspaceFerricAbsent
            before_manifest_sha256 = Get-Sha256Lower -Path (
                Join-Path $tracked 'smoke-workspace.before.json')
            after_manifest_sha256 = Get-Sha256Lower -Path (
                Join-Path $tracked 'smoke-workspace.after.json')
        }
        Write-JsonLf -Path (Join-Path $tracked 'smoke.json') -Value $smoke
        Add-AttemptJournal -Kind 'result' -Name 'single_external_trace_smoke' `
            -Details ([ordered]@{
                smoke_sha256 = Get-Sha256Lower -Path (Join-Path $tracked 'smoke.json')
                passed = [bool]$smoke.passed
            })
        if (-not $smokePassed) { throw 'external-trace grammar nonce smoke failed' }

        $phase = 'throughput'
        $templatePath = Join-Path $frozenControl `
            ([string]$context.plan.throughput.request_template)
        $templateText = Get-Content -Raw -LiteralPath $templatePath
        $escapedModel = $servedModelId | ConvertTo-Json -Compress
        $requestText = $templateText.Replace(
            '"__SERVED_MODEL_ID__"', $escapedModel)
        if ($requestText -ceq $templateText -or
            $requestText.Contains('__SERVED_MODEL_ID__')) {
            throw 'throughput model placeholder substitution failed'
        }
        $requestPath = Join-Path $tracked 'throughput-request.json'
        Write-Utf8Lf -Path $requestPath -Text $requestText
        $request = Get-Content -Raw -LiteralPath $requestPath | ConvertFrom-Json
        if ([int]$request.max_tokens -ne [int]$context.plan.throughput.max_tokens -or
            [int]$request.seed -ne [int]$context.plan.coordinate.seed -or
            $request.stream -ne $false -or [string]$request.model -cne $servedModelId) {
            throw 'throughput request fields drifted from the frozen template'
        }
        $requestHash = Get-Sha256Lower -Path $requestPath
        $samples = [System.Collections.Generic.List[object]]::new()
        $ordinal = 0
        foreach ($label in @($context.plan.throughput.sequence)) {
            $ordinal++
            $remaining = Get-RemainingMilliseconds
            if ($remaining -le 0) { throw 'attempt wall cap expired before throughput' }
            $responsePath = Join-Path $tracked `
                "throughput-$label.response.json"
            Add-AttemptJournal -Kind 'http_request' `
                -Name "throughput:$label" -Details ([ordered]@{
                    ordinal = $ordinal
                    request_sha256 = $requestHash
                    timeout_cap_seconds = [int]$context.plan.policy.request_timeout_seconds
                })
            $exchange = Invoke-HttpExchange -Method POST `
                -Uri "$base/v1/chat/completions" `
                -RequestBodyPath $requestPath -ResponsePath $responsePath `
                -TimeoutSeconds ([Math]::Max(1, [Math]::Min(
                    [int]$context.plan.policy.request_timeout_seconds,
                    [int][Math]::Floor($remaining / 1000.0)
                )))
            $response = if ($exchange.status_code -eq 200 -and
                $exchange.response_bytes -gt 0) {
                try { Get-Content -Raw -LiteralPath $responsePath | ConvertFrom-Json }
                catch { $null }
            } else { $null }
            $usage = Get-OptionalProperty -Value $response -Name 'usage'
            $timings = Get-OptionalProperty -Value $response -Name 'timings'
            $completion = Get-OptionalProperty -Value $usage -Name 'completion_tokens'
            $predicted = Get-OptionalProperty -Value $timings -Name 'predicted_n'
            $milliseconds = Get-OptionalProperty -Value $timings -Name 'predicted_ms'
            $reported = Get-OptionalProperty -Value $timings `
                -Name 'predicted_per_second'
            $computed = if ($null -ne $predicted -and $null -ne $milliseconds -and
                [double]$milliseconds -gt 0) {
                [double]$predicted / ([double]$milliseconds / 1000.0)
            } else { $null }
            $counterOkay = $null -ne $completion -and $null -ne $predicted -and
                [int]$completion -eq [int]$predicted
            $rateOkay = $null -ne $computed -and $null -ne $reported -and
                [Math]::Abs([double]$computed - [double]$reported) -le
                    [Math]::Max(0.01, [double]$computed * 0.01)
            $failure = if ($null -ne $exchange.error) { 'request_error' }
                elseif ($exchange.status_code -ne 200) { 'http_status' }
                elseif ($null -eq $response) { 'malformed_response' }
                elseif (-not $counterOkay) { 'counter_inconsistency' }
                elseif (-not $rateOkay) { 'rate_inconsistency' }
                elseif ([int]$predicted -lt
                    [int]$context.plan.throughput.minimum_decoded_tokens) {
                    'decoded_length_below_minimum'
                }
                elseif ([int]$predicted -gt
                    [int]$context.plan.throughput.max_tokens) {
                    'decoded_length_above_limit'
                } else { $null }
            $sampleRecord = [ordered]@{
                schema = 'animus-ferric-s115-throughput-sample-v1'
                ordinal = $ordinal
                label = [string]$label
                scored = [string]$label -cne 'warmup'
                request_sha256 = $requestHash
                exchange = $exchange
                usage_completion_tokens = $completion
                timings_predicted_n = $predicted
                timings_predicted_ms = $milliseconds
                timings_reported_per_second = $reported
                computed_decoded_tokens_per_second = $computed
                counter_consistent = $counterOkay
                rate_consistent = $rateOkay
                failure_cause = $failure
                valid = $null -eq $failure
            }
            $samples.Add($sampleRecord)
            Add-AttemptJournal -Kind 'http_result' `
                -Name "throughput:$label" -Details ([ordered]@{
                    ordinal = $ordinal
                    request_sha256 = $requestHash
                    response_sha256 = $exchange.response_sha256
                    valid = [bool]$sampleRecord.valid
                })
        }
        $trials = @($samples | Where-Object { $_.scored })
        $valid = @($samples | Where-Object { $_.valid })
        $validTrials = @($trials | Where-Object { $_.valid })
        $median = if ($validTrials.Count -eq 3) {
            Get-Median -Values ([double[]]@($validTrials |
                ForEach-Object { $_.computed_decoded_tokens_per_second }))
        } else { $null }
        $throughputPassed = $samples.Count -eq 4 -and $valid.Count -eq 4 -and
            $trials.Count -eq 3 -and $validTrials.Count -eq 3 -and
            $null -ne $median -and [double]$median -ge
                [double]$context.plan.throughput.minimum_median_decoded_tokens_per_second
        $throughput = [ordered]@{
            schema = 'animus-ferric-s115-throughput-summary-v1'
            passed = $throughputPassed
            request_sha256 = $requestHash
            template_sha256 = Get-Sha256Lower -Path $templatePath
            scheduled_samples = @($context.plan.throughput.sequence)
            replacement_samples = 0
            observed_samples = $samples.Count
            valid_request_count = $valid.Count
            valid_trial_count = $validTrials.Count
            median_decoded_tokens_per_second = $median
            minimum_required = [double]$context.plan.throughput.minimum_median_decoded_tokens_per_second
            samples = @($samples)
        }
        Write-JsonLf -Path (Join-Path $tracked 'throughput-summary.json') `
            -Value $throughput
        if (-not $throughputPassed) {
            throw 'one-warmup/three-trial throughput qualification failed'
        }

        $phase = 'handoff'
        $finalIdentity = Get-S115RuntimeIdentity -Context $context
        Write-JsonLf -Path (Join-Path $tracked 'runtime-identity.final.json') `
            -Value $finalIdentity
        if (-not (Test-JsonEquivalent -Left $identity -Right $finalIdentity)) {
            throw 'runtime/model identity changed during qualification'
        }
        $modelInventoryAfter = @(Get-ModelInventory)
        Write-JsonLf -Path (Join-Path $tracked 'model-inventory.after.json') `
            -Value $modelInventoryAfter
        if (-not (Test-JsonEquivalent -Left $modelInventoryBefore `
            -Right $modelInventoryAfter)) {
            throw 'models inventory changed during no-download qualification'
        }
        $finalBinding = Get-S115LiveBinding -Context $context `
            -ExpectedCreationUtc $ownedCreationUtc
        if (-not $finalBinding.passed -or
            [UInt32]$finalBinding.process.pid -ne [UInt32]$binding.process.pid) {
            throw 'the qualified process changed before handoff'
        }
        Write-JsonLf -Path (Join-Path $tracked 'final-binding.json') `
            -Value $finalBinding
        $finalPrefix = Get-S115StableFilePrefixSnapshot -Path $serverLog
        $finalServerText = Read-S115Utf8FilePrefix -Path $serverLog `
            -Bytes ([UInt64]$finalPrefix.bytes)
        $finalLogFacts = Get-S115ServerLogFacts -Text $finalServerText `
            -Context $context
        if (-not $finalLogFacts.passed) {
            throw 'final server-log prefix no longer proves the effective runtime facts'
        }
        $logAttestation.prefix = $finalPrefix
        $logAttestation.facts = $finalLogFacts
        $logAttestation.effective_gpu_layers =
            $finalLogFacts.value.effective_gpu_layers
        $logAttestation.total_layers = $finalLogFacts.value.total_layers
        $logAttestation.offload_line = $finalLogFacts.value.offload_line
        $logAttestation.kv_cache_lines = @($finalLogFacts.value.kv_cache_lines)
        $logAttestation.flash_attention_lines =
            @($finalLogFacts.value.flash_attention_lines)
        $logAttestation.thinking_lines = @($finalLogFacts.value.thinking_lines)
        $logAttestation.preserve_warning_count =
            $finalLogFacts.value.preserve_warning_count
        Write-JsonLf -Path (Join-Path $tracked 'server-log-attestation.json') `
            -Value $logAttestation
        $attestation.server_log_attestation_sha256 = Get-Sha256Lower -Path (
            Join-Path $tracked 'server-log-attestation.json')
        $attestation.server_log_facts_sha256 = [string]$finalLogFacts.sha256
        Write-JsonLf -Path (Join-Path $tracked 'server-attestation.json') `
            -Value $attestation
        $launchStreamPrefixes = [ordered]@{
            schema = 'animus-ferric-s115-live-launch-stream-prefixes-v1'
            stdout = Get-StreamPrefixObservation -Path $launchStdout
            stderr = Get-StreamPrefixObservation -Path $launchStderr
            ready_stdout_prefix = Test-S115StableFilePrefix -Path $launchStdout `
                -Bytes ([UInt64]$readyPrefix.bytes) `
                -Sha256 ([string]$readyPrefix.sha256)
        }
        Write-JsonLf -Path (Join-Path $tracked 'launch-stream-prefixes.json') `
            -Value $launchStreamPrefixes
        $handoff = [ordered]@{
            schema = 'animus-ferric-s115-runtime-handoff-v1'
            state = 'qualified_running'
            attempt = $attemptId
            qualified_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
            endpoint = "$base/v1"
            served_model_id = $servedModelId
            served_n_params = $servedNParams
            property_digest_sha256 = $propertyDigest.sha256
            server_log_facts_sha256 = [string]$finalLogFacts.sha256
            coordinate = $context.plan.coordinate
            counts = [ordered]@{
                launch = 1
                fallback = 0
                download = 0
                restart = 0
                smoke = 1
                throughput = 4
                throughput_replacement = 0
            }
            process = [ordered]@{
                pid = [UInt32]$finalBinding.process.pid
                creation_utc = [string]$finalBinding.process.creation_utc
                executable_path = [string]$finalBinding.process.executable_path
                executable_sha256 = [string]$context.plan.engine.binary_sha256
                command_line = [string]$finalBinding.process.command_line
                expected_argv = @(Get-S115ExpectedChildArgv -Context $context)
            }
            listeners = @($finalBinding.listeners)
            runfiles = [ordered]@{
                local_path = [string]$finalBinding.runfiles.local.path
                local_sha256 = [string]$finalBinding.runfiles.local.sha256
                global_path = [string]$finalBinding.runfiles.global.path
                global_sha256 = [string]$finalBinding.runfiles.global.sha256
            }
            identities = [ordered]@{
                ferric_sha256 = [string]$context.plan.qualified_release.binary_sha256
                model_sha256 = [string]$context.plan.model.sha256
                engine_sha256 = [string]$context.plan.engine.binary_sha256
                cuda_backend_sha256 = [string]$context.plan.engine.cuda_backend_sha256
                runtime_tree_manifest_sha256 = [string]$context.plan.engine.source_manifest_sha256
                release_result_sha256 = [string]$context.plan.qualified_release.result_sha256
                release_source_commit = [string]$context.plan.qualified_release.source_commit
                control_manifest_sha256 = $control.manifest_sha256
                runtime_plan_sha256 = Get-Sha256Lower -Path $context.plan_path
            }
            frozen_launch = [ordered]@{
                declaration_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'launch-command.json')
                environment = $launchEnvironment
            }
            evidence = [ordered]@{
                control_provenance_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'control-provenance.json')
                runtime_identity_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'runtime-identity.final.json')
                server_attestation_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'server-attestation.json')
                template_attestation_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'template-attestation.json')
                smoke_sha256 = Get-Sha256Lower -Path (Join-Path $tracked 'smoke.json')
                trace_sha256 = Get-Sha256Lower -Path (Join-Path $tracked 'smoke.trace.jsonl')
                throughput_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'throughput-summary.json')
                model_inventory_before_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'model-inventory.before.json')
                model_inventory_after_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'model-inventory.after.json')
                launch_stream_prefixes_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'launch-stream-prefixes.json')
                final_binding_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'final-binding.json')
                launch_provenance_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'launch-provenance.json')
                launch_ready_stdout_prefix_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked $readySnapshotName)
                engine_resolution_sha256 = Get-Sha256Lower -Path (
                    Join-Path $tracked 'engine-resolution.json')
            }
            server_log_prefix = [ordered]@{
                raw_relative_path = Get-RelativeSlashPath `
                    -Root $context.repository_root -Path $serverLog
                bytes = [UInt64]$finalPrefix.bytes
                sha256 = [string]$finalPrefix.sha256
            }
            disposition = 'leave_same_bound_process_running'
        }
        Write-JsonLf -Path (Join-Path $tracked 'handoff.json') -Value $handoff
        $liveGate = Test-S115LiveHandoff -Context $context -Handoff $handoff
        Write-JsonLf -Path (Join-Path $tracked 'live-handoff-verification.json') `
            -Value $liveGate
        if (-not $liveGate.passed) {
            throw "live handoff gate failed: $($liveGate.errors -join '; ')"
        }
        if ($wall.Elapsed.TotalSeconds -gt
            [double]$context.plan.policy.attempt_wall_seconds) {
            throw 'attempt wall cap expired before success publication'
        }
        Add-AttemptJournal -Kind 'terminal' -Name 'qualified_running' `
            -Details ([ordered]@{
                pid = $handoff.process.pid
                creation_utc = $handoff.process.creation_utc
            })
        $successWallSeconds = [Math]::Round($wall.Elapsed.TotalSeconds, 6)
        if ($successWallSeconds -gt
            [double]$context.plan.policy.attempt_wall_seconds) {
            throw 'attempt wall cap expired while publishing success'
        }
        $result = [ordered]@{
            schema = 'animus-ferric-s115-runtime-result-v1'
            passed = $true
            state = 'qualified_running'
            attempt = $attemptId
            started_at_utc = $started.ToString('o')
            completed_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
            wall_seconds = $successWallSeconds
            launch_count = 1
            smoke_invocation_count = 1
            throughput_request_count = 4
            replacement_request_count = 0
            process_pid = $handoff.process.pid
            handoff_sha256 = Get-Sha256Lower -Path (Join-Path $tracked 'handoff.json')
        }
        Write-JsonLf -Path (Join-Path $tracked 'result.json') -Value $result
        Write-HashManifest -Root $tracked `
            -OutputPath (Join-Path $tracked ([string]$context.plan.evidence.manifest))
        if ($wall.Elapsed.TotalSeconds -gt
            [double]$context.plan.policy.attempt_wall_seconds) {
            throw 'attempt wall cap expired during success-manifest finalization'
        }
        $wall.Stop()
        $result | ConvertTo-Json -Depth 16
    }
    catch {
        $failure = $_.Exception.ToString()
        $cleanup = [ordered]@{
            attempted = $false
            stopped = $false
            reason = 'strong_ownership_was_not_proven'
        }
        if ($null -ne $tracked -and
            -not [string]::IsNullOrWhiteSpace($ownedCreationUtc)) {
            try {
                $cleanup = Stop-S115StronglyBoundRuntime -Context $context `
                    -ExpectedCreationUtc $ownedCreationUtc
            }
            catch {
                $cleanup = [ordered]@{
                    attempted = $false
                    stopped = $false
                    reason = 'ownership_or_cleanup_revalidation_error'
                    error = $_.Exception.ToString()
                }
            }
        }
        if ($null -ne $tracked) {
            Write-JsonLf -Path (Join-Path $tracked 'cleanup.json') -Value $cleanup
            Add-AttemptJournal -Kind 'terminal' -Name 'failed' `
                -Details ([ordered]@{ phase = $phase; error = $failure })
            $failureClassification = switch ($phase) {
                'preflight' { 'hardware_or_isolation_preflight_failure' }
                'identity' { 'engine_compatibility_or_frozen_identity_failure' }
                'launch' { 'model_load_or_managed_startup_failure_after_engine_probe' }
                'attestation' { 'effective_runtime_attestation_failure' }
                'smoke' { 'medium_horizon_smoke_failure' }
                'throughput' { 'throughput_qualification_failure' }
                'handoff' { 'immutable_handoff_failure' }
                default { "$phase`_failure" }
            }
            $result = [ordered]@{
                schema = 'animus-ferric-s115-runtime-result-v1'
                passed = $false
                state = 'failed'
                attempt = $attemptId
                phase = $phase
                failure_classification = $failureClassification
                error = $failure
                started_at_utc = $started.ToString('o')
                completed_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
                wall_seconds = [Math]::Round($wall.Elapsed.TotalSeconds, 6)
                cleanup = $cleanup
            }
            Write-JsonLf -Path (Join-Path $tracked 'result.json') -Value $result
            Write-HashManifest -Root $tracked `
                -OutputPath (Join-Path $tracked `
                    ([string]$context.plan.evidence.manifest))
        }
        throw
    }
    finally {
        $wall.Stop()
        if ($null -ne $lock) { $lock.Dispose() }
    }
}

if ($MyInvocation.InvocationName -cne '.') {
    Invoke-S115RuntimeQualification
}
