[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'T-11501 qualification requires PowerShell 7 or newer.'
}
if (-not $IsWindows) {
    throw 'T-11501 qualifies the exact Windows release binary and must run on Windows.'
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$targetBoundary = (Resolve-Path -LiteralPath (Join-Path $repoRoot 'target')).Path
$transientAttempts = [System.IO.Path]::GetFullPath(
    (Join-Path $targetBoundary 's115-release-qualification\attempts')
)
$retainedAttempts = Join-Path $PSScriptRoot 'attempts'
$verifierPath = Join-Path $PSScriptRoot 'verify-release.ps1'
$knownUnrelatedPath = 'docs/sprints/s114/control-artifacts/model/acquisition-tests.json'
$attemptNumber = $null
$attemptLabel = $null
$runRoot = $null
$stagedEvidence = $null
$journalPath = $null
$retainedEvidence = $null
$publicationStage = $null
$cargoBuildRoot = $null
$builtBinaryPath = $null
$binaryPath = [System.IO.Path]::GetFullPath((Join-Path $targetBoundary 'release\ferric.exe'))
$gateRecords = [System.Collections.Generic.List[object]]::new()
$probeRecords = [System.Collections.Generic.List[object]]::new()
$sourceCommit = $null
$sourceBranch = $null
$knownUnrelatedPresent = $false
$binaryRecord = $null
$helpRecord = $null
$failureMessage = $null
$qualifiedSourceClean = $false

function Write-Utf8 {
    param(
        [Parameter(Mandatory)][string]$LiteralPath,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Text
    )
    [System.IO.File]::WriteAllText($LiteralPath, $Text, $utf8NoBom)
}

function Write-Json {
    param(
        [Parameter(Mandatory)][string]$LiteralPath,
        [Parameter(Mandatory)][object]$Value
    )
    Write-Utf8 -LiteralPath $LiteralPath -Text (($Value | ConvertTo-Json -Depth 30) + "`n")
}

function Get-Sha256 {
    param([Parameter(Mandatory)][string]$LiteralPath)
    (Get-FileHash -Algorithm SHA256 -LiteralPath $LiteralPath).Hash.ToLowerInvariant()
}

function Get-RelativeDisplayPath {
    param([Parameter(Mandatory)][string]$LiteralPath)
    [System.IO.Path]::GetRelativePath($repoRoot, $LiteralPath).Replace('\', '/')
}

function Assert-StrictChildPath {
    param(
        [Parameter(Mandatory)][string]$Boundary,
        [Parameter(Mandatory)][string]$Candidate,
        [Parameter(Mandatory)][string]$Label
    )
    $fullBoundary = [System.IO.Path]::GetFullPath($Boundary).TrimEnd('\')
    $fullCandidate = [System.IO.Path]::GetFullPath($Candidate).TrimEnd('\')
    if (-not $fullCandidate.StartsWith(
            $fullBoundary + '\',
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "$Label escapes its boundary: $fullCandidate"
    }
}

function Assert-NoReparseAncestors {
    param(
        [Parameter(Mandatory)][string]$ExistingPath,
        [Parameter(Mandatory)][string]$StopPath,
        [Parameter(Mandatory)][string]$Label
    )
    $stop = (Resolve-Path -LiteralPath $StopPath).Path.TrimEnd('\')
    $cursor = Get-Item -Force -LiteralPath $ExistingPath
    while ($true) {
        if ($cursor.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "$Label contains a reparse point: $($cursor.FullName)"
        }
        if ([string]::Equals(
                $cursor.FullName.TrimEnd('\'),
                $stop,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            break
        }
        $parent = Split-Path -Parent $cursor.FullName
        if ([string]::IsNullOrWhiteSpace($parent)) {
            throw "$Label never reached its expected boundary."
        }
        $cursor = Get-Item -Force -LiteralPath $parent
    }
}

function Assert-SafePathTail {
    param(
        [Parameter(Mandatory)][string]$Boundary,
        [Parameter(Mandatory)][string]$Candidate,
        [Parameter(Mandatory)][string]$Label,
        [switch]$FinalMayBeFile
    )
    $fullBoundary = [System.IO.Path]::GetFullPath($Boundary).TrimEnd('\')
    $fullCandidate = [System.IO.Path]::GetFullPath($Candidate).TrimEnd('\')
    Assert-StrictChildPath -Boundary $fullBoundary -Candidate $fullCandidate -Label $Label

    $boundaryItem = Get-Item -Force -LiteralPath $fullBoundary
    if (-not $boundaryItem.PSIsContainer -or
        $boundaryItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Label has an unsafe boundary: $fullBoundary"
    }

    $relative = [System.IO.Path]::GetRelativePath($fullBoundary, $fullCandidate)
    $segments = @($relative.Split(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.StringSplitOptions]::RemoveEmptyEntries
        ))
    $cursor = $fullBoundary
    for ($index = 0; $index -lt $segments.Count; $index++) {
        $cursor = Join-Path $cursor $segments[$index]
        if (-not (Test-Path -LiteralPath $cursor)) {
            break
        }
        $item = Get-Item -Force -LiteralPath $cursor
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "$Label contains a reparse point: $($item.FullName)"
        }
        $isFinal = $index -eq ($segments.Count - 1)
        if ($isFinal -and $FinalMayBeFile -and $item.PSIsContainer) {
            throw "$Label expected an ordinary file but found a directory: $($item.FullName)"
        }
        if (-not $item.PSIsContainer -and -not ($isFinal -and $FinalMayBeFile)) {
            throw "$Label contains a non-directory ancestor: $($item.FullName)"
        }
    }
}

function Get-ExistingAttemptNumbers {
    param([Parameter(Mandatory)][string[]]$AttemptRoots)
    $numbers = [System.Collections.Generic.List[int]]::new()
    foreach ($attemptRoot in $AttemptRoots) {
        if (-not (Test-Path -LiteralPath $attemptRoot)) {
            continue
        }
        $rootItem = Get-Item -Force -LiteralPath $attemptRoot
        if (-not $rootItem.PSIsContainer -or
            $rootItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "attempt root is not an ordinary directory: $attemptRoot"
        }
        foreach ($child in @(Get-ChildItem -Force -LiteralPath $attemptRoot)) {
            if ($child.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
                throw "attempt root contains a reparse point: $($child.FullName)"
            }
            if ($child.Name -ceq 'allocation.lock' -and -not $child.PSIsContainer) {
                continue
            }
            if ($child.Name -notmatch '^(?:\.)?(\d{3})(?:-staging)?$') {
                throw "attempt root contains an unexpected entry: $($child.FullName)"
            }
            if (-not $child.PSIsContainer) {
                throw "attempt entry is not a directory: $($child.FullName)"
            }
            $numbers.Add([int]$Matches[1])
        }
    }
    @($numbers)
}

function Get-ApplicationPath {
    param([Parameter(Mandatory)][string]$Name)
    $command = Get-Command $Name -CommandType Application -ErrorAction Stop |
        Select-Object -First 1
    $item = Get-Item -Force -LiteralPath $command.Source
    if ($item.PSIsContainer) {
        throw "tool path is a directory: $Name"
    }
    if (-not $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        return $item.FullName
    }

    # rustup intentionally installs cargo/rustc proxy symlinks. Preserve the
    # proxy path (argv[0] selects the tool), but accept it only when its single
    # target resolves to an ordinary file beside the proxy.
    $targets = @($item.Target)
    if ($targets.Count -ne 1 -or [string]::IsNullOrWhiteSpace([string]$targets[0])) {
        throw "tool reparse point has no single inspectable target: $($item.FullName)"
    }
    $targetPath = [string]$targets[0]
    if (-not [System.IO.Path]::IsPathRooted($targetPath)) {
        $targetPath = Join-Path $item.DirectoryName $targetPath
    }
    $target = Get-Item -Force -LiteralPath $targetPath
    if ($target.PSIsContainer -or
        $target.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        -not [string]::Equals(
            $target.DirectoryName,
            $item.DirectoryName,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "tool proxy target is not an ordinary sibling file: $($item.FullName)"
    }
    $item.FullName
}

function Invoke-CapturedGate {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments,
        [int[]]$ExpectedExitCodes = @(0),
        [string]$WorkingDirectory = $repoRoot,
        [Parameter(Mandatory)][ValidateRange(1, 7200)][int]$TimeoutSeconds
    )
    $ordinal = $gateRecords.Count + 1
    $safeName = $Name -replace '[^a-zA-Z0-9_-]', '-'
    $stdoutRelative = 'logs/{0:D2}-{1}.stdout.txt' -f $ordinal, $safeName
    $stderrRelative = 'logs/{0:D2}-{1}.stderr.txt' -f $ordinal, $safeName
    $stdoutPath = Join-Path $stagedEvidence $stdoutRelative
    $stderrPath = Join-Path $stagedEvidence $stderrRelative
    $started = [DateTimeOffset]::UtcNow
    $clock = [System.Diagnostics.Stopwatch]::StartNew()

    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $start.ArgumentList.Add($argument)
    }

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    $stdout = ''
    $stderr = ''
    $exitCode = -1
    $timedOut = $false
    $processId = $null
    try {
        if (-not $process.Start()) {
            throw "failed to start gate executable: $FilePath"
        }
        $processId = $process.Id
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $streamsComplete = $true
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $timedOut = $true
            try {
                $process.Kill($true)
            }
            catch {
                $stderr += "failed to terminate timed-out process tree: $($_.Exception.Message)`n"
            }
            if (-not $process.WaitForExit(30 * 1000)) {
                $streamsComplete = $false
                $stderr += 'timed-out process tree did not exit within the 30-second termination bound.' + "`n"
            }
        }
        if ($streamsComplete) {
            $stdout = $stdoutTask.GetAwaiter().GetResult()
            $stderr += $stderrTask.GetAwaiter().GetResult()
            $exitCode = $process.ExitCode
        }
        else {
            $exitCode = -2
        }
    }
    catch {
        $stderr = $_.Exception.ToString()
    }
    finally {
        $clock.Stop()
        $process.Dispose()
    }

    Write-Utf8 -LiteralPath $stdoutPath -Text $stdout
    Write-Utf8 -LiteralPath $stderrPath -Text $stderr
    $ended = [DateTimeOffset]::UtcNow
    $passed = -not $timedOut -and $ExpectedExitCodes -contains $exitCode
    $record = [ordered]@{
        ordinal = $ordinal
        name = $Name
        executable = $FilePath
        argv = @($Arguments)
        working_directory = $WorkingDirectory
        timeout_seconds = $TimeoutSeconds
        timed_out = $timedOut
        process_id = $processId
        expected_exit_codes = @($ExpectedExitCodes)
        exit_code = $exitCode
        passed = $passed
        started_at_utc = $started.ToString('o')
        ended_at_utc = $ended.ToString('o')
        duration_ms = [Math]::Round($clock.Elapsed.TotalMilliseconds, 3)
        stdout = $stdoutRelative.Replace('\', '/')
        stdout_sha256 = Get-Sha256 -LiteralPath $stdoutPath
        stderr = $stderrRelative.Replace('\', '/')
        stderr_sha256 = Get-Sha256 -LiteralPath $stderrPath
    }
    $gateObject = [pscustomobject]$record
    $gateRecords.Add($gateObject)
    [System.IO.File]::AppendAllText(
        $journalPath,
        (($gateObject | ConvertTo-Json -Depth 12 -Compress) + "`n"),
        $utf8NoBom
    )
    [pscustomobject]@{
        record = $gateObject
        stdout = $stdout
        stderr = $stderr
    }
}

function Assert-GatePassed {
    param([Parameter(Mandatory)][object]$Capture)
    if (-not $Capture.record.passed) {
        throw "gate '$($Capture.record.name)' exited $($Capture.record.exit_code); see $($Capture.record.stdout) and $($Capture.record.stderr)"
    }
}

function Get-StatusLines {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    @($Text -split "`r?`n" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
}

function Assert-AllowedRepositoryStatus {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $lines = @(Get-StatusLines -Text $Text)
    if ($lines.Count -ne 0) {
        throw "qualified Cargo/crates source is dirty: $($lines -join '; ')"
    }
    $script:qualifiedSourceClean = $true
}

function Assert-KnownUnrelatedStatus {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $lines = @(Get-StatusLines -Text $Text)
    $knownLine = " M $knownUnrelatedPath"
    if ($lines.Count -gt 1 -or ($lines.Count -eq 1 -and $lines[0] -cne $knownLine)) {
        throw "known unrelated path has an unexpected Git state: $($lines -join '; ')"
    }
    $script:knownUnrelatedPresent = $lines.Count -eq 1
}

function Get-VerbatimCanonicalPath {
    param([Parameter(Mandatory)][string]$LiteralPath)
    $resolved = (Resolve-Path -LiteralPath $LiteralPath).Path
    if ($resolved.StartsWith('\\?\')) {
        return $resolved
    }
    if ($resolved.StartsWith('\\')) {
        return '\\?\UNC\' + $resolved.TrimStart('\')
    }
    '\\?\' + $resolved
}

function Read-TraceSummary {
    param([Parameter(Mandatory)][string]$LiteralPath)
    $events = @(
        Get-Content -LiteralPath $LiteralPath |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
    $start = @($events | Where-Object { $_.event.type -eq 'session_start' })
    $end = @($events | Where-Object { $_.event.type -eq 'session_end' })
    if ($start.Count -ne 1 -or $end.Count -ne 1) {
        throw "trace must contain exactly one session_start and one session_end: $LiteralPath"
    }
    $resumedProperty = $start[0].event.PSObject.Properties['resumed_from']
    [pscustomobject]@{
        session = [string]$start[0].session
        workspace = [string]$start[0].event.workspace
        resumed_from = if ($null -eq $resumedProperty -or $null -eq $resumedProperty.Value) {
            $null
        }
        else {
            [string]$resumedProperty.Value
        }
        reason = [string]$end[0].event.reason
        event_count = $events.Count
    }
}

function Get-ResumeElements {
    param([Parameter(Mandatory)][string]$StandardError)
    $line = @($StandardError -split "`r?`n" | Where-Object { $_.StartsWith('Resume: ') })
    if ($line.Count -ne 1) {
        throw "expected exactly one public Resume line, observed $($line.Count)"
    }
    $commandText = $line[0].Substring('Resume: '.Length)
    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput(
        $commandText,
        [ref]$tokens,
        [ref]$parseErrors
    )
    if ($parseErrors.Count -ne 0) {
        throw "printed Resume command is not valid PowerShell: $($parseErrors -join '; ')"
    }
    $commands = @($ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.CommandAst] }, $true))
    if ($commands.Count -ne 1 -or $commands[0].Redirections.Count -ne 0) {
        throw 'printed Resume command must contain one command and no redirections.'
    }
    @($commands[0].CommandElements | ForEach-Object {
        if ($_ -isnot [System.Management.Automation.Language.StringConstantExpressionAst]) {
            throw "Resume command contains a non-literal element: $($_.Extent.Text)"
        }
        $_.Value
    })
}

function Assert-ExactElements {
    param(
        [Parameter(Mandatory)][string[]]$Actual,
        [Parameter(Mandatory)][string[]]$Expected,
        [Parameter(Mandatory)][string]$Label
    )
    if ($Actual.Count -ne $Expected.Count) {
        throw "$Label argv count mismatch: expected $($Expected.Count), observed $($Actual.Count)"
    }
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        if ($Actual[$index] -cne $Expected[$index]) {
            throw "$Label argv[$index] mismatch: expected '$($Expected[$index])', observed '$($Actual[$index])'"
        }
    }
}

function Copy-ProbeTrace {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$RetainedName
    )
    $probeEvidenceRoot = Join-Path $stagedEvidence 'probes'
    [System.IO.Directory]::CreateDirectory($probeEvidenceRoot) | Out-Null
    $destination = Join-Path $probeEvidenceRoot $RetainedName
    if (Test-Path -LiteralPath $destination) {
        throw "retained probe trace collision: $destination"
    }
    Copy-Item -LiteralPath $Source -Destination $destination
    [pscustomobject]@{
        retained_path = ('probes/' + $RetainedName)
        sha256 = Get-Sha256 -LiteralPath $destination
        bytes = (Get-Item -LiteralPath $destination).Length
    }
}

function Invoke-ProbePair {
    param(
        [Parameter(Mandatory)][ValidateSet('default', 'external')][string]$Kind,
        [Parameter(Mandatory)][bool]$External
    )
    $pairRoot = Join-Path (Join-Path $runRoot 'probes') $Kind
    $workspace = Join-Path $pairRoot 'workspace'
    $traceRoot = if ($External) {
        Join-Path $pairRoot 'traces'
    }
    else {
        Join-Path $workspace '.ferric\trace'
    }
    [System.IO.Directory]::CreateDirectory($workspace) | Out-Null

    $freshArguments = @(
        'query', '--mock', '--no-config', '--max-turns', '1',
        '--workspace', $workspace
    )
    if ($External) {
        $freshArguments += @('--trace-dir', $traceRoot)
    }
    $freshArguments += 'do a release qualification mock task'
    $freshCapture = Invoke-CapturedGate `
        -Name "probe-$Kind-fresh" `
        -FilePath $binaryPath `
        -Arguments $freshArguments `
        -ExpectedExitCodes @(1) `
        -TimeoutSeconds 300
    Assert-GatePassed -Capture $freshCapture

    if (-not (Test-Path -LiteralPath (Join-Path $workspace 'ferric-mock.txt') -PathType Leaf)) {
        throw "$Kind fresh probe did not execute the mock workspace write."
    }
    if ($External -and (Test-Path -LiteralPath (Join-Path $workspace '.ferric'))) {
        throw 'external fresh probe leaked workspace .ferric state.'
    }
    $freshTraces = @(Get-ChildItem -File -LiteralPath $traceRoot -Filter 'q-*.jsonl')
    if ($freshTraces.Count -ne 1) {
        throw "$Kind fresh probe expected one trace, observed $($freshTraces.Count)."
    }
    $freshTrace = $freshTraces[0].FullName
    $freshSummary = Read-TraceSummary -LiteralPath $freshTrace
    $expectedTraceWorkspace = Get-VerbatimCanonicalPath -LiteralPath $workspace
    if ($freshSummary.reason -cne 'max_turns' -or $null -ne $freshSummary.resumed_from) {
        throw "$Kind fresh trace is not the expected resumable max_turns source."
    }
    if ($freshSummary.workspace -cne $expectedTraceWorkspace) {
        throw "$Kind fresh trace recorded the wrong workspace: $($freshSummary.workspace)"
    }

    $freshElements = @(Get-ResumeElements -StandardError $freshCapture.stderr)
    $expectedFreshElements = @(
        'ferric', 'query', '--resume', (Get-VerbatimCanonicalPath -LiteralPath $freshTrace),
        '--workspace', (Get-VerbatimCanonicalPath -LiteralPath $workspace)
    )
    if ($External) {
        $expectedFreshElements += @('--trace-dir', (Get-VerbatimCanonicalPath -LiteralPath $traceRoot))
    }
    Assert-ExactElements `
        -Actual $freshElements `
        -Expected $expectedFreshElements `
        -Label "$Kind fresh Resume"
    if ($freshElements -ccontains '--answer') {
        throw "$Kind ordinary incomplete probe unexpectedly printed --answer."
    }

    $resumeArguments = @(
        'query', '--mock', '--no-config', '--resume', $freshTrace,
        '--workspace', $workspace, '--max-turns', '3'
    )
    if ($External) {
        $resumeArguments += @('--trace-dir', $traceRoot)
    }
    $resumeCapture = Invoke-CapturedGate `
        -Name "probe-$Kind-resume" `
        -FilePath $binaryPath `
        -Arguments $resumeArguments `
        -TimeoutSeconds 300
    Assert-GatePassed -Capture $resumeCapture
    if ($resumeCapture.stderr -match '(?m)^Resume: ') {
        throw "$Kind terminal resume probe printed an inapplicable Resume command."
    }
    if ($External -and (Test-Path -LiteralPath (Join-Path $workspace '.ferric'))) {
        throw 'external resume probe leaked workspace .ferric state.'
    }

    $sourceTraceSet = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )
    $sourceTraceSet.Add($freshTrace) | Out-Null
    $allTraces = @(Get-ChildItem -File -LiteralPath $traceRoot -Filter 'q-*.jsonl')
    if ($allTraces.Count -ne 2) {
        throw "$Kind resume probe expected source plus continuation, observed $($allTraces.Count)."
    }
    $continuation = @($allTraces | Where-Object { -not $sourceTraceSet.Contains($_.FullName) })
    if ($continuation.Count -ne 1) {
        throw "$Kind resume probe could not identify one continuation trace."
    }
    $continuationSummary = Read-TraceSummary -LiteralPath $continuation[0].FullName
    if ($continuationSummary.resumed_from -cne $freshSummary.session -or
        $continuationSummary.reason -cne 'task_complete') {
        throw "$Kind continuation is not linked to the source task_complete trace."
    }
    if ($continuationSummary.workspace -cne $expectedTraceWorkspace) {
        throw "$Kind continuation recorded the wrong workspace: $($continuationSummary.workspace)"
    }

    $freshCopy = Copy-ProbeTrace -Source $freshTrace -RetainedName "$Kind-fresh.jsonl"
    $resumeCopy = Copy-ProbeTrace -Source $continuation[0].FullName -RetainedName "$Kind-resume.jsonl"
    $traceRootResolved = (Resolve-Path -LiteralPath $traceRoot).Path
    $workspaceResolved = (Resolve-Path -LiteralPath $workspace).Path

    $probeRecords.Add([pscustomobject][ordered]@{
        name = "${Kind}_fresh"
        gate = $freshCapture.record.name
        passed = $true
        external = $External
        workspace = Get-RelativeDisplayPath -LiteralPath $workspaceResolved
        observed_trace_workspace = $freshSummary.workspace
        trace_workspace_exact = $true
        trace_root = Get-RelativeDisplayPath -LiteralPath $traceRootResolved
        trace_location_exact = [string]::Equals(
            (Resolve-Path -LiteralPath (Split-Path -Parent $freshTrace)).Path,
            $traceRootResolved,
            [System.StringComparison]::OrdinalIgnoreCase
        )
        workspace_dot_ferric_absent = -not (Test-Path -LiteralPath (Join-Path $workspace '.ferric'))
        session = $freshSummary.session
        stop_reason = $freshSummary.reason
        resume_argv = @($freshElements)
        resume_argv_exact = $true
        answer_absent = $true
        trace = $freshCopy
    })
    $probeRecords.Add([pscustomobject][ordered]@{
        name = "${Kind}_resume"
        gate = $resumeCapture.record.name
        passed = $true
        external = $External
        workspace = Get-RelativeDisplayPath -LiteralPath $workspaceResolved
        observed_trace_workspace = $continuationSummary.workspace
        trace_workspace_exact = $true
        trace_root = Get-RelativeDisplayPath -LiteralPath $traceRootResolved
        trace_location_exact = [string]::Equals(
            (Resolve-Path -LiteralPath (Split-Path -Parent $continuation[0].FullName)).Path,
            $traceRootResolved,
            [System.StringComparison]::OrdinalIgnoreCase
        )
        workspace_dot_ferric_absent = -not (Test-Path -LiteralPath (Join-Path $workspace '.ferric'))
        session = $continuationSummary.session
        resumed_from = $continuationSummary.resumed_from
        expected_resumed_from = $freshSummary.session
        resumed_from_matches = $true
        stop_reason = $continuationSummary.reason
        terminal_resume_hint_absent = $true
        trace = $resumeCopy
    })
}

function Write-FilesManifest {
    $manifestPath = Join-Path $stagedEvidence 'files.sha256'
    $lines = @(
        Get-ChildItem -File -Recurse -LiteralPath $stagedEvidence |
            Where-Object { $_.FullName -cne $manifestPath } |
            ForEach-Object {
                [pscustomobject]@{
                    relative = [System.IO.Path]::GetRelativePath($stagedEvidence, $_.FullName).Replace('\', '/')
                    hash = Get-Sha256 -LiteralPath $_.FullName
                }
            } |
            Sort-Object relative |
            ForEach-Object { "$($_.hash)  $($_.relative)" }
    )
    Write-Utf8 -LiteralPath $manifestPath -Text (($lines -join "`n") + "`n")
}

function Get-SourceFileRecords {
    param([Parameter(Mandatory)][string[]]$RelativePaths)
    @($RelativePaths | ForEach-Object {
        $path = Join-Path $repoRoot $_
        $item = Get-Item -LiteralPath $path
        [pscustomobject][ordered]@{
            path = $_.Replace('\', '/')
            bytes = $item.Length
            sha256 = Get-Sha256 -LiteralPath $path
        }
    })
}

function New-Result {
    param(
        [Parameter(Mandatory)][bool]$Passed,
        [AllowNull()][string]$Failure
    )
    $taskSourcePaths = @(
        'crates/ferric-cli/src/query.rs',
        'crates/ferric-cli/tests/cli.rs',
        'docs/basics-query.md',
        'docs/commands.md',
        'docs/configuration.md',
        'docs/demo-guide.md'
    )
    [ordered]@{
        schema = 'animus-ferric-s115-release-qualification-v1'
        task = 'T-11501'
        attempt = $attemptNumber
        passed = $Passed
        captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        failure = $Failure
        repository = [ordered]@{
            branch = $sourceBranch
            commit = $sourceCommit
            qualified_source_scope = @('Cargo.toml', 'Cargo.lock', 'crates')
            qualified_source_clean = $qualifiedSourceClean
            known_unrelated_edit = [ordered]@{
                path = $knownUnrelatedPath
                tolerated_status = ' M'
                present = $knownUnrelatedPresent
            }
        }
        host = [ordered]@{
            powershell = $PSVersionTable.PSVersion.ToString()
            platform = 'windows'
            execution_requirement = 'normal host access; full workspace tests may inspect processes'
        }
        source_files = if ($Passed) { @(Get-SourceFileRecords -RelativePaths $taskSourcePaths) } else { @() }
        gates = @($gateRecords)
        binary = $binaryRecord
        query_help = $helpRecord
        probes = @($probeRecords)
        publication = [ordered]@{
            transient_root = "target/s115-release-qualification/attempts/$attemptLabel"
            retained_root = "docs/sprints/s115/control-artifacts/release/attempts/$attemptLabel"
            staged_then_verified = $Passed
        }
    }
}

function Write-ResultBundle {
    param([Parameter(Mandatory)][object]$Result)
    $resultPath = Join-Path $stagedEvidence 'result.json'
    $resultHashPath = Join-Path $stagedEvidence 'result.sha256'
    Write-Json -LiteralPath $resultPath -Value $Result
    $hash = Get-Sha256 -LiteralPath $resultPath
    Write-Utf8 -LiteralPath $resultHashPath -Text "$hash  result.json`n"
    Write-FilesManifest
}

if ((Get-Item -Force -LiteralPath $targetBoundary).Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    )) {
    throw 'repository target boundary must not be a reparse point.'
}
Assert-NoReparseAncestors -ExistingPath $targetBoundary -StopPath $repoRoot -Label 'target boundary'
Assert-NoReparseAncestors -ExistingPath $PSScriptRoot -StopPath $repoRoot -Label 'release control directory'
if (-not (Test-Path -LiteralPath $verifierPath -PathType Leaf)) {
    throw "release verifier is missing: $verifierPath"
}

Assert-SafePathTail `
    -Boundary $targetBoundary `
    -Candidate $transientAttempts `
    -Label 'transient qualification attempts tail'
Assert-SafePathTail `
    -Boundary $PSScriptRoot `
    -Candidate $retainedAttempts `
    -Label 'retained qualification attempts tail'
[System.IO.Directory]::CreateDirectory($transientAttempts) | Out-Null
Assert-NoReparseAncestors `
    -ExistingPath $transientAttempts `
    -StopPath $repoRoot `
    -Label 'transient qualification attempts directory'

$allocationLockPath = Join-Path $transientAttempts 'allocation.lock'
$allocationLockItems = @(Get-ChildItem -Force -LiteralPath $transientAttempts | Where-Object {
        $_.Name -ceq 'allocation.lock'
    })
if ($allocationLockItems.Count -gt 1) {
    throw "attempt allocation lock has multiple directory entries: $allocationLockPath"
}
if ($allocationLockItems.Count -eq 1) {
    $allocationLockItem = $allocationLockItems[0]
    if ($allocationLockItem.PSIsContainer -or
        $allocationLockItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "attempt allocation lock is not an ordinary file: $allocationLockPath"
    }
}
$allocationLock = [System.IO.File]::Open(
    $allocationLockPath,
    [System.IO.FileMode]::OpenOrCreate,
    [System.IO.FileAccess]::ReadWrite,
    [System.IO.FileShare]::None
)
try {
    $existingAttempts = @(Get-ExistingAttemptNumbers -AttemptRoots @(
            $transientAttempts,
            $retainedAttempts
        ))
    $highestAttempt = if ($existingAttempts.Count -eq 0) {
        0
    }
    else {
        ($existingAttempts | Measure-Object -Maximum).Maximum
    }
    if ($highestAttempt -ge 999) {
        throw 'qualification attempt namespace is exhausted at 999.'
    }
    $attemptNumber = [int]$highestAttempt + 1
    $attemptLabel = '{0:D3}' -f $attemptNumber
    $runRoot = Join-Path $transientAttempts $attemptLabel
    $stagedEvidence = Join-Path $runRoot 'staged-evidence'
    $journalPath = Join-Path $stagedEvidence 'journal.jsonl'
    $retainedEvidence = Join-Path $retainedAttempts $attemptLabel
    $publicationStage = Join-Path $retainedAttempts ".$attemptLabel-staging"
    $cargoBuildRoot = Join-Path $runRoot 'cargo-target'
    $builtBinaryPath = Join-Path $cargoBuildRoot 'release\ferric.exe'

    foreach ($mustBeAbsent in @($runRoot, $retainedEvidence, $publicationStage, $cargoBuildRoot)) {
        if (Test-Path -LiteralPath $mustBeAbsent) {
            throw "new qualification attempt path already exists: $mustBeAbsent"
        }
    }
    Assert-SafePathTail -Boundary $targetBoundary -Candidate $runRoot -Label 'qualification run root'
    Assert-SafePathTail `
        -Boundary $PSScriptRoot `
        -Candidate $retainedEvidence `
        -Label 'retained qualification attempt tail'
    [System.IO.Directory]::CreateDirectory($runRoot) | Out-Null
    Write-Utf8 `
        -LiteralPath (Join-Path $runRoot '.qualification-claim') `
        -Text "T-11501 attempt $attemptLabel`n"
}
finally {
    $allocationLock.Dispose()
}

try {
    [System.IO.Directory]::CreateDirectory((Join-Path $stagedEvidence 'logs')) | Out-Null
    $gitPath = Get-ApplicationPath -Name 'git'
    $cargoPath = Get-ApplicationPath -Name 'cargo'
    $pwshPath = Get-ApplicationPath -Name 'pwsh'
    $status = Invoke-CapturedGate -Name 'source-status' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all',
        '--', 'Cargo.toml', 'Cargo.lock', 'crates'
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $status
    Assert-AllowedRepositoryStatus -Text $status.stdout

    $knownStatus = Invoke-CapturedGate -Name 'known-unrelated-status' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all',
        '--', $knownUnrelatedPath
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $knownStatus
    Assert-KnownUnrelatedStatus -Text $knownStatus.stdout

    $head = Invoke-CapturedGate -Name 'source-head' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'rev-parse', 'HEAD'
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $head
    $sourceCommit = $head.stdout.Trim()
    if ($sourceCommit -notmatch '^[0-9a-f]{40}$') {
        throw "source commit is not a full Git object id: $sourceCommit"
    }

    $branch = Invoke-CapturedGate -Name 'source-branch' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'branch', '--show-current'
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $branch
    $sourceBranch = $branch.stdout.Trim()
    if ($sourceBranch -cne 'dev') {
        throw "qualification must run from dev, observed '$sourceBranch'."
    }

    $fmt = Invoke-CapturedGate -Name 'fmt' -FilePath $cargoPath -Arguments @(
        '--locked', 'fmt', '--all', '--', '--check'
    ) -TimeoutSeconds 600
    Assert-GatePassed -Capture $fmt

    $clippyDefault = Invoke-CapturedGate -Name 'clippy-default' -FilePath $cargoPath -Arguments @(
        '--locked', 'clippy', '--workspace', '--all-targets', '--', '-D', 'warnings'
    ) -TimeoutSeconds 3600
    Assert-GatePassed -Capture $clippyDefault

    $clippyBackend = Invoke-CapturedGate -Name 'clippy-backend' -FilePath $cargoPath -Arguments @(
        '--locked', 'clippy', '-p', 'ferric-cli', '--all-targets', '--features', 'backend-openai',
        '--', '-D', 'warnings'
    ) -TimeoutSeconds 3600
    Assert-GatePassed -Capture $clippyBackend

    $queryTests = Invoke-CapturedGate -Name 'query-unit-tests' -FilePath $cargoPath -Arguments @(
        '--locked', 'test', '-p', 'ferric-cli', '--features', 'backend-openai',
        '--bin', 'ferric', 'query::tests'
    ) -TimeoutSeconds 3600
    Assert-GatePassed -Capture $queryTests

    $cliTests = Invoke-CapturedGate -Name 'cli-integration-tests' -FilePath $cargoPath -Arguments @(
        '--locked', 'test', '-p', 'ferric-cli', '--features', 'backend-openai', '--test', 'cli'
    ) -TimeoutSeconds 3600
    Assert-GatePassed -Capture $cliTests

    $workspaceTests = Invoke-CapturedGate -Name 'workspace-tests' -FilePath $cargoPath -Arguments @(
        '--locked', 'test', '--workspace', '--all-targets'
    ) -TimeoutSeconds 7200
    Assert-GatePassed -Capture $workspaceTests

    Assert-SafePathTail `
        -Boundary $runRoot `
        -Candidate $cargoBuildRoot `
        -Label 'fresh Cargo target tail'
    if (Test-Path -LiteralPath $cargoBuildRoot) {
        throw "fresh Cargo target already exists: $cargoBuildRoot"
    }
    $build = Invoke-CapturedGate -Name 'release-build' -FilePath $cargoPath -Arguments @(
        '--locked', 'build', '--release', '-p', 'ferric-cli', '--features', 'backend-openai',
        '--target-dir', $cargoBuildRoot
    ) -TimeoutSeconds 3600
    Assert-GatePassed -Capture $build

    if (-not (Test-Path -LiteralPath $builtBinaryPath -PathType Leaf)) {
        throw "fresh release build did not produce the exact binary: $builtBinaryPath"
    }
    Assert-SafePathTail `
        -Boundary $runRoot `
        -Candidate $builtBinaryPath `
        -Label 'fresh Cargo release binary' `
        -FinalMayBeFile
    $builtBinaryItem = Get-Item -Force -LiteralPath $builtBinaryPath
    if ($builtBinaryItem.PSIsContainer -or
        $builtBinaryItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw 'fresh Cargo release binary must be an ordinary file.'
    }
    $builtBinaryHash = Get-Sha256 -LiteralPath $builtBinaryPath

    Assert-SafePathTail `
        -Boundary $targetBoundary `
        -Candidate $binaryPath `
        -Label 'published release binary tail' `
        -FinalMayBeFile
    $releaseDirectory = Split-Path -Parent $binaryPath
    [System.IO.Directory]::CreateDirectory($releaseDirectory) | Out-Null
    Assert-NoReparseAncestors `
        -ExistingPath $releaseDirectory `
        -StopPath $targetBoundary `
        -Label 'published release binary directory'
    $binaryPublicationStage = Join-Path $releaseDirectory ".ferric-s115-$attemptLabel-staging.exe"
    if (Test-Path -LiteralPath $binaryPublicationStage) {
        throw "binary publication stage already exists: $binaryPublicationStage"
    }
    Copy-Item -LiteralPath $builtBinaryPath -Destination $binaryPublicationStage
    $stagedBinaryItem = Get-Item -Force -LiteralPath $binaryPublicationStage
    if ($stagedBinaryItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        $stagedBinaryItem.Length -ne $builtBinaryItem.Length -or
        (Get-Sha256 -LiteralPath $binaryPublicationStage) -cne $builtBinaryHash) {
        throw 'staged release binary does not exactly match the fresh Cargo output.'
    }
    Move-Item -Force -LiteralPath $binaryPublicationStage -Destination $binaryPath
    $binaryItem = Get-Item -Force -LiteralPath $binaryPath
    $binaryHash = Get-Sha256 -LiteralPath $binaryPath
    if ($binaryItem.PSIsContainer -or
        $binaryItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        $binaryItem.Length -ne $builtBinaryItem.Length -or
        $binaryHash -cne $builtBinaryHash) {
        throw 'published release binary does not exactly match the fresh Cargo output.'
    }

    $postStatus = Invoke-CapturedGate -Name 'post-source-status' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all',
        '--', 'Cargo.toml', 'Cargo.lock', 'crates'
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $postStatus
    Assert-AllowedRepositoryStatus -Text $postStatus.stdout

    $postKnownStatus = Invoke-CapturedGate -Name 'post-known-unrelated-status' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all',
        '--', $knownUnrelatedPath
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $postKnownStatus
    Assert-KnownUnrelatedStatus -Text $postKnownStatus.stdout

    $postHead = Invoke-CapturedGate -Name 'post-source-head' -FilePath $gitPath -Arguments @(
        '-C', $repoRoot, 'rev-parse', 'HEAD'
    ) -TimeoutSeconds 120
    Assert-GatePassed -Capture $postHead
    if ($postHead.stdout.Trim() -cne $sourceCommit) {
        throw 'repository HEAD changed during qualification.'
    }

    $version = Invoke-CapturedGate `
        -Name 'binary-version' `
        -FilePath $binaryPath `
        -Arguments @('--version') `
        -TimeoutSeconds 120
    Assert-GatePassed -Capture $version
    $versionText = $version.stdout.Trim()
    if (-not $versionText.StartsWith('ferric ')) {
        throw "unexpected release version output: $versionText"
    }

    $help = Invoke-CapturedGate `
        -Name 'query-help' `
        -FilePath $binaryPath `
        -Arguments @('query', '--help') `
        -TimeoutSeconds 120
    Assert-GatePassed -Capture $help
    $helpChecks = [ordered]@{
        trace_dir = $help.stdout.Contains('--trace-dir')
        default_root = $help.stdout.Contains('<workspace>/.ferric/trace')
        disjoint = $help.stdout.Contains('disjoint')
        reparse = $help.stdout.Contains('reparse')
        explicit_resume = $help.stdout.Contains('explicit') -and $help.stdout.Contains('resume')
        powershell = $help.stdout.Contains('PowerShell')
    }
    if (@($helpChecks.Values) -contains $false) {
        throw 'release query help does not expose every locked T-11414 clause.'
    }

    $binaryRecord = [ordered]@{
        display_path = 'target/release/ferric.exe'
        bytes = $binaryItem.Length
        sha256 = $binaryHash
        version = $versionText
        source_commit = $sourceCommit
        backend_openai = $true
        build_output = [ordered]@{
            display_path = Get-RelativeDisplayPath -LiteralPath $builtBinaryPath
            bytes = $builtBinaryItem.Length
            sha256 = $builtBinaryHash
        }
        published_from_exact_build_output = $true
    }
    $helpRecord = [ordered]@{
        gate = $help.record.name
        stdout = $help.record.stdout
        stdout_sha256 = $help.record.stdout_sha256
        stderr = $help.record.stderr
        stderr_sha256 = $help.record.stderr_sha256
        checks = $helpChecks
    }

    Invoke-ProbePair -Kind 'default' -External $false
    Invoke-ProbePair -Kind 'external' -External $true

    $successResult = New-Result -Passed $true -Failure $null
    Write-ResultBundle -Result $successResult

    & $pwshPath -NoLogo -NoProfile -File $verifierPath `
        -EvidenceRoot $stagedEvidence -CheckLiveBinary | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'final staged evidence verification failed.'
    }

    Assert-SafePathTail `
        -Boundary $PSScriptRoot `
        -Candidate $retainedEvidence `
        -Label 'retained publication tail'
    foreach ($mustRemainAbsent in @($publicationStage, $retainedEvidence)) {
        if (Test-Path -LiteralPath $mustRemainAbsent) {
            throw "publication destination appeared during qualification: $mustRemainAbsent"
        }
    }
    [System.IO.Directory]::CreateDirectory($retainedAttempts) | Out-Null
    Assert-NoReparseAncestors `
        -ExistingPath $retainedAttempts `
        -StopPath $repoRoot `
        -Label 'retained release attempts directory'
    Copy-Item -Recurse -LiteralPath $stagedEvidence -Destination $publicationStage
    & $pwshPath -NoLogo -NoProfile -File $verifierPath `
        -EvidenceRoot $publicationStage -CheckLiveBinary | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'publication-stage evidence verification failed.'
    }
    Move-Item -LiteralPath $publicationStage -Destination $retainedEvidence
    Write-Output $retainedEvidence
}
catch {
    $failureMessage = $_.Exception.Message
    if (Test-Path -LiteralPath $stagedEvidence -PathType Container) {
        try {
            $failureResult = New-Result -Passed $false -Failure $failureMessage
            Write-ResultBundle -Result $failureResult
        }
        catch {
            Write-Warning "could not finalize partial failure evidence: $($_.Exception.Message)"
        }
    }
    throw "T-11501 qualification failed: $failureMessage. Partial evidence remains at $stagedEvidence"
}
