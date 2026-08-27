[CmdletBinding()]
param(
    [ValidateSet('Q4_K_M', 'Q3_K_XL')]
    [string]$Quant = 'Q4_K_M'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$repoRoot = $artifactDir
for ($index = 0; $index -lt 5; $index++) {
    $repoRoot = Split-Path -Parent $repoRoot
}
$repoRoot = (Resolve-Path -LiteralPath $repoRoot).Path
$specPath = Join-Path $artifactDir 'model-spec.json'
$verifyScript = Join-Path $artifactDir 'verify-model.ps1'
$startedAt = (Get-Date).ToUniversalTime().ToString('o')
$recordStamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffffffZ')
$RecordName = "acquisition-$Quant-$recordStamp-$PID.json"
$recordPath = Join-Path $artifactDir $RecordName
$finalPath = $null

function Write-Record {
    param([Parameter(Mandatory = $true)][System.Collections.IDictionary]$Record)

    $json = $Record | ConvertTo-Json -Depth 8
    $temporaryRecord = "$recordPath.tmp-$PID"
    [System.IO.File]::WriteAllText(
        $temporaryRecord,
        "$json`n",
        [System.Text.UTF8Encoding]::new($false)
    )
    Move-Item -LiteralPath $temporaryRecord -Destination $recordPath
    $json
}

function Stop-Acquisition {
    param(
        [Parameter(Mandatory = $true)][string]$Failure,
        [Parameter(Mandatory = $true)][int]$ExitCode,
        [hashtable]$Extra = @{}
    )

    $record = [ordered]@{
        schema           = 'animus-ferric-model-acquisition-v1'
        started_at_utc   = $startedAt
        completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        quant            = $Quant
        verified         = $false
        published        = $false
        failure          = $Failure
    }
    foreach ($entry in $Extra.GetEnumerator()) {
        $record[$entry.Key] = $entry.Value
    }
    Write-Record -Record $record
    exit $ExitCode
}

function Invoke-Verification {
    param(
        [Parameter(Mandatory = $true)][string]$CandidatePath,
        [Parameter(Mandatory = $true)][string]$CandidateDisplayPath,
        [Parameter(Mandatory = $true)]$SelectedSpec
    )

    $json = & $verifyScript -Path $CandidatePath `
        -ExpectedBytes ([UInt64]$SelectedSpec.bytes) `
        -ExpectedSha256 $SelectedSpec.sha256 `
        -DisplayPath $CandidateDisplayPath
    $code = $LASTEXITCODE
    $result = $json | ConvertFrom-Json
    [pscustomobject]@{ Code = $code; Result = $result }
}

function Move-ToQuarantine {
    param([Parameter(Mandatory = $true)][string]$CandidatePath)

    $stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffffffZ')
    $quarantinePath = "$CandidatePath.rejected-$stamp-$PID"
    Move-Item -LiteralPath $CandidatePath -Destination $quarantinePath
    [System.IO.Path]::GetFileName($quarantinePath)
}

function Test-Q3Authorization {
    param(
        [Parameter(Mandatory = $true)]$Authorization,
        [Parameter(Mandatory = $true)]$PrimarySpec
    )

    $required = @(
        'schema', 'gate', 'q4_verdict', 'q3_fallback_authorized',
        'q4_file', 'q4_sha256'
    )
    foreach ($name in $required) {
        if ($Authorization.PSObject.Properties.Name -notcontains $name) {
            return $false
        }
    }
    ($Authorization.schema -eq 'animus-ferric-qwen38-viability-v1') -and
        ($Authorization.gate -eq 'E09-D') -and
        ($Authorization.q4_verdict -eq 'non_viable') -and
        ($Authorization.q3_fallback_authorized -eq $true) -and
        ($Authorization.q4_file -eq $PrimarySpec.file) -and
        ($Authorization.q4_sha256 -eq $PrimarySpec.sha256)
}

try {
    $spec = Get-Content -Raw -LiteralPath $specPath | ConvertFrom-Json
    if ($Quant -eq 'Q3_K_XL') {
        $authorizationPath = Join-Path $repoRoot `
            'docs/sprints/s114/control-artifacts/runtime/q4-viability.json'
        if (-not (Test-Path -LiteralPath $authorizationPath -PathType Leaf)) {
            Stop-Acquisition -Failure 'q3_fallback_not_authorized' -ExitCode 3 `
                -Extra @{ gate = $spec.fallback.authorization_gate; authorization_record = 'missing' }
        }
        $authorization = Get-Content -Raw -LiteralPath $authorizationPath | ConvertFrom-Json
        $authorized = Test-Q3Authorization -Authorization $authorization `
            -PrimarySpec $spec.primary
        if (-not $authorized) {
            Stop-Acquisition -Failure 'q3_fallback_not_authorized' -ExitCode 3 `
                -Extra @{ gate = $spec.fallback.authorization_gate; authorization_record = 'invalid' }
        }
        $selected = $spec.fallback
    }
    else {
        $selected = $spec.primary
    }

    $modelsDir = Join-Path $repoRoot 'models'
    [System.IO.Directory]::CreateDirectory($modelsDir) | Out-Null
    $finalPath = Join-Path $modelsDir $selected.file
    $partialPath = "$finalPath.part"
    $displayPath = "models/$($selected.file)"

    $gitCommand = Get-Command git -ErrorAction SilentlyContinue
    if ($null -eq $gitCommand) {
        if (Test-Path -LiteralPath $finalPath -PathType Leaf) {
            $quarantined = Move-ToQuarantine -CandidatePath $finalPath
        }
        else {
            $quarantined = $null
        }
        Stop-Acquisition -Failure 'git_storage_check_unavailable' -ExitCode 7 `
            -Extra @{ path = $displayPath; quarantine = $quarantined }
    }
    & $gitCommand.Source -C $repoRoot check-ignore --quiet -- $displayPath
    $gitIgnored = ($LASTEXITCODE -eq 0)
    & $gitCommand.Source -C $repoRoot ls-files --error-unmatch -- $displayPath `
        1>$null 2>$null
    $gitTracked = ($LASTEXITCODE -eq 0)
    if (-not $gitIgnored -or $gitTracked) {
        if (Test-Path -LiteralPath $finalPath -PathType Leaf) {
            $quarantined = Move-ToQuarantine -CandidatePath $finalPath
            $quarantineDisplay = "models/$quarantined"
        }
        else {
            $quarantineDisplay = $null
        }
        Stop-Acquisition -Failure 'git_storage_policy_failed' -ExitCode 7 `
            -Extra @{
                path = $displayPath
                git_ignored = $gitIgnored
                git_tracked = $gitTracked
                quarantine = $quarantineDisplay
            }
    }

    if (Test-Path -LiteralPath $finalPath -PathType Leaf) {
        $existing = Invoke-Verification -CandidatePath $finalPath `
            -CandidateDisplayPath $displayPath -SelectedSpec $selected
        if ($existing.Code -ne 0) {
            $quarantined = Move-ToQuarantine -CandidatePath $finalPath
            Stop-Acquisition -Failure 'published_file_failed_verification' -ExitCode 6 `
                -Extra @{
                    path = $displayPath
                    quarantine = "models/$quarantined"
                    verification = $existing.Result
                }
        }
        $downloaded = $false
        $finalVerification = $existing.Result
    }
    else {
        $freeBytes = [UInt64](Get-Item -LiteralPath $repoRoot).PSDrive.Free
        $minimumFreeBytes = [UInt64](25GB)
        if ($freeBytes -lt $minimumFreeBytes) {
            Stop-Acquisition -Failure 'insufficient_storage' -ExitCode 4 `
                -Extra @{
                    path = $displayPath
                    required_free_bytes = $minimumFreeBytes
                    actual_free_bytes = $freeBytes
                }
        }

        $partialReady = $false
        if (Test-Path -LiteralPath $partialPath -PathType Leaf) {
            $partialBytes = [UInt64](Get-Item -LiteralPath $partialPath).Length
            if ($partialBytes -eq [UInt64]$selected.bytes) {
                $partialVerification = Invoke-Verification -CandidatePath $partialPath `
                    -CandidateDisplayPath "$displayPath.part" -SelectedSpec $selected
                if ($partialVerification.Code -ne 0) {
                    Stop-Acquisition -Failure 'partial_file_failed_verification' -ExitCode 1 `
                        -Extra @{
                            path = "$displayPath.part"
                            verification = $partialVerification.Result
                        }
                }
                $partialReady = $true
            }
            elseif ($partialBytes -gt [UInt64]$selected.bytes) {
                Stop-Acquisition -Failure 'partial_file_too_large' -ExitCode 1 `
                    -Extra @{
                        path = "$displayPath.part"
                        expected_bytes = [UInt64]$selected.bytes
                        actual_bytes = $partialBytes
                    }
            }
        }

        if (-not $partialReady) {
            $curlCommand = Get-Command curl.exe, curl -ErrorAction SilentlyContinue |
                Select-Object -First 1
            if ($null -eq $curlCommand) {
                Stop-Acquisition -Failure 'transport_unavailable' -ExitCode 5 `
                    -Extra @{ transport = 'curl'; path = "$displayPath.part" }
            }
            & $curlCommand.Source --location --fail --retry 5 --retry-delay 5 `
                --retry-all-errors --continue-at - --output $partialPath $selected.url
            $curlExit = $LASTEXITCODE
            if ($curlExit -ne 0) {
                Stop-Acquisition -Failure 'transport_failure' -ExitCode 5 `
                    -Extra @{
                        transport = $curlCommand.Name
                        curl_exit = $curlExit
                        path = "$displayPath.part"
                    }
            }

            $partialVerification = Invoke-Verification -CandidatePath $partialPath `
                -CandidateDisplayPath "$displayPath.part" -SelectedSpec $selected
            if ($partialVerification.Code -ne 0) {
                Stop-Acquisition -Failure 'partial_file_failed_verification' -ExitCode 1 `
                    -Extra @{
                        path = "$displayPath.part"
                        verification = $partialVerification.Result
                    }
            }
        }

        Move-Item -LiteralPath $partialPath -Destination $finalPath
        $published = Invoke-Verification -CandidatePath $finalPath `
            -CandidateDisplayPath $displayPath -SelectedSpec $selected
        if ($published.Code -ne 0) {
            $quarantined = Move-ToQuarantine -CandidatePath $finalPath
            Stop-Acquisition -Failure 'post_publish_verification_failed' -ExitCode 6 `
                -Extra @{
                    path = $displayPath
                    quarantine = "models/$quarantined"
                    verification = $published.Result
                }
        }
        $downloaded = $true
        $finalVerification = $published.Result
    }

    $success = [ordered]@{
        schema                 = 'animus-ferric-model-acquisition-v1'
        started_at_utc         = $startedAt
        completed_at_utc       = (Get-Date).ToUniversalTime().ToString('o')
        conversion_publisher   = 'Unsloth'
        conversion_repository  = $spec.conversion.repository
        conversion_third_party = $spec.conversion.third_party
        official_upstream      = $spec.upstream.repository
        license                = $spec.upstream.license
        revision               = $spec.conversion.revision
        url                    = $selected.url
        quant                  = $selected.quant
        file                   = $selected.file
        path                   = $displayPath
        expected_bytes         = [UInt64]$selected.bytes
        actual_bytes           = [UInt64]$finalVerification.actual_bytes
        expected_sha256        = $selected.sha256
        actual_sha256          = $finalVerification.actual_sha256
        downloaded_this_run    = $downloaded
        git_ignored            = $gitIgnored
        git_tracked            = $gitTracked
        verified               = $true
        published              = $true
        failure                = $null
    }
    Write-Record -Record $success
    exit 0
}
catch {
    $caught = $_
    $quarantineDisplay = $null
    $quarantineError = $null
    if ($null -ne $finalPath -and
        (Test-Path -LiteralPath $finalPath -PathType Leaf)) {
        try {
            $quarantined = Move-ToQuarantine -CandidatePath $finalPath
            $quarantineDisplay = "models/$quarantined"
        }
        catch {
            $quarantineError = $_.Exception.Message
        }
    }
    $stillPublished = ($null -ne $finalPath) -and
        (Test-Path -LiteralPath $finalPath -PathType Leaf)
    $record = [ordered]@{
        schema           = 'animus-ferric-model-acquisition-v1'
        started_at_utc   = $startedAt
        completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
        quant            = $Quant
        verified         = $false
        published        = $stillPublished
        failure          = 'unhandled_acquisition_error'
        error_type       = $caught.Exception.GetType().FullName
        error_message    = $caught.Exception.Message
        quarantine       = $quarantineDisplay
        quarantine_error = $quarantineError
    }
    try {
        Write-Record -Record $record
    }
    catch {
        $record | ConvertTo-Json -Depth 8
    }
    exit 8
}
