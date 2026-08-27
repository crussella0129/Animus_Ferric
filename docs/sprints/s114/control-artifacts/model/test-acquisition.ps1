[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifactDir = $PSScriptRoot
$repoRoot = $artifactDir
for ($index = 0; $index -lt 5; $index++) {
    $repoRoot = Split-Path -Parent $repoRoot
}
$repoRoot = (Resolve-Path -LiteralPath $repoRoot).Path
$spec = Get-Content -Raw -LiteralPath (Join-Path $artifactDir 'model-spec.json') |
    ConvertFrom-Json
$attestation = Get-Content -Raw -LiteralPath (
    Join-Path $artifactDir 'acquisition-Q4_K_M.json'
) | ConvertFrom-Json
$verifyScript = Join-Path $artifactDir 'verify-model.ps1'
$modelPath = Join-Path (Join-Path $repoRoot 'models') $spec.primary.file
$displayPath = "models/$($spec.primary.file)"
$results = [System.Collections.Generic.List[object]]::new()

function Add-Result {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$Passed,
        [Parameter(Mandatory = $true)]$Evidence
    )
    $results.Add([ordered]@{
        name = $Name
        passed = $Passed
        evidence = $Evidence
    })
}

function New-IsolatedAcquisitionCase {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$VerifierSource,
        [Parameter(Mandatory = $true)][byte[]]$PartialBytes,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256
    )

    $caseRoot = Join-Path $repoRoot (
        "target/s114-experiment/model-acquisition-selftest/$PID/$Name"
    )
    $caseArtifactDir = Join-Path $caseRoot `
        'docs/sprints/s114/control-artifacts/model'
    $caseRuntimeDir = Join-Path $caseRoot `
        'docs/sprints/s114/control-artifacts/runtime'
    $caseModelsDir = Join-Path $caseRoot 'models'
    [System.IO.Directory]::CreateDirectory($caseArtifactDir) | Out-Null
    [System.IO.Directory]::CreateDirectory($caseRuntimeDir) | Out-Null
    [System.IO.Directory]::CreateDirectory($caseModelsDir) | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $caseRoot '.gitignore'),
        "models/`n",
        [System.Text.UTF8Encoding]::new($false)
    )
    & git -C $caseRoot init --quiet
    if ($LASTEXITCODE -ne 0) {
        throw "could not initialize isolated acquisition case $Name"
    }

    Copy-Item -LiteralPath (Join-Path $artifactDir 'acquire-model.ps1') `
        -Destination (Join-Path $caseArtifactDir 'acquire-model.ps1')
    Copy-Item -LiteralPath $VerifierSource `
        -Destination (Join-Path $caseArtifactDir 'verify-model.ps1')
    Copy-Item -LiteralPath (
        Join-Path (Split-Path -Parent $artifactDir) 'runtime/runtime-common.ps1'
    ) -Destination (Join-Path $caseRuntimeDir 'runtime-common.ps1')

    $tinySpec = [ordered]@{
        schema = 'animus-ferric-model-spec-v1'
        upstream = [ordered]@{ repository = 'example/tiny'; license = 'Apache-2.0' }
        conversion = [ordered]@{
            repository = 'example/tiny-gguf'
            third_party = $true
            revision = '0000000000000000000000000000000000000000'
        }
        primary = [ordered]@{
            quant = 'Q4_K_M'
            file = 'tiny.gguf'
            bytes = [UInt64]$PartialBytes.Length
            sha256 = $ExpectedSha256
            url = 'https://example.invalid/tiny.gguf'
        }
        fallback = [ordered]@{
            quant = 'Q3_K_XL'
            file = 'tiny-q3.gguf'
            bytes = 1
            sha256 = ('0' * 64)
            url = 'https://example.invalid/tiny-q3.gguf'
            authorization_gate = 'T-11409/E09-D'
        }
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $caseArtifactDir 'model-spec.json'),
        (($tinySpec | ConvertTo-Json -Depth 8) + "`n"),
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllBytes(
        (Join-Path $caseModelsDir 'tiny.gguf.part'),
        $PartialBytes
    )

    [pscustomobject]@{
        Root = $caseRoot
        ArtifactDir = $caseArtifactDir
        RuntimeDir = $caseRuntimeDir
        FinalPath = Join-Path $caseModelsDir 'tiny.gguf'
        PartialPath = Join-Path $caseModelsDir 'tiny.gguf.part'
        Q3FinalPath = Join-Path $caseModelsDir 'tiny-q3.gguf'
        Q3PartialPath = Join-Path $caseModelsDir 'tiny-q3.gguf.part'
        AcquireScript = Join-Path $caseArtifactDir 'acquire-model.ps1'
    }
}

function Invoke-IsolatedAcquisitionCase {
    param(
        [Parameter(Mandatory = $true)]$Case,
        [ValidateSet('Q4_K_M', 'Q3_K_XL')]
        [string]$Quant = 'Q4_K_M'
    )

    $output = & $Case.AcquireScript -Quant $Quant
    $code = $LASTEXITCODE
    $recordFile = Get-ChildItem -LiteralPath $Case.ArtifactDir `
        -Filter "acquisition-$Quant-*.json" -File |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    $record = Get-Content -Raw -LiteralPath $recordFile.FullName | ConvertFrom-Json
    $finalPath = if ($Quant -eq 'Q3_K_XL') {
        $Case.Q3FinalPath
    }
    else {
        $Case.FinalPath
    }
    $partialPath = if ($Quant -eq 'Q3_K_XL') {
        $Case.Q3PartialPath
    }
    else {
        $Case.PartialPath
    }
    [pscustomobject]@{
        Code = $code
        Output = @($output)
        RecordFile = $recordFile.Name
        Record = $record
        FinalExists = Test-Path -LiteralPath $finalPath -PathType Leaf
        PartialExists = Test-Path -LiteralPath $partialPath -PathType Leaf
        Quarantine = @(
            Get-ChildItem -LiteralPath (Split-Path -Parent $finalPath) `
                -Filter "$([System.IO.Path]::GetFileName($finalPath)).rejected-*" `
                -File |
                Select-Object -ExpandProperty Name
        )
    }
}

function New-ForgedQ3GateCase {
    param(
        [Parameter(Mandatory = $true)][byte[]]$FallbackBytes,
        [Parameter(Mandatory = $true)][string]$FallbackSha256
    )

    $case = New-IsolatedAcquisitionCase -Name 'forged-q3-gate' `
        -VerifierSource $verifyScript -PartialBytes ([byte[]](1, 2, 3, 4)) `
        -ExpectedSha256 ('0' * 64)
    $runtimeSource = Join-Path (Split-Path -Parent $artifactDir) 'runtime'
    foreach ($name in @(
        'runtime-plan.json',
        'verify-q4-gate.ps1',
        'verify-runtime.ps1'
    )) {
        Copy-Item -LiteralPath (Join-Path $runtimeSource $name) `
            -Destination (Join-Path $case.RuntimeDir $name)
    }

    $tinySpecPath = Join-Path $case.ArtifactDir 'model-spec.json'
    $tinySpec = Get-Content -Raw -LiteralPath $tinySpecPath | ConvertFrom-Json
    $tinySpec.fallback.bytes = [UInt64]$FallbackBytes.Length
    $tinySpec.fallback.sha256 = $FallbackSha256
    [System.IO.File]::WriteAllText(
        $tinySpecPath,
        (($tinySpec | ConvertTo-Json -Depth 8) + "`n"),
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllBytes($case.Q3PartialPath, $FallbackBytes)

    $runtimePlanPath = Join-Path $case.RuntimeDir 'runtime-plan.json'
    $runtimePlan = Get-Content -Raw -LiteralPath $runtimePlanPath |
        ConvertFrom-Json
    $runtimeValidatorPath = Join-Path $case.RuntimeDir 'verify-runtime.ps1'
    $forgedGate = [ordered]@{
        schema = 'animus-ferric-qwen38-viability-v1'
        gate = 'E09-D'
        q4_file = $runtimePlan.models.Q4_K_M.file
        q4_sha256 = $runtimePlan.models.Q4_K_M.sha256
        selected_attempt = '01-q4-32768'
        q4_verdict = 'non_viable'
        q3_fallback_authorized = $true
        fallback_basis = 'q4_functional_smoke_failed'
        reason_codes = @('functional_smoke_failed')
        median_decoded_tokens_per_second = $null
        attempt_chain = @(
            [ordered]@{
                id = '01-q4-32768'
                manifest_sha256 = ('f' * 64)
            }
        )
        attempt_verifications = @()
        attempt_verification = $null
        attempt_manifest_sha256 = ('f' * 64)
        runtime_plan_sha256 = (
            Get-FileHash -LiteralPath $runtimePlanPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        validator_sha256 = (
            Get-FileHash -LiteralPath $runtimeValidatorPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $case.RuntimeDir 'q4-viability.json'),
        (($forgedGate | ConvertTo-Json -Depth 12) + "`n"),
        [System.Text.UTF8Encoding]::new($false)
    )

    [pscustomobject]@{
        Case = $case
        ForgedGate = $forgedGate
    }
}

$verificationJson = & $verifyScript -Path $modelPath `
    -ExpectedBytes ([UInt64]$spec.primary.bytes) `
    -ExpectedSha256 $spec.primary.sha256 -DisplayPath $displayPath
$verificationCode = $LASTEXITCODE
$verification = $verificationJson | ConvertFrom-Json
$sourceTupleMatches =
    ($attestation.conversion_repository -eq $spec.conversion.repository) -and
    ($attestation.conversion_third_party -eq $true) -and
    ($attestation.official_upstream -eq $spec.upstream.repository) -and
    ($attestation.license -eq $spec.upstream.license) -and
    ($attestation.revision -eq $spec.conversion.revision) -and
    ($attestation.url -eq $spec.primary.url) -and
    ($attestation.file -eq $spec.primary.file) -and
    ([UInt64]$attestation.actual_bytes -eq [UInt64]$spec.primary.bytes) -and
    ($attestation.actual_sha256 -eq $spec.primary.sha256)
Add-Result -Name 'model_download_and_sha256_attestation' `
    -Passed (($verificationCode -eq 0) -and $verification.verified -and
        $attestation.verified -and $attestation.published -and $sourceTupleMatches) `
    -Evidence ([ordered]@{
        verification = $verification
        source_tuple_matches = $sourceTupleMatches
    })

$shortCandidate = Join-Path $artifactDir 'model-spec.json'
$negativeJson = & $verifyScript -Path $shortCandidate `
    -ExpectedBytes ([UInt64]$spec.primary.bytes) `
    -ExpectedSha256 $spec.primary.sha256 -DisplayPath 'model-spec.json'
$negativeCode = $LASTEXITCODE
$negative = $negativeJson | ConvertFrom-Json

$goodBytes = [byte[]](1, 2, 3, 4)
$wrongBytes = [byte[]](5, 6, 7, 8)
$goodSha256 = [System.Convert]::ToHexString(
    [System.Security.Cryptography.SHA256]::HashData($goodBytes)
).ToLowerInvariant()
$sameSizeCase = New-IsolatedAcquisitionCase `
    -Name 'same-size-wrong-hash' -VerifierSource $verifyScript `
    -PartialBytes $wrongBytes -ExpectedSha256 $goodSha256
$sameSize = Invoke-IsolatedAcquisitionCase -Case $sameSizeCase

$throwVerifier = Join-Path $artifactDir 'fixtures/throw-after-part-verifier.ps1'
$throwCase = New-IsolatedAcquisitionCase `
    -Name 'post-publish-verifier-throws' -VerifierSource $throwVerifier `
    -PartialBytes $goodBytes -ExpectedSha256 $goodSha256
$throwResult = Invoke-IsolatedAcquisitionCase -Case $throwCase

$sameSizePassed = ($sameSize.Code -ne 0) -and (-not $sameSize.FinalExists) -and
    $sameSize.PartialExists -and (-not $sameSize.Record.verified) -and
    (-not $sameSize.Record.published) -and
    ($sameSize.Record.failure -eq 'partial_file_failed_verification') -and
    ([UInt64]$sameSize.Record.verification.actual_bytes -eq $goodBytes.Length) -and
    ($sameSize.Record.verification.failure -eq 'sha256_mismatch')
$throwPassed = ($throwResult.Code -eq 6) -and (-not $throwResult.FinalExists) -and
    (-not $throwResult.Record.verified) -and
    (-not $throwResult.Record.published) -and
    ($throwResult.Record.failure -eq 'post_publish_verification_failed') -and
    ($null -eq $throwResult.Record.verification) -and
    ($throwResult.Quarantine.Count -eq 1)
Add-Result -Name 'model_acquisition_failure_is_not_verified' `
    -Passed (($negativeCode -ne 0) -and (-not $negative.verified) -and
        ($negative.failure -in @('size_mismatch', 'sha256_mismatch')) -and
        $sameSizePassed -and $throwPassed) `
    -Evidence ([ordered]@{
        short_transfer = $negative
        same_size_wrong_hash = [ordered]@{
            passed = $sameSizePassed
            record = $sameSize.Record
            final_exists = $sameSize.FinalExists
            partial_exists = $sameSize.PartialExists
        }
        post_publish_verifier_error = [ordered]@{
            passed = $throwPassed
            record = $throwResult.Record
            final_exists = $throwResult.FinalExists
            quarantine = $throwResult.Quarantine
        }
    })

$ignoreRule = (& git check-ignore -v -- $displayPath 2>&1 | Out-String).Trim()
$ignoreCode = $LASTEXITCODE
& git ls-files --error-unmatch -- $displayPath 2>$null
$trackedCode = $LASTEXITCODE
$status = @(& git status --short --untracked-files=all)
$blobInStatus = @($status | Where-Object { $_ -match '\.gguf(?:\.part)?$' }).Count -gt 0
Add-Result -Name 'model_is_ignored_evidence_is_tracked' `
    -Passed (($ignoreCode -eq 0) -and ($trackedCode -ne 0) -and
        (-not $blobInStatus) -and $attestation.git_ignored -and
        (-not $attestation.git_tracked) -and $sourceTupleMatches) `
    -Evidence ([ordered]@{
        ignore_rule = $ignoreRule
        git_tracked = ($trackedCode -eq 0)
        model_blob_in_status = $blobInStatus
        status = $status
    })

$q3Path = Join-Path (Join-Path $repoRoot 'models') $spec.fallback.file
$q3PartPath = "$q3Path.part"
$q3GateRecord = Get-ChildItem -LiteralPath $artifactDir -File |
    Where-Object {
        $_.Name -eq 'test-q3-gate.json' -or
        $_.Name -like 'acquisition-Q3_K_XL-*.json'
    } |
    Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
$q3Gate = Get-Content -Raw -LiteralPath $q3GateRecord.FullName | ConvertFrom-Json
$twoBit = @(
    Get-ChildItem -LiteralPath (Join-Path $repoRoot 'models') -File |
        Where-Object { $_.Name -match '(?:^|[-_])(?:I?Q2|2bit)(?:[-_.]|$)' } |
        Select-Object -ExpandProperty Name
)
$q3Absent = (-not (Test-Path -LiteralPath $q3Path)) -and
    (-not (Test-Path -LiteralPath $q3PartPath))
Add-Result -Name 'q3_fallback_download_is_gated_and_attested' `
    -Passed ($q3Absent -and (-not $q3Gate.verified) -and
        (-not $q3Gate.published) -and
        ($q3Gate.failure -eq 'q3_fallback_not_authorized') -and
        ($twoBit.Count -eq 0)) `
    -Evidence ([ordered]@{
        q3_absent = $q3Absent
        gate_record_file = $q3GateRecord.Name
        gate_record = $q3Gate
        two_bit_artifacts = $twoBit
    })

$forgedFallbackBytes = [byte[]](9, 10, 11, 12)
$forgedFallbackSha256 = [System.Convert]::ToHexString(
    [System.Security.Cryptography.SHA256]::HashData($forgedFallbackBytes)
).ToLowerInvariant()
$forgedCaseBundle = New-ForgedQ3GateCase `
    -FallbackBytes $forgedFallbackBytes `
    -FallbackSha256 $forgedFallbackSha256
$forgedResult = Invoke-IsolatedAcquisitionCase `
    -Case $forgedCaseBundle.Case -Quant Q3_K_XL
$forgedPartialSha256 = if ($forgedResult.PartialExists) {
    (Get-FileHash -LiteralPath $forgedCaseBundle.Case.Q3PartialPath `
        -Algorithm SHA256).Hash.ToLowerInvariant()
}
else {
    $null
}
$forgedGateErrors = @($forgedResult.Record.gate_verification.errors)
$forgedGatePassed =
    ($forgedResult.Code -eq 3) -and
    (-not $forgedResult.Record.verified) -and
    (-not $forgedResult.Record.published) -and
    ($forgedResult.Record.failure -eq 'q3_fallback_not_authorized') -and
    ($forgedResult.Record.authorization_record -eq 'invalid') -and
    ($forgedResult.Record.gate_verification.passed -eq $false) -and
    ($forgedGateErrors -contains
        'Q4 attempt evidence is absent: 01-q4-32768') -and
    ($forgedGateErrors -contains
        'Q4 gate fields do not equal fresh attempt-chain derivation') -and
    (-not $forgedResult.FinalExists) -and
    $forgedResult.PartialExists -and
    ($forgedPartialSha256 -eq $forgedFallbackSha256) -and
    ($forgedResult.Quarantine.Count -eq 0)
Add-Result -Name 'forged_q4_gate_cannot_authorize_q3_download' `
    -Passed $forgedGatePassed `
    -Evidence ([ordered]@{
        passed = $forgedGatePassed
        forged_claim = [ordered]@{
            q4_verdict = $forgedCaseBundle.ForgedGate.q4_verdict
            q3_fallback_authorized =
                $forgedCaseBundle.ForgedGate.q3_fallback_authorized
            selected_attempt = $forgedCaseBundle.ForgedGate.selected_attempt
            attempt_chain = $forgedCaseBundle.ForgedGate.attempt_chain
        }
        acquisition_exit_code = $forgedResult.Code
        acquisition_record = $forgedResult.Record
        q3_final_exists = $forgedResult.FinalExists
        seeded_q3_partial_preserved = $forgedResult.PartialExists
        seeded_q3_partial_sha256 = $forgedPartialSha256
        q3_quarantine = $forgedResult.Quarantine
    })

$allPassed = @($results | Where-Object { -not $_.passed }).Count -eq 0
$report = [ordered]@{
    schema = 'animus-ferric-model-acquisition-tests-v1'
    completed_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    passed = $allPassed
    tests = $results
}
$json = ($report | ConvertTo-Json -Depth 12).
    Replace("`r`n", "`n").Replace("`r", "`n")
$reportPath = Join-Path $artifactDir 'acquisition-tests.json'
$temporaryPath = "$reportPath.tmp-$PID"
[System.IO.File]::WriteAllText(
    $temporaryPath,
    "$json`n",
    [System.Text.UTF8Encoding]::new($false)
)
Move-Item -LiteralPath $temporaryPath -Destination $reportPath -Force
$json

if (-not $allPassed) {
    exit 1
}
exit 0
