Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Utf8NoBom = [System.Text.UTF8Encoding]::new($false)

function Get-RepositoryRoot {
    param([Parameter(Mandatory = $true)][string]$ArtifactDirectory)

    $root = (Resolve-Path -LiteralPath $ArtifactDirectory).Path
    for ($index = 0; $index -lt 16; $index++) {
        if ((Test-Path -LiteralPath (Join-Path $root '.git')) -and
            (Test-Path -LiteralPath (Join-Path $root 'Cargo.toml') -PathType Leaf)) {
            return $root
        }
        $parent = Split-Path -Parent $root
        if ([string]::IsNullOrWhiteSpace($parent) -or $parent -eq $root) {
            break
        }
        $root = $parent
    }
    throw "could not locate repository root above: $ArtifactDirectory"
}

function Write-Utf8Lf {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [AllowEmptyString()][Parameter(Mandatory = $true)][string]$Text
    )

    $normalized = $Text.Replace("`r`n", "`n").Replace("`r", "`n")
    [System.IO.File]::WriteAllText($Path, $normalized, $script:Utf8NoBom)
}

function Write-JsonLf {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value,
        [int]$Depth = 64,
        [switch]$Compress
    )

    $json = if ($Compress) {
        $Value | ConvertTo-Json -Depth $Depth -Compress
    }
    else {
        $Value | ConvertTo-Json -Depth $Depth
    }
    Write-Utf8Lf -Path $Path -Text ($json + "`n")
}

function Get-Sha256Lower {
    param([Parameter(Mandatory = $true)][string]$Path)

    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-Sha256Text {
    param([AllowEmptyString()][Parameter(Mandatory = $true)][string]$Text)

    $bytes = $script:Utf8NoBom.GetBytes($Text)
    $hash = [System.Security.Cryptography.SHA256]::HashData($bytes)
    [Convert]::ToHexString($hash).ToLowerInvariant()
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

function ConvertFrom-WindowsCommandLine {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$CommandLine)

    if ([string]::IsNullOrEmpty($CommandLine)) {
        throw 'Windows command line is empty'
    }
    if ([char]::IsWhiteSpace($CommandLine[0])) {
        throw 'Windows command line begins with whitespace'
    }
    if ($CommandLine.IndexOf([char]0) -ge 0) {
        throw 'Windows command line contains NUL'
    }

    $insideQuotes = $false
    $backslashes = 0
    foreach ($character in $CommandLine.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes++
            continue
        }
        if ($character -eq '"' -and ($backslashes % 2) -eq 0) {
            $insideQuotes = -not $insideQuotes
        }
        $backslashes = 0
    }
    if ($insideQuotes) {
        throw 'Windows command line has an unmatched quote'
    }

    if ($null -eq ('AnimusFerric.NativeCommandLine' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace AnimusFerric {
    public static class NativeCommandLine {
        [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        public static extern IntPtr CommandLineToArgvW(
            string commandLine,
            out int argumentCount
        );

        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern IntPtr LocalFree(IntPtr memory);
    }
}
'@
    }

    $argumentCount = 0
    $argumentVector =
        [AnimusFerric.NativeCommandLine]::CommandLineToArgvW(
            $CommandLine,
            [ref]$argumentCount
        )
    if ($argumentVector -eq [IntPtr]::Zero -or $argumentCount -lt 1) {
        $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "CommandLineToArgvW failed with Win32 error $code"
    }
    try {
        @(
            for ($index = 0; $index -lt $argumentCount; $index++) {
                $pointer = [Runtime.InteropServices.Marshal]::ReadIntPtr(
                    $argumentVector,
                    $index * [IntPtr]::Size
                )
                [Runtime.InteropServices.Marshal]::PtrToStringUni($pointer)
            }
        )
    }
    finally {
        [void][AnimusFerric.NativeCommandLine]::LocalFree($argumentVector)
    }
}

function Test-BoundWindowsProcessCommand {
    [CmdletBinding()]
    param(
        [AllowNull()][string]$ExecutablePath,
        [AllowNull()][string]$ExecutableSha256,
        [AllowNull()][string]$CommandLine,
        [Parameter(Mandatory = $true)][string]$FrozenExecutablePath,
        [Parameter(Mandatory = $true)][string]$FrozenExecutableSha256,
        [Parameter(Mandatory = $true)][string[]]$ExpectedArgv
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $observedArgv = @()
    $argv0Mode = $null
    $pathPassed = $false
    $recordedHashPassed = $false
    $liveHashPassed = $false
    $argv0Passed = $false
    $tailPassed = $false

    if ([string]::IsNullOrWhiteSpace($ExecutablePath) -or
        -not [System.IO.Path]::IsPathFullyQualified($ExecutablePath)) {
        $errors.Add('captured executable path is not fully qualified')
    }
    else {
        try {
            $pathPassed = [System.IO.Path]::GetFullPath($ExecutablePath).Equals(
                [System.IO.Path]::GetFullPath($FrozenExecutablePath),
                [System.StringComparison]::OrdinalIgnoreCase
            )
        }
        catch {
            $pathPassed = $false
        }
        if (-not $pathPassed) {
            $errors.Add('captured executable path differs from the frozen image')
        }
    }

    $recordedHashPassed =
        -not [string]::IsNullOrWhiteSpace($ExecutableSha256) -and
        $ExecutableSha256 -ceq $FrozenExecutableSha256
    if (-not $recordedHashPassed) {
        $errors.Add('captured executable hash differs from the frozen image')
    }
    if ($pathPassed -and
        (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
        $liveHashPassed =
            (Get-Sha256Lower -Path $ExecutablePath) -ceq
                $FrozenExecutableSha256
    }
    if (-not $liveHashPassed) {
        $errors.Add('live executable bytes differ from the frozen image')
    }

    if ($ExpectedArgv.Count -lt 1 -or $ExpectedArgv[0] -cne 'llama-server') {
        $errors.Add('frozen argv does not declare the exact llama-server alias')
    }
    else {
        try {
            $observedArgv = @(
                ConvertFrom-WindowsCommandLine -CommandLine $CommandLine
            )
        }
        catch {
            $errors.Add("command-line parse failed: $($_.Exception.Message)")
        }
    }

    if ($observedArgv.Count -gt 0) {
        $observedArgv0 = [string]$observedArgv[0]
        $bareAliasPassed =
            $observedArgv0 -ceq [string]$ExpectedArgv[0] -and
            $observedArgv0.IndexOfAny([char[]]@('/', '\', ':')) -lt 0 -and
            [System.IO.Path]::GetFileName($observedArgv0) -ceq $observedArgv0
        $absolutePassed = $false
        if ([System.IO.Path]::IsPathFullyQualified($observedArgv0)) {
            try {
                $absolutePassed =
                    [System.IO.Path]::GetFullPath($observedArgv0).Equals(
                        [System.IO.Path]::GetFullPath($FrozenExecutablePath),
                        [System.StringComparison]::OrdinalIgnoreCase
                    )
            }
            catch {
                $absolutePassed = $false
            }
        }
        if ($bareAliasPassed) {
            $argv0Mode = 'declared_bare_alias'
            $argv0Passed = $true
        }
        elseif ($absolutePassed) {
            $argv0Mode = 'frozen_absolute_path'
            $argv0Passed = $true
        }
        else {
            $errors.Add('command-line argv[0] is not an authorized image spelling')
        }

        if ($observedArgv.Count -eq $ExpectedArgv.Count) {
            $tailPassed = $true
            for ($index = 1; $index -lt $ExpectedArgv.Count; $index++) {
                if (-not [string]::Equals(
                        [string]$observedArgv[$index],
                        [string]$ExpectedArgv[$index],
                        [System.StringComparison]::Ordinal
                    )) {
                    $tailPassed = $false
                    break
                }
            }
        }
        if (-not $tailPassed) {
            $errors.Add('command-line argument tail differs from the frozen argv')
        }
    }

    [ordered]@{
        schema = 'animus-ferric-windows-process-command-binding-v1'
        passed = ($errors.Count -eq 0)
        executable_path_passed = $pathPassed
        recorded_executable_sha256_passed = $recordedHashPassed
        live_executable_sha256_passed = $liveHashPassed
        argv0_passed = $argv0Passed
        argv0_mode = $argv0Mode
        argument_tail_passed = $tailPassed
        expected_argc = $ExpectedArgv.Count
        observed_argc = $observedArgv.Count
        observed_argv = @($observedArgv)
        errors = @($errors)
    }
}

function ConvertTo-UtcIso8601 {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Value)

    if ($null -eq $Value) {
        throw 'date-time value is absent'
    }
    $instant = if ($Value -is [DateTimeOffset]) {
        [DateTimeOffset]$Value
    }
    elseif ($Value -is [DateTime]) {
        $dateTime = [DateTime]$Value
        if ($dateTime.Kind -eq [DateTimeKind]::Unspecified) {
            $dateTime = [DateTime]::SpecifyKind(
                $dateTime,
                [DateTimeKind]::Local
            )
        }
        [DateTimeOffset]$dateTime
    }
    else {
        [DateTimeOffset]::Parse(
            [string]$Value,
            [System.Globalization.CultureInfo]::InvariantCulture,
            [System.Globalization.DateTimeStyles]::RoundtripKind
        )
    }
    $instant.ToUniversalTime().ToString('o')
}

function Test-ProcessCreationWindow {
    [CmdletBinding()]
    param(
        [AllowNull()]$CreationDateUtc,
        [AllowNull()]$PreflightCapturedUtc,
        [AllowNull()]$AttestationCapturedUtc,
        [Parameter(Mandatory = $true)][int]$ToleranceSeconds
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $creation = [DateTimeOffset]::MinValue
    $preflightCaptured = [DateTimeOffset]::MinValue
    $attestationCaptured = [DateTimeOffset]::MinValue
    $normalizedCreation = try {
        ConvertTo-UtcIso8601 -Value $CreationDateUtc
    }
    catch { $null }
    $normalizedPreflight = try {
        ConvertTo-UtcIso8601 -Value $PreflightCapturedUtc
    }
    catch { $null }
    $normalizedAttestation = try {
        ConvertTo-UtcIso8601 -Value $AttestationCapturedUtc
    }
    catch { $null }
    $creationParsed = [DateTimeOffset]::TryParseExact(
        $normalizedCreation,
        'o',
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind,
        [ref]$creation
    )
    $preflightParsed = [DateTimeOffset]::TryParseExact(
        $normalizedPreflight,
        'o',
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind,
        [ref]$preflightCaptured
    )
    $capturedParsed = [DateTimeOffset]::TryParseExact(
        $normalizedAttestation,
        'o',
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind,
        [ref]$attestationCaptured
    )
    if (-not $creationParsed) {
        $errors.Add('process creation timestamp is not normalized ISO-8601')
    }
    if (-not $preflightParsed -or -not $capturedParsed -or
        $attestationCaptured -lt $preflightCaptured) {
        $errors.Add('preflight/attestation timestamps are invalid')
    }
    if ($ToleranceSeconds -lt 0 -or $ToleranceSeconds -gt 60) {
        $errors.Add('process creation tolerance is outside the safe range')
    }
    $windowPassed =
        $creationParsed -and
        $preflightParsed -and
        $capturedParsed -and
        $attestationCaptured -ge $preflightCaptured -and
        $ToleranceSeconds -ge 0 -and
        $ToleranceSeconds -le 60 -and
        $creation -ge $preflightCaptured.AddSeconds(-$ToleranceSeconds) -and
        $creation -le $attestationCaptured.AddSeconds($ToleranceSeconds)
    if (-not $windowPassed) {
        $errors.Add('process creation is outside the attested launch window')
    }

    [ordered]@{
        schema = 'animus-ferric-process-creation-window-v1'
        passed = ($errors.Count -eq 0)
        tolerance_seconds = $ToleranceSeconds
        creation_date_utc = if ($creationParsed) {
            $creation.ToUniversalTime().ToString('o')
        }
        else { $null }
        preflight_captured_utc = if ($preflightParsed) {
            $preflightCaptured.ToUniversalTime().ToString('o')
        }
        else { $null }
        attestation_captured_utc = if ($capturedParsed) {
            $attestationCaptured.ToUniversalTime().ToString('o')
        }
        else { $null }
        errors = @($errors)
    }
}

function ConvertTo-WindowsCommandLineArgument {
    param([AllowEmptyString()][Parameter(Mandatory = $true)][string]$Argument)

    if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') {
        return $Argument
    }
    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Argument.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes++
            continue
        }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * (($backslashes * 2) + 1)))
            [void]$builder.Append('"')
        }
        else {
            if ($backslashes -gt 0) {
                [void]$builder.Append(('\' * $backslashes))
            }
            [void]$builder.Append($character)
        }
        $backslashes = 0
    }
    if ($backslashes -gt 0) {
        [void]$builder.Append(('\' * ($backslashes * 2)))
    }
    [void]$builder.Append('"')
    $builder.ToString()
}

function Get-RelativeSlashPath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Path
    )

    [System.IO.Path]::GetRelativePath($Root, $Path).Replace('\', '/')
}

function Resolve-SafeRelativePath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath.IndexOf([char]0) -ge 0 -or
        $RelativePath -match '(^|[\\/])\.{1,2}([\\/]|$)') {
        throw "path is not a safe relative path: $RelativePath"
    }
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $rootPrefix = $rootFull + [System.IO.Path]::DirectorySeparatorChar
    $candidate = [System.IO.Path]::GetFullPath(
        (Join-Path $rootFull $RelativePath)
    )
    if (-not $candidate.StartsWith(
            $rootPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "relative path escapes its declared root: $RelativePath"
    }
    $candidate
}

function Get-TreeManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [string[]]$ExcludedPrefixes = @()
    )

    $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
    @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File -Force |
            ForEach-Object {
                $relative = Get-RelativeSlashPath -Root $resolvedRoot -Path $_.FullName
                $excluded = $false
                foreach ($prefix in $ExcludedPrefixes) {
                    if ($relative -eq $prefix -or $relative.StartsWith("$prefix/")) {
                        $excluded = $true
                        break
                    }
                }
                if (-not $excluded) {
                    [ordered]@{
                        path = $relative
                        bytes = [UInt64]$_.Length
                        sha256 = Get-Sha256Lower -Path $_.FullName
                    }
                }
            } |
            Sort-Object { $_.path }
    )
}

function Test-ManifestEqual {
    param(
        [Parameter(Mandatory = $true)]$Before,
        [Parameter(Mandatory = $true)]$After
    )

    (($Before | ConvertTo-Json -Depth 8 -Compress) -eq
        ($After | ConvertTo-Json -Depth 8 -Compress))
}

function Test-JsonEquivalent {
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Left,
        [AllowNull()][Parameter(Mandatory = $true)]$Right
    )

    ($Left | ConvertTo-Json -Depth 100 -Compress) -eq
        ($Right | ConvertTo-Json -Depth 100 -Compress)
}

function Test-RuntimePlanIdentity {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Plan)

    $null -ne $Plan -and
        $Plan.schema -ceq 'animus-ferric-runtime-plan-v3' -and
        $Plan.task -ceq 'T-11409' -and
        [int]$Plan.control_epoch -eq 3 -and
        $Plan.template_attestation.protocol -ceq
            'apply-template-differential-v1' -and
        $Plan.process_command_attestation.protocol -ceq
            'windows-bound-process-command-v1' -and
        $Plan.process_command_attestation.parser -ceq 'CommandLineToArgvW' -and
        $Plan.process_command_attestation.declared_bare_argv0 -ceq
            'llama-server' -and
        $Plan.process_command_attestation.absolute_argv0_policy -ceq
            'exact_frozen_executable_only' -and
        $Plan.process_command_attestation.tail_comparison -ceq
            'ordinal_elementwise_exact' -and
        [int]$Plan.process_command_attestation.creation_time_tolerance_seconds -eq
            5 -and
        $Plan.recovery.prior_evidence_checkpoint -ceq
            $Plan.repository_commit_before_epoch_3_runtime_controls -and
        (@($Plan.recovery.prior_epochs | ForEach-Object {
            [int]$_.epoch
        }) -join ',') -ceq '1,2'
}

function Get-EpochThreeStaticControlNames {
    @(
        '.gitattributes',
        'README.md',
        'runtime-plan.json',
        'nonce.txt',
        'smoke-prompt.txt',
        'trace-selftest-fixture.jsonl',
        'throughput-request.template.json',
        'template-probe-defaults.json',
        'template-probe-alias-false.json',
        'template-probe-all-false.json',
        'template-probe-all-true.json',
        'runtime-common.ps1',
        'freeze-runtime.ps1',
        'run-coordinate.ps1',
        'verify-runtime.ps1',
        'verify-q4-gate.ps1',
        'test-runtime.ps1',
        'record-q4-verdict.ps1',
        'finalize-selection.ps1'
    )
}

function Invoke-BoundedTextProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [int]$TimeoutMilliseconds = 10000
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "could not start process: $FilePath"
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutMilliseconds)) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit(5000)
            throw "process exceeded ${TimeoutMilliseconds}ms: $FilePath"
        }
        $tasks = [System.Threading.Tasks.Task[]]@($stdoutTask, $stderrTask)
        if (-not [System.Threading.Tasks.Task]::WaitAll($tasks, 5000)) {
            throw "process output did not close promptly: $FilePath"
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) {
            throw "process exited $($process.ExitCode): $FilePath $stderr"
        }
        @(
            $stdout.Replace("`r`n", "`n").Replace("`r", "`n").Split("`n") |
                Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
        )
    }
    finally {
        $process.Dispose()
    }
}

function Invoke-BoundedProcessResult {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [int]$TimeoutMilliseconds = 10000
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "could not start process: $FilePath"
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit($TimeoutMilliseconds)
        if ($timedOut) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit(5000)
        }
        $tasks = [System.Threading.Tasks.Task[]]@($stdoutTask, $stderrTask)
        $drained = [System.Threading.Tasks.Task]::WaitAll($tasks, 5000)
        [ordered]@{
            timed_out = ($timedOut -or -not $drained)
            execution_timed_out = $timedOut
            output_drain_timed_out = -not $drained
            exit_code = if (-not $timedOut -and $process.HasExited) {
                $process.ExitCode
            }
            else {
                $null
            }
            stdout = if ($stdoutTask.IsCompletedSuccessfully) {
                $stdoutTask.GetAwaiter().GetResult()
            }
            else {
                ''
            }
            stderr = if ($stderrTask.IsCompletedSuccessfully) {
                $stderrTask.GetAwaiter().GetResult()
            }
            else {
                ''
            }
        }
    }
    finally {
        $process.Dispose()
    }
}

function Invoke-PowerShellFileBounded {
    param(
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [string[]]$Arguments = @(),
        [int]$TimeoutMilliseconds = 300000
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = [Environment]::ProcessPath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-File', $ScriptPath
    ) + @($Arguments)) {
        $startInfo.ArgumentList.Add([string]$argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "could not start PowerShell script: $ScriptPath"
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutMilliseconds)) {
            try { $process.Kill($true) } catch { }
            [void]$process.WaitForExit(5000)
            throw "PowerShell script exceeded ${TimeoutMilliseconds}ms: $ScriptPath"
        }
        $tasks = [System.Threading.Tasks.Task[]]@($stdoutTask, $stderrTask)
        if (-not [System.Threading.Tasks.Task]::WaitAll($tasks, 5000)) {
            throw "PowerShell script output did not close promptly: $ScriptPath"
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

function Get-MemorySnapshot {
    Add-Type -AssemblyName Microsoft.VisualBasic
    $computerInfo = [Microsoft.VisualBasic.Devices.ComputerInfo]::new()
    $gpuQuery = @(Invoke-BoundedTextProcess -FilePath 'nvidia-smi.exe' `
        -Arguments @(
            '--query-gpu=name,uuid,driver_version,memory.total,memory.free,memory.used,utilization.gpu',
            '--format=csv,noheader,nounits'
        ))
    $computeApps = @(Invoke-BoundedTextProcess -FilePath 'nvidia-smi.exe' `
        -Arguments @(
            '--query-compute-apps=pid,process_name,used_memory',
            '--format=csv,noheader,nounits'
        ))
    $gpuFields = if ($gpuQuery.Count -eq 1) {
        @($gpuQuery[0].Split(',') | ForEach-Object { $_.Trim() })
    }
    else {
        @()
    }
    $gpu = if ($gpuFields.Count -eq 7) {
        [ordered]@{
            name = $gpuFields[0]
            uuid = $gpuFields[1]
            driver_version = $gpuFields[2]
            total_mib = [UInt64]$gpuFields[3]
            free_mib = [UInt64]$gpuFields[4]
            used_mib = [UInt64]$gpuFields[5]
            utilization_percent = [UInt32]$gpuFields[6]
        }
    }
    else {
        $null
    }

    [ordered]@{
        captured_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        ram = [ordered]@{
            total_visible_bytes = [UInt64]$computerInfo.TotalPhysicalMemory
            free_physical_bytes = [UInt64]$computerInfo.AvailablePhysicalMemory
            total_virtual_bytes = [UInt64]$computerInfo.TotalVirtualMemory
            free_virtual_bytes = [UInt64]$computerInfo.AvailableVirtualMemory
        }
        nvidia_smi_gpu_csv = $gpuQuery
        nvidia_smi_compute_apps_csv = $computeApps
        gpu = $gpu
    }
}

function Get-LlamaDeviceObservation {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$Output
    )

    $observations = [System.Collections.Generic.List[object]]::new()
    $pattern = '^\s*(?<backend>[A-Za-z0-9_-]+):\s+(?<name>.+?)\s+' +
        '\((?<total>[0-9]+) MiB,\s*(?<free>[0-9]+) MiB free\)\s*$'
    foreach ($line in @($Output)) {
        $match = [regex]::Match([string]$line, $pattern)
        if (-not $match.Success) {
            continue
        }
        $observations.Add([ordered]@{
            identity = [ordered]@{
                backend_id = $match.Groups['backend'].Value
                name = $match.Groups['name'].Value
                total_mib = [UInt64]$match.Groups['total'].Value
            }
            free_mib = [UInt64]$match.Groups['free'].Value
        })
    }
    if ($observations.Count -ne 1) {
        throw "llama-server device output contains $($observations.Count) parseable devices; exactly one is required"
    }
    $observations[0]
}

function Invoke-CapturedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath,
        [Parameter(Mandatory = $true)][int]$TimeoutMilliseconds,
        [hashtable]$Environment = @{}
    )

    $start = (Get-Date).ToUniversalTime()
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    foreach ($entry in $Environment.GetEnumerator()) {
        $startInfo.Environment[$entry.Key] = [string]$entry.Value
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $stdout = ''
    $stderr = ''
    $timedOut = $false
    $drainTimedOut = $false
    $exitCode = $null
    $processId = $null
    $killAttempted = $false
    $killSucceeded = $false
    $postProcessAlive = $false
    try {
        if (-not $process.Start()) {
            throw "could not start process: $FilePath"
        }
        $processId = [UInt32]$process.Id
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $exited = $process.WaitForExit($TimeoutMilliseconds)
        $timedOut = -not $exited
        if ($timedOut) {
            $killAttempted = $true
            try {
                $process.Kill($true)
                $killSucceeded = $true
            }
            catch { }
            if (-not $process.WaitForExit(5000)) {
                $drainTimedOut = $true
            }
        }
        $tasks = [System.Threading.Tasks.Task[]]@($stdoutTask, $stderrTask)
        if (-not [System.Threading.Tasks.Task]::WaitAll($tasks, 5000)) {
            $drainTimedOut = $true
        }
        if ($stdoutTask.IsCompletedSuccessfully) {
            $stdout = $stdoutTask.GetAwaiter().GetResult()
        }
        if ($stderrTask.IsCompletedSuccessfully) {
            $stderr = $stderrTask.GetAwaiter().GetResult()
        }
        if (-not $timedOut -and -not $drainTimedOut -and $process.HasExited) {
            $exitCode = $process.ExitCode
        }
        $postProcessAlive = -not $process.HasExited
    }
    finally {
        $stopwatch.Stop()
        Write-Utf8Lf -Path $StdoutPath -Text $stdout
        Write-Utf8Lf -Path $StderrPath -Text $stderr
        $process.Dispose()
    }

    [ordered]@{
        file = $FilePath
        arguments = $Arguments
        pid = $processId
        started_at_utc = $start.ToString('o')
        completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        duration_ms = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
        timed_out = ($timedOut -or $drainTimedOut)
        execution_timed_out = $timedOut
        kill_attempted = $killAttempted
        kill_succeeded = $killSucceeded
        post_process_alive = $postProcessAlive
        output_drain_timed_out = $drainTimedOut
        exit_code = $exitCode
        stdout_file = [System.IO.Path]::GetFileName($StdoutPath)
        stderr_file = [System.IO.Path]::GetFileName($StderrPath)
    }
}

function Invoke-FileRedirectedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath,
        [Parameter(Mandatory = $true)][int]$TimeoutMilliseconds,
        [Parameter(Mandatory = $true)][hashtable]$Environment
    )

    $start = (Get-Date).ToUniversalTime()
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $argumentLine = (@(
        $Arguments | ForEach-Object {
            ConvertTo-WindowsCommandLineArgument -Argument ([string]$_)
        }
    ) -join ' ')
    $process = Start-Process -FilePath $FilePath -ArgumentList $argumentLine `
        -WorkingDirectory $WorkingDirectory -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutPath -RedirectStandardError $StderrPath `
        -Environment $Environment -PassThru
    try {
        $processId = [UInt32]$process.Id
        $exited = $process.WaitForExit($TimeoutMilliseconds)
        $timedOut = -not $exited
        $postKillTimedOut = $false
        $killAttempted = $false
        $killSucceeded = $false
        if ($timedOut) {
            $killAttempted = $true
            try {
                $process.Kill($true)
                $killSucceeded = $true
            }
            catch { }
            $postKillTimedOut = -not $process.WaitForExit(5000)
        }
        $exitCode = if (-not $timedOut -and $process.HasExited) {
            $process.ExitCode
        }
        else {
            $null
        }
        $postProcessAlive = -not $process.HasExited
    }
    finally {
        $stopwatch.Stop()
        $process.Dispose()
    }

    [ordered]@{
        file = $FilePath
        arguments = $Arguments
        pid = $processId
        argument_line = $argumentLine
        started_at_utc = $start.ToString('o')
        completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        duration_ms = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
        timed_out = ($timedOut -or $postKillTimedOut)
        execution_timed_out = $timedOut
        post_kill_wait_timed_out = $postKillTimedOut
        kill_attempted = $killAttempted
        kill_succeeded = $killSucceeded
        post_process_alive = $postProcessAlive
        exit_code = $exitCode
        stdout_file = [System.IO.Path]::GetFileName($StdoutPath)
        stderr_file = [System.IO.Path]::GetFileName($StderrPath)
    }
}

function Test-StartupMemoryPressure {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string[]]$Patterns
    )

    $matches = [System.Collections.Generic.List[object]]::new()
    foreach ($pattern in $Patterns) {
        $regexMatches = [regex]::Matches(
            $Text,
            $pattern,
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        foreach ($match in $regexMatches) {
            $lineStart = $Text.LastIndexOf("`n", [Math]::Max(0, $match.Index - 1)) + 1
            $lineEnd = $Text.IndexOf("`n", $match.Index)
            if ($lineEnd -lt 0) {
                $lineEnd = $Text.Length
            }
            $matches.Add([ordered]@{
                pattern = $pattern
                offset = $match.Index
                line = $Text.Substring($lineStart, $lineEnd - $lineStart).Trim()
            })
        }
    }

    [ordered]@{
        matched = ($matches.Count -gt 0)
        matches = @($matches)
    }
}

function Get-FileIdentityManifest {
    param([Parameter(Mandatory = $true)][string]$Root)

    $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
    @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File -Force |
            ForEach-Object {
                [ordered]@{
                    path = Get-RelativeSlashPath -Root $resolvedRoot `
                        -Path $_.FullName
                    bytes = [UInt64]$_.Length
                    sha256 = Get-Sha256Lower -Path $_.FullName
                }
            } |
            Sort-Object { $_.path }
    )
}

function Test-FileIdentityManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][object[]]$Expected
    )

    $actual = @(Get-FileIdentityManifest -Root $Root)
    $errors = [System.Collections.Generic.List[string]]::new()
    if ($actual.Count -ne $Expected.Count) {
        $errors.Add(
            "runtime file count differs: expected $($Expected.Count), actual $($actual.Count)"
        )
    }
    $expectedByPath = @{}
    foreach ($entry in $Expected) {
        $expectedByPath[[string]$entry.path] = $entry
    }
    $actualByPath = @{}
    foreach ($entry in $actual) {
        $actualByPath[[string]$entry.path] = $entry
    }
    foreach ($path in @($expectedByPath.Keys | Sort-Object)) {
        if (-not $actualByPath.ContainsKey($path)) {
            $errors.Add("frozen runtime file is absent: $path")
            continue
        }
        $expectedEntry = $expectedByPath[$path]
        $actualEntry = $actualByPath[$path]
        if ([UInt64]$actualEntry.bytes -ne [UInt64]$expectedEntry.bytes -or
            $actualEntry.sha256 -ne $expectedEntry.sha256) {
            $errors.Add("frozen runtime file identity differs: $path")
        }
    }
    foreach ($path in @($actualByPath.Keys | Sort-Object)) {
        if (-not $expectedByPath.ContainsKey($path)) {
            $errors.Add("unfrozen runtime file is present: $path")
        }
    }

    [ordered]@{
        passed = ($errors.Count -eq 0)
        expected_file_count = $Expected.Count
        actual_file_count = $actual.Count
        errors = @($errors)
        actual = $actual
    }
}

function Test-RecoveryAnchors {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RepositoryRoot
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $actual = [System.Collections.Generic.List[object]]::new()
    $epochResults = [System.Collections.Generic.List[object]]::new()
    $repositoryFull = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $repositoryPrefix =
        $repositoryFull + [System.IO.Path]::DirectorySeparatorChar
    $priorEpochs = @($Plan.recovery.prior_epochs)
    if ((@($priorEpochs | ForEach-Object { [int]$_.epoch }) -join ',') -cne
        '1,2') {
        $errors.Add('recovery plan does not declare exact prior epochs 1 and 2')
    }

    foreach ($priorEpoch in $priorEpochs) {
        $epoch = [int]$priorEpoch.epoch
        $epochErrors = [System.Collections.Generic.List[string]]::new()
        $safeAnchorPaths = [System.Collections.Generic.HashSet[string]]::new(
            [System.StringComparer]::Ordinal
        )
        $anchors = @($priorEpoch.anchors)
        $anchorPaths = @($anchors | ForEach-Object {
            [string]$_.relative_path
        })
        if ($anchors.Count -lt 5 -or
            @($anchorPaths | Select-Object -Unique).Count -ne $anchorPaths.Count) {
            $epochErrors.Add('recovery anchors are missing or duplicate')
        }
        foreach ($anchor in $anchors) {
            $relativePath = [string]$anchor.relative_path
            $path = $null
            $pathSafe =
                -not [string]::IsNullOrWhiteSpace($relativePath) -and
                -not [System.IO.Path]::IsPathRooted($relativePath) -and
                $relativePath.IndexOf([char]0) -lt 0 -and
                $relativePath -notmatch '(^|[\\/])\.{1,2}([\\/]|$)'
            if ($pathSafe) {
                try {
                    $path = [System.IO.Path]::GetFullPath(
                        (Join-Path $RepositoryRoot $relativePath)
                    )
                    $pathSafe = $path.StartsWith(
                        $repositoryPrefix,
                        [System.StringComparison]::OrdinalIgnoreCase
                    )
                }
                catch {
                    $pathSafe = $false
                }
            }
            if ($pathSafe) {
                [void]$safeAnchorPaths.Add($relativePath)
            }
            else {
                $epochErrors.Add("recovery anchor path is unsafe: $relativePath")
            }
            $exists = $pathSafe -and
                (Test-Path -LiteralPath $path -PathType Leaf)
            $bytes = if ($exists) {
                [UInt64](Get-Item -LiteralPath $path).Length
            }
            else {
                $null
            }
            $sha256 = if ($exists) { Get-Sha256Lower -Path $path } else { $null }
            $passed =
                $pathSafe -and
                $exists -and
                [UInt64]$bytes -eq [UInt64]$anchor.bytes -and
                $sha256 -ceq [string]$anchor.sha256
            if (-not $passed) {
                $epochErrors.Add("recovery anchor differs: $relativePath")
            }
            $actual.Add([ordered]@{
                epoch = $epoch
                role = [string]$anchor.role
                relative_path = $relativePath
                expected_bytes = [UInt64]$anchor.bytes
                actual_bytes = $bytes
                expected_sha256 = [string]$anchor.sha256
                actual_sha256 = $sha256
                exists = $exists
                path_safe = $pathSafe
                passed = $passed
            })
        }

        $controlAnchor = @($anchors | Where-Object {
            $_.role -ceq 'control_manifest'
        })
        $digestAnchor = @($anchors | Where-Object {
            $_.role -ceq 'control_digest'
        })
        $attemptManifestAnchor = @($anchors | Where-Object {
            $_.role -ceq 'attempt_manifest'
        })
        $attemptAnchor = @($anchors | Where-Object {
            $_.role -ceq 'attempt'
        })
        $protectionAnchors = @($anchors | Where-Object {
            $_.role -in @('text_policy', 'attempt_text_policy')
        })
        $roleShapePassed =
            $controlAnchor.Count -eq 1 -and
            $digestAnchor.Count -eq 1 -and
            $attemptManifestAnchor.Count -eq 1 -and
            $attemptAnchor.Count -eq 1 -and
            $protectionAnchors.Count -ge 1
        if (-not $roleShapePassed) {
            $epochErrors.Add('recovery anchor roles are incomplete')
        }

        $controlManifestPassed = $false
        $controlDigestPassed = $false
        if ($controlAnchor.Count -eq 1 -and $digestAnchor.Count -eq 1 -and
            $safeAnchorPaths.Contains(
                [string]$controlAnchor[0].relative_path
            ) -and
            $safeAnchorPaths.Contains(
                [string]$digestAnchor[0].relative_path
            )) {
            $controlManifestPath = Join-Path $RepositoryRoot `
                ([string]$controlAnchor[0].relative_path)
            $controlDigestPath = Join-Path $RepositoryRoot `
                ([string]$digestAnchor[0].relative_path)
            try {
                $controlManifest = Get-Content -Raw `
                    -LiteralPath $controlManifestPath | ConvertFrom-Json
                $controlRoot = Split-Path -Parent $controlManifestPath
                $controlPaths = @($controlManifest.controls | ForEach-Object {
                    [string]$_.path
                })
                $controlsPassed =
                    $controlPaths.Count -gt 0 -and
                    @($controlPaths | Select-Object -Unique).Count -eq
                        $controlPaths.Count -and
                    @($controlManifest.controls | Where-Object {
                        $controlPath = [System.IO.Path]::GetFullPath(
                            (Join-Path $controlRoot ([string]$_.path))
                        )
                        $controlPrefix =
                            [System.IO.Path]::GetFullPath($controlRoot).TrimEnd(
                                [System.IO.Path]::DirectorySeparatorChar,
                                [System.IO.Path]::AltDirectorySeparatorChar
                            ) + [System.IO.Path]::DirectorySeparatorChar
                        -not $controlPath.StartsWith(
                            $controlPrefix,
                            [System.StringComparison]::OrdinalIgnoreCase
                        ) -or
                        -not (Test-Path -LiteralPath $controlPath -PathType Leaf) -or
                        [UInt64](Get-Item -LiteralPath $controlPath).Length -ne
                            [UInt64]$_.bytes -or
                        (Get-Sha256Lower -Path $controlPath) -cne
                            [string]$_.sha256
                    }).Count -eq 0
                $identityPassed =
                    $controlManifest.schema -ceq
                        [string]$priorEpoch.expected.control_schema
                $expectedTask = Get-OptionalProperty `
                    -Value $priorEpoch.expected -Name 'control_task'
                if ($null -ne $expectedTask) {
                    $identityPassed = $identityPassed -and
                        $controlManifest.task -ceq [string]$expectedTask
                }
                $expectedControlEpoch = Get-OptionalProperty `
                    -Value $priorEpoch.expected -Name 'control_epoch'
                if ($null -ne $expectedControlEpoch) {
                    $identityPassed = $identityPassed -and
                        [int]$controlManifest.control_epoch -eq
                            [int]$expectedControlEpoch
                }
                $expectedProtocol = Get-OptionalProperty `
                    -Value $priorEpoch.expected -Name 'attestation_protocol'
                if ($null -ne $expectedProtocol) {
                    $identityPassed = $identityPassed -and
                        $controlManifest.attestation_protocol -ceq
                            [string]$expectedProtocol
                }
                $controlManifestPassed = $controlsPassed -and $identityPassed
                $expectedDigest =
                    "$([string]$controlAnchor[0].sha256)  control-inputs.json"
                $actualDigest = (Get-Content -Raw `
                    -LiteralPath $controlDigestPath).Trim()
                $controlDigestPassed = $actualDigest -ceq $expectedDigest
            }
            catch {
                $controlManifestPassed = $false
                $controlDigestPassed = $false
                $epochErrors.Add(
                    "frozen control validation raised: $($_.Exception.Message)"
                )
            }
        }
        if (-not $controlManifestPassed) {
            $epochErrors.Add('frozen control files do not match their manifest')
        }
        if (-not $controlDigestPassed) {
            $epochErrors.Add('frozen control digest does not bind its manifest')
        }

        $attemptManifestPassed = $false
        $attemptIdentityPassed = $false
        $gitProtectionPassed = $false
        if ($attemptManifestAnchor.Count -eq 1 -and
            $attemptAnchor.Count -eq 1 -and
            $safeAnchorPaths.Contains(
                [string]$attemptManifestAnchor[0].relative_path
            ) -and
            $safeAnchorPaths.Contains(
                [string]$attemptAnchor[0].relative_path
            )) {
            $attemptManifestPath = Join-Path $RepositoryRoot `
                ([string]$attemptManifestAnchor[0].relative_path)
            $attemptPath = Join-Path $RepositoryRoot `
                ([string]$attemptAnchor[0].relative_path)
            if (Test-Path -LiteralPath $attemptManifestPath -PathType Leaf) {
                $attemptManifestCheck = Test-HashManifest `
                    -Root (Split-Path -Parent $attemptManifestPath) `
                    -ManifestPath $attemptManifestPath `
                    -RejectUnlistedFiles
                $attemptManifestPassed = [bool]$attemptManifestCheck.passed
            }
            try {
                $attempt = Get-Content -Raw -LiteralPath $attemptPath |
                    ConvertFrom-Json
                $attemptIdentityPassed =
                    $attempt.schema -ceq
                        [string]$priorEpoch.expected.attempt_schema -and
                    $attempt.coordinate -ceq
                        [string]$priorEpoch.expected.coordinate -and
                    $attempt.failure_classification -ceq
                        [string]$priorEpoch.expected.failure_classification -and
                    $attempt.verdict -ceq
                        [string]$priorEpoch.expected.verdict -and
                    [bool]$attempt.evidence_complete -eq
                        [bool]$priorEpoch.expected.evidence_complete -and
                    [bool]$attempt.teardown.passed -eq
                        [bool]$priorEpoch.expected.teardown_passed
            }
            catch {
                $attemptIdentityPassed = $false
            }
            try {
                $attributeOutput = @(Invoke-BoundedTextProcess -FilePath 'git' `
                    -Arguments @(
                        '-C',
                        $RepositoryRoot,
                        'check-attr',
                        'text',
                        '--',
                        [string]$attemptAnchor[0].relative_path
                    ))
                $gitProtectionPassed =
                    $attributeOutput.Count -eq 1 -and
                    $attributeOutput[0].EndsWith(
                        ': text: unset',
                        [System.StringComparison]::Ordinal
                    )
            }
            catch {
                $gitProtectionPassed = $false
            }
        }
        if (-not $attemptManifestPassed) {
            $epochErrors.Add('attempt files do not match their exact-tree manifest')
        }
        if (-not $attemptIdentityPassed) {
            $epochErrors.Add('attempt terminal identity differs from recovery record')
        }
        if (-not $gitProtectionPassed) {
            $epochErrors.Add('attempt bytes are not protected from Git text conversion')
        }

        foreach ($epochError in $epochErrors) {
            $errors.Add("epoch-${epoch}: $epochError")
        }
        $epochResults.Add([ordered]@{
            epoch = $epoch
            passed = ($epochErrors.Count -eq 0)
            errors = @($epochErrors)
            anchor_roles_passed = $roleShapePassed
            control_manifest_passed = $controlManifestPassed
            control_digest_passed = $controlDigestPassed
            attempt_manifest_passed = $attemptManifestPassed
            attempt_identity_passed = $attemptIdentityPassed
            git_protection_passed = $gitProtectionPassed
        })
    }

    [ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
        actual = @($actual)
        epochs = @($epochResults)
    }
}

function Test-EpochThreeMeasurementContinuity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RepositoryRoot
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $epochTwo = @($Plan.recovery.prior_epochs | Where-Object {
        [int]$_.epoch -eq 2
    })
    $epochTwoPlanPath = $null
    $epochTwoPlan = $null
    if ($epochTwo.Count -eq 1) {
        $controlAnchor = @($epochTwo[0].anchors | Where-Object {
            $_.role -ceq 'control_manifest'
        })
        if ($controlAnchor.Count -eq 1) {
            try {
                $controlRelative = [string]$controlAnchor[0].relative_path
                [void](Resolve-SafeRelativePath -Root $RepositoryRoot `
                    -RelativePath $controlRelative)
                $planRelative = Join-Path `
                    (Split-Path -Parent $controlRelative) 'runtime-plan.json'
                $epochTwoPlanPath = Resolve-SafeRelativePath `
                    -Root $RepositoryRoot -RelativePath $planRelative
            }
            catch {
                $errors.Add(
                    "epoch-2 continuity anchor path is unsafe: $($_.Exception.Message)"
                )
            }
        }
    }
    if ($null -eq $epochTwoPlanPath -or
        -not (Test-Path -LiteralPath $epochTwoPlanPath -PathType Leaf)) {
        $errors.Add('epoch-2 runtime plan is unavailable for continuity checks')
    }
    else {
        try {
            $epochTwoPlan = Get-Content -Raw -LiteralPath $epochTwoPlanPath |
                ConvertFrom-Json
        }
        catch {
            $errors.Add('epoch-2 runtime plan is not valid JSON')
        }
    }

    $fieldResults = [System.Collections.Generic.List[object]]::new()
    if ($null -ne $epochTwoPlan) {
        if ($epochTwoPlan.schema -cne 'animus-ferric-runtime-plan-v2' -or
            $epochTwoPlan.task -cne 'T-11409' -or
            [int]$epochTwoPlan.control_epoch -ne 2) {
            $errors.Add('epoch-2 runtime plan identity is invalid')
        }
        foreach ($field in @(
            'port',
            'quant_wall_cap_seconds',
            'server_request_timeout_seconds',
            'startup_wait_seconds',
            'teardown_safety_grace_seconds',
            'minimum_gpu_free_mib_before_launch',
            'forbidden_inherited_environment',
            'smoke',
            'throughput',
            'template_attestation',
            'models',
            'server',
            'llama_cpp',
            'ferric',
            'startup_memory_patterns',
            'selection'
        )) {
            $passed = Test-JsonEquivalent `
                -Left $Plan.$field -Right $epochTwoPlan.$field
            if (-not $passed) {
                $errors.Add("measurement contract drifted: $field")
            }
            $fieldResults.Add([ordered]@{
                field = $field
                passed = $passed
            })
        }
        $expectedRawRoot = ([string]$epochTwoPlan.raw_attempt_root).Replace(
            'runtime-epoch-2',
            'runtime-epoch-3'
        )
        $rawRootPassed =
            [string]$Plan.raw_attempt_root -ceq $expectedRawRoot
        if (-not $rawRootPassed) {
            $errors.Add('epoch-3 raw root is not the exact disjoint epoch rename')
        }
        $epochThreeCoordinates = @($Plan.coordinates) |
            ConvertTo-Json -Depth 16 -Compress
        $normalizedCoordinates = $epochThreeCoordinates.Replace('e03-', 'e02-') |
            ConvertFrom-Json
        $coordinatesPassed = Test-JsonEquivalent `
            -Left @($normalizedCoordinates) -Right @($epochTwoPlan.coordinates)
        if (-not $coordinatesPassed) {
            $errors.Add('epoch-3 coordinates change more than their epoch prefix')
        }
    }
    else {
        $rawRootPassed = $false
        $coordinatesPassed = $false
    }

    [ordered]@{
        schema = 'animus-ferric-epoch-3-measurement-continuity-v1'
        passed = ($errors.Count -eq 0)
        epoch_2_runtime_plan = if ($null -ne $epochTwoPlanPath) {
            Get-RelativeSlashPath -Root $RepositoryRoot -Path $epochTwoPlanPath
        }
        else {
            $null
        }
        fields = @($fieldResults)
        raw_root_rename_passed = $rawRootPassed
        coordinate_prefix_rename_passed = $coordinatesPassed
        errors = @($errors)
    }
}

function Get-TemplateProbeFacts {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$ArtifactDirectory,
        [Parameter(Mandatory = $true)][string]$EvidenceDirectory
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $armFacts = [System.Collections.Generic.List[object]]::new()
    $prompts = @{}
    $propsPath = Join-Path $EvidenceDirectory 'props.body.json'
    $props = $null
    if (Test-Path -LiteralPath $propsPath -PathType Leaf) {
        try {
            $props = Get-Content -Raw -LiteralPath $propsPath | ConvertFrom-Json
        }
        catch {
            $errors.Add('template probe cannot parse retained /props response')
        }
    }
    else {
        $errors.Add('template probe is missing retained /props response')
    }

    $chatTemplate = $null
    $supportsPreserveReasoning = $null
    if ($null -ne $props) {
        $chatTemplateProperty = $props.PSObject.Properties['chat_template']
        if ($null -ne $chatTemplateProperty) {
            $chatTemplate = [string]$chatTemplateProperty.Value
        }
        $capsProperty = $props.PSObject.Properties['chat_template_caps']
        if ($null -ne $capsProperty -and $null -ne $capsProperty.Value) {
            $supportProperty =
                $capsProperty.Value.PSObject.Properties['supports_preserve_reasoning']
            if ($null -ne $supportProperty) {
                $supportsPreserveReasoning = $supportProperty.Value
            }
        }
    }
    $actualTemplateSha256 = if ($null -ne $chatTemplate) {
        Get-Sha256Text -Text $chatTemplate
    }
    else {
        $null
    }
    $sourcePassed =
        -not [string]::IsNullOrEmpty($chatTemplate) -and
        $actualTemplateSha256 -eq
            [string]$Plan.template_attestation.expected_chat_template_sha256
    if (-not $sourcePassed) {
        $errors.Add('retained /props chat template differs from the frozen source hash')
    }
    if ($supportsPreserveReasoning -ne $true) {
        $errors.Add('retained chat-template capabilities do not support reasoning preservation')
    }

    $expectedNames = @('defaults', 'alias-false', 'all-false', 'all-true')
    $declaredNames = @($Plan.template_attestation.arms | ForEach-Object {
        [string]$_.name
    })
    if (($declaredNames -join "`n") -ne ($expectedNames -join "`n")) {
        $errors.Add('template probe arms differ from the frozen four-arm protocol')
    }

    $userPrior = [string]$Plan.template_attestation.user_prior_sentinel
    $reasoning = [string]$Plan.template_attestation.reasoning_sentinel
    $content = [string]$Plan.template_attestation.content_sentinel
    $userLatest = [string]$Plan.template_attestation.user_latest_sentinel
    $reasoningBlock = "<think>`n$reasoning`n</think>`n`n"
    $generationSuffix = "<|im_start|>assistant`n<think>`n"

    foreach ($arm in @($Plan.template_attestation.arms)) {
        $name = [string]$arm.name
        $controlRequestPath = Join-Path $ArtifactDirectory ([string]$arm.request_file)
        $retainedRequestPath = Join-Path $EvidenceDirectory `
            "template-probe.$name.request.json"
        $responsePath = Join-Path $EvidenceDirectory `
            "template-probe.$name.response.json"
        $armErrors = [System.Collections.Generic.List[string]]::new()

        $requestIdentityPassed =
            (Test-Path -LiteralPath $controlRequestPath -PathType Leaf) -and
            (Test-Path -LiteralPath $retainedRequestPath -PathType Leaf) -and
            ((Get-Item -LiteralPath $controlRequestPath).Length -eq
                (Get-Item -LiteralPath $retainedRequestPath).Length) -and
            ((Get-Sha256Lower -Path $controlRequestPath) -eq
                (Get-Sha256Lower -Path $retainedRequestPath))
        if (-not $requestIdentityPassed) {
            $armErrors.Add('retained request is not byte-identical to its frozen control')
        }

        $request = $null
        if (Test-Path -LiteralPath $retainedRequestPath -PathType Leaf) {
            try {
                $request = Get-Content -Raw -LiteralPath $retainedRequestPath |
                    ConvertFrom-Json
            }
            catch {
                $armErrors.Add('retained request is malformed JSON')
            }
        }
        $requestShapePassed = $false
        if ($null -ne $request) {
            $topLevelNames = @($request.PSObject.Properties.Name | Sort-Object)
            $expectedTopLevelNames = if ($name -eq 'defaults') {
                @('add_generation_prompt', 'messages')
            }
            else {
                @('add_generation_prompt', 'chat_template_kwargs', 'messages')
            }
            $messages = @($request.messages)
            $requestShapePassed =
                ($topLevelNames -join "`n") -eq
                    ($expectedTopLevelNames -join "`n") -and
                $request.add_generation_prompt -eq $true -and
                $messages.Count -eq 3 -and
                [string]$messages[0].role -eq 'user' -and
                [string]$messages[0].content -eq $userPrior -and
                [string]$messages[1].role -eq 'assistant' -and
                [string]$messages[1].reasoning_content -eq $reasoning -and
                [string]$messages[1].content -eq $content -and
                [string]$messages[2].role -eq 'user' -and
                [string]$messages[2].content -eq $userLatest

            if ($requestShapePassed) {
                $kwargsProperty =
                    $request.PSObject.Properties['chat_template_kwargs']
                switch ($name) {
                    'defaults' {
                        $requestShapePassed = $null -eq $kwargsProperty
                    }
                    'alias-false' {
                        $names = @($kwargsProperty.Value.PSObject.Properties.Name |
                            Sort-Object)
                        $requestShapePassed =
                            ($names -join "`n") -eq 'preserve_reasoning' -and
                            $kwargsProperty.Value.preserve_reasoning -eq $false
                    }
                    'all-false' {
                        $names = @($kwargsProperty.Value.PSObject.Properties.Name |
                            Sort-Object)
                        $requestShapePassed =
                            ($names -join "`n") -eq
                                "preserve_reasoning`npreserve_thinking" -and
                            $kwargsProperty.Value.preserve_reasoning -eq $false -and
                            $kwargsProperty.Value.preserve_thinking -eq $false
                    }
                    'all-true' {
                        $names = @($kwargsProperty.Value.PSObject.Properties.Name |
                            Sort-Object)
                        $requestShapePassed =
                            ($names -join "`n") -eq
                                "preserve_reasoning`npreserve_thinking" -and
                            $kwargsProperty.Value.preserve_reasoning -eq $true -and
                            $kwargsProperty.Value.preserve_thinking -eq $true
                    }
                    default {
                        $requestShapePassed = $false
                    }
                }
            }
        }
        if (-not $requestShapePassed) {
            $armErrors.Add('retained request differs from the four-arm sentinel protocol')
        }

        $response = $null
        if (Test-Path -LiteralPath $responsePath -PathType Leaf) {
            try {
                $response = Get-Content -Raw -LiteralPath $responsePath |
                    ConvertFrom-Json
            }
            catch {
                $armErrors.Add('retained response is malformed JSON')
            }
        }
        else {
            $armErrors.Add('retained response is absent')
        }
        $prompt = $null
        if ($null -ne $response) {
            $promptProperty = $response.PSObject.Properties['prompt']
            if ($null -ne $promptProperty -and $promptProperty.Value -is [string]) {
                $prompt = [string]$promptProperty.Value
                $prompts[$name] = $prompt
            }
        }
        if ($null -eq $prompt) {
            $armErrors.Add('retained response does not contain a string prompt')
        }

        $reasoningCount = if ($null -ne $prompt) {
            [regex]::Matches($prompt, [regex]::Escape($reasoning)).Count
        }
        else { 0 }
        $reasoningBlockCount = if ($null -ne $prompt) {
            [regex]::Matches($prompt, [regex]::Escape($reasoningBlock)).Count
        }
        else { 0 }
        $contentCount = if ($null -ne $prompt) {
            [regex]::Matches($prompt, [regex]::Escape($content)).Count
        }
        else { 0 }
        $priorCount = if ($null -ne $prompt) {
            [regex]::Matches($prompt, [regex]::Escape($userPrior)).Count
        }
        else { 0 }
        $latestCount = if ($null -ne $prompt) {
            [regex]::Matches($prompt, [regex]::Escape($userLatest)).Count
        }
        else { 0 }
        $generationPrefixPassed =
            $null -ne $prompt -and $prompt.EndsWith(
                $generationSuffix,
                [System.StringComparison]::Ordinal
            )
        $reasoningExpectationPassed = if ([bool]$arm.expect_reasoning) {
            $reasoningCount -eq 1 -and $reasoningBlockCount -eq 1
        }
        else {
            $reasoningCount -eq 0 -and $reasoningBlockCount -eq 0
        }
        $structurePassed =
            $null -ne $prompt -and
            $priorCount -eq 1 -and
            $contentCount -eq 1 -and
            $latestCount -eq 1 -and
            $reasoningExpectationPassed -and
            $generationPrefixPassed
        if (-not $structurePassed) {
            $armErrors.Add('rendered prompt does not meet sentinel and generation-prefix expectations')
        }

        foreach ($message in $armErrors) {
            $errors.Add("template probe arm $name`: $message")
        }
        $armFacts.Add([ordered]@{
            name = $name
            expected_reasoning = [bool]$arm.expect_reasoning
            passed = ($armErrors.Count -eq 0)
            errors = @($armErrors)
            request_file = [System.IO.Path]::GetFileName($retainedRequestPath)
            request_sha256 = if (Test-Path -LiteralPath $retainedRequestPath) {
                Get-Sha256Lower -Path $retainedRequestPath
            } else { $null }
            response_file = [System.IO.Path]::GetFileName($responsePath)
            response_sha256 = if (Test-Path -LiteralPath $responsePath) {
                Get-Sha256Lower -Path $responsePath
            } else { $null }
            request_identity_passed = $requestIdentityPassed
            request_shape_passed = $requestShapePassed
            reasoning_sentinel_count = $reasoningCount
            reasoning_block_count = $reasoningBlockCount
            content_sentinel_count = $contentCount
            prior_user_sentinel_count = $priorCount
            latest_user_sentinel_count = $latestCount
            generation_prefix_passed = $generationPrefixPassed
        })
    }

    $equalPositiveArms =
        $prompts.ContainsKey('defaults') -and
        $prompts.ContainsKey('alias-false') -and
        $prompts.ContainsKey('all-true') -and
        $prompts['defaults'] -ceq $prompts['alias-false'] -and
        $prompts['defaults'] -ceq $prompts['all-true']
    $negativeArmIsPositiveWithoutReasoning =
        $prompts.ContainsKey('defaults') -and
        $prompts.ContainsKey('all-false') -and
        $prompts['defaults'].Replace($reasoningBlock, '') -ceq
            $prompts['all-false']
    if (-not $equalPositiveArms) {
        $errors.Add('positive template-probe arms are not byte-identical')
    }
    if (-not $negativeArmIsPositiveWithoutReasoning) {
        $errors.Add('negative template-probe arm differs by more than the reasoning block')
    }

    [ordered]@{
        schema = 'animus-ferric-template-attestation-v1'
        protocol = [string]$Plan.template_attestation.protocol
        passed = ($errors.Count -eq 0)
        errors = @($errors)
        source = [ordered]@{
            expected_chat_template_sha256 =
                [string]$Plan.template_attestation.expected_chat_template_sha256
            actual_chat_template_sha256 = $actualTemplateSha256
            exact_source_passed = $sourcePassed
            supports_preserve_reasoning = $supportsPreserveReasoning
        }
        arms = @($armFacts)
        differential = [ordered]@{
            positive_arms_byte_identical = $equalPositiveArms
            negative_arm_is_positive_without_reasoning =
                $negativeArmIsPositiveWithoutReasoning
            preserve_thinking_default_effective =
                ($equalPositiveArms -and $negativeArmIsPositiveWithoutReasoning)
            thinking_generation_prefix_effective =
                @($armFacts | Where-Object {
                    -not $_.generation_prefix_passed
                }).Count -eq 0
        }
    }
}

function Invoke-HttpExchange {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('GET', 'POST')][string]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$ResponsePath,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds,
        [string]$RequestBodyPath
    )

    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)
    $request = [System.Net.Http.HttpRequestMessage]::new(
        [System.Net.Http.HttpMethod]::new($Method),
        $Uri
    )
    if ($Method -eq 'POST') {
        if ([string]::IsNullOrWhiteSpace($RequestBodyPath)) {
            throw 'POST requires RequestBodyPath'
        }
        $body = [System.IO.File]::ReadAllBytes($RequestBodyPath)
        $content = [System.Net.Http.ByteArrayContent]::new($body)
        $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new(
            'application/json'
        )
        $request.Content = $content
    }

    $started = (Get-Date).ToUniversalTime()
    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $statusCode = $null
    $reason = $null
    $errorMessage = $null
    $headers = @{}
    $response = $null
    try {
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        $statusCode = [int]$response.StatusCode
        $reason = [string]$response.ReasonPhrase
        foreach ($header in $response.Headers) {
            $headers[$header.Key] = @($header.Value)
        }
        foreach ($header in $response.Content.Headers) {
            $headers[$header.Key] = @($header.Value)
        }
        $bytes = $response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
        [System.IO.File]::WriteAllBytes($ResponsePath, $bytes)
    }
    catch {
        $errorMessage = $_.Exception.ToString()
        [System.IO.File]::WriteAllBytes($ResponsePath, [byte[]]::new(0))
    }
    finally {
        $stopwatch.Stop()
        if ($null -ne $response) {
            $response.Dispose()
        }
        $request.Dispose()
        $client.Dispose()
        $handler.Dispose()
    }

    [ordered]@{
        method = $Method
        uri = $Uri
        started_at_utc = $started.ToString('o')
        completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        wall_ms = [Math]::Round($stopwatch.Elapsed.TotalMilliseconds, 3)
        timeout_seconds = $TimeoutSeconds
        status_code = $statusCode
        reason = $reason
        headers = $headers
        error = $errorMessage
        response_file = [System.IO.Path]::GetFileName($ResponsePath)
        response_bytes = [UInt64](Get-Item -LiteralPath $ResponsePath).Length
        response_sha256 = Get-Sha256Lower -Path $ResponsePath
    }
}

function Get-TraceFacts {
    param(
        [Parameter(Mandatory = $true)][string]$TracePath,
        [Parameter(Mandatory = $true)][string]$ExpectedNonce,
        [Parameter(Mandatory = $true)][string[]]$ForbiddenTools
    )

    $records = @(
        Get-Content -LiteralPath $TracePath |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
    $events = @($records | ForEach-Object { $_.event })
    $policy = @($events | Where-Object { $_.type -eq 'policy_selected' })
    $turnEnds = @($events | Where-Object { $_.type -eq 'turn_end' })
    $constraints = @($events | Where-Object { $_.type -eq 'constraint_applied' })
    $toolCalls = @($events | Where-Object { $_.type -eq 'tool_call' })
    $toolResults = @($events | Where-Object { $_.type -eq 'tool_result' })
    $sessionEnds = @($events | Where-Object { $_.type -eq 'session_end' })
    $readCallIndex = -1
    $completeCallIndex = -1
    for ($index = 0; $index -lt $toolCalls.Count; $index++) {
        if ($readCallIndex -lt 0 -and
            $toolCalls[$index].name -eq 'read_file' -and
            [string]$toolCalls[$index].args.path -eq 'nonce.txt') {
            $readCallIndex = $index
        }
        if ($completeCallIndex -lt 0 -and $toolCalls[$index].name -eq 'task_complete') {
            $completeCallIndex = $index
        }
    }
    $readPairs = [System.Collections.Generic.List[object]]::new()
    for ($eventIndex = 0; $eventIndex -lt $events.Count; $eventIndex++) {
        $event = $events[$eventIndex]
        if ($event.type -ne 'tool_call' -or $event.name -ne 'read_file' -or
            [string]$event.args.path -ne 'nonce.txt') {
            continue
        }
        for ($resultIndex = $eventIndex + 1;
            $resultIndex -lt $events.Count;
            $resultIndex++) {
            $candidate = $events[$resultIndex]
            if ($candidate.type -eq 'tool_result' -and
                [string]$candidate.id -eq [string]$event.id) {
                if ($candidate.name -eq 'read_file' -and
                    -not [bool]$candidate.is_error -and
                    ([string]$candidate.output).TrimEnd("`r", "`n") -eq
                        $ExpectedNonce) {
                    $readPairs.Add([ordered]@{
                        call_id = [string]$event.id
                        path = [string]$event.args.path
                        call_event_index = $eventIndex
                        result_event_index = $resultIndex
                    })
                }
                break
            }
        }
    }
    $completeCalls = @($toolCalls | Where-Object { $_.name -eq 'task_complete' })
    $exactSummary = $false
    if ($completeCalls.Count -eq 1) {
        $exactSummary = ([string]$completeCalls[0].args.summary -eq $ExpectedNonce)
    }
    $forbiddenObserved = @(
        $toolCalls | Where-Object { $_.name -in $ForbiddenTools } |
            ForEach-Object { $_.name }
    )
    $allConstraintsAreSchema = ($constraints.Count -eq $turnEnds.Count) -and
        (@($constraints | Where-Object { $_.kind -ne 'json_schema' }).Count -eq 0)

    [ordered]@{
        record_count = $records.Count
        policy_count = $policy.Count
        protocol = if ($policy.Count -eq 1) { [string]$policy[0].protocol } else { $null }
        turn_count = $turnEnds.Count
        constraint_count = $constraints.Count
        all_turns_json_schema_constrained = $allConstraintsAreSchema
        tool_calls = @($toolCalls | ForEach-Object {
            [ordered]@{ name = $_.name; args = $_.args }
        })
        read_file_before_task_complete = (
            $readCallIndex -ge 0 -and $completeCallIndex -gt $readCallIndex
        )
        exact_nonce_read_result_count = $readPairs.Count
        exact_nonce_read_file_pairs = @($readPairs)
        exact_task_complete_summary = $exactSummary
        forbidden_tools_observed = $forbiddenObserved
        session_end_count = $sessionEnds.Count
        session_end_reason = if ($sessionEnds.Count -eq 1) {
            [string]$sessionEnds[0].reason
        }
        else {
            $null
        }
    }
}

function Get-Median {
    param([Parameter(Mandatory = $true)][double[]]$Values)

    if ($Values.Count -eq 0) {
        return $null
    }
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count % 2 -eq 1) {
        return [double]$sorted[[int][Math]::Floor($sorted.Count / 2)]
    }
    $upper = [int]($sorted.Count / 2)
    ([double]$sorted[$upper - 1] + [double]$sorted[$upper]) / 2.0
}

function Write-HashManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$OutputPath
    )

    $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
    $outputFull = [System.IO.Path]::GetFullPath($OutputPath)
    $lines = @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File -Force |
            Where-Object { [System.IO.Path]::GetFullPath($_.FullName) -ne $outputFull } |
            ForEach-Object {
                $relative = Get-RelativeSlashPath -Root $resolvedRoot -Path $_.FullName
                "$(Get-Sha256Lower -Path $_.FullName)  $relative"
            } |
            Sort-Object
    )
    Write-Utf8Lf -Path $OutputPath -Text (($lines -join "`n") + "`n")
}

function Get-RuntimeCoverageRecords {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$CoverageRoot,
        [Parameter(Mandatory = $true)][string]$EpochArtifactDirectory,
        [AllowNull()][string]$StageDirectory
    )

    $resolvedRoot = (Resolve-Path -LiteralPath $CoverageRoot).Path
    $resolvedEpoch = (Resolve-Path -LiteralPath $EpochArtifactDirectory).Path
    $rootPrefix = $resolvedRoot.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedEpoch.StartsWith(
            $rootPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'epoch artifact directory is outside the runtime coverage root'
    }
    $epochRelative = Get-RelativeSlashPath -Root $resolvedRoot `
        -Path $resolvedEpoch
    $stagePrefix = if ([string]::IsNullOrWhiteSpace($StageDirectory)) {
        $null
    }
    else {
        $resolvedStage = (Resolve-Path -LiteralPath $StageDirectory).Path
        $stageItem = Get-Item -LiteralPath $resolvedStage -Force
        $stageParent = Split-Path -Parent $resolvedStage
        $stageLeaf = Split-Path -Leaf $resolvedStage
        if (-not $stageParent.Equals(
                $resolvedEpoch,
                [System.StringComparison]::OrdinalIgnoreCase
            ) -or
            -not $stageLeaf.StartsWith(
                '.final-stage-',
                [System.StringComparison]::Ordinal
            ) -or
            ($stageItem.Attributes -band
                [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'stage directory is not a direct non-reparse final stage under the epoch artifact directory'
        }
        $resolvedStage.TrimEnd(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        ) + [System.IO.Path]::DirectorySeparatorChar
    }
    $excludedSelfPaths = @(
        "$epochRelative/final/artifact-manifest.json",
        "$epochRelative/final/artifact-manifest.sha256"
    )
    $records = [System.Collections.Generic.List[object]]::new()
    foreach ($file in @(Get-ChildItem -LiteralPath $resolvedRoot -Recurse `
        -File -Force -ErrorAction Stop)) {
        $fullFile = [System.IO.Path]::GetFullPath($file.FullName)
        if ($null -ne $stagePrefix -and $fullFile.StartsWith(
                $stagePrefix,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            continue
        }
        $relative = Get-RelativeSlashPath -Root $resolvedRoot -Path $fullFile
        if ($relative -cin $excludedSelfPaths) {
            continue
        }
        $records.Add([pscustomobject][ordered]@{
            path = $relative
            bytes = [UInt64]$file.Length
            sha256 = Get-Sha256Lower -Path $fullFile
        })
    }

    if (-not [string]::IsNullOrWhiteSpace($StageDirectory)) {
        foreach ($name in @('selection.json', 'runtime-verification.json')) {
            $path = Join-Path $StageDirectory $name
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "staged final artifact is absent: $name"
            }
            $item = Get-Item -LiteralPath $path
            $records.Add([pscustomobject][ordered]@{
                path = "$epochRelative/final/$name"
                bytes = [UInt64]$item.Length
                sha256 = Get-Sha256Lower -Path $path
            })
        }
    }
    @($records | Sort-Object { $_.path })
}

function Test-HashManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [switch]$RejectUnlistedFiles
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $entries = 0
    $listedPaths = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
    $rootPrefix = [System.IO.Path]::GetFullPath($resolvedRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $manifestFull = [System.IO.Path]::GetFullPath($ManifestPath)
    if (-not $manifestFull.StartsWith(
            $rootPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        return [ordered]@{
            passed = $false
            entries = 0
            errors = @('manifest path is outside its declared root')
        }
    }
    foreach ($line in Get-Content -LiteralPath $ManifestPath) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        if ($line -notmatch '^([0-9a-f]{64})  (.+)$') {
            $errors.Add("malformed manifest line: $line")
            continue
        }
        $entries++
        $expected = $Matches[1]
        $relative = $Matches[2]
        if ($relative.Contains('\')) {
            $errors.Add("manifest path is not normalized with forward slashes: $relative")
            continue
        }
        if (-not $listedPaths.Add($relative)) {
            $errors.Add("duplicate manifest path: $relative")
            continue
        }
        try {
            $path = Resolve-SafeRelativePath -Root $resolvedRoot `
                -RelativePath $relative
        }
        catch {
            $errors.Add("unsafe manifest path: $relative")
            continue
        }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $errors.Add("missing: $relative")
            continue
        }
        $actual = Get-Sha256Lower -Path $path
        if ($actual -ne $expected) {
            $errors.Add("hash mismatch: $relative")
        }
    }
    if ($RejectUnlistedFiles) {
        foreach ($file in Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File -Force) {
            if ([System.IO.Path]::GetFullPath($file.FullName).Equals(
                    $manifestFull,
                    [System.StringComparison]::OrdinalIgnoreCase
                )) {
                continue
            }
            $relative = Get-RelativeSlashPath -Root $resolvedRoot `
                -Path $file.FullName
            if (-not $listedPaths.Contains($relative)) {
                $errors.Add("unlisted: $relative")
            }
        }
    }
    [ordered]@{
        passed = ($errors.Count -eq 0 -and $entries -gt 0)
        entries = $entries
        errors = @($errors)
    }
}
