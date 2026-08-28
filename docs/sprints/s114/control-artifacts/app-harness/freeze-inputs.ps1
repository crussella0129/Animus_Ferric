[CmdletBinding(DefaultParameterSetName = 'Write')]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Verify')]
    [switch]$Verify
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'PowerShell 7 or newer is required'
}

$root = $PSScriptRoot
$manifestPath = Join-Path $root 'frozen-inputs.json'
$manifestHashPath = Join-Path $root 'frozen-inputs.sha256'

function Assert-InputTreeHygiene {
    $entries = @(Get-ChildItem -LiteralPath $root -Force -Recurse)
    $reparsePoint = $entries |
        Where-Object {
            ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0
        } |
        Select-Object -First 1
    if ($null -ne $reparsePoint) {
        $relative = [System.IO.Path]::GetRelativePath($root, $reparsePoint.FullName).
            Replace('\', '/')
        throw "operator input tree contains a reparse point: $relative"
    }

    $targetDirectory = $entries |
        Where-Object {
            $_.PSIsContainer -and $_.Name.Equals(
                'target',
                [System.StringComparison]::OrdinalIgnoreCase
            )
        } |
        Select-Object -First 1
    if ($null -ne $targetDirectory) {
        $relative = [System.IO.Path]::GetRelativePath($root, $targetDirectory.FullName).
            Replace('\', '/')
        throw "generated target directory is forbidden in operator inputs: $relative"
    }
}

function Get-InputFiles {
    Get-ChildItem -LiteralPath $root -Force -File -Recurse |
        Where-Object {
            $relative = [System.IO.Path]::GetRelativePath($root, $_.FullName).
                Replace('\', '/')
            $relative -ne 'frozen-inputs.json' -and
                $relative -ne 'frozen-inputs.sha256' -and
                (-not $relative.StartsWith('evidence/', [System.StringComparison]::Ordinal))
        } |
        Sort-Object {
            [System.IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/')
        }
}

function Get-Record {
    param([Parameter(Mandatory = $true)][System.IO.FileInfo]$File)

    [ordered]@{
        path = [System.IO.Path]::GetRelativePath($root, $File.FullName).Replace('\', '/')
        bytes = [UInt64]$File.Length
        sha256 = (Get-FileHash -LiteralPath $File.FullName -Algorithm SHA256).
            Hash.ToLowerInvariant()
    }
}

Assert-InputTreeHygiene

if ($Verify) {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $manifestHashPath -PathType Leaf)) {
        throw 'frozen input manifest is missing'
    }
    $expectedManifestHash = (Get-Content -Raw -LiteralPath $manifestHashPath).Trim()
    $actualManifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).
        Hash.ToLowerInvariant()
    if ($actualManifestHash -ne $expectedManifestHash) {
        throw 'frozen-inputs.json hash mismatch'
    }

    $expected = @(
        (Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json).files
    )
    $actual = @(Get-InputFiles | ForEach-Object { Get-Record -File $_ })
    $expectedJson = $expected | ConvertTo-Json -Depth 5 -Compress
    $actualJson = $actual | ConvertTo-Json -Depth 5 -Compress
    if ($actualJson -cne $expectedJson) {
        throw 'operator input inventory or hash mismatch'
    }
    [ordered]@{
        schema = 'mh-rs01-frozen-input-verification-v1'
        files = $actual.Count
        manifest_sha256 = $actualManifestHash
        verified = $true
    } | ConvertTo-Json -Compress
    exit 0
}

$records = @(Get-InputFiles | ForEach-Object { Get-Record -File $_ })
$manifest = [ordered]@{
    schema = 'mh-rs01-frozen-inputs-v1'
    files = $records
}
$json = ($manifest | ConvertTo-Json -Depth 5).
    Replace("`r`n", "`n").
    Replace("`r", "`n")
$temporaryManifest = "$manifestPath.tmp-$PID"
[System.IO.File]::WriteAllText(
    $temporaryManifest,
    "$json`n",
    [System.Text.UTF8Encoding]::new($false)
)
Move-Item -LiteralPath $temporaryManifest -Destination $manifestPath -Force
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).
    Hash.ToLowerInvariant()
$temporaryHash = "$manifestHashPath.tmp-$PID"
[System.IO.File]::WriteAllText(
    $temporaryHash,
    "$manifestHash`n",
    [System.Text.UTF8Encoding]::new($false)
)
Move-Item -LiteralPath $temporaryHash -Destination $manifestHashPath -Force

[ordered]@{
    schema = 'mh-rs01-frozen-input-write-v1'
    files = $records.Count
    manifest_sha256 = $manifestHash
    written = $true
} | ConvertTo-Json -Compress
