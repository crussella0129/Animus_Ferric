Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Utf8NoBom = [System.Text.UTF8Encoding]::new($false)

function Get-RepositoryRoot {
    param([Parameter(Mandatory = $true)][string]$ArtifactDirectory)

    $root = $ArtifactDirectory
    for ($index = 0; $index -lt 5; $index++) {
        $root = Split-Path -Parent $root
    }
    (Resolve-Path -LiteralPath $root).Path
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

function Split-SimpleWindowsCommandLine {
    param([Parameter(Mandatory = $true)][string]$CommandLine)

    @(
        [regex]::Matches($CommandLine, '(?:"([^"]*)"|(\S+))') |
            ForEach-Object {
                if ($_.Groups[1].Success) {
                    $_.Groups[1].Value
                }
                else {
                    $_.Groups[2].Value
                }
            }
    )
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

function Test-HashManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$ManifestPath
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $entries = 0
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
        $path = Join-Path $Root $relative.Replace('/', [System.IO.Path]::DirectorySeparatorChar)
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $errors.Add("missing: $relative")
            continue
        }
        $actual = Get-Sha256Lower -Path $path
        if ($actual -ne $expected) {
            $errors.Add("hash mismatch: $relative")
        }
    }
    [ordered]@{
        passed = ($errors.Count -eq 0 -and $entries -gt 0)
        entries = $entries
        errors = @($errors)
    }
}
