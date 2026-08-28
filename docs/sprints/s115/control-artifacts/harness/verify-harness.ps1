[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$EvidenceRoot,

    [switch]$CheckQuarantine,

    [string]$RepositoryRoot = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$pinnedFrozenManifest = '532cd39a9fec557816929bcf12e5ae539c8a30c0f4c4829a9d6f89b0ca9f358b'
$repoRoot = if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path.TrimEnd('\')
}
else {
    (Resolve-Path -LiteralPath $RepositoryRoot).Path.TrimEnd('\')
}
$liveControlRoot = Join-Path $repoRoot 'docs\sprints\s115\control-artifacts\harness'
$experimentRoot = Join-Path $repoRoot 'target\s114-experiment'
$canonicalRoots = [ordered]@{
    'app-harness' = Join-Path $experimentRoot 'app-harness'
    'self-test-workspaces' = Join-Path $experimentRoot 'self-test-workspaces'
    'app-workspace' = Join-Path $experimentRoot 'app-workspace'
    'launcher-attestation-probe' = Join-Path $experimentRoot 'launcher-attestation-probe'
}

function Get-FullPath([string]$Path) {
    [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
}

function Assert-UnderRepo([string]$Path, [switch]$AllowRepoRoot) {
    $full = Get-FullPath $Path
    if ($AllowRepoRoot -and [string]::Equals(
            $full, $repoRoot, [System.StringComparison]::OrdinalIgnoreCase
        )) {
        return $full
    }
    if (-not $full.StartsWith(
            $repoRoot + '\', [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "path escapes repository root: $full"
    }
    return $full
}

function Test-EntryExists([string]$Path) {
    $full = Get-FullPath $Path
    if ([string]::Equals(
            $full, $repoRoot, [System.StringComparison]::OrdinalIgnoreCase
        )) {
        return $null -ne (Get-Item -Force -LiteralPath $full -ErrorAction SilentlyContinue)
    }
    $parent = [System.IO.Directory]::GetParent($full)
    if ($null -eq $parent -or -not [System.IO.Directory]::Exists($parent.FullName)) {
        return $false
    }
    foreach ($entry in [System.IO.Directory]::EnumerateFileSystemEntries($parent.FullName)) {
        if ([string]::Equals(
                (Get-FullPath $entry), $full,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            return $true
        }
    }
    return $false
}

function Assert-RealDirectory([string]$Path, [string]$Label) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    $item = Get-Item -Force -LiteralPath $full -ErrorAction Stop
    if (-not $item.PSIsContainer -or
        $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Label must be a real non-reparse directory: $full"
    }
    return $full
}

function Assert-RegularFile([string]$Path, [string]$Label) {
    $full = Assert-UnderRepo $Path
    $item = Get-Item -Force -LiteralPath $full -ErrorAction Stop
    if ($item.PSIsContainer -or
        $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw "$Label must be a regular non-reparse file: $full"
    }
    return $full
}

function Get-Sha256([string]$Path) {
    $full = Assert-RegularFile $Path 'hash input'
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $full).Hash.ToLowerInvariant()
}

function Resolve-EvidenceRelative([string]$Relative, [string]$Label) {
    if ([string]::IsNullOrWhiteSpace($Relative) -or
        [System.IO.Path]::IsPathRooted($Relative) -or
        $Relative.Contains('\') -or
        $Relative.Split('/') -contains '..' -or
        $Relative.Split('/') -contains '.') {
        throw "$Label is not a safe evidence-relative path: $Relative"
    }
    $resolved = Get-FullPath (Join-Path $evidenceRootFull ($Relative -replace '/', '\'))
    if (-not $resolved.StartsWith(
            $evidenceRootFull + '\', [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "$Label escapes the evidence root: $Relative"
    }
    return $resolved
}

function Convert-ToRepoRelative([string]$Path) {
    $full = Assert-UnderRepo $Path -AllowRepoRoot
    return ([System.IO.Path]::GetRelativePath($repoRoot, $full) -replace '\\', '/')
}

function Get-LinkTarget([System.IO.FileSystemInfo]$Item) {
    $targets = @($Item.Target)
    if ($targets.Count -eq 0 -or $null -eq $targets[0]) {
        return $null
    }
    return [string]::Join("`u{001f}", [string[]]$targets)
}

function Get-TreeEntriesText([string]$RootPath) {
    $root = Assert-RealDirectory $RootPath 'tree root'
    $entries = [System.Collections.Generic.SortedDictionary[string, object]]::new(
        [System.StringComparer]::Ordinal
    )
    $entries.Add('.', [ordered]@{
            relative_path = '.'; type = 'directory'; size = 0
            sha256 = $null; link_target = $null
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
            if ($relative -eq '.' -or $segments -contains '..' -or
                $segments -contains '.' -or [System.IO.Path]::IsPathRooted($relative) -or
                $hasControl) {
                throw "unsafe tree entry: $($item.FullName)"
            }
            $isReparse = $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            if ($isReparse) {
                $entry = [ordered]@{
                    relative_path = $relative
                    type = if ($item.PSIsContainer) { 'directory_reparse' } else { 'file_reparse' }
                    size = 0
                    sha256 = $null
                    link_target = Get-LinkTarget $item
                }
            }
            elseif ($item.PSIsContainer) {
                $entry = [ordered]@{
                    relative_path = $relative; type = 'directory'; size = 0
                    sha256 = $null; link_target = $null
                }
                $stack.Push($item.FullName)
            }
            else {
                $length = [int64]$item.Length
                $sha = Get-Sha256 $item.FullName
                $rechecked = Get-Item -Force -LiteralPath $item.FullName -ErrorAction Stop
                if ($rechecked.PSIsContainer -or
                    $rechecked.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
                    [int64]$rechecked.Length -ne $length) {
                    throw "file changed type or size during verification: $($item.FullName)"
                }
                $entry = [ordered]@{
                    relative_path = $relative; type = 'regular_file'; size = $length
                    sha256 = $sha; link_target = $null
                }
            }
            $entries.Add($relative, $entry)
        }
    }
    $lines = foreach ($entry in $entries.Values) {
        $entry | ConvertTo-Json -Compress -Depth 5
    }
    return (($lines -join "`n") + "`n")
}

function Assert-ManifestBytes([string]$ManifestPath, [string]$ExpectedSha256 = '') {
    $path = Assert-RegularFile $ManifestPath 'entries manifest'
    if ($ExpectedSha256 -and (Get-Sha256 $path) -ne $ExpectedSha256) {
        throw "entries manifest SHA-256 differs: $path"
    }
    Read-EntriesManifest $path 'entries manifest' | Out-Null
}

function Assert-ExactProperties([object]$Object, [string[]]$Expected, [string]$Label) {
    $actual = @($Object.PSObject.Properties.Name | Sort-Object)
    $wanted = @($Expected | Sort-Object)
    if (($actual -join "`n") -cne ($wanted -join "`n")) {
        throw "$Label property set differs"
    }
}

function Read-EntriesManifest([string]$ManifestPath, [string]$Label) {
    $path = Assert-RegularFile $ManifestPath $Label
    $strictUtf8 = [System.Text.UTF8Encoding]::new($false, $true)
    try {
        $lines = [System.IO.File]::ReadAllLines($path, $strictUtf8)
    }
    catch {
        throw "$Label is not strict UTF-8: $path"
    }
    if ($lines.Count -lt 1) {
        throw "$Label is empty: $path"
    }
    $entries = [System.Collections.Generic.SortedDictionary[string, object]]::new(
        [System.StringComparer]::Ordinal
    )
    $previousPath = $null
    foreach ($line in $lines) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            throw "$Label contains an empty record: $path"
        }
        $document = $null
        try {
            $document = [System.Text.Json.JsonDocument]::Parse($line)
            if ($document.RootElement.ValueKind -ne [System.Text.Json.JsonValueKind]::Object) {
                throw 'record is not an object'
            }
            $propertyNames = [System.Collections.Generic.List[string]]::new()
            $propertySet = [System.Collections.Generic.HashSet[string]]::new(
                [System.StringComparer]::Ordinal
            )
            foreach ($property in $document.RootElement.EnumerateObject()) {
                $propertyNames.Add($property.Name)
                if (-not $propertySet.Add($property.Name)) {
                    throw "duplicate property: $($property.Name)"
                }
            }
            $expectedProperties = @('relative_path', 'type', 'size', 'sha256', 'link_target')
            if (($propertyNames.Count -ne $expectedProperties.Count) -or
                (@($propertyNames | Sort-Object) -join "`n") -cne
                    (@($expectedProperties | Sort-Object) -join "`n")) {
                throw 'record property set differs'
            }
            $sizeValue = [int64]0
            if (-not $document.RootElement.GetProperty('size').TryGetInt64([ref]$sizeValue)) {
                throw 'size is not an Int64 JSON number'
            }
            $entry = $line | ConvertFrom-Json
        }
        catch {
            throw "$Label contains malformed JSON: $($_.Exception.Message)"
        }
        finally {
            if ($null -ne $document) { $document.Dispose() }
        }

        $relative = [string]$entry.relative_path
        $hasControl = $false
        foreach ($character in $relative.ToCharArray()) {
            if ([int]$character -lt 32 -or [int]$character -eq 127) {
                $hasControl = $true
                break
            }
        }
        $segments = $relative.Split('/')
        if ([string]::IsNullOrWhiteSpace($relative) -or $hasControl -or
            $relative.Contains('\') -or [System.IO.Path]::IsPathRooted($relative) -or
            ($relative -ne '.' -and (
                $segments -contains '' -or $segments -contains '.' -or
                $segments -contains '..'
            ))) {
            throw "$Label contains an unsafe relative path: $relative"
        }
        if ($null -ne $previousPath -and
            [System.StringComparer]::Ordinal.Compare($previousPath, $relative) -ge 0) {
            throw "$Label paths are duplicate or not strictly sorted: $relative"
        }
        $type = [string]$entry.type
        if ($type -notin @('directory', 'regular_file', 'directory_reparse', 'file_reparse')) {
            throw "$Label contains an unknown entry type: $type"
        }
        if ($relative -eq '.' -and $type -ne 'directory') {
            throw "$Label root record is not a directory"
        }
        if ($type -eq 'regular_file') {
            if ($sizeValue -lt 0 -or [string]$entry.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
                $null -ne $entry.link_target) {
                throw "$Label has a malformed regular-file record: $relative"
            }
        }
        elseif ($type -eq 'directory') {
            if ($sizeValue -ne 0 -or $null -ne $entry.sha256 -or
                $null -ne $entry.link_target) {
                throw "$Label has a malformed directory record: $relative"
            }
        }
        else {
            $target = [string]$entry.link_target
            $targetHasControl = $false
            foreach ($character in $target.ToCharArray()) {
                if ([int]$character -lt 32 -or [int]$character -eq 127) {
                    $targetHasControl = $true
                    break
                }
            }
            if ($sizeValue -ne 0 -or $null -ne $entry.sha256 -or
                [string]::IsNullOrEmpty($target) -or $targetHasControl) {
                throw "$Label has a malformed reparse record: $relative"
            }
        }
        if (-not $entries.TryAdd($relative, [pscustomobject][ordered]@{
                    relative_path = $relative
                    type = $type
                    size = $sizeValue
                    sha256 = $entry.sha256
                    link_target = $entry.link_target
                })) {
            throw "$Label contains a duplicate path: $relative"
        }
        $previousPath = $relative
    }
    return [pscustomobject]@{
        path = $path
        entries = $entries
        entry_count = $entries.Count
        sha256 = Get-Sha256 $path
    }
}

function Assert-SafeFrozenRelativePath([string]$Relative, [string]$Label) {
    $hasControl = $false
    foreach ($character in $Relative.ToCharArray()) {
        if ([int]$character -lt 32 -or [int]$character -eq 127) {
            $hasControl = $true
            break
        }
    }
    $segments = $Relative.Split('/')
    if ([string]::IsNullOrWhiteSpace($Relative) -or $hasControl -or
        $Relative.Contains('\') -or [System.IO.Path]::IsPathRooted($Relative) -or
        $segments -contains '' -or $segments -contains '.' -or
        $segments -contains '..') {
        throw "$Label is not a safe frozen-copy relative path: $Relative"
    }
    return $Relative
}

function Get-FrozenCopyContract {
    param(
        [Parameter(Mandatory)][object]$PinnedManifest,
        [Parameter(Mandatory)][string]$PinnedManifestPath,
        [Parameter(Mandatory)][string]$PinnedCompanionPath
    )
    Assert-ExactProperties $PinnedManifest @('schema', 'files') 'pinned frozen manifest'
    if ($PinnedManifest.schema -cne 'mh-rs01-frozen-inputs-v1' -or
        @($PinnedManifest.files).Count -ne 30) {
        throw 'pinned frozen manifest schema or file count differs'
    }
    $files = [System.Collections.Generic.SortedDictionary[string, object]]::new(
        [System.StringComparer]::Ordinal
    )
    $directories = [System.Collections.Generic.SortedSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    $null = $directories.Add('.')
    $pinnedRoot = [System.IO.Path]::GetDirectoryName($PinnedManifestPath)
    foreach ($pinnedFile in @($PinnedManifest.files)) {
        Assert-ExactProperties $pinnedFile @('path', 'bytes', 'sha256') `
            'pinned frozen file'
        $relative = Assert-SafeFrozenRelativePath ([string]$pinnedFile.path) `
            'pinned frozen file'
        $sourceItem = Get-Item -LiteralPath (Assert-RegularFile `
                (Join-Path $pinnedRoot ($relative -replace '/', '\')) `
                "live pinned frozen input $relative")
        if ([int64]$pinnedFile.bytes -lt 0 -or
            [string]$pinnedFile.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
            [int64]$sourceItem.Length -ne [int64]$pinnedFile.bytes -or
            (Get-Sha256 $sourceItem.FullName) -cne [string]$pinnedFile.sha256 -or
            -not $files.TryAdd($relative, [pscustomobject]@{
                    bytes = [int64]$pinnedFile.bytes
                    sha256 = [string]$pinnedFile.sha256
                })) {
            throw "pinned frozen file identity is invalid or duplicate: $relative"
        }
        $slash = $relative.LastIndexOf('/')
        while ($slash -gt 0) {
            $null = $directories.Add($relative.Substring(0, $slash))
            $slash = $relative.LastIndexOf('/', $slash - 1)
        }
    }
    foreach ($control in @(
            [pscustomobject]@{
                path = 'frozen-inputs.json'
                source = $PinnedManifestPath
                sha256 = $pinnedFrozenManifest
            },
            [pscustomobject]@{
                path = 'frozen-inputs.sha256'
                source = $PinnedCompanionPath
                sha256 = Get-Sha256 $PinnedCompanionPath
            }
        )) {
        $item = Get-Item -LiteralPath (Assert-RegularFile $control.source `
                "frozen control $($control.path)")
        if (-not $files.TryAdd([string]$control.path, [pscustomobject]@{
                    bytes = [int64]$item.Length
                    sha256 = [string]$control.sha256
                })) {
            throw "duplicate frozen control file: $($control.path)"
        }
    }
    return [pscustomobject]@{
        files = $files
        directories = $directories
        frozen_file_count = 30
        control_file_count = 2
    }
}

function Assert-FrozenCopyInventory {
    param(
        [Parameter(Mandatory)][object]$Inventory,
        [Parameter(Mandatory)][object]$Contract,
        [Parameter(Mandatory)][bool]$AllowGeneratedEvidence,
        [Parameter(Mandatory)][string]$Label
    )
    $expectedCount = $Contract.files.Count + $Contract.directories.Count
    foreach ($directory in $Contract.directories) {
        if (-not $Inventory.entries.ContainsKey($directory)) {
            throw "$Label omits expected directory: $directory"
        }
        $record = $Inventory.entries[$directory]
        if ($record.type -cne 'directory' -or $record.size -ne 0 -or
            $null -ne $record.sha256 -or $null -ne $record.link_target) {
            throw "$Label directory identity differs: $directory"
        }
    }
    foreach ($pair in $Contract.files.GetEnumerator()) {
        if (-not $Inventory.entries.ContainsKey($pair.Key)) {
            throw "$Label omits expected regular file: $($pair.Key)"
        }
        $record = $Inventory.entries[$pair.Key]
        if ($record.type -cne 'regular_file' -or
            [int64]$record.size -ne [int64]$pair.Value.bytes -or
            [string]$record.sha256 -cne [string]$pair.Value.sha256 -or
            $null -ne $record.link_target) {
            throw "$Label regular-file identity differs: $($pair.Key)"
        }
    }
    foreach ($entry in $Inventory.entries.GetEnumerator()) {
        if ($Contract.files.ContainsKey($entry.Key) -or
            $Contract.directories.Contains($entry.Key)) {
            continue
        }
        if (-not $AllowGeneratedEvidence -or
            ($entry.Key -cne 'evidence' -and
                -not $entry.Key.StartsWith('evidence/', [System.StringComparison]::Ordinal)) -or
            $entry.Value.type -notin @('directory', 'regular_file')) {
            throw "$Label contains an unexpected entry: $($entry.Key)"
        }
    }
    if (-not $AllowGeneratedEvidence -and $Inventory.entry_count -ne $expectedCount) {
        throw "$Label does not contain exactly the pinned inputs and two controls"
    }
    if ($AllowGeneratedEvidence -and
        (-not $Inventory.entries.ContainsKey('evidence') -or
            $Inventory.entries['evidence'].type -cne 'directory')) {
        throw "$Label does not contain the generated evidence directory"
    }
}

function Assert-ExactArgv([object]$Command, [string[]]$Expected, [string]$Label) {
    $actual = @($Command.argv)
    if ($actual.Count -ne $Expected.Count) {
        throw "$Label argv count differs"
    }
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        if ([string]$actual[$index] -cne [string]$Expected[$index]) {
            throw "$Label argv differs at index $index"
        }
    }
}

function Get-CommandStdout([object]$Command) {
    $path = Resolve-EvidenceRelative ([string]$Command.stdout) `
        "command $($Command.gate) stdout"
    return Get-Content -Raw -LiteralPath $path
}

function Get-CommandStderr([object]$Command) {
    $path = Resolve-EvidenceRelative ([string]$Command.stderr) `
        "command $($Command.gate) stderr"
    return Get-Content -Raw -LiteralPath $path
}

function ConvertFrom-StrictBase64Utf8([string]$Value, [string]$Label) {
    try {
        $strictUtf8 = [System.Text.UTF8Encoding]::new($false, $true)
        $text = $strictUtf8.GetString([System.Convert]::FromBase64String($Value))
    }
    catch {
        throw "$Label is not strict base64 UTF-8"
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
        if ([string]$Values[$index] -ceq $Value) { return $index }
    }
    return -1
}

function Invoke-IndependentLiveJournalVerification {
    param(
        [Parameter(Mandatory)][string]$LiveHarnessRoot,
        [Parameter(Mandatory)][string]$ExpectedJournalSha256,
        [Parameter(Mandatory)][object]$RetainedAudit,
        [Parameter(Mandatory)][string]$WslRepositoryRoot
    )
    $root = Assert-RealDirectory $LiveHarnessRoot 'independent live journal root'
    $wslRoot = Assert-WslRepositoryPath $WslRepositoryRoot `
        'independent journal WSL repository root'
    $canonicalPrefix = "$wslRoot/target/s114-experiment/app-harness/"
    $journalPath = Join-Path $root 'command-journal.tsv'
    $companionPath = "$journalPath.sha256"
    $journalSha = Get-Sha256 $journalPath
    if ($journalSha -ne $ExpectedJournalSha256 -or
        (Get-Content -Raw -LiteralPath $companionPath).Trim() -notmatch
            ('^' + [regex]::Escape($journalSha) + '(?:\s|$)')) {
        throw 'independent live journal hash/companion verification failed'
    }
    $lines = @(Get-Content -LiteralPath $journalPath)
    $header = "schema`tsequence`tprevious_sha256`tstage_b64`tcwd_b64`targv_b64`texit_code`tstdout_path_b64`tstdout_sha256`tstderr_path_b64`tstderr_sha256`tentry_sha256"
    if ($lines.Count -lt 2 -or $lines[0] -cne $header) {
        throw 'independent live journal header/row count is invalid'
    }
    $previous = '0000000000000000000000000000000000000000000000000000000000000000'
    $outputs = [System.Collections.Generic.List[object]]::new()
    $launchers = [System.Collections.Generic.List[object]]::new()
    $sandboxCount = 0
    $canary = $false
    for ($row = 1; $row -lt $lines.Count; $row++) {
        $fields = $lines[$row].Split("`t")
        if ($fields.Count -ne 12 -or $fields[0] -cne 's114-command-journal-v1' -or
            [int]$fields[1] -ne $row -or $fields[2] -cne $previous -or
            $fields[11] -cne (Get-TextSha256 ($fields[0..10] -join "`t"))) {
            throw "independent journal chain failed at row $row"
        }
        $stage = ConvertFrom-StrictBase64Utf8 $fields[3] "stage row $row"
        $cwd = ConvertFrom-StrictBase64Utf8 $fields[4] "cwd row $row"
        $cwdHasControl = $false
        foreach ($character in $cwd.ToCharArray()) {
            if ([int]$character -lt 32 -or [int]$character -eq 127) {
                $cwdHasControl = $true
                break
            }
        }
        if ($cwdHasControl -or $cwd.Contains('\') -or $cwd -cne $wslRoot) {
            throw "independent journal cwd contract failed at row $row"
        }
        $argv = @()
        if ($fields[5].Length -gt 0) {
            foreach ($encoded in $fields[5].Split(',')) {
                $argv += ConvertFrom-StrictBase64Utf8 $encoded "argv row $row"
            }
        }
        $streamFields = @(
            [pscustomobject]@{ stream = 'stdout'; encoded = $fields[7]; sha = $fields[8] },
            [pscustomobject]@{ stream = 'stderr'; encoded = $fields[9]; sha = $fields[10] }
        )
        $stdoutPath = $null
        foreach ($stream in $streamFields) {
            $original = ConvertFrom-StrictBase64Utf8 $stream.encoded `
                "$($stream.stream) path row $row"
            $hasControl = $false
            foreach ($character in $original.ToCharArray()) {
                if ([int]$character -lt 32 -or [int]$character -eq 127) {
                    $hasControl = $true
                    break
                }
            }
            if ($hasControl -or $original.Contains('\') -or
                -not $original.StartsWith(
                    $canonicalPrefix,
                    [System.StringComparison]::Ordinal
                )) {
                throw "independent journal exact canonical prefix failed at row $row"
            }
            $tail = $original.Substring($canonicalPrefix.Length)
            $segments = $tail.Split('/')
            if ([string]::IsNullOrWhiteSpace($tail) -or $tail.StartsWith('/') -or
                [System.IO.Path]::IsPathRooted($tail) -or $segments -contains '' -or
                $segments -contains '.' -or $segments -contains '..' -or
                @($segments | Where-Object { $_ -ceq 'logs' }).Count -ne 1) {
                throw "independent journal path tail failed at row $row"
            }
            $rebased = Get-FullPath (Join-Path $root ($tail -replace '/', '\'))
            if (-not $rebased.StartsWith(
                    $root + '\', [System.StringComparison]::OrdinalIgnoreCase
                )) {
                throw "independent journal path escaped at row $row"
            }
            $actualSha = Get-Sha256 $rebased
            if ($actualSha -ne [string]$stream.sha) {
                throw "independent journal stream hash failed at row $row"
            }
            if ($stream.stream -eq 'stdout') { $stdoutPath = $rebased }
            $outputs.Add([ordered]@{
                    sequence = $row
                    stage = $stage
                    stream = $stream.stream
                    retained_path = Convert-ToRepoRelative $rebased
                    sha256 = $actualSha
                })
        }
        $bwrapIndexes = @(
            for ($argvIndex = 0; $argvIndex -lt $argv.Count; $argvIndex++) {
                if ($argv[$argvIndex] -ceq 'bwrap') { $argvIndex }
            }
        )
        if ($bwrapIndexes.Count -gt 0) {
            if ($bwrapIndexes.Count -ne 1 -or $argv.Count -lt 8 -or $argv[0] -cne 'timeout') {
                throw "independent sandbox prefix failed at row $row"
            }
            $bwrapIndex = [int]$bwrapIndexes[0]
            $bwrapSeparator = Find-ExactValueIndex ([object[]]$argv) '--' ($bwrapIndex + 1)
            if ($bwrapSeparator -le $bwrapIndex + 1 -or
                $bwrapSeparator + 1 -ge $argv.Count -or
                $argv[$bwrapSeparator + 1] -cne '/usr/bin/prlimit') {
                throw "independent bwrap/prlimit boundary failed at row $row"
            }
            $options = @($argv[($bwrapIndex + 1)..($bwrapSeparator - 1)])
            foreach ($required in @('--unshare-user', '--unshare-pid', '--unshare-net', '--json-status-fd')) {
                if ($options -notcontains $required) {
                    throw "independent bwrap option failed at row $row`: $required"
                }
            }
            $jsonIndex = Find-ExactValueIndex ([object[]]$options) '--json-status-fd' 0
            if ($jsonIndex -lt 0 -or $jsonIndex + 1 -ge $options.Count -or
                $options[$jsonIndex + 1] -cne '3') {
                throw "independent JSON status descriptor failed at row $row"
            }
            $prlimitSeparator = Find-ExactValueIndex `
                ([object[]]$argv) '--' ($bwrapSeparator + 2)
            if ($prlimitSeparator -le $bwrapSeparator + 2 -or
                $prlimitSeparator + 1 -ge $argv.Count) {
                throw "independent prlimit payload boundary failed at row $row"
            }
            $payload = @($argv[($prlimitSeparator + 1)..($argv.Count - 1)])
            $launcherPath = "$stdoutPath.launcher-attestation"
            $launcherLines = @(Get-Content -LiteralPath (Assert-RegularFile $launcherPath 'launcher'))
            if ($launcherLines.Count -lt 1 -or $launcherLines.Count -gt 2) {
                throw "independent launcher line count failed at row $row"
            }
            $child = $launcherLines[0] | ConvertFrom-Json
            foreach ($name in @('child-pid', 'mnt-namespace', 'net-namespace', 'pid-namespace')) {
                if ([int64]$child.$name -le 0) {
                    throw "independent launcher $name failed at row $row"
                }
            }
            if ($child.'mnt-namespace' -eq $child.'net-namespace' -or
                $child.'mnt-namespace' -eq $child.'pid-namespace' -or
                $child.'net-namespace' -eq $child.'pid-namespace') {
                throw "independent launcher namespace equality failed at row $row"
            }
            $exitCode = [int]$fields[6]
            if ($exitCode -notin @(124, 137) -and
                ($launcherLines.Count -ne 2 -or
                    [int](($launcherLines[1] | ConvertFrom-Json).'exit-code') -ne $exitCode)) {
                throw "independent launcher exit failed at row $row"
            }
            $sandboxCount++
            $launchers.Add([ordered]@{
                    sequence = $row
                    stage = $stage
                    retained_path = Convert-ToRepoRelative $launcherPath
                    sha256 = Get-Sha256 $launcherPath
                    child_pid = [int64]$child.'child-pid'
                    mnt_namespace = [int64]$child.'mnt-namespace'
                    net_namespace = [int64]$child.'net-namespace'
                    pid_namespace = [int64]$child.'pid-namespace'
                    unshare_net = $true
                })
            if ($stage -ceq 'preflight-containment' -and $exitCode -eq 0 -and
                $options -contains '--unshare-net' -and
                ($payload -join "`n").Contains('/proc/net/dev') -and
                ($payload -join "`n").Contains('/dev/tcp/198.51.100.1/9')) {
                $canary = $true
            }
        }
        $previous = $fields[11]
    }
    if (-not $canary -or $sandboxCount -lt 1 -or
        [string]$RetainedAudit.wsl_repository_root -cne $wslRoot -or
        [string]$RetainedAudit.journal_cwd_contract -cne 'exact_repository_root' -or
        [string]$RetainedAudit.canonical_stream_prefix -cne $canonicalPrefix -or
        $RetainedAudit.row_count -ne $lines.Count - 1 -or
        $RetainedAudit.referenced_output_count -ne $outputs.Count -or
        $RetainedAudit.sandbox_invocation_count -ne $sandboxCount -or
        (@($RetainedAudit.referenced_outputs_rehashed) | ConvertTo-Json -Compress -Depth 10) -cne
            (@($outputs) | ConvertTo-Json -Compress -Depth 10) -or
        (@($RetainedAudit.launcher_attestations) | ConvertTo-Json -Compress -Depth 10) -cne
            (@($launchers) | ConvertTo-Json -Compress -Depth 10)) {
        throw 'independent journal enumeration differs from retained qualifier audit'
    }
    return [pscustomobject]@{
        row_count = $lines.Count - 1
        output_count = $outputs.Count
        sandbox_count = $sandboxCount
        containment_canary = $canary
    }
}

function Assert-Summary([string]$SummaryRoot, [object]$Result) {
    $summaryPath = Join-Path $SummaryRoot 'self-test-summary.json'
    $companionPath = Join-Path $SummaryRoot 'self-test-summary.sha256'
    $summarySha = Get-Sha256 $summaryPath
    if ((Get-Content -Raw -LiteralPath $companionPath).Trim() -ne $summarySha -or
        $summarySha -ne [string]$Result.self_test.summary_sha256) {
        throw 'self-test summary or companion hash differs'
    }
    $summary = Get-Content -Raw -LiteralPath $summaryPath | ConvertFrom-Json
    Assert-ExactProperties $summary @(
        'schema', 'status', 'tests', 'baseline', 'known_good_dimensions',
        'deterministic_grade_replay', 'output_spoof_resistance',
        'trusted_failure_exit_mapping', 'inherited_resource_override_rejection',
        'broken_early_journal_chain_rejection', 'violation_matrix',
        'journal_snapshot', 'journal_snapshot_companion', 'journal_sha256',
        'journal_snapshot_companion_sha256', 'grader_binary_sha256',
        'grader_source_tree_sha256', 'frozen_input_manifest_sha256'
    ) 'self-test summary'
    if ($summary.schema -ne 's114-harness-self-test-v1' -or $summary.status -ne 'pass' -or
        $summary.tests.mh_rs01_seed_baseline_and_immutability -ne 'pass' -or
        $summary.tests.bubblewrap_execution_boundary_canaries -ne 'pass' -or
        $summary.tests.grader_known_good_and_violation_matrix -ne 'pass' -or
        $summary.baseline.exit_code -ne 101 -or $summary.baseline.e0583_count -ne 3 -or
        $summary.baseline.other_rust_error_codes -ne 0 -or
        $summary.violation_matrix.static_cases -ne 13 -or
        $summary.violation_matrix.dynamic_cases -ne 5 -or
        $summary.violation_matrix.dynamic_dimensions -ne 4 -or
        $summary.violation_matrix.model_registration_cases -ne 2 -or
        $summary.frozen_input_manifest_sha256 -ne $pinnedFrozenManifest) {
        throw 'self-test summary values differ from the frozen contract'
    }
    foreach ($name in @(
            'deterministic_grade_replay', 'output_spoof_resistance',
            'trusted_failure_exit_mapping', 'inherited_resource_override_rejection',
            'broken_early_journal_chain_rejection'
        )) {
        if ($summary.$name -ne 'pass') { throw "self-test field did not pass: $name" }
    }
    foreach ($name in @(
            'seed_immutability', 'dependency_policy', 'path_policy', 'plan',
            'model_tests', 'visible_contract', 'hidden_contract', 'cli_contract',
            'source_safety'
        )) {
        if ($summary.known_good_dimensions.$name -ne 'pass') {
            throw "self-test dimension did not pass: $name"
        }
    }
    if ($summary.journal_sha256 -ne [string]$Result.self_test.journal_sha256) {
        throw 'summary journal hash differs from qualification result'
    }
    $snapshot = Join-Path $SummaryRoot ([string]$summary.journal_snapshot -replace '/', '\')
    $snapshotCompanion = Join-Path $SummaryRoot ([string]$summary.journal_snapshot_companion -replace '/', '\')
    if ((Get-Sha256 $snapshot) -ne [string]$summary.journal_sha256 -or
        (Get-Sha256 $snapshotCompanion) -ne [string]$summary.journal_snapshot_companion_sha256) {
        throw 'self-test snapshot hashes differ from summary'
    }
    return $summary
}

function Assert-ControlSourceStatic([string]$QualifierPath) {
    $qualifier = Assert-RegularFile $QualifierPath 'retained qualifier source'
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $qualifier, [ref]$tokens, [ref]$errors
    )
    if ($errors.Count -ne 0) {
        throw 'qualifier does not parse cleanly'
    }
    $commands = @($ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst]
            }, $true))
    foreach ($command in $commands) {
        $name = $command.GetCommandName()
        if ($name -in @(
                'Remove-Item', 'Clear-Content', 'del', 'erase', 'rd', 'ri',
                'rmdir', 'rm'
            )) {
            throw "qualifier contains forbidden destructive command: $name"
        }
        if ($name -eq 'Copy-Item') {
            $recursive = @($command.CommandElements | Where-Object {
                    $_ -is [System.Management.Automation.Language.CommandParameterAst] -and
                    $_.ParameterName -eq 'Recurse'
                })
            if ($recursive.Count -gt 0) {
                throw 'qualifier contains forbidden recursive Copy-Item'
            }
        }
        if ($name -eq 'New-Item') {
            $literalPath = @($command.CommandElements | Where-Object {
                    $_ -is [System.Management.Automation.Language.CommandParameterAst] -and
                    $_.ParameterName -eq 'LiteralPath'
                })
            if ($literalPath.Count -gt 0) {
                throw 'qualifier contains unsupported New-Item -LiteralPath'
            }
        }
        $text = $command.Extent.Text
        if ($name -in @('git', 'git.exe') -and
            $text -match '(?i)(?:\s|["''])((clean)|(reset)|(checkout))(?=\s|["''])') {
            throw 'qualifier contains a forbidden mutating Git subcommand'
        }
    }
    $deleteMembers = @($ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -and
                [string]$node.Member.Value -eq 'Delete' -and
                $node.Expression.Extent.Text -match '(?i)(System\.IO\.(File|Directory)|FileInfo|DirectoryInfo)'
            }, $true))
    if ($deleteMembers.Count -gt 0) {
        throw 'qualifier contains a forbidden .NET file or directory delete call'
    }
    $source = Get-Content -Raw -LiteralPath $qualifier
    foreach ($required in @(
            'finally {', 'Move-Item -LiteralPath', 'byte_identical_manifests',
            'GIT_CEILING_DIRECTORIES', '--unshare-net', 'WaitForExitAsync',
            's115-preserved-preflight', 'Invoke-LiveHarnessJournalAudit'
        )) {
        if (-not $source.Contains($required)) {
            throw "qualifier static contract is missing: $required"
        }
    }
    if ($source.Contains('--separate-git-dir')) {
        throw 'qualifier contains a forbidden separate-git-dir option'
    }
}

$evidenceRootFull = Assert-RealDirectory (Get-FullPath $EvidenceRoot) 'evidence root'

$filesManifest = Assert-RegularFile (Join-Path $evidenceRootFull 'files.sha256') 'files manifest'
$listed = [System.Collections.Generic.SortedDictionary[string, string]]::new(
    [System.StringComparer]::Ordinal
)
foreach ($line in Get-Content -LiteralPath $filesManifest) {
    if ($line -cnotmatch '^([0-9a-f]{64})  ([^\r\n]+)$') {
        throw "malformed files.sha256 line: $line"
    }
    $relative = $Matches[2]
    $path = Resolve-EvidenceRelative $relative 'files.sha256 entry'
    if ($relative -eq 'files.sha256' -or -not $listed.TryAdd($relative, $Matches[1])) {
        throw "duplicate or self-referential files.sha256 entry: $relative"
    }
    if ((Get-Sha256 $path) -ne $Matches[1]) {
        throw "retained evidence hash differs: $relative"
    }
}
$actualFiles = [System.Collections.Generic.SortedDictionary[string, string]]::new(
    [System.StringComparer]::Ordinal
)
$stack = [System.Collections.Generic.Stack[string]]::new()
$stack.Push($evidenceRootFull)
while ($stack.Count -gt 0) {
    $directory = $stack.Pop()
    foreach ($child in [System.IO.Directory]::EnumerateFileSystemEntries($directory)) {
        $item = Get-Item -Force -LiteralPath $child -ErrorAction Stop
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "retained compact evidence contains a reparse point: $($item.FullName)"
        }
        if ($item.PSIsContainer) {
            $stack.Push($item.FullName)
        }
        elseif ($item.Name -ne 'files.sha256') {
            $relative = [System.IO.Path]::GetRelativePath($evidenceRootFull, $item.FullName) -replace '\\', '/'
            $actualFiles.Add($relative, (Get-Sha256 $item.FullName))
        }
    }
}
if (($listed.Keys -join "`n") -cne ($actualFiles.Keys -join "`n")) {
    throw 'files.sha256 does not describe the exact retained evidence file set'
}

$resultPath = Join-Path $evidenceRootFull 'result.json'
$resultHashPath = Join-Path $evidenceRootFull 'result.sha256'
$resultSha = Get-Sha256 $resultPath
if ((Get-Content -Raw -LiteralPath $resultHashPath).Trim() -cne "$resultSha  result.json") {
    throw 'result.sha256 does not match result.json'
}
$result = Get-Content -Raw -LiteralPath $resultPath | ConvertFrom-Json
if ($result.schema -ne 's115-harness-qualification-v1' -or
    $result.task -ne 'T-11502' -or $result.status -ne 'pass' -or
    [string]$result.attempt -cnotmatch '^\d{3}$' -or
    $result.frozen_harness.frozen_manifest_sha256 -ne $pinnedFrozenManifest -or
    $result.frozen_harness.frozen_file_count -ne 30 -or
    $result.frozen_harness.frozen_seed_file_count -ne 5 -or
    -not $result.frozen_harness.frozen_seed_hash_parity -or
    -not $result.frozen_harness.copied_inputs_unchanged_after_selftest -or
    $result.frozen_harness.depth_components_to_repo -ne 6 -or
    $result.frozen_harness.host_depth_probe -ne 'pass' -or
    $result.frozen_harness.wsl_depth_probe -ne 'pass' -or
    -not $result.frozen_harness.tracked_source_byte_identical -or
    -not $result.frozen_harness.tracked_git_status_unchanged -or
    -not $result.frozen_harness.complete_tracked_worktree_git_effect_unchanged -or
    [string]$result.frozen_harness.tracked_worktree_status_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    [string]$result.frozen_harness.tracked_worktree_diff_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    [string]$result.frozen_harness.tracked_worktree_cached_diff_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    -not $result.frozen_harness.repository_head_unchanged -or
    -not $result.frozen_harness.sprint_114_status_unchanged -or
    $result.frozen_harness.known_unrelated_edit_sha256_before -ne
        $result.frozen_harness.known_unrelated_edit_sha256_after) {
    throw 'result frozen-harness attestation is incomplete or invalid'
}
$wslRepositoryRoot = Assert-WslRepositoryPath `
    ([string]$result.frozen_harness.wsl_repository_root) `
    'result WSL repository root'
$rawAttemptExpected = Get-FullPath (
    Join-Path $repoRoot "target\s115-preserved-preflight\attempt-$($result.attempt)"
)
$publishedEvidenceExpected = Get-FullPath (
    Join-Path $liveControlRoot "attempts\$($result.attempt)"
)
$prePublicationEvidenceExpected = Join-Path $rawAttemptExpected 'evidence'
if (-not [string]::Equals(
        $evidenceRootFull,
        $publishedEvidenceExpected,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -and -not [string]::Equals(
        $evidenceRootFull,
        $prePublicationEvidenceExpected,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw 'EvidenceRoot basename/location is not bound to result.attempt'
}
if ([string]$result.preservation.target_attempt -cne
    "target/s115-preserved-preflight/attempt-$($result.attempt)") {
    throw 'result target attempt is not bound to its numeric attempt'
}

$provenancePath = Join-Path $evidenceRootFull 'control-provenance.json'
$provenance = Get-Content -Raw -LiteralPath $provenancePath | ConvertFrom-Json
$expectedControlNames = @(
    'README.md', 'qualify-harness.ps1', 'test-harness-control.ps1',
    'verify-harness.ps1'
)
if ($provenance.schema -ne 's115-harness-control-provenance-v1' -or
    (@($provenance.files.name | Sort-Object) -join "`n") -cne
        (@($expectedControlNames | Sort-Object) -join "`n") -or
    (@($result.control_provenance.files.name | Sort-Object) -join "`n") -cne
        (@($expectedControlNames | Sort-Object) -join "`n")) {
    throw 'control provenance does not name the exact four control files'
}
foreach ($record in @($provenance.files)) {
    $resultRecord = @($result.control_provenance.files | Where-Object { $_.name -ceq $record.name })
    if ($resultRecord.Count -ne 1 -or
        $resultRecord[0].sha256 -ne $record.sha256 -or
        $resultRecord[0].bytes -ne $record.bytes -or
        $record.retained -cne "control/$($record.name)" -or
        $record.source -cne "docs/sprints/s115/control-artifacts/harness/$($record.name)") {
        throw "control provenance result differs: $($record.name)"
    }
    $retainedControl = Resolve-EvidenceRelative $record.retained 'retained control file'
    if ((Get-Sha256 $retainedControl) -ne [string]$record.sha256 -or
        (Get-Item -LiteralPath $retainedControl).Length -ne [int64]$record.bytes) {
        throw "retained control file differs: $($record.name)"
    }
}
$retainedQualifier = Resolve-EvidenceRelative 'control/qualify-harness.ps1' 'retained qualifier'
Assert-ControlSourceStatic $retainedQualifier

$trackedBefore = Resolve-EvidenceRelative 'tracked-harness.before.entries.jsonl' `
    'tracked harness before manifest'
$trackedAfter = Resolve-EvidenceRelative 'tracked-harness.after.entries.jsonl' `
    'tracked harness after manifest'
if ((Get-Sha256 $trackedBefore) -ne [string]$result.frozen_harness.tracked_source_before_entries_sha256 -or
    (Get-Sha256 $trackedAfter) -ne [string]$result.frozen_harness.tracked_source_after_entries_sha256 -or
    (Get-Sha256 $trackedBefore) -ne (Get-Sha256 $trackedAfter) -or
    (Get-Content -Raw -LiteralPath $trackedBefore) -cne
        (Get-Content -Raw -LiteralPath $trackedAfter)) {
    throw 'tracked Sprint 114 harness before/after manifests are not byte-identical'
}

$pinnedManifestPath = Join-Path $repoRoot `
    'docs\sprints\s114\control-artifacts\app-harness\frozen-inputs.json'
$pinnedCompanionPath = Join-Path $repoRoot `
    'docs\sprints\s114\control-artifacts\app-harness\frozen-inputs.sha256'
if ((Get-Sha256 $pinnedManifestPath) -ne $pinnedFrozenManifest -or
    (Get-Content -Raw -LiteralPath $pinnedCompanionPath).Trim() -ne $pinnedFrozenManifest) {
    throw 'live frozen manifest no longer matches the pinned identity'
}
$pinnedManifestObject = Get-Content -Raw -LiteralPath $pinnedManifestPath | ConvertFrom-Json
$frozenContract = Get-FrozenCopyContract -PinnedManifest $pinnedManifestObject `
    -PinnedManifestPath $pinnedManifestPath -PinnedCompanionPath $pinnedCompanionPath
$frozenCopyRecord = Get-Content -Raw -LiteralPath (
    Join-Path $evidenceRootFull 'frozen-copy.json'
) | ConvertFrom-Json
if ($pinnedManifestObject.schema -ne 'mh-rs01-frozen-inputs-v1' -or
    @($pinnedManifestObject.files).Count -ne 30 -or
    $frozenCopyRecord.schema -ne 's115-frozen-harness-copy-v1' -or
    $frozenCopyRecord.frozen_manifest_sha256 -ne $pinnedFrozenManifest -or
    $frozenCopyRecord.frozen_file_count -ne 30 -or
    $frozenCopyRecord.frozen_seed_file_count -ne 5 -or
    $frozenCopyRecord.control_file_count -ne 2 -or
    $frozenCopyRecord.initial_tree_entries -cne 'frozen-copy.initial.entries.jsonl' -or
    [string]$frozenCopyRecord.initial_tree_entries_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    [string]$result.frozen_harness.copied_final_inputs_entries_sha256 -cnotmatch
        '^[0-9a-f]{64}$') {
    throw 'frozen-copy record schema or counts differ from pinned inputs'
}
foreach ($pinnedFile in @($pinnedManifestObject.files)) {
    $copyFile = @($frozenCopyRecord.files | Where-Object { $_.path -ceq $pinnedFile.path })
    if ($copyFile.Count -ne 1 -or $copyFile[0].bytes -ne $pinnedFile.bytes -or
        $copyFile[0].sha256 -ne $pinnedFile.sha256) {
        throw "frozen-copy identity differs from pinned manifest: $($pinnedFile.path)"
    }
}
if ((@($frozenCopyRecord.files.path | Sort-Object) -join "`n") -cne
    (@($pinnedManifestObject.files.path | Sort-Object) -join "`n") -or
    @($frozenCopyRecord.seed_files).Count -ne 5 -or
    @($frozenCopyRecord.seed_files | Where-Object {
            -not ([string]$_.path).StartsWith('seed/', [System.StringComparison]::Ordinal)
        }).Count -ne 0) {
    throw 'frozen-copy file or seed path set differs from pinned manifest'
}
$initialInventoryPath = Resolve-EvidenceRelative `
    ([string]$frozenCopyRecord.initial_tree_entries) 'initial frozen-copy inventory'
$initialInventory = Read-EntriesManifest $initialInventoryPath `
    'initial frozen-copy inventory'
if ($initialInventory.sha256 -cne [string]$frozenCopyRecord.initial_tree_entries_sha256) {
    throw 'initial frozen-copy inventory hash differs from its copy record'
}
Assert-FrozenCopyInventory -Inventory $initialInventory -Contract $frozenContract `
    -AllowGeneratedEvidence $false -Label 'initial frozen-copy inventory'
$finalInputsPath = Resolve-EvidenceRelative 'frozen-copy.final-inputs.entries.jsonl' `
    'final frozen-copy input inventory'
$finalInputsInventory = Read-EntriesManifest $finalInputsPath `
    'final frozen-copy input inventory'
if ($finalInputsInventory.sha256 -cne
    [string]$result.frozen_harness.copied_final_inputs_entries_sha256) {
    throw 'final frozen-copy input inventory hash differs from result.json'
}
Assert-FrozenCopyInventory -Inventory $finalInputsInventory -Contract $frozenContract `
    -AllowGeneratedEvidence $true -Label 'final frozen-copy input inventory'
if ($result.self_test.status -ne 'pass' -or $result.self_test.timed_out -or
    $result.self_test.exit_code -ne 0 -or $result.self_test.timeout_seconds -ne 1800 -or
    $result.self_test.bubblewrap_network_disabled_canaries -ne 'pass') {
    throw 'result self-test attestation is invalid'
}

$commands = @()
foreach ($line in Get-Content -LiteralPath (Join-Path $evidenceRootFull 'journal.jsonl')) {
    $commands += $line | ConvertFrom-Json
}
$byGate = @{}
foreach ($command in $commands) {
    if ($command.schema -ne 's115-harness-command-v1' -or $byGate.ContainsKey($command.gate)) {
        throw 'qualification command journal has an invalid schema or duplicate gate'
    }
    $byGate[$command.gate] = $command
    foreach ($stream in @('stdout', 'stderr')) {
        $path = Resolve-EvidenceRelative ([string]$command.$stream) "command $($command.gate) $stream"
        if ((Get-Sha256 $path) -ne [string]$command."${stream}_sha256" -or
            (Get-Item -LiteralPath $path).Length -ne [int64]$command."${stream}_bytes") {
            throw "command stream differs: $($command.gate) $stream"
        }
    }
    if ($command.timed_out) { throw "qualification command timed out: $($command.gate)" }
    if ([string]$command.file -in @('git', 'git.exe') -and
        -not $command.inherited_git_environment_cleared) {
        throw "Git environment was not cleared for gate: $($command.gate)"
    }
    $argvText = @($command.argv) -join "`n"
    if ($argvText.Contains('--separate-git-dir') -or
        @($command.argv) -contains 'clean' -or @($command.argv) -contains 'reset' -or
        @($command.argv) -contains 'checkout') {
        throw "forbidden Git surface appears in command journal: $($command.gate)"
    }
}
$requiredGates = @(
    'repository-head-before', 'repository-tree-before', 's114-status-before',
    'tracked-worktree-status-before', 'tracked-worktree-diff-before',
    'tracked-worktree-cached-diff-before',
    'tracked-harness-status-before', 'wsl-depth-probe', 'frozen-harness-self-test',
    'tracked-worktree-status-after', 'tracked-worktree-diff-after',
    'tracked-worktree-cached-diff-after',
    'tracked-harness-status-after', 'repository-head-after', 'repository-tree-after',
    's114-status-after', 'git-init-external-metadata', 'git-explicit-config', 'git-explicit-add',
    'git-explicit-commit', 'git-explicit-head', 'git-explicit-tree',
    'git-explicit-toplevel', 'git-explicit-status', 'git-explicit-git-dir',
    'git-ambient-discovery-blocked'
)
if ((@($byGate.Keys | Sort-Object) -join "`n") -cne (@($requiredGates | Sort-Object) -join "`n")) {
    throw 'qualification command journal gate set differs from the fixed control'
}
foreach ($gate in $requiredGates) {
    $expectedExit = if ($gate -eq 'git-ambient-discovery-blocked') { $null } else { 0 }
    if ($null -ne $expectedExit -and [int]$byGate[$gate].exit_code -ne $expectedExit) {
        throw "gate did not exit zero: $gate"
    }
}
if ([int]$byGate['git-ambient-discovery-blocked'].exit_code -eq 0 -or
    @($byGate['frozen-harness-self-test'].argv).Count -ne 3 -or
    $byGate['frozen-harness-self-test'].argv[0] -cne '--exec' -or
    $byGate['frozen-harness-self-test'].argv[1] -cne 'bash' -or
    $byGate['frozen-harness-self-test'].argv[2] -cne
        "target/s115-preserved-preflight/attempt-$($result.attempt)/frozen/app-harness/scripts/self-test.sh") {
    throw 'self-test invocation or ambient Git negative differs from the fixed contract'
}

$repositoryGitArgv = [ordered]@{
    'repository-head-before' = @('-C', $repoRoot, 'rev-parse', 'HEAD')
    'repository-head-after' = @('-C', $repoRoot, 'rev-parse', 'HEAD')
    'repository-tree-before' = @('-C', $repoRoot, 'rev-parse', 'HEAD^{tree}')
    'repository-tree-after' = @('-C', $repoRoot, 'rev-parse', 'HEAD^{tree}')
    's114-status-before' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
        'docs/sprints/s114'
    )
    's114-status-after' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
        'docs/sprints/s114'
    )
    'tracked-harness-status-before' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
        'docs/sprints/s114/control-artifacts/app-harness'
    )
    'tracked-harness-status-after' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--',
        'docs/sprints/s114/control-artifacts/app-harness'
    )
    'tracked-worktree-status-before' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=no'
    )
    'tracked-worktree-status-after' = @(
        '-C', $repoRoot, 'status', '--porcelain=v1', '--untracked-files=no'
    )
    'tracked-worktree-diff-before' = @(
        '-C', $repoRoot, 'diff', '--no-ext-diff', '--binary', '--'
    )
    'tracked-worktree-diff-after' = @(
        '-C', $repoRoot, 'diff', '--no-ext-diff', '--binary', '--'
    )
    'tracked-worktree-cached-diff-before' = @(
        '-C', $repoRoot, 'diff', '--cached', '--no-ext-diff', '--binary', '--'
    )
    'tracked-worktree-cached-diff-after' = @(
        '-C', $repoRoot, 'diff', '--cached', '--no-ext-diff', '--binary', '--'
    )
}
foreach ($gate in $repositoryGitArgv.Keys) {
    $command = $byGate[$gate]
    if ($command.file -cne 'git.exe' -or $command.working_directory -cne '.' -or
        -not $command.inherited_git_environment_cleared -or $command.git_locale -cne 'C' -or
        @($command.environment.PSObject.Properties.Name).Count -ne 0) {
        throw "repository Git executable/cwd/environment differs: $gate"
    }
    Assert-ExactArgv $command $repositoryGitArgv[$gate] $gate
}

$depthScript = 'set -Eeuo pipefail; scripts=$1; actual=$(cd -- "$scripts/../../../../../.." && pwd -P); expected=$(pwd -P); test "$actual" = "$expected"; printf "%s\n" "$actual"'
$depthScriptsRelative = "target/s115-preserved-preflight/attempt-$($result.attempt)/frozen/app-harness/scripts"
$depthCommand = $byGate['wsl-depth-probe']
if ($depthCommand.file -cne 'wsl.exe' -or $depthCommand.working_directory -cne '.' -or
    $depthCommand.inherited_git_environment_cleared -or
    @($depthCommand.environment.PSObject.Properties.Name).Count -ne 0) {
    throw 'WSL depth-probe executable/cwd/environment differs'
}
Assert-ExactArgv $depthCommand @(
    '--exec', 'bash', '-c', $depthScript, '_', $depthScriptsRelative
) 'wsl-depth-probe'
if ((Get-CommandStdout $depthCommand) -cne "$wslRepositoryRoot`n" -or
    -not [string]::IsNullOrEmpty((Get-CommandStderr $depthCommand))) {
    throw 'WSL depth-probe output does not bind the exact repository prefix'
}

$candidateRoot = Join-Path $experimentRoot 'app-workspace'
$metadataRoot = Join-Path $experimentRoot 'launcher-attestation-probe'
$gitRoots = @("--git-dir=$metadataRoot", "--work-tree=$candidateRoot")
$commonGit = @($gitRoots + @('-c', 'core.bare=false'))
$expectedGitArgv = [ordered]@{
    'git-init-external-metadata' = @($gitRoots + @('init'))
    'git-explicit-config' = @($gitRoots + @('config', '--get', 'core.bare'))
    'git-explicit-add' = @($commonGit + @('add', '--all'))
    'git-explicit-commit' = @($commonGit + @(
            '-c', 'user.name=Animus Ferric Harness',
            '-c', 'user.email=example@example.invalid',
            'commit', '-m', 'standalone probe'
        ))
    'git-explicit-head' = @($commonGit + @('rev-parse', 'HEAD'))
    'git-explicit-tree' = @($commonGit + @('rev-parse', 'HEAD^{tree}'))
    'git-explicit-toplevel' = @($commonGit + @('rev-parse', '--show-toplevel'))
    'git-explicit-status' = @($commonGit + @('status', '--porcelain=v1', '--untracked-files=all'))
    'git-explicit-git-dir' = @($commonGit + @('rev-parse', '--absolute-git-dir'))
    'git-ambient-discovery-blocked' = @('-C', $candidateRoot, 'rev-parse', '--show-toplevel')
}
foreach ($gate in $expectedGitArgv.Keys) {
    $command = $byGate[$gate]
    if ($command.file -cne 'git.exe' -or $command.working_directory -cne '.' -or
        -not $command.inherited_git_environment_cleared -or $command.git_locale -cne 'C') {
        throw "Git command executable/cwd/environment differs: $gate"
    }
    Assert-ExactArgv $command $expectedGitArgv[$gate] $gate
    $environmentNames = @($command.environment.PSObject.Properties.Name)
    if ($gate -eq 'git-ambient-discovery-blocked') {
        if ($environmentNames.Count -ne 1 -or
            $environmentNames[0] -cne 'GIT_CEILING_DIRECTORIES' -or
            [string]$command.environment.GIT_CEILING_DIRECTORIES -cne $experimentRoot) {
            throw 'ambient Git negative has an inexact ceiling environment'
        }
    }
    elseif ($environmentNames.Count -ne 0) {
        throw "positive Git command has unexpected explicit environment: $gate"
    }
}
$initStdout = Get-CommandStdout $byGate['git-init-external-metadata']
$configStdout = (Get-CommandStdout $byGate['git-explicit-config']).Trim()
$addStdout = Get-CommandStdout $byGate['git-explicit-add']
$commitStdout = Get-CommandStdout $byGate['git-explicit-commit']
$headStdout = (Get-CommandStdout $byGate['git-explicit-head']).Trim()
$treeStdout = (Get-CommandStdout $byGate['git-explicit-tree']).Trim()
$topStdout = (Get-CommandStdout $byGate['git-explicit-toplevel']).Trim()
$statusStdout = Get-CommandStdout $byGate['git-explicit-status']
$gitDirStdout = (Get-CommandStdout $byGate['git-explicit-git-dir']).Trim()
$ambientStdout = Get-CommandStdout $byGate['git-ambient-discovery-blocked']
$ambientStderr = Get-CommandStderr $byGate['git-ambient-discovery-blocked']
if ($initStdout -notmatch '(?m)^Initialized empty Git repository in .+[\\/]launcher-attestation-probe[\\/]?\s*$' -or
    $configStdout -cne 'false' -or -not [string]::IsNullOrWhiteSpace($addStdout) -or
    $commitStdout -notmatch '(?m)^\[[^]]+ [0-9a-f]+\] standalone probe\s*$' -or
    $headStdout -cne [string]$result.standalone_git.commit -or
    $treeStdout -cne [string]$result.standalone_git.tree -or
    -not [string]::Equals((Get-FullPath $topStdout), $candidateRoot,
        [System.StringComparison]::OrdinalIgnoreCase) -or
    -not [string]::IsNullOrWhiteSpace($statusStdout) -or
    -not [string]::Equals((Get-FullPath $gitDirStdout), $metadataRoot,
        [System.StringComparison]::OrdinalIgnoreCase) -or
    -not [string]::IsNullOrWhiteSpace($ambientStdout) -or
    $ambientStderr -notmatch '(?i)not a git repository') {
    throw 'standalone Git stdout/stderr contract differs'
}
foreach ($pair in @(
        @('repository-head-before', 'repository-head-after'),
        @('repository-tree-before', 'repository-tree-after'),
        @('s114-status-before', 's114-status-after'),
        @('tracked-harness-status-before', 'tracked-harness-status-after'),
        @('tracked-worktree-status-before', 'tracked-worktree-status-after'),
        @('tracked-worktree-diff-before', 'tracked-worktree-diff-after'),
        @('tracked-worktree-cached-diff-before', 'tracked-worktree-cached-diff-after')
    )) {
    if ($byGate[$pair[0]].stdout_sha256 -ne $byGate[$pair[1]].stdout_sha256 -or
        $byGate[$pair[0]].stdout_bytes -ne $byGate[$pair[1]].stdout_bytes -or
        $byGate[$pair[0]].stderr_sha256 -ne $byGate[$pair[1]].stderr_sha256 -or
        $byGate[$pair[0]].stderr_bytes -ne $byGate[$pair[1]].stderr_bytes) {
        throw "before/after command output differs: $($pair[0])"
    }
}
if ($result.frozen_harness.tracked_worktree_status_sha256 -cne
        $byGate['tracked-worktree-status-before'].stdout_sha256 -or
    $result.frozen_harness.tracked_worktree_diff_sha256 -cne
        $byGate['tracked-worktree-diff-before'].stdout_sha256 -or
    $result.frozen_harness.tracked_worktree_cached_diff_sha256 -cne
        $byGate['tracked-worktree-cached-diff-before'].stdout_sha256) {
    throw 'result tracked-worktree Git-effect hashes differ from captured bytes'
}
$knownStatus = (Get-Content -Raw -LiteralPath (
        Resolve-EvidenceRelative $byGate['s114-status-before'].stdout 's114 status'
    )).TrimEnd("`r", "`n")
if ($knownStatus -cne " M $($result.frozen_harness.known_unrelated_edit)") {
    throw 'Sprint 114 status is not exactly the known unrelated edit'
}
if (-not [string]::IsNullOrWhiteSpace((Get-Content -Raw -LiteralPath (
            Resolve-EvidenceRelative $byGate['tracked-harness-status-before'].stdout 'tracked harness status'
        )))) {
    throw 'tracked Sprint 114 harness was not clean'
}

$summary = Assert-Summary (Join-Path $evidenceRootFull 'generated-self-test-evidence') $result
$sourceEvidenceManifest = Resolve-EvidenceRelative `
    'generated-self-test-evidence.source.entries.jsonl' 'generated evidence source manifest'
$retainedEvidenceManifest = Resolve-EvidenceRelative `
    'generated-self-test-evidence.retained.entries.jsonl' 'generated evidence retained manifest'
if ((Get-Sha256 $sourceEvidenceManifest) -ne (Get-Sha256 $retainedEvidenceManifest)) {
    throw 'generated self-test evidence copy manifest differs from source manifest'
}

$gitProbe = $result.standalone_git
if ($gitProbe.schema -ne 's115-standalone-git-probe-v1' -or $gitProbe.status -ne 'pass' -or
    $gitProbe.candidate -ne 'target/s114-experiment/app-workspace' -or
    $gitProbe.metadata -ne 'target/s114-experiment/launcher-attestation-probe' -or
    -not $gitProbe.candidate_dot_git_absent -or -not $gitProbe.metadata_is_sibling -or
    $gitProbe.separate_git_dir_option_used -or -not $gitProbe.explicit_toplevel_matches_candidate -or
    -not $gitProbe.initialization_named_both_roots -or -not $gitProbe.core_bare_false -or
    -not $gitProbe.explicit_git_dir_matches_metadata -or -not $gitProbe.explicit_status_clean -or
    -not $gitProbe.ambient_parent_discovery_blocked -or
    [string]$gitProbe.commit -cnotmatch '^[0-9a-f]{40,64}$' -or
    [string]$gitProbe.tree -cnotmatch '^[0-9a-f]{40,64}$') {
    throw 'standalone Git evidence differs from the fixed sibling-root contract'
}

$expectedRootKeys = @($canonicalRoots.Keys | Sort-Object)
foreach ($batchProperty in @('pre_operations', 'post_operations')) {
    $operations = @($result.preservation.$batchProperty)
    $expectedBatch = if ($batchProperty -eq 'pre_operations') {
        '001-pre-selftest'
    }
    else {
        '003-post-selftest'
    }
    if ($operations.Count -ne 4 -or
        (@($operations.root_key | Sort-Object) -join "`n") -cne ($expectedRootKeys -join "`n")) {
        throw "preservation $batchProperty does not cover the exact four roots"
    }
    foreach ($operation in $operations) {
        $expectedSource = "target/s114-experiment/$($operation.root_key)"
        $expectedDestination = "target/s115-preserved-preflight/attempt-$($result.attempt)/batches/$expectedBatch/roots/$($operation.root_key)"
        if ($operation.batch -cne $expectedBatch -or
            $operation.source -cne $expectedSource -or
            $operation.destination -cne $expectedDestination) {
            throw "preservation endpoint/batch differs: $batchProperty/$($operation.root_key)"
        }
        if (-not $operation.parity) { throw "preservation parity failed: $($operation.root_key)" }
        if ($operation.present) {
            if ($operation.before_manifest -cne
                "batches/$expectedBatch/$($operation.root_key).before.entries.jsonl" -or
                $operation.after_manifest -cne
                "batches/$expectedBatch/$($operation.root_key).after.entries.jsonl") {
                throw "preservation manifest path differs: $batchProperty/$($operation.root_key)"
            }
            $before = Resolve-EvidenceRelative $operation.before_manifest 'before move manifest'
            $after = Resolve-EvidenceRelative $operation.after_manifest 'after move manifest'
            Assert-ManifestBytes $before ([string]$operation.entries_sha256)
            Assert-ManifestBytes $after ([string]$operation.entries_sha256)
            if ((Get-Sha256 $before) -ne (Get-Sha256 $after) -or
                -not $operation.byte_identical_manifests -or -not $operation.same_volume) {
                throw "preservation manifests are not byte-identical: $($operation.root_key)"
            }
        }
    }
}
if ($result.preservation.pre_batch -cne '001-pre-selftest' -or
    $result.preservation.frozen_copy_batch -cne '002-frozen-copy' -or
    $result.preservation.post_batch -cne '003-post-selftest' -or
    -not $result.preservation.copy_operation.present -or
    $result.preservation.copy_operation.batch -cne '002-frozen-copy' -or
    $result.preservation.copy_operation.root_key -cne 'depth-preserving-frozen-copy' -or
    -not $result.preservation.copy_operation.retained_in_place -or
    -not $result.preservation.copy_operation.parity -or
    $result.preservation.copy_operation.destination -ne
        "target/s115-preserved-preflight/attempt-$($result.attempt)/frozen/app-harness" -or
    $result.preservation.copy_operation.source -ne
        "target/s115-preserved-preflight/attempt-$($result.attempt)/frozen/app-harness" -or
    $result.preservation.copy_operation.final_manifest -cne
        'batches/002-frozen-copy/depth-preserving-frozen-copy.final.entries.jsonl' -or
    $result.preservation.copy_operation.entries_sha256 -cne
        $result.frozen_harness.copied_final_inputs_entries_sha256 -or
    $result.preservation.recursive_delete_used -or
    -not $result.preservation.all_moves_same_volume -or
    -not $result.preservation.all_present_move_manifests_byte_identical) {
    throw 'depth-preserving frozen-copy retention or mutation policy is invalid'
}
$computedPreBytes = [int64](@($result.preservation.pre_operations |
        Where-Object { $_.present } |
        Measure-Object -Property regular_file_bytes -Sum).Sum)
$computedPostBytes = [int64](@($result.preservation.post_operations |
        Where-Object { $_.present } |
        Measure-Object -Property regular_file_bytes -Sum).Sum)
if ([int64]$result.preservation.pre_regular_file_bytes -ne $computedPreBytes -or
    [int64]$result.preservation.post_regular_file_bytes -ne $computedPostBytes -or
    [int64]$result.preservation.retained_frozen_copy_regular_file_bytes -ne
        [int64]$result.preservation.copy_operation.regular_file_bytes -or
    [int64]$result.preservation.volume_before.total_bytes -le 0 -or
    [int64]$result.preservation.volume_after.total_bytes -ne
        [int64]$result.preservation.volume_before.total_bytes -or
    [int64]$result.preservation.volume_before.available_free_bytes -lt 0 -or
    [int64]$result.preservation.volume_after.available_free_bytes -lt 0 -or
    [string]$result.preservation.volume_before.volume_root -cne
        [string]$result.preservation.volume_after.volume_root) {
    throw 'preserved regular-file byte totals or volume observations differ'
}
$copyManifest = Resolve-EvidenceRelative $result.preservation.copy_operation.final_manifest `
    'final frozen-copy manifest'
Assert-ManifestBytes $copyManifest ([string]$result.preservation.copy_operation.entries_sha256)

$journalAuditPath = Resolve-EvidenceRelative $result.self_test.live_journal_audit 'live journal audit'
$journalAudit = Get-Content -Raw -LiteralPath $journalAuditPath | ConvertFrom-Json
if ($journalAudit.schema -ne 's115-live-harness-journal-audit-v1' -or
    $journalAudit.status -ne 'pass' -or -not $journalAudit.unshare_net_proven -or
    $journalAudit.wsl_repository_root -cne $wslRepositoryRoot -or
    $journalAudit.journal_cwd_contract -cne 'exact_repository_root' -or
    $journalAudit.canonical_stream_prefix -cne
        "$wslRepositoryRoot/target/s114-experiment/app-harness/" -or
    -not $journalAudit.loopback_only_and_network_connect_negative_proven -or
    $journalAudit.containment_canary_stage -ne 'preflight-containment' -or
    $journalAudit.containment_canary_exit_code -ne 0 -or
    $journalAudit.referenced_output_count -ne @($journalAudit.referenced_outputs_rehashed).Count -or
    $journalAudit.sandbox_invocation_count -ne @($journalAudit.launcher_attestations).Count) {
    throw 'live harness journal audit is incomplete'
}
$link = $result.self_test.live_journal_cross_link
if ($link.status -ne 'pass' -or $link.summary_journal_sha256 -ne $summary.journal_sha256 -or
    $link.live_journal_sha256 -ne $summary.journal_sha256 -or
    -not $link.unshare_net_proven -or $link.referenced_output_count -ne $journalAudit.referenced_output_count) {
    throw 'live journal cross-link differs from summary or output audit'
}

foreach ($key in $canonicalRoots.Keys) {
    if (-not $result.handoff.canonical_roots_absent.$key) {
        throw "result does not attest canonical-root absence: $key"
    }
}
if (-not $result.handoff.ready_for_t11503) {
    throw 'result does not authorize T-11503 handoff'
}

if ($CheckQuarantine) {
    $rawAttempt = $rawAttemptExpected
    Assert-RealDirectory $rawAttempt 'raw preservation attempt' | Out-Null
    if (Test-EntryExists (Join-Path $rawAttempt 'terminal-failure.json')) {
        throw 'raw attempt contains a terminal publication or verification failure record'
    }
    foreach ($key in $canonicalRoots.Keys) {
        if (Test-EntryExists $canonicalRoots[$key]) {
            throw "canonical root is not absent at live handoff: $key"
        }
    }
    foreach ($batchProperty in @('pre_operations', 'post_operations')) {
        foreach ($operation in @($result.preservation.$batchProperty)) {
            if (-not $operation.present) { continue }
            $destination = Join-Path $repoRoot ([string]$operation.destination -replace '/', '\')
            $expectedManifest = Resolve-EvidenceRelative $operation.after_manifest 'quarantine manifest'
            $actualText = Get-TreeEntriesText $destination
            $expectedText = Get-Content -Raw -LiteralPath $expectedManifest
            if ($actualText -cne $expectedText) {
                throw "ignored quarantine rewalk differs: $($operation.root_key)"
            }
        }
    }
    $copyRoot = Join-Path $repoRoot ([string]$result.preservation.copy_operation.destination -replace '/', '\')
    if ((Get-TreeEntriesText $copyRoot) -cne (Get-Content -Raw -LiteralPath $copyManifest)) {
        throw 'retained depth-preserving frozen copy differs on rewalk'
    }
    foreach ($pair in $frozenContract.files.GetEnumerator()) {
        $rawCopiedPath = Join-Path $copyRoot ($pair.Key -replace '/', '\')
        $rawCopiedItem = Get-Item -LiteralPath (Assert-RegularFile $rawCopiedPath `
                "raw frozen-copy file $($pair.Key)")
        if ([int64]$rawCopiedItem.Length -ne [int64]$pair.Value.bytes -or
            (Get-Sha256 $rawCopiedItem.FullName) -cne [string]$pair.Value.sha256) {
            throw "raw frozen-copy file differs from the pinned contract: $($pair.Key)"
        }
    }
    $liveHarnessRoot = Join-Path $rawAttempt `
        'batches\003-post-selftest\roots\app-harness'
    $independentJournal = Invoke-IndependentLiveJournalVerification `
        -LiveHarnessRoot $liveHarnessRoot `
        -ExpectedJournalSha256 ([string]$summary.journal_sha256) `
        -RetainedAudit $journalAudit -WslRepositoryRoot $wslRepositoryRoot
    if ($independentJournal.row_count -ne $journalAudit.row_count -or
        $independentJournal.output_count -ne $journalAudit.referenced_output_count -or
        $independentJournal.sandbox_count -ne $journalAudit.sandbox_invocation_count -or
        -not $independentJournal.containment_canary) {
        throw 'independent live journal result differs from retained audit counts'
    }
    foreach ($output in @($journalAudit.referenced_outputs_rehashed)) {
        $path = Join-Path $repoRoot ([string]$output.retained_path -replace '/', '\')
        if ((Get-Sha256 $path) -ne [string]$output.sha256) {
            throw "live journal output differs on quarantine rewalk: $($output.retained_path)"
        }
    }
    foreach ($launcher in @($journalAudit.launcher_attestations)) {
        $path = Join-Path $repoRoot ([string]$launcher.retained_path -replace '/', '\')
        if ((Get-Sha256 $path) -ne [string]$launcher.sha256 -or
            -not $launcher.unshare_net -or $launcher.child_pid -le 0 -or
            $launcher.mnt_namespace -le 0 -or $launcher.net_namespace -le 0 -or
            $launcher.pid_namespace -le 0) {
            throw "launcher attestation differs on quarantine rewalk: $($launcher.retained_path)"
        }
    }
}

[pscustomobject]@{
    schema = 's115-harness-verification-v1'
    status = 'pass'
    attempt = [string]$result.attempt
    retained_file_count = $listed.Count
    quarantine_rewalked = [bool]$CheckQuarantine
    canonical_roots_absent = $true
    frozen_manifest_sha256 = $pinnedFrozenManifest
    self_test_journal_sha256 = [string]$summary.journal_sha256
} | ConvertTo-Json -Depth 5
