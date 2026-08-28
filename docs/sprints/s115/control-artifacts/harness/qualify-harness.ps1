[CmdletBinding()]
param(
    [switch]$ControlSelfTest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'T-11502 qualification requires PowerShell 7 or newer.'
}

$script:Utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$script:PinnedFrozenManifestSha256 = '532cd39a9fec557816929bcf12e5ae539c8a30c0f4c4829a9d6f89b0ca9f358b'
$script:AttemptWidth = 3
$script:SelfTestTimeoutSeconds = 1800
$script:CommandTimeoutSeconds = 60

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path.TrimEnd('\')
$experimentRoot = Join-Path $repoRoot 'target\s114-experiment'
$preservationRoot = Join-Path $repoRoot 'target\s115-preserved-preflight'
$retainedAttemptsRoot = Join-Path $PSScriptRoot 'attempts'
$sourceHarnessRoot = Join-Path $repoRoot 'docs\sprints\s114\control-artifacts\app-harness'
$sourceHarnessDisplay = 'docs/sprints/s114/control-artifacts/app-harness'
$s114Display = 'docs/sprints/s114'
$knownUserEditDisplay = 'docs/sprints/s114/control-artifacts/model/acquisition-tests.json'
$knownUserEditPath = Join-Path $repoRoot ($knownUserEditDisplay -replace '/', '\')
$canonicalRoots = [ordered]@{
    'app-harness' = Join-Path $experimentRoot 'app-harness'
    'self-test-workspaces' = Join-Path $experimentRoot 'self-test-workspaces'
    'app-workspace' = Join-Path $experimentRoot 'app-workspace'
    'launcher-attestation-probe' = Join-Path $experimentRoot 'launcher-attestation-probe'
}

function Get-FullPath([string]$Path) {
    [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
}

function Test-EntryExists([string]$Path) {
    $full = Get-FullPath $Path
    if ([string]::Equals(
            $full,
            $repoRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        return $null -ne (Get-Item -Force -LiteralPath $full -ErrorAction SilentlyContinue)
    }
    $parent = [System.IO.Directory]::GetParent($full)
    if ($null -eq $parent -or -not [System.IO.Directory]::Exists($parent.FullName)) {
        return $false
    }
    foreach ($entry in [System.IO.Directory]::EnumerateFileSystemEntries($parent.FullName)) {
        if ([string]::Equals(
                (Get-FullPath $entry),
                $full,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            return $true
        }
    }
    return $false
}

function Assert-UnderRepo([string]$Path, [switch]$AllowRepoRoot) {
    $full = Get-FullPath $Path
    if ($AllowRepoRoot -and [string]::Equals(
            $full,
            $repoRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        return $full
    }
    if (-not $full.StartsWith(
            $repoRoot + '\',
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "path escapes the repository root: $full"
    }
    return $full
}

function Assert-RealDirectory([string]$Path, [string]$Label) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    $item = Get-Item -Force -LiteralPath $full -ErrorAction Stop
    if (-not $item.PSIsContainer) {
        throw "$Label is not a directory: $full"
    }
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Label must not be a reparse point: $full"
    }
    if (-not [string]::Equals(
            $item.FullName.TrimEnd('\'),
            $full,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "$Label resolves through an unexpected alias: $full"
    }
    return $full
}

function Assert-NoReparseAncestors([string]$Path) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    $cursor = $full
    while ($true) {
        if (Test-EntryExists $cursor) {
            $item = Get-Item -Force -LiteralPath $cursor -ErrorAction Stop
            if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
                throw "path has a reparse-point ancestor: $cursor"
            }
        }
        if ([string]::Equals(
                $cursor,
                $repoRoot,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            break
        }
        $parent = [System.IO.Directory]::GetParent($cursor)
        if ($null -eq $parent) {
            throw "could not walk path back to repository root: $full"
        }
        $cursor = $parent.FullName.TrimEnd('\')
    }
}

function New-SafeDirectory([string]$Path) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    $relative = [System.IO.Path]::GetRelativePath($repoRoot, $full)
    $cursor = $repoRoot
    if ($relative -eq '.') {
        return (Assert-RealDirectory $repoRoot 'repository root')
    }
    foreach ($component in $relative.Split([System.IO.Path]::DirectorySeparatorChar)) {
        if ([string]::IsNullOrWhiteSpace($component) -or $component -in @('.', '..')) {
            throw "unsafe directory component in path: $full"
        }
        $cursor = Join-Path $cursor $component
        if (-not (Test-EntryExists $cursor)) {
            New-Item -ItemType Directory -Path $cursor -ErrorAction Stop | Out-Null
        }
        Assert-RealDirectory $cursor 'created path component' | Out-Null
    }
    return $full
}

function Assert-PathAbsent([string]$Path, [string]$Label) {
    $full = Assert-UnderRepo $Path
    if (Test-EntryExists $full) {
        throw "$Label must be absent: $full"
    }
    Assert-NoReparseAncestors $full
}

function Get-Sha256([string]$Path) {
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction Stop
    if ($item.PSIsContainer -or
        $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "SHA-256 input must be a regular non-reparse file: $Path"
    }
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $item.FullName).Hash.ToLowerInvariant()
}

function Get-VolumeObservation {
    $volumeRoot = [System.IO.Path]::GetPathRoot($repoRoot)
    $drive = [System.IO.DriveInfo]::new($volumeRoot)
    return [pscustomobject][ordered]@{
        volume_root = $volumeRoot
        total_bytes = [int64]$drive.TotalSize
        available_free_bytes = [int64]$drive.AvailableFreeSpace
    }
}

function Write-NewText([string]$Path, [string]$Text) {
    $full = Assert-UnderRepo $Path
    Assert-PathAbsent $full 'new evidence file'
    New-SafeDirectory ([System.IO.Path]::GetDirectoryName($full)) | Out-Null
    [System.IO.File]::WriteAllText($full, $Text, $script:Utf8NoBom)
}

function Write-NewJson([string]$Path, [object]$Value, [int]$Depth = 20) {
    $json = $Value | ConvertTo-Json -Depth $Depth
    Write-NewText $Path ($json + "`n")
}

function Add-JournalRecord([string]$JournalPath, [object]$Record) {
    $line = ($Record | ConvertTo-Json -Compress -Depth 20) + "`n"
    [System.IO.File]::AppendAllText($JournalPath, $line, $script:Utf8NoBom)
}

function Convert-ToRepoRelative([string]$Path) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    return ([System.IO.Path]::GetRelativePath($repoRoot, $full) -replace '\\', '/')
}

function Invoke-CapturedCommand {
    param(
        [Parameter(Mandatory)][string]$Gate,
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][int]$TimeoutSeconds,
        [Parameter(Mandatory)][string]$LogsRoot,
        [Parameter(Mandatory)][string]$JournalPath,
        [hashtable]$Environment = @{}
    )

    if ($Gate -notmatch '^[a-z0-9][a-z0-9-]*$') {
        throw "unsafe gate name: $Gate"
    }
    $working = Assert-RealDirectory $WorkingDirectory 'command working directory'
    New-SafeDirectory $LogsRoot | Out-Null
    $stdoutPath = Join-Path $LogsRoot "$Gate.stdout.txt"
    $stderrPath = Join-Path $LogsRoot "$Gate.stderr.txt"
    Assert-PathAbsent $stdoutPath 'stdout capture'
    Assert-PathAbsent $stderrPath 'stderr capture'

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $working
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $clearGitEnvironment = [System.IO.Path]::GetFileName($FilePath) -in @('git', 'git.exe')
    if ($clearGitEnvironment) {
        foreach ($gitVariable in @(
                'GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY',
                'GIT_ALTERNATE_OBJECT_DIRECTORIES', 'GIT_COMMON_DIR',
                'GIT_CEILING_DIRECTORIES', 'GIT_DISCOVERY_ACROSS_FILESYSTEM'
            )) {
            $null = $startInfo.Environment.Remove($gitVariable)
        }
        $startInfo.Environment['LANG'] = 'C'
        $startInfo.Environment['LC_ALL'] = 'C'
    }
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    foreach ($name in $Environment.Keys) {
        $startInfo.Environment[[string]$name] = [string]$Environment[$name]
    }

    $startedAt = [DateTimeOffset]::UtcNow
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "failed to start gate $Gate"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $timedOut = $false
    try {
        $null = $process.WaitForExitAsync().WaitAsync(
            [TimeSpan]::FromSeconds($TimeoutSeconds)
        ).GetAwaiter().GetResult()
    }
    catch [System.TimeoutException] {
        $timedOut = $true
        try {
            $process.Kill($true)
        }
        catch {
            # Preserve the original timeout classification; postconditions
            # below still require the process to exit before evidence moves.
        }
        $null = $process.WaitForExitAsync().WaitAsync(
            [TimeSpan]::FromSeconds(15)
        ).GetAwaiter().GetResult()
    }
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $endedAt = [DateTimeOffset]::UtcNow
    [System.IO.File]::WriteAllText($stdoutPath, $stdout, $script:Utf8NoBom)
    [System.IO.File]::WriteAllText($stderrPath, $stderr, $script:Utf8NoBom)

    $evidenceBase = [System.IO.Directory]::GetParent((Get-FullPath $LogsRoot)).FullName
    $record = [ordered]@{
        schema = 's115-harness-command-v1'
        gate = $Gate
        file = $FilePath
        argv = @($Arguments)
        working_directory = Convert-ToRepoRelative $working
        environment = [ordered]@{}
        inherited_git_environment_cleared = $clearGitEnvironment
        git_locale = if ($clearGitEnvironment) { 'C' } else { $null }
        timeout_seconds = $TimeoutSeconds
        timed_out = $timedOut
        exit_code = if ($process.HasExited) { $process.ExitCode } else { $null }
        started_at_utc = $startedAt.ToString('o')
        ended_at_utc = $endedAt.ToString('o')
        duration_ms = [Math]::Round(($endedAt - $startedAt).TotalMilliseconds)
        stdout = ([System.IO.Path]::GetRelativePath($evidenceBase, $stdoutPath) -replace '\\', '/')
        stderr = ([System.IO.Path]::GetRelativePath($evidenceBase, $stderrPath) -replace '\\', '/')
        stdout_bytes = (Get-Item -LiteralPath $stdoutPath).Length
        stderr_bytes = (Get-Item -LiteralPath $stderrPath).Length
        stdout_sha256 = Get-Sha256 $stdoutPath
        stderr_sha256 = Get-Sha256 $stderrPath
    }
    foreach ($name in ($Environment.Keys | Sort-Object)) {
        $record.environment[[string]$name] = [string]$Environment[$name]
    }
    Add-JournalRecord $JournalPath $record
    return [pscustomobject]$record
}

function Assert-CapturedCommandRecordCollection {
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Records,
        [Parameter(Mandatory)][int]$ExpectedCount,
        [Parameter(Mandatory)][string]$Label
    )
    if ($Records.Count -ne $ExpectedCount) {
        $types = @($Records | ForEach-Object {
                if ($null -eq $_) { '<null>' } else { $_.GetType().FullName }
            }) -join ', '
        throw "$Label emitted $($Records.Count) objects instead of $ExpectedCount; types: $types"
    }
    foreach ($record in $Records) {
        $properties = @($record.PSObject.Properties.Name)
        foreach ($required in @(
                'schema', 'gate', 'timed_out', 'exit_code',
                'stdout_bytes', 'stderr_bytes', 'stdout_sha256', 'stderr_sha256'
            )) {
            if ($properties -notcontains $required) {
                throw "$Label record omits required property $required; type: $($record.GetType().FullName)"
            }
        }
        if ([string]$record.schema -cne 's115-harness-command-v1') {
            throw "$Label contains a record with an unexpected schema"
        }
    }
}

function Assert-CapturedCommandParity {
    param(
        [Parameter(Mandatory)][object[]]$Before,
        [Parameter(Mandatory)][object[]]$After,
        [Parameter(Mandatory)][string]$Label
    )
    $records = @($Before) + @($After)
    Assert-CapturedCommandRecordCollection -Records $records -ExpectedCount 2 -Label $Label
    $beforeRecord = $records[0]
    $afterRecord = $records[1]
    if ($beforeRecord.timed_out -or $afterRecord.timed_out -or
        $beforeRecord.exit_code -ne 0 -or $afterRecord.exit_code -ne 0 -or
        $beforeRecord.stdout_sha256 -cne $afterRecord.stdout_sha256 -or
        $beforeRecord.stdout_bytes -ne $afterRecord.stdout_bytes -or
        $beforeRecord.stderr_sha256 -cne $afterRecord.stderr_sha256 -or
        $beforeRecord.stderr_bytes -ne $afterRecord.stderr_bytes) {
        throw "$Label command records do not have byte-identical successful output"
    }
    return [pscustomobject]@{
        schema = 's115-command-record-parity-v1'
        status = 'pass'
        label = $Label
        before_gate = [string]$beforeRecord.gate
        after_gate = [string]$afterRecord.gate
    }
}

function Invoke-CommandRecordSemanticSelfTest {
    $emptySha = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855'
    $before = [pscustomobject]@{
        schema = 's115-harness-command-v1'; gate = 'semantic-before'
        timed_out = $false; exit_code = 0
        stdout_bytes = 0; stderr_bytes = 0
        stdout_sha256 = $emptySha; stderr_sha256 = $emptySha
    }
    $after = [pscustomobject]@{
        schema = 's115-harness-command-v1'; gate = 'semantic-after'
        timed_out = $false; exit_code = 0
        stdout_bytes = 0; stderr_bytes = 0
        stdout_sha256 = $emptySha; stderr_sha256 = $emptySha
    }
    Assert-CapturedCommandRecordCollection -Records @($before, $after) `
        -ExpectedCount 2 -Label 'semantic valid collection'
    $parity = Assert-CapturedCommandParity -Before $before -After $after `
        -Label 'semantic valid parity'

    $voidTaskResultRejected = $false
    $leakedCollection = @(
        [System.Threading.Tasks.Task]::CompletedTask.GetAwaiter().GetResult()
        $before
    )
    try {
        Assert-CapturedCommandRecordCollection -Records $leakedCollection `
            -ExpectedCount 1 -Label 'semantic leaked task result'
    }
    catch {
        $voidTaskResultRejected = $true
    }
    if (-not $voidTaskResultRejected) {
        throw 'semantic control did not reject an extra task-result output object'
    }

    $mismatchRejected = $false
    $mismatch = $after.PSObject.Copy()
    $mismatch.stdout_bytes = 1
    try {
        Assert-CapturedCommandParity -Before $before -After $mismatch `
            -Label 'semantic mismatch' | Out-Null
    }
    catch {
        $mismatchRejected = $true
    }
    if (-not $mismatchRejected) {
        throw 'semantic control did not reject command-record byte mismatch'
    }
    return [pscustomobject]@{
        schema = 's115-command-record-semantic-selftest-v1'
        status = 'pass'
        valid_collection_count = 2
        parity_status = [string]$parity.status
        extra_task_result_rejected = $voidTaskResultRejected
        byte_mismatch_rejected = $mismatchRejected
        attempt_created = $false
        preservation_move_run = $false
    }
}

function Get-LinkTarget([System.IO.FileSystemInfo]$Item) {
    $targets = @($Item.Target)
    if ($targets.Count -eq 0 -or $null -eq $targets[0]) {
        return $null
    }
    return [string]::Join("`u{001f}", [string[]]$targets)
}

function Write-TreeEntriesManifest([string]$RootPath, [string]$OutputPath) {
    $root = Assert-RealDirectory $RootPath 'manifest root'
    $entries = [System.Collections.Generic.SortedDictionary[string, object]]::new(
        [System.StringComparer]::Ordinal
    )
    $regularFileBytes = [int64]0
    $regularFileCount = 0
    $reparseCount = 0
    $entries.Add('.', [ordered]@{
            relative_path = '.'
            type = 'directory'
            size = 0
            sha256 = $null
            link_target = $null
        })
    $stack = [System.Collections.Generic.Stack[string]]::new()
    $stack.Push($root)

    while ($stack.Count -gt 0) {
        $directory = $stack.Pop()
        $children = @([System.IO.Directory]::EnumerateFileSystemEntries($directory))
        [Array]::Sort($children, [System.StringComparer]::Ordinal)
        foreach ($child in $children) {
            $item = Get-Item -Force -LiteralPath $child -ErrorAction Stop
            $relative = [System.IO.Path]::GetRelativePath($root, $item.FullName) -replace '\\', '/'
            $segments = $relative.Split('/')
            $hasControl = $false
            foreach ($character in $relative.ToCharArray()) {
                if ([int]$character -lt 32 -or [int]$character -eq 127) {
                    $hasControl = $true
                    break
                }
            }
            if ($relative -eq '.' -or $relative.StartsWith('../') -or
                [System.IO.Path]::IsPathRooted($relative) -or
                $segments -contains '..' -or $segments -contains '.' -or $hasControl) {
                throw "manifest entry escaped its root: $($item.FullName)"
            }
            $isReparse = $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            if ($isReparse) {
                $reparseCount++
                $kind = if ($item.PSIsContainer) { 'directory_reparse' } else { 'file_reparse' }
                $entry = [ordered]@{
                    relative_path = $relative
                    type = $kind
                    size = 0
                    sha256 = $null
                    link_target = Get-LinkTarget $item
                }
            }
            elseif ($item.PSIsContainer) {
                $entry = [ordered]@{
                    relative_path = $relative
                    type = 'directory'
                    size = 0
                    sha256 = $null
                    link_target = $null
                }
                $stack.Push($item.FullName)
            }
            else {
                $lengthBefore = [int64]$item.Length
                $sha = Get-Sha256 $item.FullName
                $rechecked = Get-Item -Force -LiteralPath $item.FullName -ErrorAction Stop
                if ($rechecked.PSIsContainer -or
                    $rechecked.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
                    [int64]$rechecked.Length -ne $lengthBefore) {
                    throw "manifest entry changed type or size while hashing: $($item.FullName)"
                }
                $entry = [ordered]@{
                    relative_path = $relative
                    type = 'regular_file'
                    size = $lengthBefore
                    sha256 = $sha
                    link_target = $null
                }
                $regularFileCount++
                $regularFileBytes += $lengthBefore
            }
            $entries.Add($relative, $entry)
        }
    }

    $lines = [System.Collections.Generic.List[string]]::new()
    foreach ($entry in $entries.Values) {
        $lines.Add(($entry | ConvertTo-Json -Compress -Depth 5))
    }
    Write-NewText $OutputPath (($lines -join "`n") + "`n")
    return [pscustomobject]@{
        entry_count = $entries.Count
        bytes = (Get-Item -LiteralPath $OutputPath).Length
        sha256 = Get-Sha256 $OutputPath
        regular_file_count = $regularFileCount
        regular_file_bytes = $regularFileBytes
        reparse_count = $reparseCount
    }
}

function Test-FilesByteEqual([string]$Left, [string]$Right) {
    $leftInfo = Get-Item -LiteralPath $Left -ErrorAction Stop
    $rightInfo = Get-Item -LiteralPath $Right -ErrorAction Stop
    if ($leftInfo.Length -ne $rightInfo.Length) {
        return $false
    }
    return (Get-Sha256 $Left) -eq (Get-Sha256 $Right)
}

function Invoke-ManifestedMove {
    param(
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$Destination,
        [Parameter(Mandatory)][string]$RootKey,
        [Parameter(Mandatory)][string]$BatchKey,
        [Parameter(Mandatory)][string]$EvidenceRoot
    )

    $sourceFull = Assert-UnderRepo $Source
    $destinationFull = Assert-UnderRepo $Destination
    $manifestRoot = New-SafeDirectory (Join-Path $EvidenceRoot "batches\$BatchKey")
    if (-not (Test-EntryExists $sourceFull)) {
        Assert-NoReparseAncestors $sourceFull
        $record = [ordered]@{
            schema = 's115-preserved-root-v1'
            batch = $BatchKey
            root_key = $RootKey
            present = $false
            source = Convert-ToRepoRelative $sourceFull
            destination = Convert-ToRepoRelative $destinationFull
            parity = $true
        }
        Write-NewJson (Join-Path $manifestRoot "$RootKey.move.json") $record
        return [pscustomobject]$record
    }

    Assert-RealDirectory $sourceFull "source root $RootKey" | Out-Null
    Assert-NoReparseAncestors $sourceFull
    Assert-PathAbsent $destinationFull "destination root $RootKey"
    New-SafeDirectory ([System.IO.Path]::GetDirectoryName($destinationFull)) | Out-Null

    $sourceVolume = [System.IO.Path]::GetPathRoot($sourceFull)
    $destinationVolume = [System.IO.Path]::GetPathRoot($destinationFull)
    if (-not [string]::Equals(
            $sourceVolume,
            $destinationVolume,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "source and destination are not on the same volume for $RootKey"
    }

    $beforePath = Join-Path $manifestRoot "$RootKey.before.entries.jsonl"
    $afterPath = Join-Path $manifestRoot "$RootKey.after.entries.jsonl"
    $before = Write-TreeEntriesManifest $sourceFull $beforePath
    # Close the manifest-to-rename gap as far as standard path APIs permit:
    # re-resolve both fixed endpoints and their types immediately before the
    # same-volume rename. Descendant content parity is checked again after it.
    Assert-RealDirectory $sourceFull "source root $RootKey immediately before move" | Out-Null
    Assert-NoReparseAncestors $sourceFull
    Assert-RealDirectory ([System.IO.Path]::GetDirectoryName($destinationFull)) `
        "destination parent $RootKey immediately before move" | Out-Null
    Assert-PathAbsent $destinationFull "destination root $RootKey immediately before move"
    Move-Item -LiteralPath $sourceFull -Destination $destinationFull -ErrorAction Stop
    Assert-PathAbsent $sourceFull "moved source root $RootKey"
    Assert-RealDirectory $destinationFull "moved destination root $RootKey" | Out-Null
    $after = Write-TreeEntriesManifest $destinationFull $afterPath
    $byteIdentical = Test-FilesByteEqual $beforePath $afterPath
    if (-not $byteIdentical -or $before.entry_count -ne $after.entry_count) {
        throw "pre/post manifests differ after moving $RootKey"
    }

    $record = [ordered]@{
        schema = 's115-preserved-root-v1'
        batch = $BatchKey
        root_key = $RootKey
        present = $true
        source = Convert-ToRepoRelative $sourceFull
        destination = Convert-ToRepoRelative $destinationFull
        same_volume = $true
        before_manifest = ([System.IO.Path]::GetRelativePath($EvidenceRoot, $beforePath) -replace '\\', '/')
        after_manifest = ([System.IO.Path]::GetRelativePath($EvidenceRoot, $afterPath) -replace '\\', '/')
        entry_count = $before.entry_count
        regular_file_count = $before.regular_file_count
        regular_file_bytes = $before.regular_file_bytes
        reparse_count = $before.reparse_count
        manifest_bytes = $before.bytes
        entries_sha256 = $before.sha256
        byte_identical_manifests = $byteIdentical
        parity = $true
    }
    Write-NewJson (Join-Path $manifestRoot "$RootKey.move.json") $record
    return [pscustomobject]$record
}

function Copy-TreeWithoutReparse([string]$Source, [string]$Destination) {
    $sourceRoot = Assert-RealDirectory $Source 'copy source root'
    Assert-PathAbsent $Destination 'copy destination root'
    New-SafeDirectory $Destination | Out-Null
    $stack = [System.Collections.Generic.Stack[object]]::new()
    $stack.Push([pscustomobject]@{ Source = $sourceRoot; Destination = Get-FullPath $Destination })
    while ($stack.Count -gt 0) {
        $pair = $stack.Pop()
        $children = @([System.IO.Directory]::EnumerateFileSystemEntries($pair.Source))
        [Array]::Sort($children, [System.StringComparer]::Ordinal)
        foreach ($child in $children) {
            $item = Get-Item -Force -LiteralPath $child -ErrorAction Stop
            if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
                throw "copy source contains a forbidden reparse point: $($item.FullName)"
            }
            $destinationChild = Join-Path $pair.Destination $item.Name
            Assert-PathAbsent $destinationChild 'copy destination entry'
            if ($item.PSIsContainer) {
                New-SafeDirectory $destinationChild | Out-Null
                $stack.Push([pscustomobject]@{
                        Source = $item.FullName
                        Destination = $destinationChild
                    })
            }
            else {
                [System.IO.File]::Copy($item.FullName, $destinationChild, $false)
            }
        }
    }
}

function Get-NextAttempt {
    New-SafeDirectory $preservationRoot | Out-Null
    New-SafeDirectory $retainedAttemptsRoot | Out-Null
    for ($number = 1; $number -le 999; $number++) {
        $name = $number.ToString("D$($script:AttemptWidth)")
        $targetAttempt = Join-Path $preservationRoot "attempt-$name"
        $retainedAttempt = Join-Path $retainedAttemptsRoot $name
        if ((Test-EntryExists $targetAttempt) -or
            (Test-EntryExists $retainedAttempt)) {
            continue
        }
        try {
            New-Item -ItemType Directory -Path $targetAttempt -ErrorAction Stop | Out-Null
            return [pscustomobject]@{
                number = $number
                name = $name
                target_root = $targetAttempt
                retained_root = $retainedAttempt
            }
        }
        catch [System.IO.IOException] {
            continue
        }
    }
    throw 'no free T-11502 attempt number remains in the 001-999 range'
}

function Assert-RegularSourceFile([string]$Path, [string]$Label) {
    $item = Get-Item -Force -LiteralPath $Path -ErrorAction Stop
    if ($item.PSIsContainer -or
        $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Label must be a regular non-reparse file: $Path"
    }
    Assert-NoReparseAncestors $item.FullName
    return $item
}

function Materialize-FrozenHarness {
    param(
        [Parameter(Mandatory)][string]$DestinationHarness,
        [Parameter(Mandatory)][string]$EvidenceRoot
    )

    Assert-RealDirectory $sourceHarnessRoot 'source frozen harness' | Out-Null
    $manifestPath = Join-Path $sourceHarnessRoot 'frozen-inputs.json'
    $companionPath = Join-Path $sourceHarnessRoot 'frozen-inputs.sha256'
    Assert-RegularSourceFile $manifestPath 'frozen manifest' | Out-Null
    Assert-RegularSourceFile $companionPath 'frozen manifest companion' | Out-Null
    $manifestSha = Get-Sha256 $manifestPath
    $companionValue = (Get-Content -Raw -LiteralPath $companionPath).Trim()
    if ($manifestSha -ne $script:PinnedFrozenManifestSha256 -or
        $companionValue -ne $script:PinnedFrozenManifestSha256) {
        throw 'the frozen input manifest or companion differs from the pinned Sprint 114 identity'
    }

    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    if ($manifest.schema -ne 'mh-rs01-frozen-inputs-v1' -or
        @($manifest.files).Count -ne 30) {
        throw 'the frozen input manifest schema or file count is unexpected'
    }
    Assert-PathAbsent $DestinationHarness 'transient harness root'
    New-SafeDirectory $DestinationHarness | Out-Null
    $seen = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    $copies = [System.Collections.Generic.List[object]]::new()
    foreach ($entry in @($manifest.files)) {
        $relative = [string]$entry.path
        if ([string]::IsNullOrWhiteSpace($relative) -or
            $relative.Contains('\') -or
            [System.IO.Path]::IsPathRooted($relative) -or
            $relative.Split('/') -contains '..' -or
            -not $seen.Add($relative)) {
            throw "unsafe or duplicate frozen input path: $relative"
        }
        $source = Join-Path $sourceHarnessRoot ($relative -replace '/', '\')
        $destination = Join-Path $DestinationHarness ($relative -replace '/', '\')
        $sourceItem = Assert-RegularSourceFile $source "frozen input $relative"
        $sourceSha = Get-Sha256 $source
        if ($sourceItem.Length -ne [int64]$entry.bytes -or
            $sourceSha -ne [string]$entry.sha256) {
            throw "frozen input does not match its manifest: $relative"
        }
        New-SafeDirectory ([System.IO.Path]::GetDirectoryName($destination)) | Out-Null
        Assert-PathAbsent $destination 'transient frozen input'
        [System.IO.File]::Copy($source, $destination, $false)
        if ((Get-Sha256 $destination) -ne $sourceSha -or
            (Get-Item -LiteralPath $destination).Length -ne $sourceItem.Length) {
            throw "copied frozen input differs from source: $relative"
        }
        $copies.Add([ordered]@{
                path = $relative
                bytes = [int64]$sourceItem.Length
                sha256 = $sourceSha
            })
    }
    foreach ($controlFile in @('frozen-inputs.json', 'frozen-inputs.sha256')) {
        $source = Join-Path $sourceHarnessRoot $controlFile
        $destination = Join-Path $DestinationHarness $controlFile
        Assert-PathAbsent $destination 'transient manifest control file'
        [System.IO.File]::Copy($source, $destination, $false)
        if ((Get-Sha256 $source) -ne (Get-Sha256 $destination)) {
            throw "copied manifest control file differs: $controlFile"
        }
    }

    $inventoryPath = Join-Path $EvidenceRoot 'frozen-copy.initial.entries.jsonl'
    $inventory = Write-TreeEntriesManifest $DestinationHarness $inventoryPath
    $expectedFiles = @($seen) + @('frozen-inputs.json', 'frozen-inputs.sha256')
    $actualFiles = @(
        Get-Content -LiteralPath $inventoryPath |
            ForEach-Object { $_ | ConvertFrom-Json } |
            Where-Object { $_.type -eq 'regular_file' } |
            ForEach-Object { [string]$_.relative_path }
    )
    $expectedSorted = @($expectedFiles | Sort-Object)
    $actualSorted = @($actualFiles | Sort-Object)
    if (($expectedSorted -join "`n") -cne ($actualSorted -join "`n")) {
        throw 'the transient harness contains missing or extra input files'
    }
    $seedCopies = @($copies | Where-Object { $_.path.StartsWith('seed/', [System.StringComparison]::Ordinal) })
    if ($seedCopies.Count -ne 5) {
        throw 'the frozen harness manifest no longer contains the exact five-file seed'
    }
    $record = [ordered]@{
        schema = 's115-frozen-harness-copy-v1'
        source = $sourceHarnessDisplay
        destination = Convert-ToRepoRelative $DestinationHarness
        frozen_manifest_sha256 = $manifestSha
        frozen_file_count = $copies.Count
        frozen_seed_file_count = $seedCopies.Count
        control_file_count = 2
        initial_tree_entries = ([System.IO.Path]::GetRelativePath($EvidenceRoot, $inventoryPath) -replace '\\', '/')
        initial_tree_entries_sha256 = $inventory.sha256
        files = @($copies)
        seed_files = @($seedCopies)
    }
    Write-NewJson (Join-Path $EvidenceRoot 'frozen-copy.json') $record 30
    return [pscustomobject]$record
}

function Assert-ExactPropertyNames([object]$Value, [string[]]$Expected, [string]$Label) {
    $actual = @($Value.PSObject.Properties.Name | Sort-Object)
    $wanted = @($Expected | Sort-Object)
    if (($actual -join "`n") -cne ($wanted -join "`n")) {
        throw "$Label property set differs from the frozen schema"
    }
}

function Assert-FrozenCopyInputsStillMatch {
    param(
        [Parameter(Mandatory)][string]$CopiedHarness,
        [Parameter(Mandatory)][string]$EvidenceRoot
    )
    $manifestPath = Join-Path $sourceHarnessRoot 'frozen-inputs.json'
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
    $expected = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($entry in @($manifest.files)) {
        $relative = [string]$entry.path
        $null = $expected.Add($relative)
        $copied = Join-Path $CopiedHarness ($relative -replace '/', '\')
        $item = Assert-RegularSourceFile $copied "post-selftest frozen copy $relative"
        if ($item.Length -ne [int64]$entry.bytes -or
            (Get-Sha256 $copied) -ne [string]$entry.sha256) {
            throw "self-test changed a frozen copied input: $relative"
        }
    }
    foreach ($controlFile in @('frozen-inputs.json', 'frozen-inputs.sha256')) {
        $null = $expected.Add($controlFile)
        $source = Join-Path $sourceHarnessRoot $controlFile
        $copied = Join-Path $CopiedHarness $controlFile
        Assert-RegularSourceFile $copied "post-selftest copied $controlFile" | Out-Null
        if ((Get-Sha256 $source) -ne (Get-Sha256 $copied)) {
            throw "self-test changed copied $controlFile"
        }
    }
    $finalPath = Join-Path $EvidenceRoot 'frozen-copy.final-inputs.entries.jsonl'
    $tree = Write-TreeEntriesManifest $CopiedHarness $finalPath
    foreach ($line in Get-Content -LiteralPath $finalPath) {
        $entry = $line | ConvertFrom-Json
        if ([string]$entry.type -like '*reparse') {
            throw "copied harness contains a reparse point after self-test: $($entry.relative_path)"
        }
        if ($entry.type -eq 'regular_file' -and
            -not ([string]$entry.relative_path).StartsWith('evidence/', [System.StringComparison]::Ordinal) -and
            -not $expected.Contains([string]$entry.relative_path)) {
            throw "self-test created an unexpected file outside copied evidence: $($entry.relative_path)"
        }
    }
    return [pscustomobject]@{
        frozen_file_count = 30
        control_file_count = 2
        tree_entry_count = $tree.entry_count
        final_entries_sha256 = $tree.sha256
        frozen_inputs_unchanged = $true
        only_generated_files_are_under_evidence = $true
    }
}

function Assert-SelfTestSummary([string]$GeneratedEvidenceRoot) {
    $summaryPath = Join-Path $GeneratedEvidenceRoot 'self-test-summary.json'
    $summaryCompanion = Join-Path $GeneratedEvidenceRoot 'self-test-summary.sha256'
    Assert-RegularSourceFile $summaryPath 'generated self-test summary' | Out-Null
    Assert-RegularSourceFile $summaryCompanion 'generated self-test summary companion' | Out-Null
    $summarySha = Get-Sha256 $summaryPath
    if ((Get-Content -Raw -LiteralPath $summaryCompanion).Trim() -ne $summarySha) {
        throw 'generated self-test summary companion does not match the summary'
    }
    $summary = Get-Content -Raw -LiteralPath $summaryPath | ConvertFrom-Json
    Assert-ExactPropertyNames $summary @(
        'schema', 'status', 'tests', 'baseline', 'known_good_dimensions',
        'deterministic_grade_replay', 'output_spoof_resistance',
        'trusted_failure_exit_mapping', 'inherited_resource_override_rejection',
        'broken_early_journal_chain_rejection', 'violation_matrix',
        'journal_snapshot', 'journal_snapshot_companion', 'journal_sha256',
        'journal_snapshot_companion_sha256', 'grader_binary_sha256',
        'grader_source_tree_sha256', 'frozen_input_manifest_sha256'
    ) 'self-test summary'
    Assert-ExactPropertyNames $summary.tests @(
        'mh_rs01_seed_baseline_and_immutability',
        'bubblewrap_execution_boundary_canaries',
        'grader_known_good_and_violation_matrix'
    ) 'self-test tests'
    Assert-ExactPropertyNames $summary.baseline @(
        'exit_code', 'e0583_count', 'other_rust_error_codes'
    ) 'self-test baseline'
    Assert-ExactPropertyNames $summary.known_good_dimensions @(
        'seed_immutability', 'dependency_policy', 'path_policy', 'plan',
        'model_tests', 'visible_contract', 'hidden_contract', 'cli_contract',
        'source_safety'
    ) 'self-test dimensions'
    Assert-ExactPropertyNames $summary.violation_matrix @(
        'static_cases', 'dynamic_cases', 'dynamic_dimensions', 'model_registration_cases'
    ) 'self-test violation matrix'
    if ($summary.schema -ne 's114-harness-self-test-v1' -or
        $summary.status -ne 'pass' -or
        $summary.tests.mh_rs01_seed_baseline_and_immutability -ne 'pass' -or
        $summary.tests.bubblewrap_execution_boundary_canaries -ne 'pass' -or
        $summary.tests.grader_known_good_and_violation_matrix -ne 'pass' -or
        $summary.baseline.exit_code -ne 101 -or
        $summary.baseline.e0583_count -ne 3 -or
        $summary.baseline.other_rust_error_codes -ne 0 -or
        $summary.deterministic_grade_replay -ne 'pass' -or
        $summary.output_spoof_resistance -ne 'pass' -or
        $summary.trusted_failure_exit_mapping -ne 'pass' -or
        $summary.inherited_resource_override_rejection -ne 'pass' -or
        $summary.broken_early_journal_chain_rejection -ne 'pass' -or
        $summary.frozen_input_manifest_sha256 -ne $script:PinnedFrozenManifestSha256) {
        throw 'generated self-test summary does not satisfy the frozen positive/negative contract'
    }
    foreach ($dimension in @(
            'seed_immutability', 'dependency_policy', 'path_policy', 'plan',
            'model_tests', 'visible_contract', 'hidden_contract',
            'cli_contract', 'source_safety'
        )) {
        if ($summary.known_good_dimensions.$dimension -ne 'pass') {
            throw "generated self-test summary did not pass dimension: $dimension"
        }
    }
    if ($summary.violation_matrix.static_cases -ne 13 -or
        $summary.violation_matrix.dynamic_cases -ne 5 -or
        $summary.violation_matrix.dynamic_dimensions -ne 4 -or
        $summary.violation_matrix.model_registration_cases -ne 2) {
        throw 'generated self-test violation matrix differs from the frozen contract'
    }
    foreach ($hashField in @(
            'journal_sha256', 'journal_snapshot_companion_sha256',
            'grader_binary_sha256', 'grader_source_tree_sha256',
            'frozen_input_manifest_sha256'
        )) {
        if ([string]$summary.$hashField -cnotmatch '^[0-9a-f]{64}$') {
            throw "generated self-test summary has an invalid $hashField"
        }
    }
    foreach ($relativeField in @('journal_snapshot', 'journal_snapshot_companion')) {
        $relativeValue = [string]$summary.$relativeField
        if ([string]::IsNullOrWhiteSpace($relativeValue) -or
            [System.IO.Path]::IsPathRooted($relativeValue) -or
            $relativeValue.Split('/') -contains '..' -or
            $relativeValue.Contains('\')) {
            throw "generated self-test summary has an unsafe $relativeField"
        }
    }
    $expectedSnapshot = "journal-snapshot-$($summary.journal_sha256)/command-journal.tsv"
    if ([string]$summary.journal_snapshot -cne $expectedSnapshot -or
        [string]$summary.journal_snapshot_companion -cne "$expectedSnapshot.sha256") {
        throw 'generated self-test snapshot paths do not bind the journal content hash'
    }
    $snapshot = Join-Path $GeneratedEvidenceRoot ([string]$summary.journal_snapshot -replace '/', '\')
    $snapshotCompanion = Join-Path $GeneratedEvidenceRoot ([string]$summary.journal_snapshot_companion -replace '/', '\')
    Assert-RegularSourceFile $snapshot 'generated journal snapshot' | Out-Null
    Assert-RegularSourceFile $snapshotCompanion 'generated journal companion' | Out-Null
    if ((Get-Sha256 $snapshot) -ne [string]$summary.journal_sha256 -or
        (Get-Sha256 $snapshotCompanion) -ne [string]$summary.journal_snapshot_companion_sha256) {
        throw 'generated self-test journal evidence differs from its summary'
    }
    return [pscustomobject]@{
        summary_sha256 = $summarySha
        summary_companion_sha256 = Get-Sha256 $summaryCompanion
        journal_sha256 = [string]$summary.journal_sha256
        journal_companion_sha256 = [string]$summary.journal_snapshot_companion_sha256
        grader_binary_sha256 = [string]$summary.grader_binary_sha256
        grader_source_tree_sha256 = [string]$summary.grader_source_tree_sha256
    }
}

function Invoke-StandaloneGitProbe {
    param(
        [Parameter(Mandatory)][string]$CandidateRoot,
        [Parameter(Mandatory)][string]$MetadataRoot,
        [Parameter(Mandatory)][string]$LogsRoot,
        [Parameter(Mandatory)][string]$JournalPath,
        [Parameter(Mandatory)][string]$EvidenceRoot
    )

    Assert-PathAbsent $CandidateRoot 'standalone Git candidate root'
    Assert-PathAbsent $MetadataRoot 'standalone Git metadata root'
    $candidate = New-SafeDirectory $CandidateRoot
    $metadata = Get-FullPath $MetadataRoot
    $candidateParent = [System.IO.Directory]::GetParent($candidate).FullName
    $metadataParent = [System.IO.Directory]::GetParent($metadata).FullName
    if (-not [string]::Equals(
            $candidateParent,
            $experimentRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -or -not [string]::Equals(
            $metadataParent,
            $experimentRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -or [string]::Equals(
            $candidate,
            $metadata,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'standalone Git candidate and metadata must be distinct exact siblings'
    }
    $readme = Join-Path $candidate 'README.md'
    Write-NewText $readme "standalone external Git metadata probe`n"

    $gitRoots = @(
        "--git-dir=$metadata",
        "--work-tree=$candidate"
    )
    $init = Invoke-CapturedCommand -Gate 'git-init-external-metadata' -FilePath 'git.exe' `
        -Arguments @($gitRoots + @('init')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    if ($init.timed_out -or $init.exit_code -ne 0) {
        throw 'external Git metadata initialization failed'
    }
    Assert-RealDirectory $metadata 'external Git metadata root' | Out-Null

    $commonGit = @($gitRoots + @('-c', 'core.bare=false'))
    $config = Invoke-CapturedCommand -Gate 'git-explicit-config' -FilePath 'git.exe' `
        -Arguments @($gitRoots + @('config', '--get', 'core.bare')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    $configText = (Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-config.stdout.txt')).Trim()
    if ($config.timed_out -or $config.exit_code -ne 0 -or $configText -cne 'false') {
        throw 'explicit external-metadata Git configuration is not a work-tree repository'
    }
    $add = Invoke-CapturedCommand -Gate 'git-explicit-add' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('add', '--all')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    if ($add.timed_out -or $add.exit_code -ne 0) {
        throw 'explicit external-metadata Git add failed'
    }
    $commit = Invoke-CapturedCommand -Gate 'git-explicit-commit' -FilePath 'git.exe' `
        -Arguments @($commonGit + @(
                '-c', 'user.name=Animus Ferric Harness',
                '-c', 'user.email=example@example.invalid',
                'commit', '-m', 'standalone probe'
            )) -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $LogsRoot -JournalPath $JournalPath
    if ($commit.timed_out -or $commit.exit_code -ne 0) {
        throw 'explicit external-metadata Git commit failed'
    }
    $head = Invoke-CapturedCommand -Gate 'git-explicit-head' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('rev-parse', 'HEAD')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    $tree = Invoke-CapturedCommand -Gate 'git-explicit-tree' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('rev-parse', 'HEAD^{tree}')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    $top = Invoke-CapturedCommand -Gate 'git-explicit-toplevel' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('rev-parse', '--show-toplevel')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    $status = Invoke-CapturedCommand -Gate 'git-explicit-status' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('status', '--porcelain=v1', '--untracked-files=all')) `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $LogsRoot -JournalPath $JournalPath
    $gitDir = Invoke-CapturedCommand -Gate 'git-explicit-git-dir' -FilePath 'git.exe' `
        -Arguments @($commonGit + @('rev-parse', '--absolute-git-dir')) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath
    $ambient = Invoke-CapturedCommand -Gate 'git-ambient-discovery-blocked' -FilePath 'git.exe' `
        -Arguments @('-C', $candidate, 'rev-parse', '--show-toplevel') -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $LogsRoot -JournalPath $JournalPath `
        -Environment @{ GIT_CEILING_DIRECTORIES = $experimentRoot }

    $topText = (Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-toplevel.stdout.txt')).Trim()
    $statusText = Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-status.stdout.txt')
    $gitDirText = (Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-git-dir.stdout.txt')).Trim()
    $candidateDotGit = Join-Path $candidate '.git'
    $topMatches = [string]::Equals(
        (Get-FullPath $topText),
        (Get-FullPath $candidate),
        [System.StringComparison]::OrdinalIgnoreCase
    )
    $gitDirMatches = [string]::Equals(
        (Get-FullPath $gitDirText),
        (Get-FullPath $metadata),
        [System.StringComparison]::OrdinalIgnoreCase
    )
    $headText = (Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-head.stdout.txt')).Trim()
    $treeText = (Get-Content -Raw -LiteralPath (Join-Path $LogsRoot 'git-explicit-tree.stdout.txt')).Trim()
    $passed = -not $head.timed_out -and $head.exit_code -eq 0 -and
        $headText -cmatch '^[0-9a-f]{40,64}$' -and
        -not $tree.timed_out -and $tree.exit_code -eq 0 -and
        $treeText -cmatch '^[0-9a-f]{40,64}$' -and
        -not $top.timed_out -and $top.exit_code -eq 0 -and $topMatches -and
        -not $status.timed_out -and $status.exit_code -eq 0 -and
        [string]::IsNullOrWhiteSpace($statusText) -and
        -not $gitDir.timed_out -and $gitDir.exit_code -eq 0 -and $gitDirMatches -and
        -not $ambient.timed_out -and $ambient.exit_code -ne 0 -and
        -not (Test-EntryExists $candidateDotGit)
    if (-not $passed) {
        throw 'standalone Git candidate probe did not satisfy the external-metadata and ceiling contract'
    }

    $record = [ordered]@{
        schema = 's115-standalone-git-probe-v1'
        status = 'pass'
        candidate = Convert-ToRepoRelative $candidate
        metadata = Convert-ToRepoRelative $metadata
        candidate_dot_git_absent = $true
        metadata_is_sibling = $true
        separate_git_dir_option_used = $false
        initialization_named_both_roots = $true
        core_bare_false = $true
        explicit_toplevel_matches_candidate = $topMatches
        explicit_git_dir_matches_metadata = $gitDirMatches
        explicit_status_clean = $true
        commit = $headText
        tree = $treeText
        git_ceiling_directories = Convert-ToRepoRelative $experimentRoot
        ambient_parent_discovery_blocked = $true
    }
    Write-NewJson (Join-Path $EvidenceRoot 'standalone-git.json') $record
    return [pscustomobject]$record
}

function ConvertFrom-Base64Utf8([string]$Value, [string]$Label) {
    try {
        $strictUtf8 = [System.Text.UTF8Encoding]::new($false, $true)
        $text = $strictUtf8.GetString(
            [System.Convert]::FromBase64String($Value)
        )
    }
    catch {
        throw "$Label is not valid base64 UTF-8"
    }
    if ($text.Contains([char]0)) {
        throw "$Label contains a NUL byte"
    }
    return $text
}

function Assert-WslRepositoryPath([string]$Path, [string]$Label) {
    $hasControl = $false
    foreach ($character in $Path.ToCharArray()) {
        if ([int]$character -lt 32 -or [int]$character -eq 127) {
            $hasControl = $true
            break
        }
    }
    $segments = $Path.Split('/')
    if ([string]::IsNullOrWhiteSpace($Path) -or $hasControl -or
        $Path.Contains('\') -or $Path.EndsWith('/') -or
        $Path -cnotmatch '^/mnt/[a-z]/' -or
        $segments.Count -lt 5 -or $segments[0] -cne '' -or
        $segments -contains '.' -or $segments -contains '..' -or
        @($segments | Where-Object { $_ -ceq '' }).Count -ne 1) {
        throw "$Label is not a safe exact WSL repository path: $Path"
    }
    return $Path
}

function Get-TextSha256([string]$Text) {
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
    return [System.Convert]::ToHexString(
        [System.Security.Cryptography.SHA256]::HashData($bytes)
    ).ToLowerInvariant()
}

function Find-ExactValueIndex([object[]]$Values, [string]$Value, [int]$StartIndex) {
    for ($index = $StartIndex; $index -lt $Values.Count; $index++) {
        if ([string]$Values[$index] -ceq $Value) {
            return $index
        }
    }
    return -1
}

function Invoke-LiveHarnessJournalAudit {
    param(
        [Parameter(Mandatory)][string]$LiveHarnessRoot,
        [Parameter(Mandatory)][string]$ExpectedJournalSha256,
        [Parameter(Mandatory)][string]$EvidenceRoot,
        [Parameter(Mandatory)][string]$WslRepositoryRoot
    )

    $root = Assert-RealDirectory $LiveHarnessRoot 'post-quarantined live harness root'
    $wslRoot = Assert-WslRepositoryPath $WslRepositoryRoot 'journal WSL repository root'
    $canonicalPrefix = "$wslRoot/target/s114-experiment/app-harness/"
    $journalPath = Join-Path $root 'command-journal.tsv'
    $companionPath = "$journalPath.sha256"
    Assert-RegularSourceFile $journalPath 'post-quarantined live harness journal' | Out-Null
    Assert-RegularSourceFile $companionPath 'post-quarantined live journal companion' | Out-Null
    $journalSha = Get-Sha256 $journalPath
    $companionText = (Get-Content -Raw -LiteralPath $companionPath).Trim()
    if ($journalSha -ne $ExpectedJournalSha256 -or
        $companionText -notmatch ('^' + [regex]::Escape($journalSha) + '(?:\s|$)')) {
        throw 'live journal, generated snapshot, and companion hashes do not agree'
    }

    $lines = @(Get-Content -LiteralPath $journalPath)
    $expectedHeader = "schema`tsequence`tprevious_sha256`tstage_b64`tcwd_b64`targv_b64`texit_code`tstdout_path_b64`tstdout_sha256`tstderr_path_b64`tstderr_sha256`tentry_sha256"
    if ($lines.Count -lt 2 -or $lines[0] -cne $expectedHeader) {
        throw 'live harness journal header or row count is invalid'
    }
    $previous = '0000000000000000000000000000000000000000000000000000000000000000'
    $outputs = [System.Collections.Generic.List[object]]::new()
    $launchers = [System.Collections.Generic.List[object]]::new()
    $sandboxCount = 0
    $containmentCanary = $false

    for ($index = 1; $index -lt $lines.Count; $index++) {
        $fields = $lines[$index].Split("`t")
        if ($fields.Count -ne 12 -or $fields[0] -cne 's114-command-journal-v1' -or
            [int]$fields[1] -ne $index -or $fields[2] -cne $previous -or
            $fields[11] -cne (Get-TextSha256 (($fields[0..10] -join "`t")))) {
            throw "live harness journal chain is invalid at row $index"
        }
        $stage = ConvertFrom-Base64Utf8 $fields[3] "journal stage row $index"
        $cwd = ConvertFrom-Base64Utf8 $fields[4] "journal cwd row $index"
        $cwdHasControl = $false
        foreach ($character in $cwd.ToCharArray()) {
            if ([int]$character -lt 32 -or [int]$character -eq 127) {
                $cwdHasControl = $true
                break
            }
        }
        if ($cwdHasControl -or $cwd.Contains('\') -or $cwd -cne $wslRoot) {
            throw "journal cwd does not equal the exact WSL repository root at row $index"
        }
        $argv = @()
        if (-not [string]::IsNullOrEmpty($fields[5])) {
            foreach ($encodedArg in $fields[5].Split(',')) {
                $argv += ConvertFrom-Base64Utf8 $encodedArg "journal argv row $index"
            }
        }
        $decodedPaths = @(
            [pscustomobject]@{ stream = 'stdout'; encoded = $fields[7]; expected = $fields[8] },
            [pscustomobject]@{ stream = 'stderr'; encoded = $fields[9]; expected = $fields[10] }
        )
        $stdoutLivePath = $null
        foreach ($pathRecord in $decodedPaths) {
            $originalPath = ConvertFrom-Base64Utf8 $pathRecord.encoded `
                "journal $($pathRecord.stream) path row $index"
            $pathHasControl = $false
            foreach ($character in $originalPath.ToCharArray()) {
                if ([int]$character -lt 32 -or [int]$character -eq 127) {
                    $pathHasControl = $true
                    break
                }
            }
            if ($pathHasControl -or $originalPath.Contains('\')) {
                throw "journal output path contains a control character or backslash at row $index"
            }
            if (-not $originalPath.StartsWith(
                    $canonicalPrefix,
                    [System.StringComparison]::Ordinal
                )) {
                throw "journal output path does not start at the exact canonical WSL app-harness root at row $index"
            }
            $tail = $originalPath.Substring($canonicalPrefix.Length)
            $tailSegments = $tail.Split('/')
            $logsSegments = @($tailSegments | Where-Object { $_ -ceq 'logs' })
            if ([string]::IsNullOrWhiteSpace($tail) -or
                [System.IO.Path]::IsPathRooted($tail) -or
                $tail.StartsWith('/') -or $tailSegments -contains '' -or
                $tailSegments -contains '..' -or $tailSegments -contains '.' -or
                $logsSegments.Count -ne 1) {
                throw "journal output path has an unsafe or non-log tail at row $index"
            }
            $livePath = Join-Path $root ($tail -replace '/', '\')
            $livePath = Get-FullPath $livePath
            if (-not $livePath.StartsWith(
                    $root + '\',
                    [System.StringComparison]::OrdinalIgnoreCase
                )) {
                throw "rebased journal output escaped the post-quarantine root at row $index"
            }
            Assert-RegularSourceFile $livePath "rebased journal $($pathRecord.stream) row $index" | Out-Null
            $actualSha = Get-Sha256 $livePath
            if ($actualSha -ne [string]$pathRecord.expected) {
                throw "rebased journal $($pathRecord.stream) hash differs at row $index"
            }
            if ($pathRecord.stream -eq 'stdout') {
                $stdoutLivePath = $livePath
            }
            $outputs.Add([ordered]@{
                    sequence = $index
                    stage = $stage
                    stream = $pathRecord.stream
                    retained_path = Convert-ToRepoRelative $livePath
                    sha256 = $actualSha
                })
        }

        $bwrapIndexes = @(
            for ($argvIndex = 0; $argvIndex -lt $argv.Count; $argvIndex++) {
                if ($argv[$argvIndex] -ceq 'bwrap') { $argvIndex }
            }
        )
        if ($bwrapIndexes.Count -gt 0) {
            if ($bwrapIndexes.Count -ne 1 -or $argv.Count -lt 8 -or
                $argv[0] -cne 'timeout') {
                throw "sandbox journal row $index has an invalid timeout/bwrap prefix"
            }
            $bwrapIndex = [int]$bwrapIndexes[0]
            $bwrapSeparator = Find-ExactValueIndex ([object[]]$argv) '--' ($bwrapIndex + 1)
            if ($bwrapSeparator -le $bwrapIndex + 1 -or
                $bwrapSeparator + 1 -ge $argv.Count -or
                $argv[$bwrapSeparator + 1] -cne '/usr/bin/prlimit') {
                throw "sandbox journal row $index has an invalid bwrap/prlimit boundary"
            }
            $bwrapOptions = @($argv[($bwrapIndex + 1)..($bwrapSeparator - 1)])
            $sandboxCount++
            foreach ($requiredArgument in @('--unshare-user', '--unshare-pid', '--unshare-net', '--json-status-fd')) {
                if ($bwrapOptions -notcontains $requiredArgument) {
                    throw "sandbox journal row $index omits $requiredArgument"
                }
            }
            $jsonStatusIndex = Find-ExactValueIndex ([object[]]$bwrapOptions) '--json-status-fd' 0
            if ($jsonStatusIndex -lt 0 -or $jsonStatusIndex + 1 -ge $bwrapOptions.Count -or
                $bwrapOptions[$jsonStatusIndex + 1] -cne '3') {
                throw "sandbox journal row $index has an invalid JSON status descriptor"
            }
            $prlimitSeparator = Find-ExactValueIndex `
                ([object[]]$argv) '--' ($bwrapSeparator + 2)
            if ($prlimitSeparator -le $bwrapSeparator + 2 -or
                $prlimitSeparator + 1 -ge $argv.Count) {
                throw "sandbox journal row $index has an invalid prlimit/payload boundary"
            }
            $payload = @($argv[($prlimitSeparator + 1)..($argv.Count - 1)])
            $launcherPath = "$stdoutLivePath.launcher-attestation"
            Assert-RegularSourceFile $launcherPath "sandbox launcher attestation row $index" | Out-Null
            $launcherLines = @(Get-Content -LiteralPath $launcherPath)
            if ($launcherLines.Count -lt 1 -or $launcherLines.Count -gt 2) {
                throw "sandbox launcher attestation row $index has an invalid line count"
            }
            $child = $launcherLines[0] | ConvertFrom-Json
            foreach ($field in @('child-pid', 'mnt-namespace', 'net-namespace', 'pid-namespace')) {
                if ([int64]$child.$field -le 0) {
                    throw "sandbox launcher attestation row $index has an invalid $field"
                }
            }
            if ($child.'mnt-namespace' -eq $child.'net-namespace' -or
                $child.'mnt-namespace' -eq $child.'pid-namespace' -or
                $child.'net-namespace' -eq $child.'pid-namespace') {
                throw "sandbox launcher attestation row $index did not record distinct namespaces"
            }
            $exitCode = [int]$fields[6]
            if ($exitCode -notin @(124, 137)) {
                if ($launcherLines.Count -ne 2 -or
                    [int](($launcherLines[1] | ConvertFrom-Json).'exit-code') -ne $exitCode) {
                    throw "sandbox launcher exit record differs at row $index"
                }
            }
            $launchers.Add([ordered]@{
                    sequence = $index
                    stage = $stage
                    retained_path = Convert-ToRepoRelative $launcherPath
                    sha256 = Get-Sha256 $launcherPath
                    child_pid = [int64]$child.'child-pid'
                    mnt_namespace = [int64]$child.'mnt-namespace'
                    net_namespace = [int64]$child.'net-namespace'
                    pid_namespace = [int64]$child.'pid-namespace'
                    unshare_net = $true
                })
            if ($stage -eq 'preflight-containment' -and
                $bwrapOptions -contains '--unshare-net' -and
                ($payload -join "`n").Contains('/proc/net/dev') -and
                ($payload -join "`n").Contains('/dev/tcp/198.51.100.1/9') -and
                $exitCode -eq 0) {
                $containmentCanary = $true
            }
        }
        $previous = $fields[11]
    }
    if ($sandboxCount -lt 1 -or -not $containmentCanary) {
        throw 'live journal does not prove the Bubblewrap network-disabled containment canary'
    }
    $record = [ordered]@{
        schema = 's115-live-harness-journal-audit-v1'
        status = 'pass'
        journal = Convert-ToRepoRelative $journalPath
        journal_sha256 = $journalSha
        wsl_repository_root = $wslRoot
        journal_cwd_contract = 'exact_repository_root'
        canonical_stream_prefix = $canonicalPrefix
        row_count = $lines.Count - 1
        referenced_output_count = $outputs.Count
        referenced_outputs_rehashed = @($outputs)
        sandbox_invocation_count = $sandboxCount
        launcher_attestations = @($launchers)
        containment_canary_stage = 'preflight-containment'
        containment_canary_exit_code = 0
        unshare_net_proven = $true
        loopback_only_and_network_connect_negative_proven = $true
    }
    Write-NewJson (Join-Path $EvidenceRoot 'live-harness-journal-audit.json') $record 40
    return [pscustomobject]$record
}

function Write-FilesManifest([string]$EvidenceRoot) {
    $root = Assert-RealDirectory $EvidenceRoot 'evidence root'
    $manifestPath = Join-Path $root 'files.sha256'
    Assert-PathAbsent $manifestPath 'evidence files manifest'
    $treePath = Join-Path ([System.IO.Path]::GetTempPath()) ("s115-harness-files-{0}.jsonl" -f [Guid]::NewGuid())
    try {
        # This temporary manifest is outside the retained tree, avoiding a
        # self-reference. It is read-only scratch and is removed by .NET after
        # the final file list has been materialized below.
        $entries = [System.Collections.Generic.SortedDictionary[string, string]]::new(
            [System.StringComparer]::Ordinal
        )
        $stack = [System.Collections.Generic.Stack[string]]::new()
        $stack.Push($root)
        while ($stack.Count -gt 0) {
            $directory = $stack.Pop()
            foreach ($child in [System.IO.Directory]::EnumerateFileSystemEntries($directory)) {
                $item = Get-Item -Force -LiteralPath $child -ErrorAction Stop
                if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
                    throw "retained evidence contains a reparse point: $($item.FullName)"
                }
                if ($item.PSIsContainer) {
                    $stack.Push($item.FullName)
                }
                else {
                    $relative = [System.IO.Path]::GetRelativePath($root, $item.FullName) -replace '\\', '/'
                    $entries.Add($relative, (Get-Sha256 $item.FullName))
                }
            }
        }
        $lines = foreach ($pair in $entries.GetEnumerator()) {
            "$($pair.Value)  $($pair.Key)"
        }
        Write-NewText $manifestPath (($lines -join "`n") + "`n")
    }
    finally {
        # $treePath is intentionally never created; no recursive or broad
        # cleanup operation is part of this control.
        $null = $treePath
    }
}

function Capture-ControlProvenance([string]$EvidenceRoot) {
    $controlRoot = New-SafeDirectory (Join-Path $EvidenceRoot 'control')
    $sources = [ordered]@{
        'qualify-harness.ps1' = Join-Path $PSScriptRoot 'qualify-harness.ps1'
        'verify-harness.ps1' = Join-Path $PSScriptRoot 'verify-harness.ps1'
        'test-harness-control.ps1' = Join-Path $PSScriptRoot 'test-harness-control.ps1'
        'README.md' = Join-Path $PSScriptRoot 'README.md'
    }
    $records = [System.Collections.Generic.List[object]]::new()
    foreach ($entry in $sources.GetEnumerator()) {
        $source = Assert-RegularSourceFile $entry.Value "control source $($entry.Key)"
        $sourceSha = Get-Sha256 $source.FullName
        $destination = Join-Path $controlRoot $entry.Key
        Assert-PathAbsent $destination 'retained control source'
        [System.IO.File]::Copy($source.FullName, $destination, $false)
        if ((Get-Sha256 $destination) -ne $sourceSha -or
            (Get-Item -LiteralPath $destination).Length -ne $source.Length -or
            (Get-Sha256 $source.FullName) -ne $sourceSha) {
            throw "retained control source differs during capture: $($entry.Key)"
        }
        $records.Add([ordered]@{
                name = $entry.Key
                source = Convert-ToRepoRelative $source.FullName
                retained = "control/$($entry.Key)"
                bytes = [int64]$source.Length
                sha256 = $sourceSha
            })
    }
    $record = [ordered]@{
        schema = 's115-harness-control-provenance-v1'
        files = @($records)
    }
    Write-NewJson (Join-Path $EvidenceRoot 'control-provenance.json') $record 10
    return [pscustomobject]$record
}

function Assert-ControlSourcesUnchanged([object]$ControlProvenance) {
    foreach ($record in @($ControlProvenance.files)) {
        $source = Join-Path $repoRoot ([string]$record.source -replace '/', '\')
        Assert-RegularSourceFile $source "live control source $($record.name)" | Out-Null
        if ((Get-Sha256 $source) -ne [string]$record.sha256 -or
            (Get-Item -LiteralPath $source).Length -ne [int64]$record.bytes) {
            throw "control source changed during qualification: $($record.name)"
        }
    }
}

function Write-TerminalFailureRecord {
    param(
        [Parameter(Mandatory)][string]$AttemptRoot,
        [Parameter(Mandatory)][string]$AttemptName,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][string]$Message,
        [Parameter(Mandatory)][System.Management.Automation.ErrorRecord]$ErrorRecord,
        [Parameter(Mandatory)][bool]$CompactEvidencePublished,
        [Parameter(Mandatory)][string]$CompactEvidencePath
    )
    $recordPath = Join-Path $AttemptRoot 'terminal-failure.json'
    $hashPath = Join-Path $AttemptRoot 'terminal-failure.sha256'
    $record = [ordered]@{
        schema = 's115-harness-terminal-failure-v1'
        attempt = $AttemptName
        status = 'terminal_failure'
        phase = $Phase
        captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        message = $Message
        diagnostic = ConvertTo-FailureDiagnostic -ErrorRecord $ErrorRecord `
            -Scope 'terminal' -Context $Phase
        compact_evidence_published = $CompactEvidencePublished
        compact_evidence = $CompactEvidencePath
        rollback_attempted = $false
        raw_quarantine_preserved = $true
    }
    Write-NewJson $recordPath $record 8
    Write-NewText $hashPath ((Get-Sha256 $recordPath) + "  terminal-failure.json`n")
}

function ConvertTo-FailureDiagnostic {
    param(
        [Parameter(Mandatory)][System.Management.Automation.ErrorRecord]$ErrorRecord,
        [Parameter(Mandatory)][string]$Scope,
        [Parameter(Mandatory)][string]$Context
    )
    return [pscustomobject][ordered]@{
        scope = $Scope
        context = $Context
        message = [string]$ErrorRecord.Exception.Message
        exception_type = $ErrorRecord.Exception.GetType().FullName
        inner_exception_type = if ($null -eq $ErrorRecord.Exception.InnerException) {
            $null
        }
        else {
            $ErrorRecord.Exception.InnerException.GetType().FullName
        }
        inner_exception_message = if ($null -eq $ErrorRecord.Exception.InnerException) {
            $null
        }
        else {
            [string]$ErrorRecord.Exception.InnerException.Message
        }
        error_record_type = $ErrorRecord.GetType().FullName
        fully_qualified_error_id = [string]$ErrorRecord.FullyQualifiedErrorId
        category = [string]$ErrorRecord.CategoryInfo.Category
        target_object_type = if ($null -eq $ErrorRecord.TargetObject) {
            $null
        }
        else {
            $ErrorRecord.TargetObject.GetType().FullName
        }
        invocation_name = [string]$ErrorRecord.InvocationInfo.InvocationName
        position_message = [string]$ErrorRecord.InvocationInfo.PositionMessage
        script_stack_trace = [string]$ErrorRecord.ScriptStackTrace
    }
}

if ($ControlSelfTest) {
    Invoke-CommandRecordSemanticSelfTest | ConvertTo-Json -Depth 5
    return
}

Assert-RealDirectory $repoRoot 'repository root' | Out-Null
Assert-RealDirectory $sourceHarnessRoot 'source frozen harness' | Out-Null
Assert-NoReparseAncestors $preservationRoot
Assert-NoReparseAncestors $retainedAttemptsRoot
New-SafeDirectory $preservationRoot | Out-Null
$runLockPath = Join-Path $preservationRoot 'qualification.lock'
$runLock = $null
try {
    $runLock = [System.IO.FileStream]::new(
        $runLockPath,
        [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
}
catch [System.IO.IOException] {
    throw 'another T-11502 qualifier owns the global preservation lock; no attempt was allocated'
}

try {
$attempt = Get-NextAttempt
$attemptRoot = $attempt.target_root
$evidenceRoot = New-SafeDirectory (Join-Path $attemptRoot 'evidence')
$logsRoot = New-SafeDirectory (Join-Path $evidenceRoot 'logs')
$journalPath = Join-Path $evidenceRoot 'journal.jsonl'
Write-NewText $journalPath ''
$controlProvenance = Capture-ControlProvenance $evidenceRoot
$volumeBefore = Get-VolumeObservation
$transientHarnessRoot = Join-Path $attemptRoot 'frozen\app-harness'
$transientScriptsRoot = Join-Path $transientHarnessRoot 'scripts'
$preBatchKey = '001-pre-selftest'
$copyBatchKey = '002-frozen-copy'
$postBatchKey = '003-post-selftest'
$failures = [System.Collections.Generic.List[string]]::new()
$primaryFailures = [System.Collections.Generic.List[string]]::new()
$preservationFailures = [System.Collections.Generic.List[string]]::new()
$primaryFailureDetails = [System.Collections.Generic.List[object]]::new()
$preservationFailureDetails = [System.Collections.Generic.List[object]]::new()
$preOperations = [System.Collections.Generic.List[object]]::new()
$postOperations = [System.Collections.Generic.List[object]]::new()
$copyOperation = $null
$frozenCopy = $null
$frozenCopyFinal = $null
$selfTest = $null
$selfTestEvidence = $null
$standaloneGit = $null
$sourceBefore = $null
$sourceAfter = $null
$gitBefore = $null
$gitAfter = $null
$s114StatusBefore = $null
$s114StatusAfter = $null
$headBefore = $null
$headAfter = $null
$treeBefore = $null
$treeAfter = $null
$knownUserEditShaBefore = $null
$knownUserEditShaAfter = $null
$liveJournalCrossLink = $null
$liveJournalAudit = $null
$wslRepositoryRoot = $null

try {
    Assert-RegularSourceFile $knownUserEditPath 'known unrelated user edit' | Out-Null
    $knownUserEditShaBefore = Get-Sha256 $knownUserEditPath
    $headBefore = Invoke-CapturedCommand -Gate 'repository-head-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD') -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $logsRoot -JournalPath $journalPath
    $treeBefore = Invoke-CapturedCommand -Gate 'repository-tree-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD^{tree}') -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedWorktreeStatusBefore = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-status-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=no') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedWorktreeDiffBefore = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-diff-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'diff', '--no-ext-diff', '--binary', '--') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedWorktreeCachedDiffBefore = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-cached-diff-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'diff', '--cached', '--no-ext-diff', '--binary', '--') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedBaselineRecords = @(
        $trackedWorktreeStatusBefore,
        $trackedWorktreeDiffBefore,
        $trackedWorktreeCachedDiffBefore
    )
    Assert-CapturedCommandRecordCollection -Records $trackedBaselineRecords `
        -ExpectedCount 3 -Label 'tracked-worktree Git baseline'
    foreach ($trackedBaselineGate in $trackedBaselineRecords) {
        if ($trackedBaselineGate.timed_out -or $trackedBaselineGate.exit_code -ne 0) {
            throw 'could not capture the complete tracked-worktree Git baseline'
        }
    }
    $s114StatusBefore = Invoke-CapturedCommand -Gate 's114-status-before' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', $s114Display) `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $s114StatusText = (Get-Content -Raw -LiteralPath (Join-Path $logsRoot 's114-status-before.stdout.txt')).TrimEnd("`r", "`n")
    if ($headBefore.timed_out -or $headBefore.exit_code -ne 0 -or
        $treeBefore.timed_out -or $treeBefore.exit_code -ne 0 -or
        $s114StatusBefore.timed_out -or $s114StatusBefore.exit_code -ne 0 -or
        $s114StatusText -cne " M $knownUserEditDisplay") {
        throw 'Sprint 114 Git state differs from the single known unrelated user edit'
    }
    $gitBefore = Invoke-CapturedCommand -Gate 'tracked-harness-status-before' -FilePath 'git.exe' `
        -Arguments @(
            '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
            $sourceHarnessDisplay
        ) -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    if ($gitBefore.timed_out -or $gitBefore.exit_code -ne 0 -or
        -not [string]::IsNullOrWhiteSpace(
            (Get-Content -Raw -LiteralPath (Join-Path $logsRoot 'tracked-harness-status-before.stdout.txt'))
        )) {
        throw 'tracked Sprint 114 harness is not clean before qualification'
    }
    $sourceBefore = Write-TreeEntriesManifest $sourceHarnessRoot `
        (Join-Path $evidenceRoot 'tracked-harness.before.entries.jsonl')

    $preBatchRoots = New-SafeDirectory (Join-Path $attemptRoot "batches\$preBatchKey\roots")
    foreach ($entry in $canonicalRoots.GetEnumerator()) {
        $preOperations.Add((Invoke-ManifestedMove -Source $entry.Value `
                -Destination (Join-Path $preBatchRoots $entry.Key) -RootKey $entry.Key `
                -BatchKey $preBatchKey -EvidenceRoot $evidenceRoot))
    }
    foreach ($entry in $canonicalRoots.GetEnumerator()) {
        Assert-PathAbsent $entry.Value "canonical root $($entry.Key) before self-test"
    }

    $frozenCopy = Materialize-FrozenHarness -DestinationHarness $transientHarnessRoot `
        -EvidenceRoot $evidenceRoot
    $derivedRoot = Get-FullPath (Join-Path $transientScriptsRoot '..\..\..\..\..\..')
    if (-not [string]::Equals(
            $derivedRoot,
            $repoRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw 'the depth-preserving harness copy does not derive the real repository root'
    }
    $relativeScripts = Convert-ToRepoRelative $transientScriptsRoot
    $depthScript = 'set -Eeuo pipefail; scripts=$1; actual=$(cd -- "$scripts/../../../../../.." && pwd -P); expected=$(pwd -P); test "$actual" = "$expected"; printf "%s\n" "$actual"'
    $depthProbe = Invoke-CapturedCommand -Gate 'wsl-depth-probe' -FilePath 'wsl.exe' `
        -Arguments @('--exec', 'bash', '-c', $depthScript, '_', $relativeScripts) `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    if ($depthProbe.timed_out -or $depthProbe.exit_code -ne 0) {
        throw 'WSL did not resolve the copied script depth to the real repository root'
    }
    $wslDepthStdout = Get-Content -Raw -LiteralPath `
        (Join-Path $logsRoot 'wsl-depth-probe.stdout.txt')
    $wslRepositoryRoot = $wslDepthStdout.TrimEnd("`r", "`n")
    Assert-WslRepositoryPath $wslRepositoryRoot 'WSL depth-probe repository root' | Out-Null
    if ($wslDepthStdout -cne "$wslRepositoryRoot`n") {
        throw 'WSL depth probe did not emit exactly one repository-root line'
    }

    $selfTestRelative = Convert-ToRepoRelative (Join-Path $transientScriptsRoot 'self-test.sh')
    $selfTest = Invoke-CapturedCommand -Gate 'frozen-harness-self-test' -FilePath 'wsl.exe' `
        -Arguments @('--exec', 'bash', $selfTestRelative) -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:SelfTestTimeoutSeconds -LogsRoot $logsRoot -JournalPath $journalPath
    if ($selfTest.timed_out -or $selfTest.exit_code -ne 0) {
        throw "frozen harness self-test failed or timed out (exit $($selfTest.exit_code))"
    }

    $frozenCopyFinal = Assert-FrozenCopyInputsStillMatch `
        -CopiedHarness $transientHarnessRoot -EvidenceRoot $evidenceRoot
    $generatedEvidence = Join-Path $transientHarnessRoot 'evidence'
    $selfTestEvidence = Assert-SelfTestSummary $generatedEvidence
    $retainedSelfTestEvidence = Join-Path $evidenceRoot 'generated-self-test-evidence'
    Copy-TreeWithoutReparse $generatedEvidence $retainedSelfTestEvidence
    $generatedManifest = Write-TreeEntriesManifest $generatedEvidence `
        (Join-Path $evidenceRoot 'generated-self-test-evidence.source.entries.jsonl')
    $retainedManifest = Write-TreeEntriesManifest $retainedSelfTestEvidence `
        (Join-Path $evidenceRoot 'generated-self-test-evidence.retained.entries.jsonl')
    if ($generatedManifest.entry_count -ne $retainedManifest.entry_count -or
        -not (Test-FilesByteEqual `
            (Join-Path $evidenceRoot 'generated-self-test-evidence.source.entries.jsonl') `
            (Join-Path $evidenceRoot 'generated-self-test-evidence.retained.entries.jsonl'))) {
        throw 'retained generated self-test evidence differs from the transient source'
    }

    $sourceAfter = Write-TreeEntriesManifest $sourceHarnessRoot `
        (Join-Path $evidenceRoot 'tracked-harness.after.entries.jsonl')
    if ($sourceBefore.entry_count -ne $sourceAfter.entry_count -or
        -not (Test-FilesByteEqual `
            (Join-Path $evidenceRoot 'tracked-harness.before.entries.jsonl') `
            (Join-Path $evidenceRoot 'tracked-harness.after.entries.jsonl'))) {
        throw 'tracked Sprint 114 harness bytes changed during the copied self-test'
    }
    $gitAfter = Invoke-CapturedCommand -Gate 'tracked-harness-status-after' -FilePath 'git.exe' `
        -Arguments @(
            '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
            $sourceHarnessDisplay
        ) -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    if ($gitAfter.timed_out -or $gitAfter.exit_code -ne 0 -or
        (Get-Sha256 (Join-Path $logsRoot 'tracked-harness-status-before.stdout.txt')) -ne
        (Get-Sha256 (Join-Path $logsRoot 'tracked-harness-status-after.stdout.txt'))) {
        throw 'tracked Sprint 114 harness Git state changed during qualification'
    }
    $knownUserEditShaAfter = Get-Sha256 $knownUserEditPath
    $headAfter = Invoke-CapturedCommand -Gate 'repository-head-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD') -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $logsRoot -JournalPath $journalPath
    $treeAfter = Invoke-CapturedCommand -Gate 'repository-tree-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD^{tree}') -WorkingDirectory $repoRoot `
        -TimeoutSeconds $script:CommandTimeoutSeconds -LogsRoot $logsRoot -JournalPath $journalPath
    $s114StatusAfter = Invoke-CapturedCommand -Gate 's114-status-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', $s114Display) `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    if ($headAfter.timed_out -or $headAfter.exit_code -ne 0 -or
        $treeAfter.timed_out -or $treeAfter.exit_code -ne 0 -or
        $s114StatusAfter.timed_out -or $s114StatusAfter.exit_code -ne 0 -or
        $knownUserEditShaBefore -ne $knownUserEditShaAfter -or
        (Get-Sha256 (Join-Path $logsRoot 'repository-head-before.stdout.txt')) -ne
        (Get-Sha256 (Join-Path $logsRoot 'repository-head-after.stdout.txt')) -or
        (Get-Sha256 (Join-Path $logsRoot 'repository-tree-before.stdout.txt')) -ne
        (Get-Sha256 (Join-Path $logsRoot 'repository-tree-after.stdout.txt')) -or
        (Get-Sha256 (Join-Path $logsRoot 's114-status-before.stdout.txt')) -ne
        (Get-Sha256 (Join-Path $logsRoot 's114-status-after.stdout.txt'))) {
        throw 'repository HEAD, Sprint 114 status, or known user edit changed during qualification'
    }

    $standaloneGit = Invoke-StandaloneGitProbe `
        -CandidateRoot $canonicalRoots['app-workspace'] `
        -MetadataRoot $canonicalRoots['launcher-attestation-probe'] `
        -LogsRoot $logsRoot -JournalPath $journalPath -EvidenceRoot $evidenceRoot
    Assert-ControlSourcesUnchanged $controlProvenance
    $trackedWorktreeStatusAfter = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-status-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=no') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedWorktreeDiffAfter = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-diff-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'diff', '--no-ext-diff', '--binary', '--') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    $trackedWorktreeCachedDiffAfter = Invoke-CapturedCommand `
        -Gate 'tracked-worktree-cached-diff-after' -FilePath 'git.exe' `
        -Arguments @('-C', $repoRoot, 'diff', '--cached', '--no-ext-diff', '--binary', '--') `
        -WorkingDirectory $repoRoot -TimeoutSeconds $script:CommandTimeoutSeconds `
        -LogsRoot $logsRoot -JournalPath $journalPath
    Assert-CapturedCommandRecordCollection -Records @(
        $trackedWorktreeStatusAfter,
        $trackedWorktreeDiffAfter,
        $trackedWorktreeCachedDiffAfter
    ) -ExpectedCount 3 -Label 'tracked-worktree Git final state'
    Assert-CapturedCommandParity -Before $trackedWorktreeStatusBefore `
        -After $trackedWorktreeStatusAfter -Label 'tracked-worktree status' | Out-Null
    Assert-CapturedCommandParity -Before $trackedWorktreeDiffBefore `
        -After $trackedWorktreeDiffAfter -Label 'tracked-worktree unstaged diff' | Out-Null
    Assert-CapturedCommandParity -Before $trackedWorktreeCachedDiffBefore `
        -After $trackedWorktreeCachedDiffAfter -Label 'tracked-worktree cached diff' | Out-Null
}
catch {
    $primaryFailures.Add($_.Exception.Message)
    $primaryFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                -Scope 'primary' -Context 'qualification body')) | Out-Null
}
finally {

# The depth-preserving copy lives inside the retained raw attempt from its
# first byte. Capture its final tree, but do not relocate it or create an
# orphan control root elsewhere under target/.
try {
    $copyManifestRoot = New-SafeDirectory (Join-Path $evidenceRoot "batches\$copyBatchKey")
    if (Test-EntryExists $transientHarnessRoot) {
        $copyFinalManifestPath = Join-Path $copyManifestRoot 'depth-preserving-frozen-copy.final.entries.jsonl'
        $copyFinal = Write-TreeEntriesManifest $transientHarnessRoot $copyFinalManifestPath
        $copyOperation = [pscustomobject][ordered]@{
            schema = 's115-preserved-root-v1'
            batch = $copyBatchKey
            root_key = 'depth-preserving-frozen-copy'
            present = $true
            source = Convert-ToRepoRelative $transientHarnessRoot
            destination = Convert-ToRepoRelative $transientHarnessRoot
            retained_in_place = $true
            same_volume = $true
            final_manifest = ([System.IO.Path]::GetRelativePath($evidenceRoot, $copyFinalManifestPath) -replace '\\', '/')
            entry_count = $copyFinal.entry_count
            entries_sha256 = $copyFinal.sha256
            regular_file_count = $copyFinal.regular_file_count
            regular_file_bytes = $copyFinal.regular_file_bytes
            reparse_count = $copyFinal.reparse_count
            parity = $true
        }
    }
    else {
        $copyOperation = [pscustomobject][ordered]@{
            schema = 's115-preserved-root-v1'
            batch = $copyBatchKey
            root_key = 'depth-preserving-frozen-copy'
            present = $false
            source = Convert-ToRepoRelative $transientHarnessRoot
            destination = Convert-ToRepoRelative $transientHarnessRoot
            retained_in_place = $true
            parity = $true
        }
    }
    Write-NewJson (Join-Path $copyManifestRoot 'depth-preserving-frozen-copy.record.json') $copyOperation
}
catch {
    $preservationFailures.Add("copied harness preservation failed: $($_.Exception.Message)")
    $preservationFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                -Scope 'preservation' -Context 'copied harness preservation')) | Out-Null
}

try {
    $postBatchRoots = New-SafeDirectory (Join-Path $attemptRoot "batches\$postBatchKey\roots")
    foreach ($entry in $canonicalRoots.GetEnumerator()) {
        try {
            $postOperations.Add((Invoke-ManifestedMove -Source $entry.Value `
                    -Destination (Join-Path $postBatchRoots $entry.Key) -RootKey $entry.Key `
                    -BatchKey $postBatchKey -EvidenceRoot $evidenceRoot))
        }
        catch {
            $preservationFailures.Add("post-selftest preservation failed for $($entry.Key): $($_.Exception.Message)")
            $preservationFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                        -Scope 'preservation' `
                        -Context "post-selftest preservation for $($entry.Key)")) | Out-Null
        }
    }
    foreach ($entry in $canonicalRoots.GetEnumerator()) {
        try {
            Assert-PathAbsent $entry.Value "canonical root $($entry.Key) at handoff"
        }
        catch {
            $preservationFailures.Add($_.Exception.Message)
            $preservationFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                        -Scope 'preservation' `
                        -Context "canonical-root absence for $($entry.Key)")) | Out-Null
        }
    }
}
catch {
    $preservationFailures.Add("post-selftest batch setup failed: $($_.Exception.Message)")
    $preservationFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                -Scope 'preservation' -Context 'post-selftest batch setup')) | Out-Null
}

if ($primaryFailures.Count -eq 0 -and $preservationFailures.Count -eq 0) {
    try {
        $postHarnessRoot = Join-Path $postBatchRoots 'app-harness'
        $liveJournalPath = Join-Path $postHarnessRoot 'command-journal.tsv'
        $liveJournalCompanionPath = "$liveJournalPath.sha256"
        Assert-RegularSourceFile $liveJournalPath 'post-quarantined live harness journal' | Out-Null
        Assert-RegularSourceFile $liveJournalCompanionPath 'post-quarantined live journal companion' | Out-Null
        $liveJournalSha = Get-Sha256 $liveJournalPath
        $liveCompanionText = (Get-Content -Raw -LiteralPath $liveJournalCompanionPath).Trim()
        if ($liveJournalSha -ne [string]$selfTestEvidence.journal_sha256 -or
            $liveCompanionText -notmatch ('^' + [regex]::Escape($liveJournalSha) + '(?:\s|$)')) {
            throw 'generated summary journal does not match the post-quarantined live journal and companion'
        }
        $liveJournalAudit = Invoke-LiveHarnessJournalAudit `
            -LiveHarnessRoot $postHarnessRoot `
            -ExpectedJournalSha256 ([string]$selfTestEvidence.journal_sha256) `
            -EvidenceRoot $evidenceRoot -WslRepositoryRoot $wslRepositoryRoot
        $postHarnessOperation = @($postOperations | Where-Object { $_.root_key -eq 'app-harness' })
        if ($postHarnessOperation.Count -ne 1 -or -not $postHarnessOperation[0].present) {
            throw 'post-selftest app-harness preservation record is missing'
        }
        $postHarnessManifest = Join-Path $evidenceRoot `
            ([string]$postHarnessOperation[0].after_manifest -replace '/', '\')
        $liveLogCount = @(
            Get-Content -LiteralPath $postHarnessManifest |
                ForEach-Object { $_ | ConvertFrom-Json } |
                Where-Object {
                    $_.type -eq 'regular_file' -and
                    $_.relative_path -match '(^|/)logs/'
                }
        ).Count
        if ($liveLogCount -lt 1) {
            throw 'post-quarantined app-harness manifest contains no retained command logs'
        }
        $liveJournalCrossLink = [ordered]@{
            schema = 's115-selftest-live-journal-link-v1'
            summary_journal_sha256 = [string]$selfTestEvidence.journal_sha256
            copied_snapshot_sha256 = [string]$selfTestEvidence.journal_sha256
            live_journal = Convert-ToRepoRelative $liveJournalPath
            live_journal_sha256 = $liveJournalSha
            live_journal_companion = Convert-ToRepoRelative $liveJournalCompanionPath
            live_journal_companion_sha256 = Get-Sha256 $liveJournalCompanionPath
            post_quarantine_manifest = [string]$postHarnessOperation[0].after_manifest
            retained_log_file_count = $liveLogCount
            journal_row_count = [int]$liveJournalAudit.row_count
            referenced_output_count = [int]$liveJournalAudit.referenced_output_count
            sandbox_invocation_count = [int]$liveJournalAudit.sandbox_invocation_count
            unshare_net_proven = [bool]$liveJournalAudit.unshare_net_proven
            status = 'pass'
        }
        Write-NewJson (Join-Path $evidenceRoot 'selftest-live-journal-link.json') $liveJournalCrossLink
    }
    catch {
        $preservationFailures.Add("self-test live-journal cross-link failed: $($_.Exception.Message)")
        $preservationFailureDetails.Add((ConvertTo-FailureDiagnostic -ErrorRecord $_ `
                    -Scope 'preservation' -Context 'self-test live-journal cross-link')) | Out-Null
    }
}
}

foreach ($failure in $primaryFailures) {
    $failures.Add("primary: $failure")
}
foreach ($failure in $preservationFailures) {
    $failures.Add("preservation: $failure")
}

if ($failures.Count -gt 0) {
    $trackedMutationState = 'unknown'
    $trackedMutationValue = $null
    if ($null -ne $sourceBefore -and $null -ne $sourceAfter) {
        $trackedMutationValue = $sourceBefore.sha256 -ne $sourceAfter.sha256
        $trackedMutationState = if ($trackedMutationValue) { 'true' } else { 'false' }
    }
    $failureRecord = [ordered]@{
        schema = 's115-harness-qualification-failure-v1'
        attempt = $attempt.name
        status = 'infrastructure_failure'
        captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
        primary_failures = @($primaryFailures)
        preservation_failures = @($preservationFailures)
        primary_failure_details = @($primaryFailureDetails)
        preservation_failure_details = @($preservationFailureDetails)
        failures = @($failures)
        tracked_s114_harness_mutation_state = $trackedMutationState
        tracked_s114_harness_mutated = $trackedMutationValue
        canonical_roots_absent = [ordered]@{}
        partial_evidence = Convert-ToRepoRelative $evidenceRoot
    }
    foreach ($entry in $canonicalRoots.GetEnumerator()) {
        $failureRecord.canonical_roots_absent[$entry.Key] = -not (Test-EntryExists $entry.Value)
    }
    try {
        Write-NewJson (Join-Path $evidenceRoot 'failure.json') $failureRecord
        Write-NewText (Join-Path $evidenceRoot 'failure.sha256') `
            ((Get-Sha256 (Join-Path $evidenceRoot 'failure.json')) + "  failure.json`n")
    }
    catch {
        # The original failure remains primary. The attempt directory and raw
        # moved roots are still retained even if compact failure capture fails.
    }
    throw ("T-11502 qualification failed; partial attempt {0} is retained: {1}" -f `
        $attempt.name, ($failures -join '; '))
}

$result = [ordered]@{
    schema = 's115-harness-qualification-v1'
    task = 'T-11502'
    attempt = $attempt.name
    status = 'pass'
    completed_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    frozen_harness = [ordered]@{
        source = $sourceHarnessDisplay
        transient_copy = Convert-ToRepoRelative $transientHarnessRoot
        retained_copy = [string]$copyOperation.destination
        depth_components_to_repo = 6
        host_depth_probe = 'pass'
        wsl_depth_probe = 'pass'
        wsl_repository_root = $wslRepositoryRoot
        frozen_manifest_sha256 = $script:PinnedFrozenManifestSha256
        frozen_file_count = 30
        frozen_seed_file_count = 5
        frozen_seed_hash_parity = $true
        copied_inputs_unchanged_after_selftest = [bool]$frozenCopyFinal.frozen_inputs_unchanged
        copied_final_inputs_entries_sha256 = [string]$frozenCopyFinal.final_entries_sha256
        tracked_source_before_entries_sha256 = $sourceBefore.sha256
        tracked_source_after_entries_sha256 = $sourceAfter.sha256
        tracked_source_byte_identical = $true
        tracked_git_status_unchanged = $true
        complete_tracked_worktree_git_effect_unchanged = $true
        tracked_worktree_status_sha256 = [string]$trackedWorktreeStatusBefore.stdout_sha256
        tracked_worktree_diff_sha256 = [string]$trackedWorktreeDiffBefore.stdout_sha256
        tracked_worktree_cached_diff_sha256 = [string]$trackedWorktreeCachedDiffBefore.stdout_sha256
        repository_head_unchanged = $true
        repository_commit = (Get-Content -Raw -LiteralPath (Join-Path $logsRoot 'repository-head-before.stdout.txt')).Trim()
        repository_tree = (Get-Content -Raw -LiteralPath (Join-Path $logsRoot 'repository-tree-before.stdout.txt')).Trim()
        sprint_114_status_unchanged = $true
        known_unrelated_edit = $knownUserEditDisplay
        known_unrelated_edit_sha256_before = $knownUserEditShaBefore
        known_unrelated_edit_sha256_after = $knownUserEditShaAfter
    }
    control_provenance = $controlProvenance
    self_test = [ordered]@{
        status = 'pass'
        timeout_seconds = $script:SelfTestTimeoutSeconds
        timed_out = [bool]$selfTest.timed_out
        exit_code = [int]$selfTest.exit_code
        invocation = @('wsl.exe', '--exec', 'bash', (Convert-ToRepoRelative (Join-Path $transientScriptsRoot 'self-test.sh')))
        copied_script_sha256 = Get-Sha256 (Join-Path $sourceHarnessRoot 'scripts\self-test.sh')
        summary_sha256 = $selfTestEvidence.summary_sha256
        journal_sha256 = $selfTestEvidence.journal_sha256
        bubblewrap_network_disabled_canaries = 'pass'
        generated_evidence = 'generated-self-test-evidence'
        live_journal_cross_link = $liveJournalCrossLink
        live_journal_audit = 'live-harness-journal-audit.json'
    }
    standalone_git = $standaloneGit
    preservation = [ordered]@{
        target_attempt = Convert-ToRepoRelative $attemptRoot
        pre_batch = $preBatchKey
        frozen_copy_batch = $copyBatchKey
        post_batch = $postBatchKey
        pre_operations = @($preOperations)
        copy_operation = $copyOperation
        post_operations = @($postOperations)
        recursive_delete_used = $false
        all_moves_same_volume = $true
        all_present_move_manifests_byte_identical = $true
        pre_regular_file_bytes = [int64](
            @($preOperations | Where-Object { $_.present } |
                Measure-Object -Property regular_file_bytes -Sum).Sum
        )
        post_regular_file_bytes = [int64](
            @($postOperations | Where-Object { $_.present } |
                Measure-Object -Property regular_file_bytes -Sum).Sum
        )
        retained_frozen_copy_regular_file_bytes = [int64]$copyOperation.regular_file_bytes
        volume_before = $volumeBefore
        volume_after = Get-VolumeObservation
    }
    handoff = [ordered]@{
        canonical_roots_absent = [ordered]@{}
        ready_for_t11503 = $true
    }
}
foreach ($entry in $canonicalRoots.GetEnumerator()) {
    $result.handoff.canonical_roots_absent[$entry.Key] = $true
}

$resultPath = Join-Path $evidenceRoot 'result.json'
$publicationPhase = 'result-sealing'
$compactPublished = $false
$compactPath = Convert-ToRepoRelative $evidenceRoot
try {
    Write-NewJson $resultPath $result 40
    Write-NewText (Join-Path $evidenceRoot 'result.sha256') `
        ((Get-Sha256 $resultPath) + "  result.json`n")
    Write-FilesManifest $evidenceRoot

    $verifierPath = Join-Path $evidenceRoot 'control\verify-harness.ps1'
    $publicationPhase = 'pre-publication-verification'
    & pwsh -NoLogo -NoProfile -File $verifierPath -EvidenceRoot $evidenceRoot `
        -CheckQuarantine -RepositoryRoot $repoRoot
    if ($LASTEXITCODE -ne 0) {
        throw "pre-publication verifier rejected attempt $($attempt.name)"
    }
    $publicationPhase = 'compact-evidence-publication'
    Assert-PathAbsent $attempt.retained_root 'retained evidence attempt'
    New-SafeDirectory ([System.IO.Path]::GetDirectoryName($attempt.retained_root)) | Out-Null
    Move-Item -LiteralPath $evidenceRoot -Destination $attempt.retained_root -ErrorAction Stop
    Assert-PathAbsent $evidenceRoot 'transient evidence after publication'
    $compactPublished = $true
    $compactPath = Convert-ToRepoRelative $attempt.retained_root
    $publicationPhase = 'post-publication-verification'
    $publishedVerifierPath = Join-Path $attempt.retained_root 'control\verify-harness.ps1'
    & pwsh -NoLogo -NoProfile -File $publishedVerifierPath `
        -EvidenceRoot $attempt.retained_root -CheckQuarantine -RepositoryRoot $repoRoot
    if ($LASTEXITCODE -ne 0) {
        throw "published verifier rejected immutable attempt $($attempt.name)"
    }
}
catch {
    Write-TerminalFailureRecord -AttemptRoot $attemptRoot -AttemptName $attempt.name `
        -Phase $publicationPhase -Message $_.Exception.Message -ErrorRecord $_ `
        -CompactEvidencePublished $compactPublished -CompactEvidencePath $compactPath
    throw ("T-11502 terminal {0} failure; attempt {1} remains retained without rollback: {2}" -f `
        $publicationPhase, $attempt.name, $_.Exception.Message)
}

Write-Output $attempt.retained_root
}
finally {
    if ($null -ne $runLock) {
        $runLock.Dispose()
    }
}
