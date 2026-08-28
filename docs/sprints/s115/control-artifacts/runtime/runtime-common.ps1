Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:S115Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$script:S115ArtifactDirectory = $PSScriptRoot
$script:S115FrozenCommonSha256 =
    'c096088f3399000cfa4aec88101b7ea987f559ab086f9cc1c2cd09dda69aada5'

function Get-S115RawSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Find-S115RepositoryRoot {
    param([Parameter(Mandatory = $true)][string]$Start)
    $cursor = [System.IO.Path]::GetFullPath($Start)
    for ($index = 0; $index -lt 16; $index++) {
        if ((Test-Path -LiteralPath (Join-Path $cursor '.git')) -and
            (Test-Path -LiteralPath (Join-Path $cursor 'Cargo.toml') -PathType Leaf)) {
            return $cursor
        }
        $parent = Split-Path -Parent $cursor
        if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $cursor) {
            break
        }
        $cursor = $parent
    }
    throw "could not locate repository root above $Start"
}

$script:S115RepositoryRoot = Find-S115RepositoryRoot -Start $PSScriptRoot
$script:S115FrozenCommonPath = Join-Path $script:S115RepositoryRoot `
    'docs/sprints/s114/control-artifacts/runtime/epoch-3/runtime-common.ps1'
if (-not (Test-Path -LiteralPath $script:S115FrozenCommonPath -PathType Leaf) -or
    (Get-S115RawSha256 -Path $script:S115FrozenCommonPath) -cne
        $script:S115FrozenCommonSha256) {
    throw 'the hash-bound Sprint 114 runtime helper is absent or changed'
}
. $script:S115FrozenCommonPath

function Get-S115Context {
    $planPath = Join-Path $script:S115ArtifactDirectory 'runtime-plan.json'
    if (-not (Test-Path -LiteralPath $planPath -PathType Leaf)) {
        throw 'runtime-plan.json is absent'
    }
    $plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
    if ($plan.schema -cne 'animus-ferric-s115-runtime-plan-v1' -or
        $plan.task -cne 'T-11503') {
        throw 'runtime plan schema/task mismatch'
    }
    $trackedRoot = Join-Path $script:S115RepositoryRoot `
        ([string]$plan.evidence.tracked_attempt_root)
    $rawRoot = Join-Path $script:S115RepositoryRoot `
        ([string]$plan.evidence.raw_attempt_root)
    [pscustomobject][ordered]@{
        artifact_directory = $script:S115ArtifactDirectory
        repository_root = $script:S115RepositoryRoot
        plan_path = $planPath
        plan = $plan
        tracked_attempt_root = $trackedRoot
        raw_attempt_root = $rawRoot
        lock_path = Join-Path $script:S115RepositoryRoot ([string]$plan.evidence.lock_path)
        ferric_path = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.qualified_release.binary_path)
        model_path = Join-Path $script:S115RepositoryRoot ([string]$plan.model.path)
        engine_root = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.engine.runtime_root)
        engine_path = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.engine.binary_path)
        cuda_path = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.engine.cuda_backend_path)
        source_manifest_path = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.engine.source_manifest_path)
        release_result_path = Join-Path $script:S115RepositoryRoot `
            ([string]$plan.qualified_release.result_path)
    }
}

function Assert-S115ControlInputs {
    param([Parameter(Mandatory = $true)]$Context)
    $manifest = Join-Path $Context.artifact_directory 'control-inputs.sha256'
    if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
        throw 'control-inputs.sha256 is absent'
    }
    $listed = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        if ($line -notmatch '^([0-9a-f]{64})  ([^/\\]+)$') {
            throw "malformed control manifest line: $line"
        }
        $expected = $Matches[1]
        $name = $Matches[2]
        if ($name -ceq 'control-inputs.sha256' -or -not $listed.Add($name)) {
            throw "invalid duplicate/self control manifest entry: $name"
        }
        $path = Join-Path $Context.artifact_directory $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or
            (Get-Sha256Lower -Path $path) -cne $expected) {
            throw "frozen runtime control changed: $name"
        }
    }
    $required = @(
        '.gitattributes', 'README.md', 'runtime-plan.json', 'runtime-common.ps1',
        'qualify-runtime.ps1', 'verify-runtime.ps1', 'verify-handoff.ps1',
        'test-runtime-control.ps1', 'nonce.txt', 'smoke-prompt.txt',
        'throughput-request.template.json', 'template-probe-defaults.json',
        'template-probe-alias-false.json', 'template-probe-all-false.json',
        'template-probe-all-true.json'
    )
    foreach ($name in $required) {
        if (-not $listed.Contains($name)) {
            throw "required frozen control is unlisted: $name"
        }
    }
    if ($listed.Count -ne $required.Count) {
        throw 'control manifest must contain exactly the fifteen required names'
    }
    [pscustomobject]@{
        manifest_path = $manifest
        manifest_sha256 = Get-Sha256Lower -Path $manifest
        entries = $listed.Count
    }
}

function Test-S115SafeDirectoryTraversal {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$RequireTarget
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $pathFull = [System.IO.Path]::GetFullPath($Path).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $inside = $pathFull.Equals($rootFull,
        [System.StringComparison]::OrdinalIgnoreCase) -or
        $pathFull.StartsWith(
            $rootFull + [System.IO.Path]::DirectorySeparatorChar,
            [System.StringComparison]::OrdinalIgnoreCase
        )
    if (-not $inside) { $errors.Add('directory escapes the repository root') }
    if (-not (Test-Path -LiteralPath $rootFull -PathType Container)) {
        $errors.Add('repository root is absent')
    }
    elseif ((Get-Item -LiteralPath $rootFull -Force).Attributes -band
        [System.IO.FileAttributes]::ReparsePoint) {
        $errors.Add('repository root is a reparse point')
    }
    if ($inside) {
        $cursor = $rootFull
        $relative = [System.IO.Path]::GetRelativePath($rootFull, $pathFull)
        foreach ($component in @($relative -split '[\\/]' | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_) -and $_ -cne '.'
        })) {
            $cursor = Join-Path $cursor $component
            if (-not (Test-Path -LiteralPath $cursor)) {
                if ($RequireTarget) { $errors.Add("directory is absent: $cursor") }
                break
            }
            $item = Get-Item -LiteralPath $cursor -Force
            if (-not $item.PSIsContainer) {
                $errors.Add("path component is not a directory: $cursor")
                break
            }
            if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                $errors.Add("directory is a reparse point: $cursor")
            }
        }
    }
    [pscustomobject][ordered]@{
        passed = $errors.Count -eq 0
        root = $rootFull
        path = $pathFull
        require_target = [bool]$RequireTarget
        errors = @($errors)
    }
}

function New-S115SafeDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $precheck = Test-S115SafeDirectoryTraversal -Root $Root -Path $Path
    if (-not $precheck.passed) { throw ($precheck.errors -join '; ') }
    $cursor = $precheck.root
    $relative = [System.IO.Path]::GetRelativePath($precheck.root, $precheck.path)
    foreach ($component in @($relative -split '[\\/]' | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_) -and $_ -cne '.'
    })) {
        $cursor = Join-Path $cursor $component
        if (-not (Test-Path -LiteralPath $cursor)) {
            [System.IO.Directory]::CreateDirectory($cursor) | Out-Null
        }
        $item = Get-Item -LiteralPath $cursor -Force
        if (-not $item.PSIsContainer -or
            ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "unsafe directory materialized during allocation: $cursor"
        }
    }
    $postcheck = Test-S115SafeDirectoryTraversal -Root $Root -Path $Path `
        -RequireTarget
    if (-not $postcheck.passed) { throw ($postcheck.errors -join '; ') }
    $postcheck.path
}

function Open-S115RuntimeLock {
    param([Parameter(Mandatory = $true)][string]$Path)
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container) -or
        ((Get-Item -LiteralPath $parent -Force).Attributes -band
            [System.IO.FileAttributes]::ReparsePoint)) {
        throw 'runtime lock parent must be an existing non-reparse directory'
    }
    $lockItem = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -ne $lockItem) {
        if ($lockItem.PSIsContainer -or
            ($lockItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'runtime lock path must be a regular non-reparse file'
        }
    }
    try {
        [System.IO.FileStream]::new(
            $Path,
            [System.IO.FileMode]::OpenOrCreate,
            [System.IO.FileAccess]::ReadWrite,
            [System.IO.FileShare]::None
        )
    }
    catch [System.IO.IOException] {
        throw "another T-11503 runtime qualifier holds the exclusive lock: $Path"
    }
}

function Get-S115NumericAttemptNames {
    param([Parameter(Mandatory = $true)][string[]]$Roots)
    @(
        foreach ($root in $Roots) {
            if (-not (Test-Path -LiteralPath $root -PathType Container)) { continue }
            if ((Get-Item -LiteralPath $root -Force).Attributes -band
                [System.IO.FileAttributes]::ReparsePoint) {
                throw "runtime attempt root is a reparse point: $root"
            }
            Get-ChildItem -LiteralPath $root -Directory -Force | ForEach-Object {
                if ($_.Name -notmatch '^[0-9]{3}$') {
                    throw "non-numeric runtime attempt directory: $($_.FullName)"
                }
                if ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                    throw "runtime attempt directory is a reparse point: $($_.FullName)"
                }
                [int]$_.Name
            }
        }
    )
}

function Get-S115NextAttemptId {
    param(
        [Parameter(Mandatory = $true)][string]$TrackedRoot,
        [Parameter(Mandatory = $true)][string]$RawRoot
    )
    $numbers = @(Get-S115NumericAttemptNames -Roots @($TrackedRoot, $RawRoot))
    $next = if ($numbers.Count -eq 0) { 1 } else {
        ([int]($numbers | Measure-Object -Maximum).Maximum) + 1
    }
    if ($next -gt 999) { throw 'runtime attempt namespace exhausted' }
    $next.ToString('000', [System.Globalization.CultureInfo]::InvariantCulture)
}

function Get-S115RunfilePaths {
    param([Parameter(Mandatory = $true)]$Context)
    if ([string]::IsNullOrWhiteSpace([string]$env:APPDATA)) {
        throw 'APPDATA is required to resolve Ferric global runfile path on Windows'
    }
    [pscustomobject]@{
        local = Join-Path $Context.repository_root '.ferric/server.json'
        global = Join-Path (Join-Path $env:APPDATA 'ferric') 'server.json'
    }
}

function Get-S115RunfileObservation {
    param([Parameter(Mandatory = $true)][string]$Path)
    $present = Test-Path -LiteralPath $Path -PathType Leaf
    $bytes = if ($present) { [System.IO.File]::ReadAllBytes($Path) } else { $null }
    $text = if ($present) { $script:S115Utf8NoBom.GetString($bytes) } else { $null }
    $value = $null
    $parseError = $null
    if ($present) {
        try { $value = $text | ConvertFrom-Json }
        catch { $parseError = $_.Exception.Message }
    }
    [pscustomobject][ordered]@{
        path = $Path
        present = $present
        bytes = if ($present) { [UInt64]$bytes.Length } else { 0 }
        sha256 = if ($present) {
            [Convert]::ToHexString(
                [System.Security.Cryptography.SHA256]::HashData($bytes)
            ).ToLowerInvariant()
        } else { $null }
        content = $text
        parse_error = $parseError
        value = $value
    }
}

function Get-S115ExpectedChildArgv {
    param([Parameter(Mandatory = $true)]$Context)
    @($Context.plan.expected_child_argv | ForEach-Object {
        if ([string]$_ -ceq '__MODEL__') { $Context.model_path } else { [string]$_ }
    })
}

function Get-S115BareEngineResolutionProof {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$LaunchPath
    )
    $engineName = 'llama-server.exe'
    $pinned = [System.IO.Path]::GetFullPath($Context.engine_path)
    $priorityDirectories = [System.Collections.Generic.List[string]]::new()
    foreach ($candidate in @(
        (Split-Path -Parent $Context.ferric_path),
        $Context.repository_root,
        [Environment]::SystemDirectory,
        $(if (-not [string]::IsNullOrWhiteSpace([string]$env:WINDIR)) {
            Join-Path $env:WINDIR 'System'
        }),
        $env:WINDIR
    )) {
        if ([string]::IsNullOrWhiteSpace([string]$candidate)) { continue }
        $full = [System.IO.Path]::GetFullPath([string]$candidate)
        if (-not @($priorityDirectories | Where-Object {
            $_.Equals($full, [System.StringComparison]::OrdinalIgnoreCase)
        }).Count) { $priorityDirectories.Add($full) }
    }
    $priority = @(
        foreach ($directory in $priorityDirectories) {
            $candidate = Join-Path $directory $engineName
            $item = Get-Item -LiteralPath $candidate -Force -ErrorAction SilentlyContinue
            $present = $null -ne $item
            $reparse = $present -and [bool]($item.Attributes -band
                [System.IO.FileAttributes]::ReparsePoint)
            $regular = $present -and -not $item.PSIsContainer -and -not $reparse
            [ordered]@{
                directory = $directory
                candidate = $candidate
                present = $present
                regular_file = $regular
                is_pinned = $regular -and
                    [System.IO.Path]::GetFullPath($candidate).Equals(
                        $pinned, [System.StringComparison]::OrdinalIgnoreCase)
                sha256 = if ($regular) { Get-Sha256Lower -Path $candidate } else { $null }
                reparse_point = $reparse
            }
        }
    )
    $pathMatches = @(
        foreach ($directory in @($LaunchPath -split ';')) {
            if ([string]::IsNullOrWhiteSpace($directory)) { continue }
            try { $candidate = Join-Path ([System.IO.Path]::GetFullPath($directory)) $engineName }
            catch { continue }
            $item = Get-Item -LiteralPath $candidate -Force -ErrorAction SilentlyContinue
            if ($null -ne $item) {
                $reparse = [bool]($item.Attributes -band
                    [System.IO.FileAttributes]::ReparsePoint)
                $regular = -not $item.PSIsContainer -and -not $reparse
                [ordered]@{
                    path = [System.IO.Path]::GetFullPath($candidate)
                    regular_file = $regular
                    sha256 = if ($regular) { Get-Sha256Lower -Path $candidate } else { $null }
                    reparse_point = $reparse
                }
            }
        }
    )
    $first = if ($pathMatches.Count -gt 0) { $pathMatches[0] } else { $null }
    $shadowCount = @($priority | Where-Object { $_.present -and -not $_.is_pinned }).Count
    $passed = $shadowCount -eq 0 -and $null -ne $first -and
        [System.IO.Path]::GetFullPath([string]$first.path).Equals(
            $pinned, [System.StringComparison]::OrdinalIgnoreCase) -and
        [string]$first.sha256 -ceq [string]$Context.plan.engine.binary_sha256 -and
        -not [bool]$first.reparse_point
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-bare-engine-resolution-v1'
        passed = $passed
        executable_name = $engineName
        pinned_path = $pinned
        pinned_sha256 = [string]$Context.plan.engine.binary_sha256
        higher_priority_candidates = $priority
        launch_path_matches = $pathMatches
        first_path_match = $first
        errors = @(
            if ($shadowCount -ne 0) { 'higher-priority shadow executable exists' }
            if ($null -eq $first) { 'launch PATH does not resolve the bare engine name' }
            elseif (-not [System.IO.Path]::GetFullPath([string]$first.path).Equals(
                $pinned, [System.StringComparison]::OrdinalIgnoreCase)) {
                'launch PATH first match is not the pinned engine'
            }
            elseif ([string]$first.sha256 -cne [string]$Context.plan.engine.binary_sha256 -or
                [bool]$first.reparse_point) {
                'launch PATH first match is not the pinned regular engine bytes'
            }
        )
    }
}

function Get-S115RuntimeIdentity {
    param([Parameter(Mandatory = $true)]$Context)
    $plan = $Context.plan
    foreach ($pair in @(
        @($Context.release_result_path, [string]$plan.qualified_release.result_sha256),
        @($Context.ferric_path, [string]$plan.qualified_release.binary_sha256),
        @($Context.model_path, [string]$plan.model.sha256),
        @($Context.engine_path, [string]$plan.engine.binary_sha256),
        @($Context.cuda_path, [string]$plan.engine.cuda_backend_sha256),
        @($Context.source_manifest_path, [string]$plan.engine.source_manifest_sha256)
    )) {
        if (-not (Test-Path -LiteralPath $pair[0] -PathType Leaf)) {
            throw "required runtime identity file is absent: $($pair[0])"
        }
        if ((Get-Sha256Lower -Path $pair[0]) -cne $pair[1]) {
            throw "runtime identity hash mismatch: $($pair[0])"
        }
    }
    if ([UInt64](Get-Item -LiteralPath $Context.ferric_path).Length -ne
            [UInt64]$plan.qualified_release.binary_bytes -or
        [UInt64](Get-Item -LiteralPath $Context.model_path).Length -ne
            [UInt64]$plan.model.bytes -or
        [UInt64](Get-Item -LiteralPath $Context.engine_path).Length -ne
            [UInt64]$plan.engine.binary_bytes -or
        [UInt64](Get-Item -LiteralPath $Context.cuda_path).Length -ne
            [UInt64]$plan.engine.cuda_backend_bytes) {
        throw 'runtime identity byte length mismatch'
    }
    $release = Get-Content -Raw -LiteralPath $Context.release_result_path |
        ConvertFrom-Json
    if (-not [bool]$release.passed -or
        [string]$release.binary.sha256 -cne [string]$plan.qualified_release.binary_sha256 -or
        [string]$release.binary.source_commit -cne [string]$plan.qualified_release.source_commit -or
        -not [bool]$release.binary.backend_openai) {
        throw 'T-11501 release result does not bind the requested Ferric binary'
    }
    $sourceManifest = Get-Content -Raw -LiteralPath $Context.source_manifest_path |
        ConvertFrom-Json
    $runtimeCheck = Test-FileIdentityManifest -Root $Context.engine_root `
        -Expected @($sourceManifest.binaries.llama_runtime.files)
    if (-not $runtimeCheck.passed) {
        throw "pinned llama runtime tree mismatch: $($runtimeCheck.errors -join '; ')"
    }
    [pscustomobject][ordered]@{
        release_result_sha256 = Get-Sha256Lower -Path $Context.release_result_path
        ferric = [ordered]@{
            path = $Context.ferric_path
            bytes = [UInt64](Get-Item -LiteralPath $Context.ferric_path).Length
            sha256 = Get-Sha256Lower -Path $Context.ferric_path
        }
        model = [ordered]@{
            path = $Context.model_path
            bytes = [UInt64](Get-Item -LiteralPath $Context.model_path).Length
            sha256 = Get-Sha256Lower -Path $Context.model_path
        }
        engine = [ordered]@{
            path = $Context.engine_path
            bytes = [UInt64](Get-Item -LiteralPath $Context.engine_path).Length
            sha256 = Get-Sha256Lower -Path $Context.engine_path
        }
        cuda_backend = [ordered]@{
            path = $Context.cuda_path
            bytes = [UInt64](Get-Item -LiteralPath $Context.cuda_path).Length
            sha256 = Get-Sha256Lower -Path $Context.cuda_path
        }
        runtime_tree = $runtimeCheck
    }
}

function Get-S115HostSnapshot {
    param([Parameter(Mandatory = $true)]$Context)
    $paths = Get-S115RunfilePaths -Context $Context
    $local = Get-S115RunfileObservation -Path $paths.local
    $global = Get-S115RunfileObservation -Path $paths.global
    $ownedPids = @(
        foreach ($observation in @($local, $global)) {
            if ($null -ne $observation.value -and
                $null -ne $observation.value.PSObject.Properties['pid']) {
                [UInt32]$observation.value.pid
            }
        }
    ) | Select-Object -Unique
    $qualifiedImages = @(
        [System.IO.Path]::GetFullPath($Context.ferric_path),
        [System.IO.Path]::GetFullPath($Context.engine_path)
    )
    $allProcesses = @(Get-CimInstance Win32_Process -OperationTimeoutSec 10)
    $processes = @($allProcesses | Where-Object {
        $candidate = $_
        if ($ownedPids -contains [UInt32]$candidate.ProcessId) { return $true }
        if ([string]::IsNullOrWhiteSpace([string]$candidate.ExecutablePath)) {
            return $false
        }
        $resolved = [System.IO.Path]::GetFullPath([string]$candidate.ExecutablePath)
        @($qualifiedImages | Where-Object {
            $_.Equals($resolved, [System.StringComparison]::OrdinalIgnoreCase)
        }).Count -gt 0
    } | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath,
        CommandLine, CreationDate)
    $relevantPids = @($processes | ForEach-Object { [UInt32]$_.ProcessId })
    $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
        Where-Object {
            [int]$_.LocalPort -eq [int]$Context.plan.coordinate.port -or
            $relevantPids -contains [UInt32]$_.OwningProcess
        } | Select-Object LocalAddress, LocalPort, State, OwningProcess)
    Add-Type -AssemblyName Microsoft.VisualBasic
    $computer = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
    $os = Get-CimInstance Win32_OperatingSystem -OperationTimeoutSec 10
    $memory = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory `
        -OperationTimeoutSec 10
    $volume = Get-Volume -DriveLetter ([System.IO.Path]::GetPathRoot(
        $Context.repository_root).Substring(0, 1))
    $gpuRows = @(Invoke-BoundedTextProcess -FilePath 'nvidia-smi.exe' -Arguments @(
        '--query-gpu=name,uuid,driver_version,memory.total,memory.used,memory.free,utilization.gpu,temperature.gpu,pstate,power.draw',
        '--format=csv,noheader,nounits'
    ))
    $computeRows = @(Invoke-BoundedTextProcess -FilePath 'nvidia-smi.exe' -Arguments @(
        '--query-compute-apps=pid,process_name,used_memory',
        '--format=csv,noheader,nounits'
    ))
    if ($gpuRows.Count -ne 1) { throw 'exactly one NVIDIA GPU is required' }
    $gpu = @($gpuRows[0].Split(',') | ForEach-Object { $_.Trim() })
    if ($gpu.Count -ne 10) { throw 'unexpected nvidia-smi GPU row shape' }
    $wslStatus = Invoke-BoundedProcessResult -FilePath 'wsl.exe' `
        -Arguments @('--status') -TimeoutMilliseconds 30000
    $wslList = Invoke-BoundedProcessResult -FilePath 'wsl.exe' `
        -Arguments @('--list', '--verbose') -TimeoutMilliseconds 30000
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-host-preflight-v1'
        captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        boot_time_utc = (ConvertTo-UtcIso8601 -Value $os.LastBootUpTime)
        memory = [ordered]@{
            total_physical_bytes = [UInt64]$computer.TotalPhysicalMemory
            available_physical_bytes = [UInt64]$computer.AvailablePhysicalMemory
            committed_bytes = [UInt64]$memory.CommittedBytes
            commit_limit_bytes = [UInt64]$memory.CommitLimit
            commit_available_bytes = [UInt64]$memory.CommitLimit -
                [UInt64]$memory.CommittedBytes
        }
        disk = [ordered]@{
            drive = [string]$volume.DriveLetter
            total_bytes = [UInt64]$volume.Size
            free_bytes = [UInt64]$volume.SizeRemaining
        }
        gpu = [ordered]@{
            name = $gpu[0]
            uuid = $gpu[1]
            driver_version = $gpu[2]
            total_mib = [UInt64]$gpu[3]
            used_mib = [UInt64]$gpu[4]
            free_mib = [UInt64]$gpu[5]
            utilization_percent = [UInt32]$gpu[6]
            temperature_c = [UInt32]$gpu[7]
            power_state = $gpu[8]
            power_draw_watts = [double]$gpu[9]
            raw_row = $gpuRows[0]
            compute_apps = $computeRows
        }
        qualified_or_runfile_owned_processes = $processes
        qualified_process_record_fields = @(
            'ProcessId', 'ParentProcessId', 'Name', 'ExecutablePath',
            'CommandLine', 'CreationDate'
        )
        relevant_listeners = $listeners
        listener_record_fields = @(
            'LocalAddress', 'LocalPort', 'State', 'OwningProcess'
        )
        runfiles = [ordered]@{ local = $local; global = $global }
        wsl = [ordered]@{
            status = $wslStatus
            distributions = $wslList
        }
    }
}

function Get-S115BubblewrapVersionFacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Output,
        [Parameter(Mandatory = $true)]
        [ValidatePattern('^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$')]
        [string]$ExpectedVersion
    )
    $lines = @($Output.Replace("`r", '').Split("`n") | Where-Object {
        -not [string]::IsNullOrWhiteSpace($_)
    })
    $candidateLines = @($lines | Where-Object {
        [regex]::IsMatch(
            [string]$_,
            '(?i)(?:^|[^A-Za-z0-9])(?:bubblewrap|bwrap)(?:[^A-Za-z0-9]|$)'
        )
    })
    $exactMatches = [System.Collections.Generic.List[object]]::new()
    foreach ($line in $candidateLines) {
        $match = [regex]::Match(
            [string]$line,
            '^bubblewrap (?<major>0|[1-9][0-9]*)\.(?<minor>0|[1-9][0-9]*)\.(?<patch>0|[1-9][0-9]*)\z',
            [System.Text.RegularExpressions.RegexOptions]::CultureInvariant
        )
        if ($match.Success) { $exactMatches.Add($match) }
    }
    $observedVersion = if ($exactMatches.Count -eq 1) {
        '{0}.{1}.{2}' -f
            $exactMatches[0].Groups['major'].Value,
            $exactMatches[0].Groups['minor'].Value,
            $exactMatches[0].Groups['patch'].Value
    } else { $null }
    $errors = [System.Collections.Generic.List[string]]::new()
    if ($candidateLines.Count -ne 1) {
        $errors.Add('probe output must contain exactly one Bubblewrap candidate line')
    }
    if ($exactMatches.Count -ne 1) {
        $errors.Add('Bubblewrap version line is missing or malformed')
    }
    elseif ($observedVersion -cne $ExpectedVersion) {
        $errors.Add('Bubblewrap version differs from the frozen version')
    }
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-bubblewrap-version-v1'
        passed = $errors.Count -eq 0
        expected_version = $ExpectedVersion
        observed_version = $observedVersion
        exact_line = if ($exactMatches.Count -eq 1) {
            [string]$exactMatches[0].Value
        } else { $null }
        candidate_lines = $candidateLines
        errors = @($errors)
    }
}

function Invoke-S115WslIsolationProbe {
    param([Parameter(Mandatory = $true)]$Context)
    $distributionList = Invoke-BoundedProcessResult -FilePath 'wsl.exe' `
        -Arguments @('--list', '--verbose') -TimeoutMilliseconds 30000
    $normalizedList = ([string]$distributionList.stdout).Replace("`0", '')
    $distributionPattern = '(?m)^\s*\*?\s*' +
        [regex]::Escape([string]$Context.plan.wsl.distribution) +
        '\s+(?<state>\S+)\s+(?<version>\d+)\s*$'
    $distributionMatch = [regex]::Match($normalizedList, $distributionPattern)
    $observedVersion = if ($distributionMatch.Success) {
        [int]$distributionMatch.Groups['version'].Value
    } else { $null }
    $observedState = if ($distributionMatch.Success) {
        [string]$distributionMatch.Groups['state'].Value
    } else { $null }
    $script = @'
set -euo pipefail
uname -srmo
bwrap --version
bwrap --unshare-user --uid 0 --gid 0 --unshare-pid --unshare-net \
  --ro-bind / / --proc /proc --dev /dev --new-session -- \
  sh -eu -c '
    non_loopback=$(awk -F: "NR > 2 { gsub(/[[:space:]]/, \"\", \$1); if (\$1 != \"lo\") print \$1 }" /proc/net/dev)
    ipv4_routes=$(awk "NR > 1 { print \$1 }" /proc/net/route)
    test -z "$non_loopback"
    test -z "$ipv4_routes"
    printf "%s\n" S115_NETWORK_NAMESPACE_ONLY_LOOPBACK=1
  '
'@
    $result = Invoke-BoundedProcessResult -FilePath 'wsl.exe' -Arguments @(
        '--distribution', [string]$Context.plan.wsl.distribution,
        '--exec', 'bash', '-lc', $script
    ) -TimeoutMilliseconds 60000
    $bubblewrapVersion = Get-S115BubblewrapVersionFacts `
        -Output ([string]$result.stdout) `
        -ExpectedVersion ([string]$Context.plan.wsl.bubblewrap_version)
    $passed = -not $distributionList.timed_out -and
        $distributionList.exit_code -eq 0 -and $distributionMatch.Success -and
        $observedVersion -eq [int]$Context.plan.wsl.version -and
        -not $result.timed_out -and $result.exit_code -eq 0 -and
        $bubblewrapVersion.passed -and
        $result.stdout.Contains('S115_NETWORK_NAMESPACE_ONLY_LOOPBACK=1')
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-wsl-isolation-v1'
        passed = $passed
        distribution = [string]$Context.plan.wsl.distribution
        expected_version = [int]$Context.plan.wsl.version
        observed_version = $observedVersion
        observed_state = $observedState
        distribution_list = $distributionList
        bubblewrap_version = $bubblewrapVersion
        result = $result
    }
}

function Get-S115LiveBinding {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [AllowNull()][string]$PreflightCapturedUtc,
        [AllowNull()][string]$ExpectedCreationUtc
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $paths = Get-S115RunfilePaths -Context $Context
    $local = Get-S115RunfileObservation -Path $paths.local
    $global = Get-S115RunfileObservation -Path $paths.global
    if (-not $local.present -or -not $global.present -or
        $null -ne $local.parse_error -or $null -ne $global.parse_error) {
        $errors.Add('both parseable local/global runfiles are required')
    }
    if ($local.present -and $global.present -and $local.sha256 -cne $global.sha256) {
        $errors.Add('local/global runfiles are not byte-identical')
    }
    $runfile = $local.value
    $expectedBase = "http://127.0.0.1:$($Context.plan.coordinate.port)/v1"
    if ($null -ne $runfile) {
        if ([string]$runfile.engine -cne 'llama-server' -or
            [int]$runfile.port -ne [int]$Context.plan.coordinate.port -or
            [string]$runfile.base_url -cne $expectedBase -or
            [bool]$runfile.tailscale -or
            [string]$runfile.model -cne $Context.model_path -or
            [int]$runfile.context_size -ne [int]$Context.plan.coordinate.context -or
            [int]$runfile.sampling_seed -ne [int]$Context.plan.coordinate.seed -or
            [int]$runfile.parallel_slots -ne [int]$Context.plan.coordinate.parallel_slots) {
            $errors.Add('runfile values differ from the frozen coordinate')
        }
    }
    $process = $null
    $listeners = @()
    $commandBinding = $null
    $creationUtc = $null
    $creationBinding = $null
    if ($null -ne $runfile) {
        $process = Get-CimInstance Win32_Process -Filter "ProcessId = $([UInt32]$runfile.pid)" `
            -OperationTimeoutSec 10
        if ($null -eq $process) {
            $errors.Add('runfile PID is absent')
        }
        else {
            $liveHash = if (Test-Path -LiteralPath $process.ExecutablePath -PathType Leaf) {
                Get-Sha256Lower -Path $process.ExecutablePath
            } else { $null }
            $commandBinding = Test-BoundWindowsProcessCommand `
                -ExecutablePath ([string]$process.ExecutablePath) `
                -ExecutableSha256 $liveHash `
                -CommandLine ([string]$process.CommandLine) `
                -FrozenExecutablePath $Context.engine_path `
                -FrozenExecutableSha256 ([string]$Context.plan.engine.binary_sha256) `
                -ExpectedArgv @(Get-S115ExpectedChildArgv -Context $Context)
            if (-not $commandBinding.passed) {
                $errors.Add('live process executable/argv binding failed')
            }
            $creationUtc = ConvertTo-UtcIso8601 -Value $process.CreationDate
            if (-not [string]::IsNullOrWhiteSpace($ExpectedCreationUtc)) {
                if ($creationUtc -cne $ExpectedCreationUtc) {
                    $errors.Add('live process creation time differs from frozen handoff')
                }
                $creationBinding = [ordered]@{
                    passed = $creationUtc -ceq $ExpectedCreationUtc
                    expected_creation_utc = $ExpectedCreationUtc
                    observed_creation_utc = $creationUtc
                }
            }
            elseif (-not [string]::IsNullOrWhiteSpace($PreflightCapturedUtc)) {
                $creationBinding = Test-ProcessCreationWindow `
                    -CreationDateUtc $creationUtc `
                    -PreflightCapturedUtc $PreflightCapturedUtc `
                    -AttestationCapturedUtc ([DateTimeOffset]::UtcNow.ToString('o')) `
                    -ToleranceSeconds ([int]$Context.plan.policy.creation_time_tolerance_seconds)
                if (-not $creationBinding.passed) {
                    $errors.Add('live process is outside the qualifier launch window')
                }
            }
        }
        $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
            Where-Object {
                [UInt32]$_.OwningProcess -eq [UInt32]$runfile.pid -or
                [int]$_.LocalPort -eq [int]$Context.plan.coordinate.port
            } | Select-Object LocalAddress, LocalPort, State, OwningProcess)
        if ($listeners.Count -ne 1 -or
            [string]$listeners[0].LocalAddress -cne '127.0.0.1' -or
            [int]$listeners[0].LocalPort -ne [int]$Context.plan.coordinate.port -or
            [UInt32]$listeners[0].OwningProcess -ne [UInt32]$runfile.pid) {
            $errors.Add('the process exact sole loopback listener is not proven')
        }
    }
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-live-binding-v1'
        passed = $errors.Count -eq 0
        captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        runfiles = [ordered]@{ local = $local; global = $global }
        runfile = $runfile
        process = if ($null -ne $process) {
            [ordered]@{
                pid = [UInt32]$process.ProcessId
                parent_pid = [UInt32]$process.ParentProcessId
                name = [string]$process.Name
                executable_path = [string]$process.ExecutablePath
                command_line = [string]$process.CommandLine
                creation_utc = $creationUtc
                command_binding = $commandBinding
                creation_binding = $creationBinding
            }
        } else { $null }
        listeners = $listeners
        errors = @($errors)
    }
}

function Get-S115StablePropertyDigest {
    param(
        [Parameter(Mandatory = $true)]$Props,
        [Parameter(Mandatory = $true)]$ModelEntry
    )
    $caps = Get-OptionalProperty -Value $Props -Name 'chat_template_caps'
    $settings = Get-OptionalProperty -Value $Props `
        -Name 'default_generation_settings'
    $meta = Get-OptionalProperty -Value $ModelEntry -Name 'meta'
    $nParams = Get-OptionalProperty -Value $meta -Name 'n_params'
    if ($null -eq $nParams) {
        $nParams = Get-OptionalProperty -Value $ModelEntry -Name 'n_params'
    }
    $value = [ordered]@{
        served_model_id = [string](Get-OptionalProperty -Value $ModelEntry -Name 'id')
        served_n_params = $nParams
        served_ftype = Get-OptionalProperty -Value $meta -Name 'ftype'
        context = Get-OptionalProperty -Value $settings -Name 'n_ctx'
        seed = Get-OptionalProperty -Value (
            Get-OptionalProperty -Value $settings -Name 'params'
        ) -Name 'seed'
        total_slots = Get-OptionalProperty -Value $Props -Name 'total_slots'
        chat_template_sha256 = Get-Sha256Text -Text ([string](
            Get-OptionalProperty -Value $Props -Name 'chat_template'
        ))
        supports_preserve_reasoning = Get-OptionalProperty -Value $caps `
            -Name 'supports_preserve_reasoning'
    }
    $json = $value | ConvertTo-Json -Depth 16 -Compress
    [pscustomobject][ordered]@{
        value = $value
        sha256 = Get-Sha256Text -Text $json
    }
}

function Get-S115ServerLogFacts {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)]$Context
    )
    $offloads = [regex]::Matches(
        $Text, 'offloaded\s+(\d+)\s*/\s*(\d+)\s+layers', 'IgnoreCase')
    $offload = if ($offloads.Count -gt 0) {
        $offloads[$offloads.Count - 1]
    } else { $null }
    $kv = [regex]::Matches(
        $Text, 'K\s*\(q8_0\)\s*:[^\r\n]*V\s*\(q8_0\)\s*:', 'IgnoreCase')
    $flash = [regex]::Matches(
        $Text,
        '(?:flash_attn\s*=\s*(?:1|on|enabled)\b|flash attention is enabled)',
        'IgnoreCase'
    )
    $thinking = [regex]::Matches(
        $Text, 'chat template,\s*thinking\s*=\s*1\b', 'IgnoreCase')
    $preserveWarning = [regex]::Matches(
        $Text,
        'supports preserving reasoning,\s*consider enabling|does not support[^\r\n]*reasoning[- ]preserve',
        'IgnoreCase'
    )
    $value = [ordered]@{
        effective_gpu_layers = if ($null -ne $offload) {
            [int]$offload.Groups[1].Value
        } else { $null }
        total_layers = if ($null -ne $offload) {
            [int]$offload.Groups[2].Value
        } else { $null }
        offload_line = if ($null -ne $offload) { $offload.Value } else { $null }
        kv_cache_lines = @($kv | ForEach-Object { $_.Value })
        flash_attention_lines = @($flash | ForEach-Object { $_.Value })
        thinking_lines = @($thinking | ForEach-Object { $_.Value })
        preserve_warning_count = $preserveWarning.Count
    }
    $passed = $null -ne $offload -and
        [int]$value.effective_gpu_layers -eq
            [int]$Context.plan.coordinate.gpu_layers -and
        @($value.kv_cache_lines).Count -ge 1 -and
        @($value.flash_attention_lines).Count -ge 1 -and
        @($value.thinking_lines).Count -ge 1 -and
        [int]$value.preserve_warning_count -eq 0
    $json = $value | ConvertTo-Json -Depth 16 -Compress
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-server-log-facts-v1'
        passed = $passed
        value = $value
        sha256 = Get-Sha256Text -Text $json
    }
}

function Test-S115StableFilePrefix {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][UInt64]$Bytes,
        [Parameter(Mandatory = $true)][string]$Sha256
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $actualHash = $null
    $actualLength = [UInt64]0
    if ($Bytes -eq 0) {
        $errors.Add('frozen server-log prefix length must be nonzero')
    }
    elseif (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $errors.Add('raw live server log is absent')
    }
    else {
        $file = Get-Item -LiteralPath $Path
        $actualLength = [UInt64]$file.Length
        if ($actualLength -lt $Bytes) {
            $errors.Add('raw live server log is shorter than its frozen prefix')
        }
        else {
            $stream = [System.IO.File]::Open(
                $Path,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::ReadWrite
            )
            $hash = [System.Security.Cryptography.SHA256]::Create()
            try {
                $buffer = [byte[]]::new(1048576)
                [UInt64]$remaining = $Bytes
                while ($remaining -gt 0) {
                    $requested = [int][Math]::Min([UInt64]$buffer.Length, $remaining)
                    $read = $stream.Read($buffer, 0, $requested)
                    if ($read -le 0) { throw 'unexpected end of server-log prefix' }
                    $hash.TransformBlock($buffer, 0, $read, $buffer, 0) | Out-Null
                    $remaining -= [UInt64]$read
                }
                $hash.TransformFinalBlock([byte[]]::new(0), 0, 0) | Out-Null
                $actualHash = [Convert]::ToHexString($hash.Hash).ToLowerInvariant()
            }
            finally {
                $hash.Dispose()
                $stream.Dispose()
            }
            if ($actualHash -cne $Sha256) {
                $errors.Add('raw live server-log prefix hash changed')
            }
        }
    }
    [pscustomobject][ordered]@{
        passed = $errors.Count -eq 0
        path = $Path
        expected_bytes = $Bytes
        observed_file_bytes = $actualLength
        expected_sha256 = $Sha256
        observed_sha256 = $actualHash
        errors = @($errors)
    }
}

function Get-S115StableFilePrefixSnapshot {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "file is absent for stable-prefix capture: $Path"
    }
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::ReadWrite
    )
    $hash = [System.Security.Cryptography.SHA256]::Create()
    try {
        [UInt64]$bytes = $stream.Length
        if ($bytes -eq 0) { throw 'server-log prefix cannot be empty' }
        $buffer = [byte[]]::new(1048576)
        [UInt64]$remaining = $bytes
        while ($remaining -gt 0) {
            $requested = [int][Math]::Min([UInt64]$buffer.Length, $remaining)
            $read = $stream.Read($buffer, 0, $requested)
            if ($read -le 0) { throw 'unexpected end during prefix capture' }
            $hash.TransformBlock($buffer, 0, $read, $buffer, 0) | Out-Null
            $remaining -= [UInt64]$read
        }
        $hash.TransformFinalBlock([byte[]]::new(0), 0, 0) | Out-Null
        [pscustomobject][ordered]@{
            bytes = $bytes
            sha256 = [Convert]::ToHexString($hash.Hash).ToLowerInvariant()
        }
    }
    finally {
        $hash.Dispose()
        $stream.Dispose()
    }
}

function Read-S115Utf8FilePrefix {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][UInt64]$Bytes
    )
    if ($Bytes -gt [int]::MaxValue) {
        throw 'server-log prefix is too large for compact text attestation'
    }
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::ReadWrite
    )
    try {
        $buffer = [byte[]]::new([int]$Bytes)
        $offset = 0
        while ($offset -lt $buffer.Length) {
            $read = $stream.Read($buffer, $offset, $buffer.Length - $offset)
            if ($read -le 0) { throw 'unexpected end while reading server-log prefix' }
            $offset += $read
        }
        $script:S115Utf8NoBom.GetString($buffer)
    }
    finally { $stream.Dispose() }
}

function Invoke-S115JsonGet {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [int]$TimeoutSeconds = 30
    )
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)
    try {
        $response = $client.GetAsync($Uri).GetAwaiter().GetResult()
        try {
            $bytes = $response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
            $text = $script:S115Utf8NoBom.GetString($bytes)
            [pscustomobject]@{
                status_code = [int]$response.StatusCode
                text = $text
                json = try { $text | ConvertFrom-Json } catch { $null }
            }
        }
        finally { $response.Dispose() }
    }
    finally {
        $client.Dispose()
        $handler.Dispose()
    }
}

function Test-S115PathComponentsAreNotReparsePoints {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )
    $full = Resolve-SafeRelativePath -Root $Root -RelativePath $RelativePath
    $relativeDirectory = Split-Path -Parent $RelativePath
    $cursor = [System.IO.Path]::GetFullPath($Root)
    $errors = [System.Collections.Generic.List[string]]::new()
    if ((Get-Item -LiteralPath $cursor -Force).Attributes -band
        [System.IO.FileAttributes]::ReparsePoint) {
        $errors.Add('repository root is a reparse point')
    }
    foreach ($component in @($relativeDirectory -split '[\\/]' |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) })) {
        $cursor = Join-Path $cursor $component
        if (-not (Test-Path -LiteralPath $cursor -PathType Container)) {
            $errors.Add("path component is absent: $component")
            break
        }
        if ((Get-Item -LiteralPath $cursor -Force).Attributes -band
            [System.IO.FileAttributes]::ReparsePoint) {
            $errors.Add("path component is a reparse point: $component")
        }
    }
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
        $errors.Add('final path is absent or is not a regular file')
    }
    else {
        $leaf = Get-Item -LiteralPath $full -Force
        if ($leaf.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            $errors.Add('final path is a reparse point')
        }
    }
    [pscustomobject][ordered]@{
        passed = $errors.Count -eq 0
        full_path = $full
        errors = @($errors)
    }
}

function Test-S115LiveHandoff {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)]$Handoff
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $binding = Get-S115LiveBinding -Context $Context `
        -ExpectedCreationUtc ([string]$Handoff.process.creation_utc)
    if (-not $binding.passed) { $errors.AddRange([string[]]$binding.errors) }
    if ($null -eq $binding.process -or
        [UInt32]$binding.process.pid -ne [UInt32]$Handoff.process.pid) {
        $errors.Add('live process PID differs from the frozen handoff')
    }
    if ($null -ne $binding.runfiles.local -and
        [string]$binding.runfiles.local.sha256 -cne
            [string]$Handoff.runfiles.local_sha256 -or
        $null -ne $binding.runfiles.global -and
        [string]$binding.runfiles.global.sha256 -cne
            [string]$Handoff.runfiles.global_sha256) {
        $errors.Add('live runfile bytes differ from the frozen handoff')
    }
    try { $identity = Get-S115RuntimeIdentity -Context $Context }
    catch { $identity = $null; $errors.Add($_.Exception.Message) }
    $base = "http://127.0.0.1:$($Context.plan.coordinate.port)/v1"
    if ([string]$Handoff.endpoint -cne $base) {
        $errors.Add('handoff endpoint is not the frozen loopback endpoint')
    }
    $propertyDigest = $null
    try {
        $health = Invoke-S115JsonGet -Uri ($base.Replace('/v1', '/health'))
        $models = Invoke-S115JsonGet -Uri "$base/models"
        $props = Invoke-S115JsonGet -Uri ($base.Replace('/v1', '/props'))
        if ($health.status_code -ne 200 -or $models.status_code -ne 200 -or
            $props.status_code -ne 200) {
            $errors.Add('live handoff endpoints did not all return HTTP 200')
        }
        if ([string]$health.json.status -cne 'ok') {
            $errors.Add('live health body status is not exactly ok')
        }
        $entries = @($models.json.data)
        if ($entries.Count -ne 1 -or [string]$entries[0].id -cne
            [string]$Handoff.served_model_id) {
            $errors.Add('live served model identity changed')
        }
        $meta = if ($entries.Count -eq 1) {
            Get-OptionalProperty -Value $entries[0] -Name 'meta'
        } else { $null }
        $nParams = Get-OptionalProperty -Value $meta -Name 'n_params'
        if ($null -eq $nParams -and $entries.Count -eq 1) {
            $nParams = Get-OptionalProperty -Value $entries[0] -Name 'n_params'
        }
        if ($null -eq $nParams -or [UInt64]$nParams -ne
            [UInt64]$Handoff.served_n_params -or [UInt64]$nParams -ne
            [UInt64]$Context.plan.model.parameters) {
            $errors.Add('live served parameter count changed')
        }
        if ([int64]$props.json.default_generation_settings.n_ctx -ne
            [int64]$Context.plan.coordinate.context -or
            [int]$props.json.total_slots -ne [int]$Context.plan.coordinate.parallel_slots) {
            $errors.Add('live served properties changed')
        }
        if ($entries.Count -eq 1) {
            $propertyDigest = Get-S115StablePropertyDigest -Props $props.json `
                -ModelEntry $entries[0]
            if ($propertyDigest.sha256 -cne [string]$Handoff.property_digest_sha256) {
                $errors.Add('live stable property digest changed')
            }
        }
    }
    catch {
        $health = $null; $models = $null; $props = $null
        $errors.Add("live endpoint verification failed: $($_.Exception.Message)")
    }
    $expectedLogRelative = "target/s115-runtime-qualification/attempts/$($Handoff.attempt)/server-live.log"
    try {
        if ([string]$Handoff.server_log_prefix.raw_relative_path -cne
            $expectedLogRelative) {
            throw 'handoff raw log path differs from its exact attempt path'
        }
        $pathSafety = Test-S115PathComponentsAreNotReparsePoints `
            -Root $Context.repository_root -RelativePath $expectedLogRelative
        if (-not $pathSafety.passed) {
            throw ($pathSafety.errors -join '; ')
        }
        $rawLogPath = $pathSafety.full_path
    }
    catch {
        $rawLogPath = $null
        $errors.Add("server-log path is unsafe: $($_.Exception.Message)")
    }
    try {
        if ($null -eq $rawLogPath) {
            $prefix = $null
            $serverLogFacts = $null
        }
        else {
            $prefix = Test-S115StableFilePrefix -Path $rawLogPath `
                -Bytes ([UInt64]$Handoff.server_log_prefix.bytes) `
                -Sha256 ([string]$Handoff.server_log_prefix.sha256)
            if (-not $prefix.passed) { $errors.AddRange([string[]]$prefix.errors) }
            $prefixText = Read-S115Utf8FilePrefix -Path $rawLogPath `
                -Bytes ([UInt64]$Handoff.server_log_prefix.bytes)
            $serverLogFacts = Get-S115ServerLogFacts -Text $prefixText `
                -Context $Context
            if (-not $serverLogFacts.passed -or
                [string]$serverLogFacts.sha256 -cne
                    [string]$Handoff.server_log_facts_sha256) {
                $errors.Add('live server-log prefix effective facts changed')
            }
        }
    }
    catch {
        $prefix = $null
        $serverLogFacts = $null
        $errors.Add("server-log prefix verification failed: $($_.Exception.Message)")
    }
    try {
        $finalBinding = Get-S115LiveBinding -Context $Context `
            -ExpectedCreationUtc ([string]$Handoff.process.creation_utc)
        if (-not $finalBinding.passed -or $null -eq $finalBinding.process -or
            [UInt32]$finalBinding.process.pid -ne [UInt32]$Handoff.process.pid -or
            [string]$finalBinding.runfiles.local.sha256 -cne
                [string]$Handoff.runfiles.local_sha256 -or
            [string]$finalBinding.runfiles.global.sha256 -cne
                [string]$Handoff.runfiles.global_sha256) {
            $errors.Add('final live binding changed during handoff verification')
        }
    }
    catch {
        $finalBinding = $null
        $errors.Add("final live binding failed: $($_.Exception.Message)")
    }
    [pscustomobject][ordered]@{
        schema = 'animus-ferric-s115-live-handoff-verification-v1'
        passed = $errors.Count -eq 0
        checked_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        binding = $binding
        runtime_identity = $identity
        endpoints = [ordered]@{ health = $health; models = $models; props = $props }
        server_log_prefix = $prefix
        server_log_facts = $serverLogFacts
        final_binding = $finalBinding
        errors = @($errors)
    }
}

function Stop-S115StronglyBoundRuntime {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$ExpectedCreationUtc
    )
    $before = Get-S115LiveBinding -Context $Context `
        -ExpectedCreationUtc $ExpectedCreationUtc
    if (-not $before.passed) {
        return [pscustomobject]@{
            attempted = $false
            stopped = $false
            reason = 'ownership_revalidation_failed'
            binding = $before
        }
    }
    $process = $null
    $observedStartUtc = $null
    $observedPath = $null
    $killed = $false
    $waitExited = $false
    $hasExited = $false
    try {
        $process = [System.Diagnostics.Process]::GetProcessById(
            [int]$before.process.pid
        )
        # Force a durable OS process handle before any final identity check.
        $null = $process.Handle
        $observedStart = $process.StartTime.ToUniversalTime()
        $expectedStart = [DateTimeOffset]::Parse(
            $ExpectedCreationUtc,
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::RoundtripKind
        ).UtcDateTime
        $observedStartUtc = $observedStart.ToString('o')
        $observedPath = [string]$process.MainModule.FileName
        if ($observedStart.Ticks -ne $expectedStart.Ticks -or
            -not [System.IO.Path]::GetFullPath($observedPath).Equals(
                [System.IO.Path]::GetFullPath($Context.engine_path),
                [System.StringComparison]::OrdinalIgnoreCase
            ) -or
            (Get-Sha256Lower -Path $observedPath) -cne
                [string]$Context.plan.engine.binary_sha256) {
            return [pscustomobject]@{
                attempted = $false
                stopped = $false
                reason = 'held_process_handle_identity_mismatch'
                binding = $before
                observed_start_utc = $observedStartUtc
                observed_executable_path = $observedPath
            }
        }
        $process.Kill()
        $killed = $true
        $waitExited = $process.WaitForExit(
            [int]$Context.plan.policy.teardown_timeout_seconds * 1000
        )
        $hasExited = $process.HasExited
    }
    finally {
        if ($null -ne $process) { $process.Dispose() }
    }
    Start-Sleep -Milliseconds 750
    $pidAlive = $null -ne (Get-Process -Id ([UInt32]$before.process.pid) `
        -ErrorAction SilentlyContinue)
    $listeners = @(Get-NetTCPConnection -State Listen `
        -LocalPort ([int]$Context.plan.coordinate.port) `
        -ErrorAction SilentlyContinue)
    $terminationConfirmed = $killed -and $waitExited -and $hasExited -and
        -not $pidAlive -and $listeners.Count -eq 0
    $paths = Get-S115RunfilePaths -Context $Context
    $runfileCleanup = [System.Collections.Generic.List[object]]::new()
    foreach ($pair in @(
        @('local', $paths.local, [string]$before.runfiles.local.sha256),
        @('global', $paths.global, [string]$before.runfiles.global.sha256)
    )) {
        $observation = Get-S115RunfileObservation -Path $pair[1]
        $removed = $false
        $reason = if (-not $terminationConfirmed) {
            'retained_process_or_listener_exit_unconfirmed'
        }
        elseif (-not $observation.present) { 'already_absent' }
        elseif ([string]$observation.sha256 -cne $pair[2]) {
            'retained_hash_changed'
        }
        else {
            Remove-Item -LiteralPath $pair[1] -Force
            $removed = $true
            'removed_exact_bound_bytes'
        }
        $runfileCleanup.Add([ordered]@{
            scope = $pair[0]
            path = $pair[1]
            expected_sha256 = $pair[2]
            observed_sha256 = $observation.sha256
            removed = $removed
            reason = $reason
        })
    }
    $runfilesAfter = [ordered]@{
        local = Get-S115RunfileObservation -Path $paths.local
        global = Get-S115RunfileObservation -Path $paths.global
    }
    $stopped = $terminationConfirmed
    $coldState = $stopped -and -not $runfilesAfter.local.present -and
        -not $runfilesAfter.global.present
    [pscustomobject]@{
        attempted = $true
        stopped = $stopped
        cold_state = $coldState
        reason = if ($coldState) { 'strongly_bound_owned_runtime_stopped' }
        elseif ($stopped) {
            'strongly_bound_owned_runtime_stopped_runfiles_retained'
        }
        else {
            'held_process_termination_did_not_reach_cold_state'
        }
        binding = $before
        held_process = [ordered]@{
            pid = [UInt32]$before.process.pid
            observed_start_utc = $observedStartUtc
            observed_executable_path = $observedPath
            executable_sha256 = [string]$Context.plan.engine.binary_sha256
            kill_called = $killed
            wait_exited = $waitExited
            has_exited = $hasExited
        }
        pid_alive = $pidAlive
        listeners = $listeners
        runfile_cleanup = @($runfileCleanup)
        runfiles_after = $runfilesAfter
    }
}

function Resolve-S115AttemptDirectory {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Attempt
    )
    $name = $Attempt
    if ($Attempt -ceq 'latest') {
        $numbers = @(Get-S115NumericAttemptNames -Roots @($Context.tracked_attempt_root))
        if ($numbers.Count -eq 0) { throw 'no retained runtime attempts exist' }
        $name = ([int]($numbers | Measure-Object -Maximum).Maximum).ToString('000')
    }
    if ($name -notmatch '^[0-9]{3}$') { throw 'attempt must be latest or three digits' }
    $path = Join-Path $Context.tracked_attempt_root $name
    $safe = Test-S115SafeDirectoryTraversal -Root $Context.repository_root `
        -Path $path -RequireTarget
    if (-not $safe.passed) {
        throw "runtime attempt path is unsafe: $($safe.errors -join '; ')"
    }
    [pscustomobject]@{ id = $name; path = (Resolve-Path -LiteralPath $path).Path }
}
