[CmdletBinding()]
param(
    [string]$ArtifactRoot = $PSScriptRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactRoot = (Resolve-Path -LiteralPath $ArtifactRoot).Path
$evidenceRoot = Join-Path $artifactRoot 'evidence'
$manifestPath = Join-Path $evidenceRoot 'files.sha256'
$verdictPath = Join-Path $evidenceRoot 'capability-verdict.json'
$reportPath = Join-Path $artifactRoot 'capability-report.md'
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$pinnedVerdictSha256 = '501e8494accd951262caccb9351e765bee6bfd3859a9897be42a9a33296754fe'
$expectedSourceCommit = '4acc1fd6e0b964ea4bcbedd17c44cb2ca8ca0066'
$expectedSourceTree = '3420c3d9858b6d3049b81f2334ca21a9d1fdaade'
$expectedSourceRemote = 'https://github.com/crussella0129/Animus_Sprint_Loops.git'
$expectedFerricSha256 = 'af75612b3498a1721e5b5f1b2f6309bf851d65b9bd13ad45e76cf8e370cf10f2'
$utf8NoBom = [System.Text.UTF8Encoding]::new($false)

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
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

function Read-Evidence([string]$Name) {
    Get-Content -Raw -LiteralPath (Join-Path $evidenceRoot $Name)
}

function Get-GitTreeMap([string]$Text, [string]$RequiredPrefix = '') {
    $map = @{}
    foreach ($line in @($Text -split "`r?`n" | Where-Object { $_ -ne '' })) {
        if ($line -notmatch '^([0-7]{6}) blob ([0-9a-f]{40})\t(.+)$') {
            throw "unsupported Git tree evidence entry: $line"
        }
        $path = $Matches[3]
        if ($RequiredPrefix -ne '' -and
            -not $path.StartsWith($RequiredPrefix, [System.StringComparison]::Ordinal)) {
            throw "Git tree evidence escapes required prefix ${RequiredPrefix}: $path"
        }
        $caseKey = $path.ToLowerInvariant()
        if ($map.ContainsKey($caseKey)) {
            throw "duplicate or case-colliding Git tree path: $path"
        }
        $map[$caseKey] = [pscustomobject]@{
            path = $path
            mode = $Matches[1]
            blob = $Matches[2]
        }
    }
    $map
}

function Test-NormalizedRelativePath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or
        [System.IO.Path]::IsPathRooted($Path) -or
        $Path.Contains('\') -or
        $Path.Contains('//') -or
        $Path.StartsWith('/') -or
        $Path.EndsWith('/')) {
        return $false
    }
    foreach ($segment in $Path.Split('/')) {
        if ($segment -in @('', '.', '..')) {
            return $false
        }
    }
    $true
}

function Test-JsonManifest(
    [object]$Manifest,
    [string]$Schema,
    [int]$ExpectedCount,
    [bool]$RequireGitBinding
) {
    if ([string]$Manifest.schema -ne $Schema) {
        throw "unexpected manifest schema: $($Manifest.schema)"
    }
    $files = @($Manifest.files)
    if ([int]$Manifest.file_count -ne $files.Count -or $files.Count -ne $ExpectedCount) {
        throw "manifest file count mismatch for $Schema"
    }
    $map = @{}
    $paths = @()
    foreach ($entry in $files) {
        $path = [string]$entry.path
        if (-not (Test-NormalizedRelativePath $path)) {
            throw "manifest path is not a normalized relative path: $path"
        }
        $caseKey = $path.ToLowerInvariant()
        if ($map.ContainsKey($caseKey)) {
            throw "duplicate or case-colliding manifest path: $path"
        }
        if ([long]$entry.bytes -lt 0 -or [string]$entry.sha256 -notmatch '^[0-9a-f]{64}$') {
            throw "malformed manifest file identity: $path"
        }
        if ($RequireGitBinding -and
            ([string]$entry.git_mode -notmatch '^[0-7]{6}$' -or
                [string]$entry.git_blob -notmatch '^[0-9a-f]{40}$')) {
            throw "manifest entry lacks a valid Git binding: $path"
        }
        $map[$caseKey] = $entry
        $paths += $path
    }
    if (($paths -join "`n") -ne (($paths | Sort-Object) -join "`n")) {
        throw "manifest paths are not sorted: $Schema"
    }
    if ([string]$Manifest.content_tree_sha256 -ne (Get-ManifestDigest $files)) {
        throw "manifest content-tree digest mismatch: $Schema"
    }
    [pscustomobject]@{
        files = $files
        map = $map
    }
}

function Test-StringArrayExact([object[]]$Actual, [string[]]$Expected) {
    (@($Actual | ForEach-Object { [string]$_ }) -join "`0") -eq ($Expected -join "`0")
}

foreach ($required in @($artifactRoot, $evidenceRoot, $manifestPath, $verdictPath, $reportPath)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "required probe artifact is missing: $required"
    }
}
foreach ($requiredRoot in @($artifactRoot, $evidenceRoot)) {
    if ((Get-Item -Force -LiteralPath $requiredRoot).Attributes.HasFlag(
            [System.IO.FileAttributes]::ReparsePoint
        )) {
        throw "probe artifact root must not be a reparse point: $requiredRoot"
    }
}

$evidenceItems = @(Get-ChildItem -Force -Recurse -LiteralPath $evidenceRoot)
foreach ($item in $evidenceItems) {
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "evidence must not contain a reparse point: $($item.FullName)"
    }
    if ($item.PSIsContainer) {
        throw "evidence payloads must be flat files: $($item.FullName)"
    }
}

$listed = @{}
foreach ($line in Get-Content -LiteralPath $manifestPath) {
    if ([string]::IsNullOrWhiteSpace($line)) {
        continue
    }
    if ($line -notmatch '^([0-9a-f]{64})  ([^/\\]+)$') {
        throw "invalid files.sha256 entry: $line"
    }
    $hash = $Matches[1]
    $name = $Matches[2]
    $caseKey = $name.ToLowerInvariant()
    if ($name -eq 'files.sha256' -or $listed.ContainsKey($caseKey)) {
        throw "duplicate, case-colliding, or self-referential files.sha256 entry: $name"
    }
    $path = Join-Path $evidenceRoot $name
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "manifest entry is missing: $name"
    }
    if ((Get-Sha256 $path) -ne $hash) {
        throw "manifest hash mismatch: $name"
    }
    $listed[$caseKey] = [pscustomobject]@{
        name = $name
        hash = $hash
    }
}

$actualNames = @(
    $evidenceItems |
        Where-Object Name -ne 'files.sha256' |
        ForEach-Object Name |
        Sort-Object
)
$listedNames = @($listed.Values | ForEach-Object name | Sort-Object)
if (($actualNames -join "`n") -ne ($listedNames -join "`n")) {
    throw 'files.sha256 coverage is not exact'
}

$verdict = Get-Content -Raw -LiteralPath $verdictPath | ConvertFrom-Json
$sourceManifest = Read-Evidence 'source-manifest.json' | ConvertFrom-Json
$installedManifest = Read-Evidence 'installed-scripts-manifest.json' | ConvertFrom-Json
$workspaceManifest = Read-Evidence 'workspace-manifest.json' | ConvertFrom-Json
$sourceChecked = Test-JsonManifest $sourceManifest `
    'animus-ferric-s114-sprint-loop-source-manifest-v1' 52 $true
$installedChecked = Test-JsonManifest $installedManifest `
    'animus-ferric-s114-sprint-loop-install-manifest-v1' 28 $false
$workspaceChecked = Test-JsonManifest $workspaceManifest `
    'animus-ferric-s114-sprint-loop-workspace-manifest-v1' 28 $true

$sourceGitTree = Get-GitTreeMap (Read-Evidence 'source-ls-tree.stdout.txt') 'open-harnesses/'
$workspaceGitTree = Get-GitTreeMap (Read-Evidence 'workspace-ls-tree.stdout.txt')
if ($sourceGitTree.Count -ne $sourceChecked.files.Count -or
    $workspaceGitTree.Count -ne $workspaceChecked.files.Count) {
    throw 'Git-tree and JSON-manifest counts differ'
}
foreach ($entry in $sourceChecked.files) {
    $gitKey = ('open-harnesses/' + [string]$entry.path).ToLowerInvariant()
    if (-not $sourceGitTree.ContainsKey($gitKey) -or
        [string]$entry.git_mode -ne $sourceGitTree[$gitKey].mode -or
        [string]$entry.git_blob -ne $sourceGitTree[$gitKey].blob) {
        throw "source manifest is not bound to raw Git-tree evidence: $($entry.path)"
    }
}
foreach ($entry in $workspaceChecked.files) {
    $gitKey = ([string]$entry.path).ToLowerInvariant()
    if (-not $workspaceGitTree.ContainsKey($gitKey) -or
        [string]$entry.git_mode -ne $workspaceGitTree[$gitKey].mode -or
        [string]$entry.git_blob -ne $workspaceGitTree[$gitKey].blob) {
        throw "workspace manifest is not bound to raw Git-tree evidence: $($entry.path)"
    }
}

$installedTreeMatches = $true
foreach ($entry in $installedChecked.files) {
    $sourceKey = ('scripts/' + [string]$entry.path).ToLowerInvariant()
    if (-not $sourceChecked.map.ContainsKey($sourceKey) -or
        -not $workspaceChecked.map.ContainsKey($sourceKey) -or
        [long]$entry.bytes -ne [long]$sourceChecked.map[$sourceKey].bytes -or
        [string]$entry.sha256 -ne [string]$sourceChecked.map[$sourceKey].sha256 -or
        [long]$entry.bytes -ne [long]$workspaceChecked.map[$sourceKey].bytes -or
        [string]$entry.sha256 -ne [string]$workspaceChecked.map[$sourceKey].sha256) {
        $installedTreeMatches = $false
    }
}

$commandsByName = @{}
foreach ($command in @($verdict.commands)) {
    $name = [string]$command.name
    if ($commandsByName.ContainsKey($name)) {
        throw "duplicate command record: $name"
    }
    $commandsByName[$name] = $command
}

$sourceRoot = Join-Path $repoRoot 'target\s114-experiment\sprint-loop-source'
$workspaceRoot = Join-Path $repoRoot 'target\s114-experiment\sprint-loop-workspace'
$sourceSafe = $sourceRoot.Replace('\', '/')
$workspaceSafe = $workspaceRoot.Replace('\', '/')
$wslSourceLines = @(
    (Read-Evidence 'wsl-source-path.stdout.txt') -split "`r?`n" | Where-Object { $_ -ne '' }
)
$wslWorkspaceLines = @(
    (Read-Evidence 'wsl-workspace-path.stdout.txt') -split "`r?`n" | Where-Object { $_ -ne '' }
)
$wslSourceRoundtripLines = @(
    (Read-Evidence 'wsl-source-roundtrip.stdout.txt') -split "`r?`n" | Where-Object { $_ -ne '' }
)
$wslWorkspaceRoundtripLines = @(
    (Read-Evidence 'wsl-workspace-roundtrip.stdout.txt') -split "`r?`n" | Where-Object { $_ -ne '' }
)
$wslOutputShapeExact = $wslSourceLines.Count -eq 1 -and
    $wslWorkspaceLines.Count -eq 1 -and
    $wslSourceRoundtripLines.Count -eq 1 -and
    $wslWorkspaceRoundtripLines.Count -eq 1
$wslSourceRaw = if ($wslSourceLines.Count -eq 1) { $wslSourceLines[0] } else { '' }
$wslWorkspaceRaw = if ($wslWorkspaceLines.Count -eq 1) { $wslWorkspaceLines[0] } else { '' }
$wslSourceRoundtripRaw = if ($wslSourceRoundtripLines.Count -eq 1) {
    $wslSourceRoundtripLines[0]
} else { '' }
$wslWorkspaceRoundtripRaw = if ($wslWorkspaceRoundtripLines.Count -eq 1) {
    $wslWorkspaceRoundtripLines[0]
} else { '' }
$expectedCommands = [ordered]@{
    'source-commit' = @('git', 0, @('-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'rev-parse', 'HEAD'))
    'source-tree' = @('git', 0, @('-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'rev-parse', 'HEAD^{tree}'))
    'source-status' = @('git', 0, @('-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'status', '--porcelain=v1'))
    'source-remote' = @('git', 0, @('-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'remote', 'get-url', 'origin'))
    'source-ls-tree' = @('git', 0, @('-c', "safe.directory=$sourceSafe", '-C', $sourceRoot, 'ls-tree', '-r', '--full-tree', $expectedSourceCommit, '--', 'open-harnesses'))
    'workspace-commit' = @('git', 0, @('-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'rev-parse', 'HEAD'))
    'workspace-tree' = @('git', 0, @('-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'rev-parse', 'HEAD^{tree}'))
    'workspace-ls-tree' = @('git', 0, @('-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'ls-tree', '-r', '--full-tree', 'HEAD'))
    'wsl-source-path' = @('wsl.exe', 0, @('--exec', 'wslpath', '-a', $sourceRoot))
    'wsl-workspace-path' = @('wsl.exe', 0, @('--exec', 'wslpath', '-a', $workspaceRoot))
    'wsl-source-roundtrip' = @('wsl.exe', 0, @('--exec', 'wslpath', '-w', $wslSourceRaw))
    'wsl-workspace-roundtrip' = @('wsl.exe', 0, @('--exec', 'wslpath', '-w', $wslWorkspaceRaw))
    'install' = @('wsl.exe', 0, @('--exec', 'bash', "$wslSourceRaw/open-harnesses/install.sh", $wslWorkspaceRaw))
    'skills-list' = @('ferric.exe', 0, @('skills', 'list', '--workspace', $workspaceRoot))
    'check-book' = @('wsl.exe', 1, @('--cd', $wslWorkspaceRaw, '--exec', 'bash', 'scripts/check-book.sh'))
    'current-phase' = @('wsl.exe', 0, @('--cd', $wslWorkspaceRaw, '--exec', 'bash', 'scripts/current-phase.sh'))
    'workspace-status' = @('git', 0, @('-c', "safe.directory=$workspaceSafe", '-C', $workspaceRoot, 'status', '--porcelain=v1'))
}

$commandSetExact = (($commandsByName.Keys | Sort-Object) -join "`n") -eq
    (($expectedCommands.Keys | Sort-Object) -join "`n")
$commandSpecsExact = $commandSetExact
$streamPaths = @{}
if ($commandSpecsExact) {
    foreach ($expected in $expectedCommands.GetEnumerator()) {
        $record = $commandsByName[$expected.Key]
        $spec = $expected.Value
        if ([string]$record.executable -ne [string]$spec[0] -or
            [int]$record.exit_code -ne [int]$spec[1] -or
            [int]$record.timeout_milliseconds -ne 120000 -or
            $record.timed_out -ne $false -or
            -not (Test-StringArrayExact @($record.arguments) @($spec[2]))) {
            $commandSpecsExact = $false
        }
        foreach ($stream in @('stdout', 'stderr')) {
            $fileProperty = "${stream}_file"
            $hashProperty = "${stream}_sha256"
            $expectedPath = "evidence/$($expected.Key).$stream.txt"
            $actualPath = [string]$record.$fileProperty
            if ($actualPath -ne $expectedPath -or $streamPaths.ContainsKey($actualPath)) {
                $commandSpecsExact = $false
                continue
            }
            $streamPaths[$actualPath] = $true
            $fileName = "$($expected.Key).$stream.txt"
            $listedKey = $fileName.ToLowerInvariant()
            if (-not $listed.ContainsKey($listedKey) -or
                [string]$record.$hashProperty -ne $listed[$listedKey].hash) {
                $commandSpecsExact = $false
            }
        }
    }
}
$streamCoverageExact = $streamPaths.Count -eq 34

$sourceCommitRaw = (Read-Evidence 'source-commit.stdout.txt').Trim()
$sourceTreeRaw = (Read-Evidence 'source-tree.stdout.txt').Trim()
$sourceRemoteRaw = (Read-Evidence 'source-remote.stdout.txt').Trim()
$workspaceCommitRaw = (Read-Evidence 'workspace-commit.stdout.txt').Trim()
$workspaceTreeRaw = (Read-Evidence 'workspace-tree.stdout.txt').Trim()
$skillsListRaw = Read-Evidence 'skills-list.stdout.txt'
$checkBookStdoutRaw = Read-Evidence 'check-book.stdout.txt'
$checkBookStderrRaw = Read-Evidence 'check-book.stderr.txt'
$currentPhaseRaw = (Read-Evidence 'current-phase.stdout.txt').Trim()
$installRaw = Read-Evidence 'install.stdout.txt'
$sourceStatusEmpty = [string]::IsNullOrEmpty((Read-Evidence 'source-status.stdout.txt')) -and
    [string]::IsNullOrEmpty((Read-Evidence 'source-status.stderr.txt'))
$workspaceStatusEmpty = [string]::IsNullOrEmpty((Read-Evidence 'workspace-status.stdout.txt')) -and
    [string]::IsNullOrEmpty((Read-Evidence 'workspace-status.stderr.txt'))
$successfulStderrEmpty = $true
foreach ($name in $expectedCommands.Keys) {
    if ([int]$expectedCommands[$name][1] -eq 0 -and
        -not [string]::IsNullOrEmpty((Read-Evidence "$name.stderr.txt"))) {
        $successfulStderrEmpty = $false
    }
}

$gated = 'not-runnable-after-packaging-failure'
$ungatedBehavioralLayers = @(@(
    $verdict.capability_layers.authorized,
    $verdict.capability_layers.top_level_instruction_injected,
    $verdict.capability_layers.resource_accessible_natively,
    $verdict.capability_layers.resource_accessible_after_operator_materialization,
    $verdict.capability_layers.helper_tool_exposed,
    $verdict.capability_layers.book_advanced_with_typed_tools,
    $verdict.capability_layers.cross_run_resumed,
    $verdict.capability_layers.git_write_offered,
    $verdict.capability_layers.git_write_attempted,
    $verdict.capability_layers.git_write_succeeded
) | Where-Object { $_ -ne $gated })
$ungatedControlArms = @(@(
    $verdict.controls.provider_request_capture,
    $verdict.controls.native_resource_arm,
    $verdict.controls.assisted_resource_arm,
    $verdict.controls.typed_book_arm,
    $verdict.controls.evidence_ring2_git_arm,
    $verdict.controls.legacy_ring2_git_arm
) | Where-Object { $_ -ne $gated })

$builtinSource = Join-Path $repoRoot 'crates\ferric-tools\src\builtin\mod.rs'
$gitWriteSource = Join-Path $repoRoot 'crates\ferric-tools\src\builtin\git_write.rs'
$querySource = Join-Path $repoRoot 'crates\ferric-cli\src\query.rs'
$ferricPath = Join-Path $repoRoot 'target\release\ferric.exe'
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
$expectedBuiltinRegistrations = @(
    'ReadFile', 'WriteFile', 'EditFile', 'ListDir', 'MovePath', 'MakeDir',
    'SearchFiles', 'DeletePath', 'FindFiles', 'CopyFile', 'MultiEdit',
    'ApplyPatch', 'GitRead', 'GitWrite'
)
$builtinRegistrations = @(
    [regex]::Matches($builtinBlock, 'registry\.register\(Box::new\(([A-Za-z0-9_]+)\)\);') |
        ForEach-Object { $_.Groups[1].Value }
)
$humanRegistrations = @(
    [regex]::Matches($humanBlock, 'registry\.register\(Box::new\(([A-Za-z0-9_]+)\)\);') |
        ForEach-Object { $_.Groups[1].Value }
)
$gitWriteSpecExact = $gitWriteText -match '(?s)name: "git_write"\.to_string\(\).*?ring: 2,' -and
    $gitWriteText.Contains('"enum": ["add", "commit", "checkout", "branch"]') -and
    -not $gitWriteText.Contains('"push"')
$nativeRemoteToolRegistered = @(
    $builtinRegistrations |
        Where-Object { $_ -match '(?i)(remote|push|pull|fetch|github|gitlab)' }
).Count -ne 0
$staticToolFactsExact =
    -not [string]::IsNullOrWhiteSpace($buildRunConfigBlock) -and
    $buildRunConfigBlock.Contains('register_builtin_tools(&mut registry);') -and
    -not $buildRunConfigBlock.Contains('register_human_tools(&mut registry);') -and
    (Test-StringArrayExact $builtinRegistrations $expectedBuiltinRegistrations) -and
    (Test-StringArrayExact $humanRegistrations @('ShellExec', 'ManageTask')) -and
    $queryText.Contains('register_run_checks(&mut config.registry, checks)') -and
    $gitWriteSpecExact -and
    -not $nativeRemoteToolRegistered
$facts = $verdict.static_tool_boundary.facts
$verdictToolFactsExact =
    $facts.query_uses_builtin_registry -eq $true -and
    $facts.query_uses_human_registry -eq $false -and
    (Test-StringArrayExact @($facts.builtin_registered_types) $expectedBuiltinRegistrations) -and
    $facts.builtin_registered_types_exact -eq $true -and
    $facts.conditional_run_check_registration -eq $true -and
    $facts.git_write_registered_for_query -eq $true -and
    [int]$facts.git_write_ring -eq 2 -and
    $facts.shell_exec_registered_for_query -eq $false -and
    $facts.manage_task_registered_for_query -eq $false -and
    (Test-StringArrayExact @($facts.human_registered_types) @('ShellExec', 'ManageTask')) -and
    $facts.human_registered_types_exact -eq $true -and
    $facts.shell_exec_is_human_surface -eq $true -and
    $facts.manage_task_is_human_surface -eq $true -and
    $facts.native_remote_tool_registered -eq $false
$staticSourceHashesExact =
    (Get-Sha256 $builtinSource) -eq [string]$verdict.static_tool_boundary.builtin_registry_sha256 -and
    (Get-Sha256 $gitWriteSource) -eq [string]$verdict.static_tool_boundary.git_write_sha256 -and
    (Get-Sha256 $querySource) -eq [string]$verdict.static_tool_boundary.query_sha256
$remoteAdapterKey = 'scripts/remote-adapter.sh'
$remoteAdapterHashExact = $sourceChecked.map.ContainsKey($remoteAdapterKey) -and
    [string]$sourceChecked.map[$remoteAdapterKey].sha256 -eq
        [string]$verdict.static_tool_boundary.remote_adapter_sha256
$reportText = Get-Content -Raw -LiteralPath $reportPath

$assertions = [ordered]@{
    pinned_verdict_hash = (Get-Sha256 $verdictPath) -eq $pinnedVerdictSha256
    evidence_payload_count = $listed.Count -eq 38
    schema = [string]$verdict.schema -eq 'animus-ferric-s114-sprint-loop-capability-verdict-v1'
    task = [string]$verdict.task -eq 'T-11411'
    packaging_failure = [string]$verdict.result -eq 'packaging_failure'
    source_commit = [string]$verdict.source.commit -eq $expectedSourceCommit
    source_tree = [string]$verdict.source.tree -eq $expectedSourceTree
    source_remote = [string]$verdict.source.repository -eq $expectedSourceRemote
    source_commit_raw = $sourceCommitRaw -eq $expectedSourceCommit
    source_tree_raw = $sourceTreeRaw -eq $expectedSourceTree
    source_remote_raw = $sourceRemoteRaw -eq $expectedSourceRemote
    source_status_raw = $sourceStatusEmpty
    source_manifest_identity =
        [string]$sourceManifest.git_commit -eq $sourceCommitRaw -and
        [string]$sourceManifest.git_tree -eq $sourceTreeRaw -and
        [string]$verdict.source.source_manifest_sha256 -eq
            $listed['source-manifest.json'].hash -and
        [string]$verdict.source.source_content_tree_sha256 -eq
            [string]$sourceManifest.content_tree_sha256
    source_manifest_git_bound = $sourceGitTree.Count -eq 52
    installed_manifest_identity =
        [string]$verdict.installation.installed_manifest_sha256 -eq
            $listed['installed-scripts-manifest.json'].hash
    workspace_manifest_identity =
        [string]$workspaceManifest.git_commit -eq $workspaceCommitRaw -and
        [string]$workspaceManifest.git_tree -eq $workspaceTreeRaw -and
        [string]$verdict.installation.workspace_manifest_sha256 -eq
            $listed['workspace-manifest.json'].hash -and
        [string]$verdict.installation.workspace_content_tree_sha256 -eq
            [string]$workspaceManifest.content_tree_sha256
    workspace_manifest_git_bound = $workspaceGitTree.Count -eq 28
    source_install_workspace_mapping = $installedTreeMatches
    installed_script_count =
        [int]$verdict.installation.installed_script_count -eq 28 -and
        [int]$verdict.installation.pinned_source_script_count -eq 28
    installed_bytes_match = $verdict.installation.installed_bytes_match_pinned_source -eq $true
    workspace_commit_raw = $workspaceCommitRaw -eq [string]$verdict.installation.disposable_git_commit
    workspace_tree_raw = $workspaceTreeRaw -eq [string]$verdict.installation.disposable_git_tree
    workspace_status_raw = $workspaceStatusEmpty
    wsl_paths_raw =
        $wslOutputShapeExact -and
        $wslSourceRaw -match '^/.+/target/s114-experiment/sprint-loop-source$' -and
        $wslWorkspaceRaw -match '^/.+/target/s114-experiment/sprint-loop-workspace$'
    wsl_roundtrip_raw =
        [string]::Equals(
            [System.IO.Path]::GetFullPath($wslSourceRoundtripRaw).TrimEnd('\'),
            [System.IO.Path]::GetFullPath($sourceRoot).TrimEnd('\'),
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        [string]::Equals(
            [System.IO.Path]::GetFullPath($wslWorkspaceRoundtripRaw).TrimEnd('\'),
            [System.IO.Path]::GetFullPath($workspaceRoot).TrimEnd('\'),
            [System.StringComparison]::OrdinalIgnoreCase
        )
    install_raw =
        $installRaw.Contains("removed prior install: $wslWorkspaceRaw/scripts") -and
        $installRaw.Contains("installed: $wslWorkspaceRaw/scripts")
    no_open_harness_skill_package =
        -not $sourceChecked.map.ContainsKey('skill.md') -and
        -not $workspaceChecked.map.ContainsKey('.ferric/skills/sprint-loop/skill.md') -and
        $verdict.packaging.open_harness_root_skill_md_present -eq $false -and
        $verdict.packaging.ferric_skill_root_present -eq $false -and
        $verdict.packaging.ferric_sprint_loop_skill_md_present -eq $false
    no_skill_discovered =
        [string]$verdict.capability_layers.discovered -eq 'no' -and
        $skillsListRaw.Contains('No skills installed') -and
        $skillsListRaw.Contains('.ferric\skills')
    behavioral_layers_gated = $ungatedBehavioralLayers.Count -eq 0
    control_arms_gated = $ungatedControlArms.Count -eq 0
    operator_check_book_raw =
        [string]::IsNullOrEmpty($checkBookStdoutRaw) -and
        $checkBookStderrRaw.Trim() -eq 'check-book: Sprint Loops Book is not initialized' -and
        [int]$verdict.operator_validation.check_book_exit_code -eq 1
    operator_router_uninitialized =
        $currentPhaseRaw -eq 'uninitialized' -and
        [string]$verdict.operator_validation.router_result -eq 'uninitialized'
    qwen_runtime_boundary =
        [string]$verdict.runtime.qwen38_selection -eq 'selected_q4' -and
        $verdict.runtime.behavioral_model_run -eq $false -and
        $verdict.runtime.fallback_control_activated -eq $false -and
        [string]$verdict.runtime.ferric_sha256 -eq $expectedFerricSha256 -and
        (Test-Path -LiteralPath $ferricPath -PathType Leaf) -and
        (Get-Sha256 $ferricPath) -eq $expectedFerricSha256
    static_tool_facts = $staticToolFactsExact
    verdict_tool_facts = $verdictToolFactsExact
    static_source_hashes = $staticSourceHashesExact
    remote_adapter_hash = $remoteAdapterHashExact
    git_write_static_only =
        $verdict.capability_layers.git_write_registered -eq $true -and
        [string]$verdict.capability_layers.git_write_offered -eq $gated
    remote_checkpoint_boundary =
        [string]$verdict.capability_layers.remote_checkpoint -eq
            'no_native_remote_tool_registered' -and
        -not $nativeRemoteToolRegistered
    no_remote_mutation =
        $verdict.operator_validation.remote_mutation_attempted -eq $false
    qwen_fallback_not_activated =
        [string]$verdict.controls.qwen25_fallback_arm -eq
            'not_authorized_qwen38_is_viable'
    command_set_exact = $commandSetExact
    command_specs_and_outputs_exact = $commandSpecsExact
    command_stream_coverage_exact = $streamCoverageExact
    successful_command_stderr_empty = $successfulStderrEmpty
    report_verdict_anchor =
        $reportText.Contains($pinnedVerdictSha256) -and
        $reportText.Contains('cannot yet use') -and
        $reportText.Contains('38 non-self evidence payloads')
}

$failed = @($assertions.GetEnumerator() | Where-Object Value -ne $true)
if ($failed.Count -ne 0) {
    throw "probe verdict assertions failed: $($failed.Name -join ', ')"
}

[pscustomobject]@{
    schema = 'animus-ferric-s114-sprint-loop-verification-v1'
    passed = $true
    manifest_entries = $listed.Count
    assertions = $assertions.Count
    commands = $commandsByName.Count
    result = $verdict.result
} | ConvertTo-Json -Depth 4
