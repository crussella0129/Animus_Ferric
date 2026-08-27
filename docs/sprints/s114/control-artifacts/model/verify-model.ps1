[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [UInt64]$ExpectedBytes,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{64}$')]
    [string]$ExpectedSha256,

    [string]$DisplayPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($DisplayPath)) {
    $DisplayPath = [System.IO.Path]::GetFileName($Path)
}

if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    [ordered]@{
        schema          = 'animus-ferric-model-verification-v1'
        path            = $DisplayPath
        expected_bytes  = $ExpectedBytes
        actual_bytes    = $null
        expected_sha256 = $ExpectedSha256.ToLowerInvariant()
        actual_sha256   = $null
        verified        = $false
        failure         = 'missing_file'
    } | ConvertTo-Json -Compress
    exit 2
}

$item = Get-Item -LiteralPath $Path
$actualBytes = [UInt64]$item.Length
$actualSha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
$verified = ($actualBytes -eq $ExpectedBytes) -and
    ($actualSha256 -eq $ExpectedSha256.ToLowerInvariant())

$failure = $null
if ($actualBytes -ne $ExpectedBytes) {
    $failure = 'size_mismatch'
}
elseif ($actualSha256 -ne $ExpectedSha256.ToLowerInvariant()) {
    $failure = 'sha256_mismatch'
}

[ordered]@{
    schema          = 'animus-ferric-model-verification-v1'
    path            = $DisplayPath
    expected_bytes  = $ExpectedBytes
    actual_bytes    = $actualBytes
    expected_sha256 = $ExpectedSha256.ToLowerInvariant()
    actual_sha256   = $actualSha256
    verified        = $verified
    failure         = $failure
} | ConvertTo-Json -Compress

if (-not $verified) {
    exit 1
}

exit 0
