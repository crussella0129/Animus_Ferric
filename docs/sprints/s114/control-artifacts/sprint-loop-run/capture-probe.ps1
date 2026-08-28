[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$PinnedSourceCommit = '4acc1fd6e0b964ea4bcbedd17c44cb2ca8ca0066'
$PinnedSourceTree = '3420c3d9858b6d3049b81f2334ca21a9d1fdaade'
$PinnedSourceRemote = 'https://github.com/crussella0129/Animus_Sprint_Loops.git'
$PinnedFerricSha256 = 'af75612b3498a1721e5b5f1b2f6309bf851d65b9bd13ad45e76cf8e370cf10f2'
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$sourceRoot = Join-Path $repoRoot 'target\s114-experiment\sprint-loop-source'
$adapterRoot = Join-Path $sourceRoot 'open-harnesses'
$workspaceRoot = Join-Path $repoRoot 'target\s114-experiment\sprint-loop-workspace'
$artifactRoot = (Resolve-Path -LiteralPath $PSScriptRoot).Path
$finalEvidenceRoot = Join-Path $artifactRoot 'evidence'
$evidenceRoot = Join-Path $artifactRoot ('.evidence-capture-' + [Guid]::NewGuid().ToString('N'))

if ((Get-Item -Force -LiteralPath $artifactRoot).Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    )) {
    throw "probe artifact root must not be a reparse point: $artifactRoot"
}
if (Test-Path -LiteralPath $finalEvidenceRoot) {
    throw "probe evidence already exists: $finalEvidenceRoot"
}
New-Item -ItemType Directory -Path $evidenceRoot | Out-Null
trap {
    if (-not [string]::IsNullOrWhiteSpace($evidenceRoot) -and
        (Test-Path -LiteralPath $evidenceRoot)) {
        Remove-Item -Recurse -Force -LiteralPath $evidenceRoot
    }
    throw $_
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Write-Utf8([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, $utf8NoBom)
}

function Write-Json([string]$Path, [object]$Value, [int]$Depth = 10) {
    $json = $Value | ConvertTo-Json -Depth $Depth
    Write-Utf8 $Path ($json + "`n")
}

function Invoke-Captured(
    [string]$Name,
    [string]$FilePath,
    [string[]]$ArgumentList,
    [string]$WorkingDirectory = $repoRoot,
    [int]$TimeoutMilliseconds = 120000
) {
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $ArgumentList) {
        $start.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) {
        throw "failed to start $FilePath for $Name"
    }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($TimeoutMilliseconds)) {
        $process.Kill($true)
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        Write-Utf8 (Join-Path $evidenceRoot "$Name.stdout.txt") $stdout
        Write-Utf8 (Join-Path $evidenceRoot "$Name.stderr.txt") $stderr
        throw "command timed out after $TimeoutMilliseconds ms: $Name"
    }
    $process.WaitForExit()
    $stdout = $stdoutTask.GetAwaiter().GetResult()
    $stderr = $stderrTask.GetAwaiter().GetResult()
    $stdoutPath = Join-Path $evidenceRoot "$Name.stdout.txt"
    $stderrPath = Join-Path $evidenceRoot "$Name.stderr.txt"
    Write-Utf8 $stdoutPath $stdout
    Write-Utf8 $stderrPath $stderr
    [pscustomobject]@{
        name = $Name
        executable = $FilePath
        arguments = @($ArgumentList)
        working_directory = $WorkingDirectory
        exit_code = $process.ExitCode
        timeout_milliseconds = $TimeoutMilliseconds
        timed_out = $false
        stdout = $stdout
        stderr = $stderr
        stdout_file = "evidence/$Name.stdout.txt"
        stderr_file = "evidence/$Name.stderr.txt"
        stdout_sha256 = Get-Sha256 $stdoutPath
        stderr_sha256 = Get-Sha256 $stderrPath
    }
}

function Assert-NoReparseTree([string]$Root) {
    foreach ($item in @(
        Get-Item -Force -LiteralPath $Root
        Get-ChildItem -Force -Recurse -LiteralPath $Root
    )) {
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "probe input must not contain a reparse point: $($item.FullName)"
        }
    }
}

function Assert-NoReparseAncestors([string]$Path, [string]$Boundary) {
    $fullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    $fullBoundary = [System.IO.Path]::GetFullPath($Boundary).TrimEnd('\')
    if (-not [string]::Equals($fullPath, $fullBoundary, [System.StringComparison]::OrdinalIgnoreCase) -and
        -not $fullPath.StartsWith($fullBoundary + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "path escapes its reparse-check boundary: $fullPath"
    }
    $cursor = Get-Item -Force -LiteralPath $fullPath
    while ($null -ne $cursor) {
        if ($cursor.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "probe path ancestor must not be a reparse point: $($cursor.FullName)"
        }
        if ([string]::Equals(
                $cursor.FullName.TrimEnd('\'),
                $fullBoundary,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            return
        }
        $parentPath = Split-Path -Parent $cursor.FullName
        if ([string]::IsNullOrWhiteSpace($parentPath)) {
            break
        }
        $cursor = Get-Item -Force -LiteralPath $parentPath
    }
    throw "reparse-check boundary was not reached for: $fullPath"
}

function Get-GitBlobSha1([string]$Path) {
    $item = Get-Item -Force -LiteralPath $Path
    $prefix = [System.Text.Encoding]::UTF8.GetBytes("blob $($item.Length)`0")
    $hasher = [System.Security.Cryptography.IncrementalHash]::CreateHash(
        [System.Security.Cryptography.HashAlgorithmName]::SHA1
    )
    try {
        $hasher.AppendData($prefix)
        $stream = [System.IO.File]::OpenRead($item.FullName)
        try {
            $buffer = [byte[]]::new(1048576)
            while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $hasher.AppendData($buffer, 0, $read)
            }
        }
        finally {
            $stream.Dispose()
        }
        [Convert]::ToHexString($hasher.GetHashAndReset()).ToLowerInvariant()
    }
    finally {
        $hasher.Dispose()
    }
}

function Get-GitTreeMap([string]$Text, [string]$RequiredPrefix = '') {
    $map = @{}
    foreach ($line in @($Text -split "`r?`n" | Where-Object { $_ -ne '' })) {
        if ($line -notmatch '^([0-7]{6}) blob ([0-9a-f]{40})\t(.+)$') {
            throw "unsupported Git tree entry: $line"
        }
        $path = $Matches[3]
        if ($RequiredPrefix -ne '' -and
            -not $path.StartsWith($RequiredPrefix, [System.StringComparison]::Ordinal)) {
            throw "Git tree entry escapes required prefix ${RequiredPrefix}: $path"
        }
        if ($map.ContainsKey($path)) {
            throw "duplicate Git tree path: $path"
        }
        $map[$path] = [ordered]@{
            mode = $Matches[1]
            blob = $Matches[2]
        }
    }
    $map
}

function Get-FileManifest(
    [string]$Root,
    [string]$GitPrefix = '',
    [hashtable]$GitTree = @{},
    [switch]$ExcludeDotGit
) {
    $resolvedRoot = (Resolve-Path -LiteralPath $Root).Path
    $manifest = @(
        Get-ChildItem -LiteralPath $resolvedRoot -Recurse -File -Force |
            Sort-Object FullName |
            ForEach-Object {
                $relative = [System.IO.Path]::GetRelativePath($resolvedRoot, $_.FullName).Replace('\', '/')
                if (-not ($ExcludeDotGit -and
                        ($relative -eq '.git' -or
                            $relative.StartsWith('.git/', [System.StringComparison]::Ordinal)))) {
                    $gitPath = if ($GitPrefix -eq '') { $relative } else { "$GitPrefix/$relative" }
                    $entry = [ordered]@{
                        path = $relative
                        bytes = $_.Length
                        sha256 = Get-Sha256 $_.FullName
                    }
                    if ($GitTree.Count -ne 0) {
                        if (-not $GitTree.ContainsKey($gitPath)) {
                            throw "disk file is absent from the pinned Git tree: $gitPath"
                        }
                        $blob = Get-GitBlobSha1 $_.FullName
                        if ($blob -ne $GitTree[$gitPath].blob) {
                            throw "disk bytes do not match the pinned Git blob: $gitPath"
                        }
                        $entry.git_mode = $GitTree[$gitPath].mode
                        $entry.git_blob = $blob
                    }
                    $entry
                }
            }
    )
    if ($GitTree.Count -ne 0) {
        $manifestPaths = @(
            foreach ($entry in $manifest) {
                if ($GitPrefix -eq '') { $entry.path } else { "$GitPrefix/$($entry.path)" }
            }
        )
        if (($manifestPaths | Sort-Object) -join "`n" -ne
            (($GitTree.Keys | Sort-Object) -join "`n")) {
            throw 'pinned Git tree and disk manifest do not have exact path coverage'
        }
    }
    $manifest
}

function Get-ManifestDigest([object[]]$Manifest) {
    $lines = @(
        foreach ($entry in $Manifest) {
            '{0}`t{1}`t{2}' -f $entry.path, $entry.bytes, $entry.sha256
        }
    )
    $bytes = $utf8NoBom.GetBytes(($lines -join "`n") + "`n")
    [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}

foreach ($requiredDirectory in @($sourceRoot, $adapterRoot, $workspaceRoot)) {
    if (-not (Test-Path -LiteralPath $requiredDirectory -PathType Container)) {
        throw "required directory is missing: $requiredDirectory"
    }
}

$experimentRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $repoRoot 'target\s114-experiment')
).TrimEnd('\')
$resolvedSourceRoot = (Resolve-Path -LiteralPath $sourceRoot).Path.TrimEnd('\')
$resolvedWorkspaceRoot = (Resolve-Path -LiteralPath $workspaceRoot).Path.TrimEnd('\')
if (-not [string]::Equals(
        $resolvedSourceRoot,
        [System.IO.Path]::GetFullPath($sourceRoot).TrimEnd('\'),
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or
    -not [string]::Equals(
        $resolvedWorkspaceRoot,
        [System.IO.Path]::GetFullPath($workspaceRoot).TrimEnd('\'),
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw 'probe source or workspace resolves through an unexpected path alias'
}
foreach ($resolvedRoot in @($resolvedSourceRoot, $resolvedWorkspaceRoot)) {
    if (-not $resolvedRoot.StartsWith(
            $experimentRoot + '\',
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "probe root escapes the isolated experiment directory: $resolvedRoot"
    }
    if ((Get-Item -Force -LiteralPath $resolvedRoot).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) {
        throw "probe root must not be a reparse point: $resolvedRoot"
    }
}
if ([string]::Equals(
        $resolvedSourceRoot,
        $resolvedWorkspaceRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw 'probe source and workspace must be disjoint'
}
foreach ($protectedPath in @(
    (Join-Path $adapterRoot 'install.sh'),
    (Join-Path $workspaceRoot '.git')
)) {
    if (-not (Test-Path -LiteralPath $protectedPath)) {
        throw "probe boundary input is missing: $protectedPath"
    }
    if ((Get-Item -Force -LiteralPath $protectedPath).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) {
        throw "probe boundary input must not be a reparse point: $protectedPath"
    }
}
$workspaceGitRoot = Join-Path $workspaceRoot '.git'
if (-not (Test-Path -LiteralPath $workspaceGitRoot -PathType Container)) {
    throw 'the disposable workspace must be a standalone Git repository'
}
$preexistingScripts = Join-Path $workspaceRoot 'scripts'
if ((Test-Path -LiteralPath $preexistingScripts) -and
    (Get-Item -Force -LiteralPath $preexistingScripts).Attributes.HasFlag(
        [System.IO.FileAttributes]::ReparsePoint
    )) {
    throw 'the installer-owned workspace scripts path must not be a reparse point'
}
foreach ($boundaryPath in @(
    $repoRoot,
    (Join-Path $repoRoot 'target'),
    $experimentRoot,
    $sourceRoot,
    $adapterRoot,
    (Join-Path $adapterRoot 'install.sh'),
    $workspaceRoot,
    (Join-Path $workspaceRoot '.git'),
    $artifactRoot
)) {
    Assert-NoReparseAncestors $boundaryPath $repoRoot
}
if (Test-Path -LiteralPath $preexistingScripts) {
    Assert-NoReparseAncestors $preexistingScripts $repoRoot
}
Assert-NoReparseTree $adapterRoot
Assert-NoReparseTree $workspaceRoot

$ferricPath = Join-Path $repoRoot 'target\release\ferric.exe'
if (-not (Test-Path -LiteralPath $ferricPath -PathType Leaf)) {
    throw "Ferric binary is missing: $ferricPath"
}
if ((Get-Sha256 $ferricPath) -ne $PinnedFerricSha256) {
    throw 'Ferric binary does not match the T-11409-calibrated identity'
}

$sourceSafe = $sourceRoot.Replace('\', '/')
$workspaceSafe = $workspaceRoot.Replace('\', '/')
$sourceCommit = Invoke-Captured 'source-commit' 'git' @(
    '-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'rev-parse', 'HEAD'
)
$sourceTree = Invoke-Captured 'source-tree' 'git' @(
    '-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'rev-parse', 'HEAD^{tree}'
)
$sourceStatus = Invoke-Captured 'source-status' 'git' @(
    '-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'status', '--porcelain=v1'
)
$sourceRemote = Invoke-Captured 'source-remote' 'git' @(
    '-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'remote', 'get-url', 'origin'
)
$sourceLsTree = Invoke-Captured 'source-ls-tree' 'git' @(
    '-c', "safe.directory=$sourceSafe", '-C', $sourceRoot,
    'ls-tree', '-r', '--full-tree', $PinnedSourceCommit, '--', 'open-harnesses'
)
if ($sourceCommit.exit_code -ne 0 -or $sourceCommit.stdout.Trim() -ne $PinnedSourceCommit) {
    throw 'upstream source commit does not match the frozen pin'
}
if ($sourceTree.exit_code -ne 0 -or $sourceTree.stdout.Trim() -ne $PinnedSourceTree) {
    throw 'upstream source tree does not match the frozen pin'
}
if ($sourceStatus.exit_code -ne 0 -or -not [string]::IsNullOrWhiteSpace($sourceStatus.stdout)) {
    throw 'upstream source checkout is not clean'
}
if ($sourceRemote.exit_code -ne 0 -or $sourceRemote.stdout.Trim() -ne $PinnedSourceRemote) {
    throw 'upstream source remote does not match the frozen repository identity'
}
if ($sourceLsTree.exit_code -ne 0 -or [string]::IsNullOrWhiteSpace($sourceLsTree.stdout)) {
    throw 'could not enumerate the pinned Open Harnesses Git tree'
}
$sourceGitTree = Get-GitTreeMap $sourceLsTree.stdout 'open-harnesses/'

$workspaceCommit = Invoke-Captured 'workspace-commit' 'git' @(
    '-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'rev-parse', 'HEAD'
)
$workspaceTree = Invoke-Captured 'workspace-tree' 'git' @(
    '-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'rev-parse', 'HEAD^{tree}'
)
$workspaceLsTree = Invoke-Captured 'workspace-ls-tree' 'git' @(
    '-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot,
    'ls-tree', '-r', '--full-tree', 'HEAD'
)
if ($workspaceCommit.exit_code -ne 0 -or
    $workspaceCommit.stdout.Trim() -notmatch '^[0-9a-f]{40}$') {
    throw 'could not establish the disposable workspace commit identity'
}
if ($workspaceTree.exit_code -ne 0 -or
    $workspaceTree.stdout.Trim() -notmatch '^[0-9a-f]{40}$') {
    throw 'could not establish the disposable workspace tree identity'
}
if ($workspaceLsTree.exit_code -ne 0 -or [string]::IsNullOrWhiteSpace($workspaceLsTree.stdout)) {
    throw 'could not enumerate the disposable workspace Git tree'
}
$workspaceGitTree = Get-GitTreeMap $workspaceLsTree.stdout
$workspaceManifestBefore = Get-FileManifest $workspaceRoot '' $workspaceGitTree -ExcludeDotGit

$wslSource = Invoke-Captured 'wsl-source-path' 'wsl.exe' @('--exec', 'wslpath', '-a', $sourceRoot)
$wslWorkspace = Invoke-Captured 'wsl-workspace-path' 'wsl.exe' @('--exec', 'wslpath', '-a', $workspaceRoot)
if ($wslSource.exit_code -ne 0 -or $wslWorkspace.exit_code -ne 0) {
    throw 'could not resolve WSL paths for the isolated probe'
}
$wslSourceLines = @($wslSource.stdout -split "`r?`n" | Where-Object { $_ -ne '' })
$wslWorkspaceLines = @($wslWorkspace.stdout -split "`r?`n" | Where-Object { $_ -ne '' })
if ($wslSourceLines.Count -ne 1 -or $wslWorkspaceLines.Count -ne 1 -or
    -not $wslSourceLines[0].StartsWith('/', [System.StringComparison]::Ordinal) -or
    -not $wslWorkspaceLines[0].StartsWith('/', [System.StringComparison]::Ordinal) -or
    $wslSourceLines[0].Contains([char]0) -or $wslWorkspaceLines[0].Contains([char]0)) {
    throw 'WSL path conversion did not return one absolute POSIX path per root'
}
$wslSourcePath = $wslSourceLines[0]
$wslWorkspacePath = $wslWorkspaceLines[0]
$wslSourceRoundtrip = Invoke-Captured 'wsl-source-roundtrip' 'wsl.exe' @(
    '--exec', 'wslpath', '-w', $wslSourcePath
)
$wslWorkspaceRoundtrip = Invoke-Captured 'wsl-workspace-roundtrip' 'wsl.exe' @(
    '--exec', 'wslpath', '-w', $wslWorkspacePath
)
$wslSourceRoundtripLines = @(
    $wslSourceRoundtrip.stdout -split "`r?`n" | Where-Object { $_ -ne '' }
)
$wslWorkspaceRoundtripLines = @(
    $wslWorkspaceRoundtrip.stdout -split "`r?`n" | Where-Object { $_ -ne '' }
)
if ($wslSourceRoundtrip.exit_code -ne 0 -or $wslWorkspaceRoundtrip.exit_code -ne 0 -or
    $wslSourceRoundtripLines.Count -ne 1 -or $wslWorkspaceRoundtripLines.Count -ne 1 -or
    -not [System.IO.Path]::IsPathFullyQualified($wslSourceRoundtripLines[0]) -or
    -not [System.IO.Path]::IsPathFullyQualified($wslWorkspaceRoundtripLines[0]) -or
    -not [string]::Equals(
        [System.IO.Path]::GetFullPath($wslSourceRoundtripLines[0]).TrimEnd('\'),
        $resolvedSourceRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or
    -not [string]::Equals(
        [System.IO.Path]::GetFullPath($wslWorkspaceRoundtripLines[0]).TrimEnd('\'),
        $resolvedWorkspaceRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw 'WSL path conversion does not round-trip to the validated Windows roots'
}
$wslInstaller = $wslSourcePath + '/open-harnesses/install.sh'

$install = Invoke-Captured 'install' 'wsl.exe' @(
    '--exec', 'bash', $wslInstaller, $wslWorkspacePath
)
if ($install.exit_code -ne 0) {
    throw "open-harness installer failed with exit code $($install.exit_code)"
}

$sourceManifest = Get-FileManifest $adapterRoot 'open-harnesses' $sourceGitTree
$installedScriptsRoot = Join-Path $workspaceRoot 'scripts'
$installedManifest = Get-FileManifest $installedScriptsRoot
$sourceScriptsManifest = @(
    foreach ($entry in $sourceManifest) {
        if ($entry.path.StartsWith('scripts/', [System.StringComparison]::Ordinal)) {
            [ordered]@{
                path = $entry.path.Substring('scripts/'.Length)
                bytes = $entry.bytes
                sha256 = $entry.sha256
            }
        }
    }
)
$sourceManifestPath = Join-Path $evidenceRoot 'source-manifest.json'
$installedManifestPath = Join-Path $evidenceRoot 'installed-scripts-manifest.json'
Write-Json $sourceManifestPath ([ordered]@{
    schema = 'animus-ferric-s114-sprint-loop-source-manifest-v1'
    git_commit = $PinnedSourceCommit
    git_tree = $PinnedSourceTree
    root = 'open-harnesses'
    file_count = $sourceManifest.Count
    content_tree_sha256 = Get-ManifestDigest $sourceManifest
    files = $sourceManifest
}) 12
Write-Json $installedManifestPath ([ordered]@{
    schema = 'animus-ferric-s114-sprint-loop-install-manifest-v1'
    root = 'target/s114-experiment/sprint-loop-workspace/scripts'
    file_count = $installedManifest.Count
    content_tree_sha256 = Get-ManifestDigest $installedManifest
    files = $installedManifest
}) 12

$sourceMap = @{}
foreach ($entry in $sourceScriptsManifest) {
    $sourceMap[$entry.path] = "$($entry.bytes):$($entry.sha256)"
}
$installedMap = @{}
foreach ($entry in $installedManifest) {
    $installedMap[$entry.path] = "$($entry.bytes):$($entry.sha256)"
}
$installBytesMatch = ($sourceMap.Count -eq $installedMap.Count)
if ($installBytesMatch) {
    foreach ($key in $sourceMap.Keys) {
        if (-not $installedMap.ContainsKey($key) -or $installedMap[$key] -ne $sourceMap[$key]) {
            $installBytesMatch = $false
            break
        }
    }
}
if (-not $installBytesMatch) {
    throw 'installed helper scripts are not byte-identical to the pinned adapter'
}

$skillsList = Invoke-Captured 'skills-list' $ferricPath @(
    'skills', 'list', '--workspace', $workspaceRoot
)
if ($skillsList.exit_code -ne 0) {
    throw "ferric skills list failed with exit code $($skillsList.exit_code)"
}
$ferricSkillPath = Join-Path $workspaceRoot '.ferric\skills\sprint-loop\SKILL.md'
$ferricSkillsRoot = Join-Path $workspaceRoot '.ferric\skills'
$openHarnessSkillPath = Join-Path $adapterRoot 'SKILL.md'
$layoutFailure = -not (Test-Path -LiteralPath $openHarnessSkillPath) -and
    -not (Test-Path -LiteralPath $ferricSkillPath) -and
    $skillsList.stdout.Contains('No skills installed')
if (-not $layoutFailure) {
    throw 'expected open-harness/Ferric packaging boundary was not observed'
}

$checkBook = Invoke-Captured 'check-book' 'wsl.exe' @(
    '--cd', $wslWorkspacePath, '--exec', 'bash', 'scripts/check-book.sh'
)
$currentPhase = Invoke-Captured 'current-phase' 'wsl.exe' @(
    '--cd', $wslWorkspacePath, '--exec', 'bash', 'scripts/current-phase.sh'
)
if ($checkBook.exit_code -eq 0 -or
    -not (($checkBook.stdout + $checkBook.stderr).Contains('Sprint Loops Book is not initialized'))) {
    throw 'operator Book validation did not report the expected uninitialized state'
}
if ($currentPhase.exit_code -ne 0 -or $currentPhase.stdout.Trim() -ne 'uninitialized') {
    throw 'operator router did not report the expected uninitialized state'
}

$workspaceStatus = Invoke-Captured 'workspace-status' 'git' @(
    '-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'status', '--porcelain=v1'
)
if ($workspaceStatus.exit_code -ne 0 -or -not [string]::IsNullOrWhiteSpace($workspaceStatus.stdout)) {
    throw 'isolated workspace changed after the idempotent installer rerun'
}
$workspaceManifest = Get-FileManifest $workspaceRoot '' $workspaceGitTree -ExcludeDotGit
if ((Get-ManifestDigest $workspaceManifestBefore) -ne (Get-ManifestDigest $workspaceManifest)) {
    throw 'disposable workspace bytes changed across the idempotent installer rerun'
}
$workspaceManifestPath = Join-Path $evidenceRoot 'workspace-manifest.json'
Write-Json $workspaceManifestPath ([ordered]@{
    schema = 'animus-ferric-s114-sprint-loop-workspace-manifest-v1'
    git_commit = $workspaceCommit.stdout.Trim()
    git_tree = $workspaceTree.stdout.Trim()
    root = 'target/s114-experiment/sprint-loop-workspace'
    excludes = @('.git/')
    file_count = $workspaceManifest.Count
    content_tree_sha256 = Get-ManifestDigest $workspaceManifest
    files = $workspaceManifest
}) 12

$builtinSource = Join-Path $repoRoot 'crates\ferric-tools\src\builtin\mod.rs'
$gitWriteSource = Join-Path $repoRoot 'crates\ferric-tools\src\builtin\git_write.rs'
$querySource = Join-Path $repoRoot 'crates\ferric-cli\src\query.rs'
$remoteAdapterSource = Join-Path $adapterRoot 'scripts\remote-adapter.sh'
$builtinText = Get-Content -Raw -LiteralPath $builtinSource
$gitWriteText = Get-Content -Raw -LiteralPath $gitWriteSource
$queryText = Get-Content -Raw -LiteralPath $querySource
$builtinBlock = [regex]::Match(
    $builtinText,
    '(?s)pub fn register_builtin_tools.*?^\}',
    [System.Text.RegularExpressions.RegexOptions]::Multiline
).Value
$humanBlock = [regex]::Match(
    $builtinText,
    '(?s)pub fn register_human_tools.*?^\}',
    [System.Text.RegularExpressions.RegexOptions]::Multiline
).Value
$buildRunConfigBlock = [regex]::Match(
    $queryText,
    '(?s)pub\(crate\) fn build_run_config\(.*?^\}',
    [System.Text.RegularExpressions.RegexOptions]::Multiline
).Value
if ([string]::IsNullOrWhiteSpace($buildRunConfigBlock)) {
    throw 'could not isolate the production build_run_config function'
}
$expectedBuiltinRegistrations = @(
    'ReadFile',
    'WriteFile',
    'EditFile',
    'ListDir',
    'MovePath',
    'MakeDir',
    'SearchFiles',
    'DeletePath',
    'FindFiles',
    'CopyFile',
    'MultiEdit',
    'ApplyPatch',
    'GitRead',
    'GitWrite'
)
$builtinRegistrations = @(
    [regex]::Matches($builtinBlock, 'registry\.register\(Box::new\(([A-Za-z0-9_]+)\)\);') |
        ForEach-Object { $_.Groups[1].Value }
)
$humanRegistrations = @(
    [regex]::Matches($humanBlock, 'registry\.register\(Box::new\(([A-Za-z0-9_]+)\)\);') |
        ForEach-Object { $_.Groups[1].Value }
)
$builtinRegistrationsExact = ($builtinRegistrations -join "`n") -eq
    ($expectedBuiltinRegistrations -join "`n")
$humanRegistrationsExact = ($humanRegistrations -join "`n") -eq "ShellExec`nManageTask"
$gitWriteSpecExact = $gitWriteText -match '(?s)name: "git_write"\.to_string\(\).*?ring: 2,' -and
    $gitWriteText.Contains('"enum": ["add", "commit", "checkout", "branch"]') -and
    -not $gitWriteText.Contains('"push"')
$nativeRemoteToolRegistered = @(
    $builtinRegistrations |
        Where-Object { $_ -match '(?i)(remote|push|pull|fetch|github|gitlab)' }
).Count -ne 0
$toolFacts = [ordered]@{
    query_uses_builtin_registry = $buildRunConfigBlock.Contains('register_builtin_tools(&mut registry);')
    query_uses_human_registry = $buildRunConfigBlock.Contains('register_human_tools(&mut registry);')
    builtin_registered_types = $builtinRegistrations
    builtin_registered_types_exact = $builtinRegistrationsExact
    conditional_run_check_registration = $queryText.Contains('register_run_checks(&mut config.registry, checks)')
    git_write_registered_for_query = $builtinRegistrations -contains 'GitWrite'
    git_write_ring = if ($gitWriteSpecExact) { 2 } else { $null }
    shell_exec_registered_for_query = $builtinRegistrations -contains 'ShellExec'
    manage_task_registered_for_query = $builtinRegistrations -contains 'ManageTask'
    human_registered_types = $humanRegistrations
    human_registered_types_exact = $humanRegistrationsExact
    shell_exec_is_human_surface = $humanRegistrations -contains 'ShellExec'
    manage_task_is_human_surface = $humanRegistrations -contains 'ManageTask'
    native_remote_tool_registered = $nativeRemoteToolRegistered
}
if (-not $toolFacts.query_uses_builtin_registry -or
    $toolFacts.query_uses_human_registry -or
    -not $toolFacts.builtin_registered_types_exact -or
    -not $toolFacts.conditional_run_check_registration -or
    -not $toolFacts.git_write_registered_for_query -or
    $toolFacts.git_write_ring -ne 2 -or
    $toolFacts.shell_exec_registered_for_query -or
    $toolFacts.manage_task_registered_for_query -or
    -not $toolFacts.human_registered_types_exact -or
    -not $toolFacts.shell_exec_is_human_surface -or
    -not $toolFacts.manage_task_is_human_surface -or
    $toolFacts.native_remote_tool_registered) {
    throw 'Ferric static tool-boundary assertions changed'
}

$commands = @(
    $sourceCommit,
    $sourceTree,
    $sourceStatus,
    $sourceRemote,
    $sourceLsTree,
    $workspaceCommit,
    $workspaceTree,
    $workspaceLsTree,
    $wslSource,
    $wslWorkspace,
    $wslSourceRoundtrip,
    $wslWorkspaceRoundtrip,
    $install,
    $skillsList,
    $checkBook,
    $currentPhase,
    $workspaceStatus
)
$commandRecords = @(
    foreach ($command in $commands) {
        [ordered]@{
            name = $command.name
            executable = [System.IO.Path]::GetFileName($command.executable)
            arguments = $command.arguments
            exit_code = $command.exit_code
            timeout_milliseconds = $command.timeout_milliseconds
            timed_out = $command.timed_out
            stdout_file = $command.stdout_file
            stdout_sha256 = $command.stdout_sha256
            stderr_file = $command.stderr_file
            stderr_sha256 = $command.stderr_sha256
        }
    }
)

$verdict = [ordered]@{
    schema = 'animus-ferric-s114-sprint-loop-capability-verdict-v1'
    task = 'T-11411'
    captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    result = 'packaging_failure'
    source = [ordered]@{
        repository = $sourceRemote.stdout.Trim()
        commit = $PinnedSourceCommit
        tree = $PinnedSourceTree
        clean = $true
        adapter = 'open-harnesses'
        source_manifest_file = 'evidence/source-manifest.json'
        source_manifest_sha256 = Get-Sha256 $sourceManifestPath
        source_content_tree_sha256 = Get-ManifestDigest $sourceManifest
    }
    runtime = [ordered]@{
        ferric_display_path = 'target/release/ferric.exe'
        ferric_sha256 = Get-Sha256 $ferricPath
        qwen38_selection = 'selected_q4'
        behavioral_model_run = $false
        fallback_control_activated = $false
    }
    installation = [ordered]@{
        installer_exit_code = $install.exit_code
        installed_script_count = $installedManifest.Count
        pinned_source_script_count = $sourceScriptsManifest.Count
        installed_bytes_match_pinned_source = $installBytesMatch
        installed_manifest_file = 'evidence/installed-scripts-manifest.json'
        installed_manifest_sha256 = Get-Sha256 $installedManifestPath
        workspace_manifest_file = 'evidence/workspace-manifest.json'
        workspace_manifest_sha256 = Get-Sha256 $workspaceManifestPath
        workspace_content_tree_sha256 = Get-ManifestDigest $workspaceManifest
        disposable_git_commit = $workspaceCommit.stdout.Trim()
        disposable_git_tree = $workspaceTree.stdout.Trim()
        disposable_git_clean_after_idempotent_reinstall = $true
    }
    packaging = [ordered]@{
        open_harness_root_skill_md_present = Test-Path -LiteralPath $openHarnessSkillPath
        ferric_skill_root_present = Test-Path -LiteralPath $ferricSkillsRoot
        ferric_sprint_loop_skill_md_present = Test-Path -LiteralPath $ferricSkillPath
        ferric_skills_list_exit_code = $skillsList.exit_code
        ferric_skills_list_reports_none = $skillsList.stdout.Contains('No skills installed')
        classification = 'not_discovered_open_harness_has_no_ferric_skill_package'
    }
    capability_layers = [ordered]@{
        installed = 'yes_operator_helpers_only'
        discovered = 'no'
        authorized = 'not-runnable-after-packaging-failure'
        top_level_instruction_injected = 'not-runnable-after-packaging-failure'
        resource_accessible_natively = 'not-runnable-after-packaging-failure'
        resource_accessible_after_operator_materialization = 'not-runnable-after-packaging-failure'
        helper_tool_exposed = 'not-runnable-after-packaging-failure'
        book_advanced_with_typed_tools = 'not-runnable-after-packaging-failure'
        book_operator_validated = 'no_book_initialized'
        cross_run_resumed = 'not-runnable-after-packaging-failure'
        git_write_registered = $toolFacts.git_write_registered_for_query
        git_write_offered = 'not-runnable-after-packaging-failure'
        git_write_attempted = 'not-runnable-after-packaging-failure'
        git_write_succeeded = 'not-runnable-after-packaging-failure'
        remote_checkpoint = 'no_native_remote_tool_registered'
    }
    operator_validation = [ordered]@{
        check_book_exit_code = $checkBook.exit_code
        check_book_result = 'not_initialized'
        router_exit_code = $currentPhase.exit_code
        router_result = $currentPhase.stdout.Trim()
        remote_mutation_attempted = $false
    }
    static_tool_boundary = [ordered]@{
        facts = $toolFacts
        builtin_registry_source = 'crates/ferric-tools/src/builtin/mod.rs'
        builtin_registry_sha256 = Get-Sha256 $builtinSource
        git_write_source = 'crates/ferric-tools/src/builtin/git_write.rs'
        git_write_sha256 = Get-Sha256 $gitWriteSource
        query_source = 'crates/ferric-cli/src/query.rs'
        query_sha256 = Get-Sha256 $querySource
        remote_adapter_source = 'open-harnesses/scripts/remote-adapter.sh'
        remote_adapter_sha256 = Get-Sha256 $remoteAdapterSource
    }
    controls = [ordered]@{
        provider_request_capture = 'not-runnable-after-packaging-failure'
        native_resource_arm = 'not-runnable-after-packaging-failure'
        assisted_resource_arm = 'not-runnable-after-packaging-failure'
        typed_book_arm = 'not-runnable-after-packaging-failure'
        evidence_ring2_git_arm = 'not-runnable-after-packaging-failure'
        legacy_ring2_git_arm = 'not-runnable-after-packaging-failure'
        qwen25_fallback_arm = 'not_authorized_qwen38_is_viable'
    }
    commands = $commandRecords
}

$verdictPath = Join-Path $evidenceRoot 'capability-verdict.json'
Write-Json $verdictPath $verdict 14

$manifestEntries = @(
    Get-ChildItem -LiteralPath $evidenceRoot -File |
        Where-Object Name -ne 'files.sha256' |
        Sort-Object Name |
        ForEach-Object {
            [ordered]@{
                path = $_.Name
                bytes = $_.Length
                sha256 = Get-Sha256 $_.FullName
            }
        }
)
$manifestPath = Join-Path $evidenceRoot 'files.sha256'
$manifestText = ($manifestEntries | ForEach-Object { "$($_.sha256)  $($_.path)" }) -join "`n"
Write-Utf8 $manifestPath ($manifestText + "`n")
Move-Item -LiteralPath $evidenceRoot -Destination $finalEvidenceRoot
$evidenceRoot = $null
Write-Output (Join-Path $finalEvidenceRoot 'capability-verdict.json')
