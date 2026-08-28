[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [UInt64]$ExpectedBytes,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedSha256,

    [string]$DisplayPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $Path.EndsWith('.part', [System.StringComparison]::Ordinal)) {
    throw [System.IO.IOException]::new(
        'injected post-publication verification I/O failure'
    )
}

$actualBytes = [UInt64](Get-Item -LiteralPath $Path).Length
$actualSha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
$verified = ($actualBytes -eq $ExpectedBytes) -and
    ($actualSha256 -eq $ExpectedSha256.ToLowerInvariant())

[ordered]@{
    schema          = 'animus-ferric-model-verification-v1'
    path            = $DisplayPath
    expected_bytes  = $ExpectedBytes
    actual_bytes    = $actualBytes
    expected_sha256 = $ExpectedSha256.ToLowerInvariant()
    actual_sha256   = $actualSha256
    verified        = $verified
    failure         = if ($verified) { $null } else { 'fixture_mismatch' }
} | ConvertTo-Json -Compress

if (-not $verified) {
    exit 1
}
exit 0
