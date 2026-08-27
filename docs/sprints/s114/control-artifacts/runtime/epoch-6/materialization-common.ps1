function Get-EpochSixStaticControlNames {
    [CmdletBinding()]
    param()

    @(
        '.gitattributes'
        'README.md'
        'incident.json'
        'runtime-plan.json'
        'materialization-common.ps1'
        'test-materialization.ps1'
        'freeze-materialization.ps1'
        'materialize-e05-evidence.ps1'
    )
}

function ConvertTo-EpochSixJsonObject {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Value,
        [ValidateRange(2, 100)][int]$Depth = 64
    )

    $Value | ConvertTo-Json -Depth $Depth -Compress |
        ConvertFrom-Json -DateKind String
}

function Test-EpochSixExactPropertySequence {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string[]]$Expected
    )

    $null -ne $Value -and
        (@($Value.PSObject.Properties.Name) -join "`n") -ceq
            ($Expected -join "`n")
}

function Test-EpochSixStrictUtc {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Value)

    if ($Value -isnot [string] -or
        [string]$Value -cnotmatch
            '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$') {
        return $false
    }
    $instant = [DateTimeOffset]::MinValue
    [DateTimeOffset]::TryParseExact(
        [string]$Value,
        "yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'",
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::AssumeUniversal,
        [ref]$instant
    )
}

function Test-EpochSixAnchorShape {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Anchor)

    if ($null -eq $Anchor) { return $false }
    try {
        $relativePath = [string]$Anchor.relative_path
        -not [string]::IsNullOrWhiteSpace($relativePath) -and
            -not [System.IO.Path]::IsPathRooted($relativePath) -and
            $relativePath.IndexOf([char]0) -lt 0 -and
            $relativePath.IndexOf(':') -lt 0 -and
            $relativePath -notmatch '(^|[\\/])\.{1,2}([\\/]|$)' -and
            [UInt64]$Anchor.bytes -gt 0 -and
            [string]$Anchor.sha256 -cmatch '^[0-9a-f]{64}$'
    }
    catch { $false }
}

function Test-EpochSixPlanIdentity {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Plan)

    if ($null -eq $Plan) { return $false }
    try {
        $operation = $Plan.operation
        $terminal = $operation.expected_terminal
        $anchors = @(
            $Plan.model
            $operation.manifest
            $operation.attempt
            $operation.attestation
            $Plan.prefreeze_self_test_failure
            $Plan.epoch_5.runtime_plan
            $Plan.epoch_5.control_manifest
            $Plan.epoch_5.control_digest
            $Plan.epoch_5.publication_self_test
            $Plan.epoch_5.publication_common
            $Plan.epoch_5.frozen_failed_publisher
            $Plan.epoch_4.runtime_plan
            $Plan.epoch_4.raw_source_anchor
            $Plan.epoch_4.runtime_common
            $Plan.epoch_4.control_manifest
            $Plan.epoch_4.control_digest
            $Plan.epoch_4.runtime_self_test
            $Plan.epoch_4.verifier
            $Plan.epoch_4.frozen_failed_publisher
            $Plan.epoch_3.control_manifest
            $Plan.epoch_3.control_digest
            $Plan.epoch_3.runtime_plan
            $Plan.epoch_3.runtime_self_test
        )
        $anchorsPassed = @($anchors | Where-Object {
                -not (Test-EpochSixAnchorShape -Anchor $_)
            }).Count -eq 0

        $Plan.schema -ceq
            'animus-ferric-runtime-evidence-materialization-plan-v6' -and
            $Plan.task -ceq 'T-11409' -and
            [int]$Plan.execution_epoch -eq 3 -and
            [int]$Plan.failed_publication_epoch -eq 4 -and
            [int]$Plan.failed_correction_epoch -eq 5 -and
            [int]$Plan.materialization_epoch -eq 6 -and
            $Plan.timestamp_protocol -ceq
                'powershell-json-datekind-string-rfc3339-v1' -and
            $Plan.repository_commit_before_epoch_6_controls -ceq
                'a1306e5191591600551ef7c2c8676f061e8d554f' -and
            $Plan.source_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-3' -and
            $Plan.publication_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-4' -and
            $Plan.correction_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-5' -and
            $Plan.materialization_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-6' -and
            $Plan.model.relative_path -ceq
                'models/Qwen3.8-27B-UD-Q4_K_M.gguf' -and
            [UInt64]$Plan.model.bytes -eq 16464440224 -and
            $Plan.model.sha256 -ceq
                '322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482' -and
            $operation.id -ceq
                'r06-materialize-e05-publication-evidence' -and
            $operation.correction_operation_id -ceq
                'r05-publish-e03-01-q4-32768-after-e04-wrapper-failure' -and
            $operation.failed_operation_id -ceq
                'r04-publish-e03-01-q4-32768' -and
            $operation.coordinate -ceq 'e03-01-q4-32768' -and
            $operation.source_attempt_schema -ceq
                'animus-ferric-runtime-attempt-v3' -and
            $operation.source_raw_relative_path -ceq
                'target/s114-experiment/runtime-epoch-3/smoke/e03-01-q4-32768' -and
            $operation.destination_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-3/attempts/e03-01-q4-32768' -and
            $operation.legacy_envelope_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-4/recovery-publication.json' -and
            $operation.correction_evidence_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-5/publication-correction.json' -and
            $operation.materialization_evidence_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-6/materialization.json' -and
            [int]$operation.exact_manifest_entries -eq 49 -and
            $terminal.quant -ceq 'Q4_K_M' -and
            [int]$terminal.context -eq 32768 -and
            $terminal.verdict -ceq 'viable' -and
            [bool]$terminal.evidence_complete -and
            [bool]$terminal.startup_healthy -and
            [bool]$terminal.attestation_passed -and
            [bool]$terminal.smoke_passed -and
            [bool]$terminal.throughput_passed -and
            [bool]$terminal.teardown_passed -and
            [double]$terminal.median_decoded_tokens_per_second -eq
                3.2064850358228254 -and
            $Plan.published_destination.relative_path -ceq
                $operation.destination_relative_path -and
            $Plan.published_destination.manifest_sha256 -ceq
                '4ba753e79f59d2441eade7d7e7bab7131f7f6cfeed6a702bcf719faf8fde430a' -and
            [int]$Plan.published_destination.entries -eq 49 -and
            [UInt64]$Plan.published_destination.payload_bytes -eq 437140 -and
            $Plan.published_destination.attempt_sha256 -ceq
                '167a964e471fec93bc7e58ff0ec76bbba45f3025f18c2fe84060248732b4fae4' -and
            $Plan.published_destination.attestation_sha256 -ceq
                '792ae02c6323deafcaae9b89b247b43fccdc07cabb3ff470a0b7edfee78b0a99' -and
            $Plan.published_destination.verdict -ceq 'viable' -and
            [bool]$Plan.published_destination.evidence_complete -and
            $Plan.prefreeze_self_test_failure.relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-6/materialization-self-test.failed-01.json' -and
            [UInt64]$Plan.prefreeze_self_test_failure.bytes -eq 39692 -and
            $Plan.prefreeze_self_test_failure.sha256 -ceq
                'de7ce31a000cd78abe55455db5d6ed5b6931ef00e76c18a2c5e25b03822e27ce' -and
            $Plan.prefreeze_self_test_failure.schema -ceq
                'animus-ferric-runtime-materialization-self-test-v6' -and
            $Plan.prefreeze_self_test_failure.tested_at_utc -ceq
                '2026-08-27T23:05:55.6845975Z' -and
            $Plan.prefreeze_self_test_failure.passed -is [bool] -and
            -not [bool]$Plan.prefreeze_self_test_failure.passed -and
            [int]$Plan.prefreeze_self_test_failure.test_count -eq 22 -and
            [int]$Plan.prefreeze_self_test_failure.exact_model_hashes -eq 1 -and
            $Plan.prefreeze_self_test_failure.sole_failed_test -ceq
                'frozen_epoch_5_ordered_dictionary_bug_remains_exact_incident_evidence' -and
            $Plan.prefreeze_self_test_failure.cause -ceq
                'ambiguous_first_occurrence_text_search' -and
            $Plan.prefreeze_self_test_failure.controls_frozen -is [bool] -and
            -not [bool]$Plan.prefreeze_self_test_failure.controls_frozen -and
            $Plan.prefreeze_self_test_failure.official_outputs_created -is [bool] -and
            -not [bool]$Plan.prefreeze_self_test_failure.official_outputs_created -and
            $Plan.epoch_5.control_manifest_digest_line -ceq
                "$($Plan.epoch_5.control_manifest.sha256)  control-inputs.json" -and
            $Plan.epoch_4.control_manifest_digest_line -ceq
                "$($Plan.epoch_4.control_manifest.sha256)  control-inputs.json" -and
            $Plan.epoch_3.control_manifest_digest_line -ceq
                "$($Plan.epoch_3.control_manifest.sha256)  control-inputs.json" -and
            $Plan.outputs.legacy_envelope.schema -ceq
                'animus-ferric-runtime-recovery-publication-v4' -and
            $Plan.outputs.legacy_envelope.path -ceq
                $operation.legacy_envelope_relative_path -and
            $Plan.outputs.correction_evidence.schema -ceq
                'animus-ferric-runtime-publication-correction-v5' -and
            $Plan.outputs.correction_evidence.path -ceq
                $operation.correction_evidence_relative_path -and
            $Plan.outputs.materialization_evidence.schema -ceq
                'animus-ferric-runtime-evidence-materialization-v6' -and
            $Plan.outputs.materialization_evidence.path -ceq
                $operation.materialization_evidence_relative_path -and
            $anchorsPassed
    }
    catch { $false }
}

function Test-EpochSixFailedSelfTestReport {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Report,
        [Parameter(Mandatory = $true)]$Plan
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    try {
        if (-not (Test-EpochSixPlanIdentity -Plan $Plan)) {
            throw 'epoch-6 plan identity differs'
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Report -Expected @(
                'schema', 'task', 'operation_id', 'execution_epoch',
                'failed_publication_epoch', 'failed_correction_epoch',
                'materialization_epoch', 'timestamp_protocol', 'tested_at_utc',
                'passed', 'test_count', 'exact_model_hashes', 'static_controls',
                'direct_anchors', 'dependency_verification',
                'source_verification', 'destination_verification',
                'frozen_epoch_4_verifier', 'duplicate_test_names', 'results'
            ))) {
            $errors.Add('failed self-test does not have the exact v6 report contract')
        }
        $anchor = $Plan.prefreeze_self_test_failure
        $results = @($Report.results)
        $failed = @($results | Where-Object { -not [bool]$_.passed })
        $names = @($results | ForEach-Object { [string]$_.name })
        if ([string]$Report.schema -cne [string]$anchor.schema -or
            [string]$Report.task -cne 'T-11409' -or
            [string]$Report.operation_id -cne [string]$Plan.operation.id -or
            [int]$Report.execution_epoch -ne 3 -or
            [int]$Report.failed_publication_epoch -ne 4 -or
            [int]$Report.failed_correction_epoch -ne 5 -or
            [int]$Report.materialization_epoch -ne 6 -or
            [string]$Report.timestamp_protocol -cne
                [string]$Plan.timestamp_protocol -or
            [string]$Report.tested_at_utc -cne [string]$anchor.tested_at_utc -or
            -not (Test-EpochSixStrictUtc -Value $Report.tested_at_utc) -or
            $Report.passed -isnot [bool] -or [bool]$Report.passed -or
            [int]$Report.test_count -ne [int]$anchor.test_count -or
            [int]$Report.exact_model_hashes -ne
                [int]$anchor.exact_model_hashes -or
            $results.Count -ne [int]$anchor.test_count -or
            @($names | Select-Object -Unique).Count -ne $names.Count -or
            @($Report.duplicate_test_names).Count -ne 0 -or
            $failed.Count -ne 1 -or
            [string]$failed[0].name -cne [string]$anchor.sole_failed_test -or
            @($Report.static_controls).Count -ne 8 -or
            @($Report.direct_anchors).Count -ne 22 -or
            @($Report.direct_anchors | Where-Object {
                    -not [bool]$_.passed
                }).Count -ne 0 -or
            -not [bool]$Report.dependency_verification.passed -or
            -not [bool]$Report.source_verification.passed -or
            -not [bool]$Report.destination_verification.passed -or
            -not [bool]$Report.frozen_epoch_4_verifier.passed -or
            [string]$Report.frozen_epoch_4_verifier.live_model_sha256 -cne
                [string]$Plan.model.sha256) {
            $errors.Add('failed self-test identity or sole-failure boundary differs')
        }
    }
    catch { $errors.Add("failed self-test is malformed: $($_.Exception.Message)") }

    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function Resolve-EpochSixRepoRelativePath {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$RelativePath
    )

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath.IndexOf([char]0) -ge 0 -or
        $RelativePath.IndexOf(':') -ge 0 -or
        $RelativePath -match '(^|[\\/])\.{1,2}([\\/]|$)') {
        throw "unsafe epoch-6 repository-relative path: $RelativePath"
    }
    $root = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $resolved = [System.IO.Path]::GetFullPath((Join-Path $root $RelativePath))
    $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
    if (-not $resolved.StartsWith(
            $prefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "epoch-6 path escaped the repository: $RelativePath"
    }
    $resolved
}

function Test-EpochSixNonReparseDirectoryChain {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $checked = 0
    try {
        $root = [System.IO.Path]::GetFullPath($RepositoryRoot).TrimEnd(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        )
        $resolved = [System.IO.Path]::GetFullPath($Path).TrimEnd(
            [System.IO.Path]::DirectorySeparatorChar,
            [System.IO.Path]::AltDirectorySeparatorChar
        )
        $prefix = "$root$([System.IO.Path]::DirectorySeparatorChar)"
        if (-not $resolved.Equals(
                $root,
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            -not $resolved.StartsWith(
                $prefix,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            throw 'directory chain is outside the repository'
        }

        $cursor = $root
        $relative = [System.IO.Path]::GetRelativePath($root, $resolved)
        $segments = if ($relative -ceq '.') {
            @()
        }
        else {
            @($relative -split '[\\/]' | Where-Object {
                    -not [string]::IsNullOrEmpty([string]$_)
                })
        }
        if (-not (Test-Path -LiteralPath $cursor -PathType Container)) {
            throw "directory-chain component is absent: $cursor"
        }
        $rootItem = Get-Item -LiteralPath $cursor -Force
        if ($rootItem.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) {
            throw "directory-chain component is a reparse point: $cursor"
        }
        $checked++
        foreach ($segment in $segments) {
            $cursor = Join-Path $cursor ([string]$segment)
            if (-not (Test-Path -LiteralPath $cursor -PathType Container)) {
                throw "directory-chain component is absent: $cursor"
            }
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                )) {
                throw "directory-chain component is a reparse point: $cursor"
            }
            $checked++
        }
    }
    catch { $errors.Add($_.Exception.Message) }

    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        components_checked = $checked
        errors = @($errors)
    }
}

function Test-EpochSixFileAnchor {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [AllowNull()][Parameter(Mandatory = $true)]$Anchor,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $resolved = $null
    try {
        if (-not (Test-EpochSixAnchorShape -Anchor $Anchor)) {
            throw 'anchor shape differs'
        }
        $resolved = Resolve-EpochSixRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$Anchor.relative_path)
        if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
            throw 'anchored file is absent or not a regular file'
        }
        $item = Get-Item -LiteralPath $resolved -Force
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw 'anchored file is a reparse point'
        }
        if ([UInt64]$item.Length -ne [UInt64]$Anchor.bytes) {
            throw 'anchored byte count differs'
        }
        if ((Get-Sha256Lower -Path $resolved) -cne [string]$Anchor.sha256) {
            throw 'anchored SHA-256 differs'
        }
    }
    catch { $errors.Add("${Label}: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        relative_path = if ($null -eq $Anchor) {
            $null
        }
        else { [string]$Anchor.relative_path }
        resolved_path = $resolved
        bytes = if ($null -eq $Anchor) { 0 } else { [UInt64]$Anchor.bytes }
        sha256 = if ($null -eq $Anchor) { $null } else { [string]$Anchor.sha256 }
        errors = @($errors)
    }
}

function Test-EpochSixExactTree {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)]$ManifestAnchor,
        [Parameter(Mandatory = $true)][int]$ExpectedEntries
    )

    Test-EpochFiveExactTree -Root $Root -ManifestAnchor $ManifestAnchor `
        -ExpectedEntries $ExpectedEntries
}

function Test-EpochSixDestinationVerification {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Report,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)]$SourcePlan,
        [Parameter(Mandatory = $true)][string]$DestinationPath
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    try {
        if (-not (Test-EpochSixPlanIdentity -Plan $Plan)) {
            throw 'epoch-6 plan identity differs'
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Report -Expected @(
                'schema', 'task', 'operation_id', 'execution_epoch',
                'publication_epoch', 'source_attempt_schema',
                'timestamp_protocol', 'control_epoch', 'attestation_protocol',
                'process_command_protocol', 'live_model_identity',
                'attempt_path', 'coordinate', 'verdict', 'passed', 'manifest',
                'recovery_anchor', 'control_anchor_mode', 'throughput_rows',
                'errors'
            ))) {
            $errors.Add('destination verifier report is not the full exact v4 contract')
        }
        if (-not (Test-EpochSixExactPropertySequence `
                -Value $Report.live_model_identity `
                -Expected @('checked', 'mode', 'sha256'))) {
            $errors.Add('destination verifier model identity shape differs')
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Report.manifest `
                -Expected @('passed', 'entries', 'errors'))) {
            $errors.Add('destination verifier manifest shape differs')
        }
        if (-not (Test-EpochSixExactPropertySequence `
                -Value $Report.recovery_anchor `
                -Expected @(
                    'applicable', 'passed', 'expected_entries',
                    'observed_entries'
                ))) {
            $errors.Add('destination verifier recovery-anchor shape differs')
        }
        $check = Test-EpochFourVerificationReport -Report $Report `
            -RecoveryPlan $EpochFourPlan -SourcePlan $SourcePlan `
            -ExpectedAttemptPath $DestinationPath `
            -ExpectedAnchorMode 'epoch_4_frozen_recovery'
        foreach ($message in @($check.errors)) {
            $errors.Add([string]$message)
        }
    }
    catch { $errors.Add("destination verification is malformed: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function New-EpochSixLegacyRecoveryEnvelope {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)]$VerificationReport,
        [Parameter(Mandatory = $true)][string]$PublishedAtUtc,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    ConvertTo-EpochSixJsonObject -Value ([pscustomobject][ordered]@{
        schema = 'animus-ferric-runtime-recovery-publication-v4'
        task = 'T-11409'
        operation_id = [string]$EpochFourPlan.operation.id
        execution_epoch = 3
        publication_epoch = 4
        timestamp_protocol = [string]$EpochFourPlan.timestamp_protocol
        published_at_utc = $PublishedAtUtc
        control_manifest_sha256 = $EpochFourControlSha256
        source = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.source_raw_relative_path
            manifest_sha256 = [string]$SourceCheck.manifest_sha256
            entries = [int]$SourceCheck.entries
        }
        destination = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.destination_relative_path
            manifest_sha256 = [string]$DestinationCheck.manifest_sha256
            entries = [int]$DestinationCheck.entries
        }
        stage_verification = $VerificationReport
        published_verification = $VerificationReport
        resumed_existing_destination = $ResumedExistingDestination
        passed = $true
    })
}

function Test-EpochSixLegacyRecoveryEnvelope {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Envelope,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)]$SourcePlan,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [Parameter(Mandatory = $true)]$VerificationReport,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-EpochSixExactPropertySequence -Value $Envelope -Expected @(
            'schema', 'task', 'operation_id', 'execution_epoch',
            'publication_epoch', 'timestamp_protocol', 'published_at_utc',
            'control_manifest_sha256', 'source', 'destination',
            'stage_verification', 'published_verification',
            'resumed_existing_destination', 'passed'
        ))) {
        $errors.Add('legacy envelope does not have the exact 14-field contract')
    }
    try {
        if ([string]$Envelope.schema -cne
                'animus-ferric-runtime-recovery-publication-v4' -or
            [string]$Envelope.task -cne 'T-11409' -or
            [string]$Envelope.operation_id -cne
                [string]$EpochFourPlan.operation.id -or
            [int]$Envelope.execution_epoch -ne 3 -or
            [int]$Envelope.publication_epoch -ne 4 -or
            [string]$Envelope.timestamp_protocol -cne
                [string]$EpochFourPlan.timestamp_protocol -or
            -not (Test-EpochSixStrictUtc -Value $Envelope.published_at_utc) -or
            [string]$Envelope.control_manifest_sha256 -cne
                $EpochFourControlSha256 -or
            $Envelope.resumed_existing_destination -isnot [bool] -or
            [bool]$Envelope.resumed_existing_destination -ne
                $ResumedExistingDestination -or
            -not $ResumedExistingDestination -or
            -not [bool]$Envelope.passed) {
            $errors.Add('legacy envelope identity differs')
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Envelope.source `
                -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
            [string]$Envelope.source.relative_path -cne
                [string]$Plan.operation.source_raw_relative_path -or
            [string]$Envelope.source.manifest_sha256 -cne
                [string]$SourceCheck.manifest_sha256 -or
            [int]$Envelope.source.entries -ne [int]$SourceCheck.entries) {
            $errors.Add('legacy source binding differs')
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Envelope.destination `
                -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
            [string]$Envelope.destination.relative_path -cne
                [string]$Plan.operation.destination_relative_path -or
            [string]$Envelope.destination.manifest_sha256 -cne
                [string]$DestinationCheck.manifest_sha256 -or
            [int]$Envelope.destination.entries -ne [int]$DestinationCheck.entries) {
            $errors.Add('legacy destination binding differs')
        }
        $verificationCheck = Test-EpochSixDestinationVerification `
            -Report $VerificationReport -Plan $Plan `
            -EpochFourPlan $EpochFourPlan -SourcePlan $SourcePlan `
            -DestinationPath $DestinationPath
        foreach ($message in @($verificationCheck.errors)) {
            $errors.Add("frozen destination verification: $message")
        }
        if (-not (Test-JsonEquivalent -Left $Envelope.stage_verification `
                -Right $VerificationReport)) {
            $errors.Add('legacy stage verification differs from the frozen report')
        }
        if (-not (Test-JsonEquivalent -Left $Envelope.published_verification `
                -Right $VerificationReport)) {
            $errors.Add('legacy published verification differs from the frozen report')
        }
    }
    catch { $errors.Add("legacy envelope is malformed: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function New-EpochSixCorrectionEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)][string]$EpochFiveControlSha256,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)][string]$LegacyEnvelopeSha256,
        [Parameter(Mandatory = $true)][UInt64]$LegacyEnvelopeBytes,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][string]$CorrectedAtUtc,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    ConvertTo-EpochSixJsonObject -Value ([pscustomobject][ordered]@{
        schema = 'animus-ferric-runtime-publication-correction-v5'
        task = 'T-11409'
        operation_id = [string]$Plan.operation.correction_operation_id
        failed_operation_id = [string]$EpochFourPlan.operation.id
        execution_epoch = 3
        failed_publication_epoch = 4
        correction_epoch = 5
        timestamp_protocol = [string]$Plan.timestamp_protocol
        corrected_at_utc = $CorrectedAtUtc
        control_manifest_sha256 = $EpochFiveControlSha256
        failed_epoch_control_manifest_sha256 = $EpochFourControlSha256
        legacy_envelope = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.legacy_envelope_relative_path
            bytes = $LegacyEnvelopeBytes
            sha256 = $LegacyEnvelopeSha256
        }
        source = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.source_raw_relative_path
            manifest_sha256 = [string]$SourceCheck.manifest_sha256
            entries = [int]$SourceCheck.entries
        }
        destination = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.destination_relative_path
            manifest_sha256 = [string]$DestinationCheck.manifest_sha256
            entries = [int]$DestinationCheck.entries
        }
        resumed_existing_destination = $ResumedExistingDestination
        legacy_envelope_validation = [pscustomobject][ordered]@{
            contract = 'animus-ferric-runtime-recovery-publication-v4'
            passed = $true
        }
        passed = $true
    })
}

function Test-EpochSixCorrectionEvidence {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)]$EpochFourPlan,
        [Parameter(Mandatory = $true)][string]$EpochFiveControlSha256,
        [Parameter(Mandatory = $true)][string]$EpochFourControlSha256,
        [Parameter(Mandatory = $true)][string]$LegacyEnvelopeSha256,
        [Parameter(Mandatory = $true)][UInt64]$LegacyEnvelopeBytes,
        [Parameter(Mandatory = $true)]$SourceCheck,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-EpochSixExactPropertySequence -Value $Evidence -Expected @(
            'schema', 'task', 'operation_id', 'failed_operation_id',
            'execution_epoch', 'failed_publication_epoch', 'correction_epoch',
            'timestamp_protocol', 'corrected_at_utc', 'control_manifest_sha256',
            'failed_epoch_control_manifest_sha256', 'legacy_envelope', 'source',
            'destination', 'resumed_existing_destination',
            'legacy_envelope_validation', 'passed'
        ))) {
        $errors.Add('correction evidence does not have the exact v5 field contract')
    }
    try {
        if ([string]$Evidence.schema -cne
                'animus-ferric-runtime-publication-correction-v5' -or
            [string]$Evidence.task -cne 'T-11409' -or
            [string]$Evidence.operation_id -cne
                [string]$Plan.operation.correction_operation_id -or
            [string]$Evidence.failed_operation_id -cne
                [string]$EpochFourPlan.operation.id -or
            [int]$Evidence.execution_epoch -ne 3 -or
            [int]$Evidence.failed_publication_epoch -ne 4 -or
            [int]$Evidence.correction_epoch -ne 5 -or
            [string]$Evidence.timestamp_protocol -cne
                [string]$Plan.timestamp_protocol -or
            -not (Test-EpochSixStrictUtc -Value $Evidence.corrected_at_utc) -or
            [string]$Evidence.control_manifest_sha256 -cne
                $EpochFiveControlSha256 -or
            [string]$Evidence.failed_epoch_control_manifest_sha256 -cne
                $EpochFourControlSha256 -or
            $Evidence.resumed_existing_destination -isnot [bool] -or
            [bool]$Evidence.resumed_existing_destination -ne
                $ResumedExistingDestination -or
            -not $ResumedExistingDestination -or
            -not [bool]$Evidence.passed) {
            $errors.Add('correction evidence identity differs')
        }
        if (-not (Test-EpochSixExactPropertySequence `
                -Value $Evidence.legacy_envelope `
                -Expected @('relative_path', 'bytes', 'sha256')) -or
            [string]$Evidence.legacy_envelope.relative_path -cne
                [string]$Plan.operation.legacy_envelope_relative_path -or
            [UInt64]$Evidence.legacy_envelope.bytes -ne $LegacyEnvelopeBytes -or
            [string]$Evidence.legacy_envelope.sha256 -cne
                $LegacyEnvelopeSha256) {
            $errors.Add('correction legacy-envelope binding differs')
        }
        foreach ($binding in @(
                [pscustomobject]@{
                    name = 'source'; value = $Evidence.source
                    path = [string]$Plan.operation.source_raw_relative_path
                    check = $SourceCheck
                },
                [pscustomobject]@{
                    name = 'destination'; value = $Evidence.destination
                    path = [string]$Plan.operation.destination_relative_path
                    check = $DestinationCheck
                }
            )) {
            if (-not (Test-EpochSixExactPropertySequence -Value $binding.value `
                    -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
                [string]$binding.value.relative_path -cne $binding.path -or
                [string]$binding.value.manifest_sha256 -cne
                    [string]$binding.check.manifest_sha256 -or
                [int]$binding.value.entries -ne [int]$binding.check.entries) {
                $errors.Add("correction $($binding.name) binding differs")
            }
        }
        if (-not (Test-EpochSixExactPropertySequence `
                -Value $Evidence.legacy_envelope_validation `
                -Expected @('contract', 'passed')) -or
            [string]$Evidence.legacy_envelope_validation.contract -cne
                'animus-ferric-runtime-recovery-publication-v4' -or
            -not [bool]$Evidence.legacy_envelope_validation.passed) {
            $errors.Add('correction legacy-envelope validation differs')
        }
    }
    catch { $errors.Add("correction evidence is malformed: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function New-EpochSixMaterializationEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$EpochSixControlSha256,
        [Parameter(Mandatory = $true)][string]$EpochFiveControlSha256,
        [Parameter(Mandatory = $true)][string]$LegacyEnvelopeSha256,
        [Parameter(Mandatory = $true)][UInt64]$LegacyEnvelopeBytes,
        [Parameter(Mandatory = $true)][string]$CorrectionEvidenceSha256,
        [Parameter(Mandatory = $true)][UInt64]$CorrectionEvidenceBytes,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][string]$MaterializedAtUtc,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    ConvertTo-EpochSixJsonObject -Value ([pscustomobject][ordered]@{
        schema = 'animus-ferric-runtime-evidence-materialization-v6'
        task = 'T-11409'
        operation_id = [string]$Plan.operation.id
        correction_operation_id = [string]$Plan.operation.correction_operation_id
        failed_operation_id = [string]$Plan.operation.failed_operation_id
        execution_epoch = 3
        failed_publication_epoch = 4
        failed_correction_epoch = 5
        materialization_epoch = 6
        timestamp_protocol = [string]$Plan.timestamp_protocol
        materialized_at_utc = $MaterializedAtUtc
        control_manifest_sha256 = $EpochSixControlSha256
        correction_epoch_control_manifest_sha256 = $EpochFiveControlSha256
        legacy_envelope = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.legacy_envelope_relative_path
            bytes = $LegacyEnvelopeBytes
            sha256 = $LegacyEnvelopeSha256
        }
        correction_evidence = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.correction_evidence_relative_path
            bytes = $CorrectionEvidenceBytes
            sha256 = $CorrectionEvidenceSha256
        }
        destination = [pscustomobject][ordered]@{
            relative_path = [string]$Plan.operation.destination_relative_path
            manifest_sha256 = [string]$DestinationCheck.manifest_sha256
            entries = [int]$DestinationCheck.entries
        }
        resumed_existing_destination = $ResumedExistingDestination
        authoritative_revalidation = [pscustomobject][ordered]@{
            publisher_relative_path =
                [string]$Plan.epoch_5.frozen_failed_publisher.relative_path
            publisher_bytes = [UInt64]$Plan.epoch_5.frozen_failed_publisher.bytes
            publisher_sha256 =
                [string]$Plan.epoch_5.frozen_failed_publisher.sha256
            exit_code = 0
            correction_json_equivalent = $true
            passed = $true
        }
        passed = $true
    })
}

function Test-EpochSixMaterializationEvidence {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$EpochSixControlSha256,
        [Parameter(Mandatory = $true)][string]$EpochFiveControlSha256,
        [Parameter(Mandatory = $true)][string]$LegacyEnvelopeSha256,
        [Parameter(Mandatory = $true)][UInt64]$LegacyEnvelopeBytes,
        [Parameter(Mandatory = $true)][string]$CorrectionEvidenceSha256,
        [Parameter(Mandatory = $true)][UInt64]$CorrectionEvidenceBytes,
        [Parameter(Mandatory = $true)]$DestinationCheck,
        [Parameter(Mandatory = $true)][bool]$ResumedExistingDestination
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-EpochSixExactPropertySequence -Value $Evidence -Expected @(
            'schema', 'task', 'operation_id', 'correction_operation_id',
            'failed_operation_id', 'execution_epoch', 'failed_publication_epoch',
            'failed_correction_epoch', 'materialization_epoch',
            'timestamp_protocol', 'materialized_at_utc',
            'control_manifest_sha256',
            'correction_epoch_control_manifest_sha256', 'legacy_envelope',
            'correction_evidence', 'destination', 'resumed_existing_destination',
            'authoritative_revalidation', 'passed'
        ))) {
        $errors.Add('materialization evidence does not have the exact v6 field contract')
    }
    try {
        if ([string]$Evidence.schema -cne
                'animus-ferric-runtime-evidence-materialization-v6' -or
            [string]$Evidence.task -cne 'T-11409' -or
            [string]$Evidence.operation_id -cne [string]$Plan.operation.id -or
            [string]$Evidence.correction_operation_id -cne
                [string]$Plan.operation.correction_operation_id -or
            [string]$Evidence.failed_operation_id -cne
                [string]$Plan.operation.failed_operation_id -or
            [int]$Evidence.execution_epoch -ne 3 -or
            [int]$Evidence.failed_publication_epoch -ne 4 -or
            [int]$Evidence.failed_correction_epoch -ne 5 -or
            [int]$Evidence.materialization_epoch -ne 6 -or
            [string]$Evidence.timestamp_protocol -cne
                [string]$Plan.timestamp_protocol -or
            -not (Test-EpochSixStrictUtc -Value $Evidence.materialized_at_utc) -or
            [string]$Evidence.control_manifest_sha256 -cne
                $EpochSixControlSha256 -or
            [string]$Evidence.correction_epoch_control_manifest_sha256 -cne
                $EpochFiveControlSha256 -or
            $Evidence.resumed_existing_destination -isnot [bool] -or
            [bool]$Evidence.resumed_existing_destination -ne
                $ResumedExistingDestination -or
            -not $ResumedExistingDestination -or
            -not [bool]$Evidence.passed) {
            $errors.Add('materialization evidence identity differs')
        }
        foreach ($binding in @(
                [pscustomobject]@{
                    name = 'legacy'; value = $Evidence.legacy_envelope
                    path = [string]$Plan.operation.legacy_envelope_relative_path
                    bytes = $LegacyEnvelopeBytes; sha256 = $LegacyEnvelopeSha256
                },
                [pscustomobject]@{
                    name = 'correction'; value = $Evidence.correction_evidence
                    path = [string]$Plan.operation.correction_evidence_relative_path
                    bytes = $CorrectionEvidenceBytes; sha256 = $CorrectionEvidenceSha256
                }
            )) {
            if (-not (Test-EpochSixExactPropertySequence -Value $binding.value `
                    -Expected @('relative_path', 'bytes', 'sha256')) -or
                [string]$binding.value.relative_path -cne $binding.path -or
                [UInt64]$binding.value.bytes -ne [UInt64]$binding.bytes -or
                [string]$binding.value.sha256 -cne [string]$binding.sha256) {
                $errors.Add("materialization $($binding.name) binding differs")
            }
        }
        if (-not (Test-EpochSixExactPropertySequence -Value $Evidence.destination `
                -Expected @('relative_path', 'manifest_sha256', 'entries')) -or
            [string]$Evidence.destination.relative_path -cne
                [string]$Plan.operation.destination_relative_path -or
            [string]$Evidence.destination.manifest_sha256 -cne
                [string]$DestinationCheck.manifest_sha256 -or
            [int]$Evidence.destination.entries -ne [int]$DestinationCheck.entries) {
            $errors.Add('materialization destination binding differs')
        }
        $revalidation = $Evidence.authoritative_revalidation
        if (-not (Test-EpochSixExactPropertySequence -Value $revalidation `
                -Expected @(
                    'publisher_relative_path', 'publisher_bytes',
                    'publisher_sha256', 'exit_code',
                    'correction_json_equivalent', 'passed'
                )) -or
            [string]$revalidation.publisher_relative_path -cne
                [string]$Plan.epoch_5.frozen_failed_publisher.relative_path -or
            [UInt64]$revalidation.publisher_bytes -ne
                [UInt64]$Plan.epoch_5.frozen_failed_publisher.bytes -or
            [string]$revalidation.publisher_sha256 -cne
                [string]$Plan.epoch_5.frozen_failed_publisher.sha256 -or
            [int]$revalidation.exit_code -ne 0 -or
            $revalidation.correction_json_equivalent -isnot [bool] -or
            -not [bool]$revalidation.correction_json_equivalent -or
            -not [bool]$revalidation.passed) {
            $errors.Add('materialization authoritative revalidation differs')
        }
    }
    catch { $errors.Add("materialization evidence is malformed: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function Test-EpochSixMaterializationState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][bool]$DestinationExists,
        [Parameter(Mandatory = $true)][bool]$DestinationContainer,
        [Parameter(Mandatory = $true)][bool]$DestinationNonReparse,
        [Parameter(Mandatory = $true)][bool]$DestinationExact,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeExists,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeLeaf,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeNonReparse,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeExact,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceExists,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceLeaf,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceNonReparse,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceExact,
        [Parameter(Mandatory = $true)][bool]$MaterializationEvidenceExists,
        [Parameter(Mandatory = $true)][bool]$MaterializationEvidenceLeaf,
        [Parameter(Mandatory = $true)][bool]$MaterializationEvidenceNonReparse,
        [Parameter(Mandatory = $true)][bool]$MaterializationEvidenceExact
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if (-not $DestinationExists -or -not $DestinationContainer -or
        -not $DestinationNonReparse -or -not $DestinationExact) {
        $errors.Add('the exact non-reparse destination must already exist')
    }
    foreach ($state in @(
            [pscustomobject]@{
                name = 'legacy envelope'; exists = $LegacyEnvelopeExists
                leaf = $LegacyEnvelopeLeaf; non_reparse = $LegacyEnvelopeNonReparse
                exact = $LegacyEnvelopeExact
            },
            [pscustomobject]@{
                name = 'correction evidence'; exists = $CorrectionEvidenceExists
                leaf = $CorrectionEvidenceLeaf; non_reparse = $CorrectionEvidenceNonReparse
                exact = $CorrectionEvidenceExact
            },
            [pscustomobject]@{
                name = 'materialization evidence'; exists = $MaterializationEvidenceExists
                leaf = $MaterializationEvidenceLeaf
                non_reparse = $MaterializationEvidenceNonReparse
                exact = $MaterializationEvidenceExact
            }
        )) {
        if ([bool]$state.exists -and
            (-not [bool]$state.leaf -or -not [bool]$state.non_reparse -or
                -not [bool]$state.exact)) {
            $errors.Add("existing $($state.name) is not an exact non-reparse file")
        }
    }
    if ($CorrectionEvidenceExists -and -not $LegacyEnvelopeExists) {
        $errors.Add('correction evidence exists without its legacy envelope')
    }
    if ($MaterializationEvidenceExists -and
        (-not $LegacyEnvelopeExists -or -not $CorrectionEvidenceExists)) {
        $errors.Add('materialization evidence exists without both predecessor records')
    }
    $action = $null
    if ($errors.Count -eq 0) {
        if (-not $LegacyEnvelopeExists -and -not $CorrectionEvidenceExists -and
            -not $MaterializationEvidenceExists) {
            $action = 'materialize_legacy_correction_and_record'
        }
        elseif ($LegacyEnvelopeExists -and -not $CorrectionEvidenceExists -and
            -not $MaterializationEvidenceExists) {
            $action = 'materialize_correction_and_record'
        }
        elseif ($LegacyEnvelopeExists -and $CorrectionEvidenceExists -and
            -not $MaterializationEvidenceExists) {
            $action = 'revalidate_and_record'
        }
        elseif ($LegacyEnvelopeExists -and $CorrectionEvidenceExists -and
            $MaterializationEvidenceExists) {
            $action = 'revalidate_complete'
        }
        else {
            $errors.Add('materialization state is not an authorized transition')
        }
    }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        action = $action
        errors = @($errors)
    }
}

function Test-EpochSixFrozenDependencySet {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$ExpectedHead
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $epochFiveStaticChecked = 0
    $epochFourStaticChecked = 0
    $transitiveEpochThreeChecked = 0
    try {
        if (-not (Test-EpochSixPlanIdentity -Plan $Plan)) {
            throw 'epoch-6 plan identity differs'
        }
        $epochFiveDir = Resolve-EpochSixRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$Plan.correction_artifact_relative_path)
        $epochFiveControlCheck = Test-EpochSixFileAnchor `
            -RepositoryRoot $RepositoryRoot -Anchor $Plan.epoch_5.control_manifest `
            -Label 'epoch-5 control manifest'
        $epochFiveDigestCheck = Test-EpochSixFileAnchor `
            -RepositoryRoot $RepositoryRoot -Anchor $Plan.epoch_5.control_digest `
            -Label 'epoch-5 control digest'
        foreach ($check in @($epochFiveControlCheck, $epochFiveDigestCheck)) {
            foreach ($message in @($check.errors)) { $errors.Add([string]$message) }
        }
        if ($errors.Count -gt 0) { throw 'epoch-5 anchors differ' }
        $epochFiveControlPath = [string]$epochFiveControlCheck.resolved_path
        $epochFiveDigestPath = [string]$epochFiveDigestCheck.resolved_path
        $epochFiveDigest = (Get-Content -Raw `
                -LiteralPath $epochFiveDigestPath).TrimEnd("`r", "`n")
        if ($epochFiveDigest -cne
                [string]$Plan.epoch_5.control_manifest_digest_line -or
            $epochFiveDigest -cne
                "$((Get-Sha256Lower -Path $epochFiveControlPath))  control-inputs.json") {
            $errors.Add('epoch-5 frozen control digest differs')
        }
        $epochFiveControls = Get-Content -Raw -LiteralPath $epochFiveControlPath |
            ConvertFrom-Json -DateKind String
        $epochFivePlanPath = Resolve-EpochSixRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$Plan.epoch_5.runtime_plan.relative_path)
        $epochFivePlan = Get-Content -Raw -LiteralPath $epochFivePlanPath |
            ConvertFrom-Json -DateKind String
        if (-not (Test-EpochFivePlanIdentity -Plan $epochFivePlan) -or
            [string]$epochFiveControls.schema -cne
                'animus-ferric-runtime-publication-correction-control-inputs-v5' -or
            [string]$epochFiveControls.task -cne 'T-11409' -or
            [string]$epochFiveControls.operation_id -cne
                [string]$Plan.operation.correction_operation_id -or
            [string]$epochFiveControls.failed_operation_id -cne
                [string]$Plan.operation.failed_operation_id -or
            [int]$epochFiveControls.correction_epoch -ne 5 -or
            [string]$epochFiveControls.repository.head_at_freeze -cne $ExpectedHead -or
            [string]$epochFiveControls.runtime_plan_sha256 -cne
                [string]$Plan.epoch_5.runtime_plan.sha256 -or
            [string]$epochFiveControls.publication_self_test.relative_path -cne
                [string]$Plan.epoch_5.publication_self_test.relative_path -or
            [UInt64]$epochFiveControls.publication_self_test.bytes -ne
                [UInt64]$Plan.epoch_5.publication_self_test.bytes -or
            [string]$epochFiveControls.publication_self_test.sha256 -cne
                [string]$Plan.epoch_5.publication_self_test.sha256 -or
            -not [bool]$epochFiveControls.publication_self_test.passed -or
            -not [bool]$epochFiveControls.epoch_4.passed -or
            -not [bool]$epochFiveControls.raw_source.passed) {
            $errors.Add('epoch-5 frozen control identity differs')
        }
        $expectedNames = @(Get-EpochFiveStaticControlNames)
        $entries = @($epochFiveControls.static_controls)
        if ($expectedNames.Count -ne 8 -or $entries.Count -ne 8) {
            $errors.Add('epoch-5 static control count differs')
        }
        else {
            for ($index = 0; $index -lt $expectedNames.Count; $index++) {
                $name = [string]$expectedNames[$index]
                $entry = $entries[$index]
                $path = Join-Path $epochFiveDir $name
                if ([string]$entry.path -cne $name -or
                    -not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    $errors.Add("epoch-5 static control is absent or reordered: $name")
                    continue
                }
                $item = Get-Item -LiteralPath $path -Force
                if ($item.Attributes.HasFlag(
                        [System.IO.FileAttributes]::ReparsePoint
                    ) -or [UInt64]$item.Length -ne [UInt64]$entry.bytes -or
                    (Get-Sha256Lower -Path $path) -cne [string]$entry.sha256) {
                    $errors.Add("epoch-5 static control differs: $name")
                    continue
                }
                $epochFiveStaticChecked++
            }
        }
        foreach ($anchorName in @(
                'runtime_plan', 'publication_self_test', 'publication_common',
                'frozen_failed_publisher'
            )) {
            $check = Test-EpochSixFileAnchor -RepositoryRoot $RepositoryRoot `
                -Anchor $Plan.epoch_5.$anchorName -Label "epoch-5 $anchorName"
            foreach ($message in @($check.errors)) { $errors.Add([string]$message) }
        }
        $selfTestPath = Resolve-EpochSixRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$Plan.epoch_5.publication_self_test.relative_path)
        $selfTest = Get-Content -Raw -LiteralPath $selfTestPath |
            ConvertFrom-Json -DateKind String
        if (-not [bool]$selfTest.passed) {
            $errors.Add('epoch-5 publication self-test is not a clean pass')
        }
        foreach ($anchorName in @(
                'runtime_plan', 'raw_source_anchor', 'control_manifest',
                'control_digest', 'runtime_self_test', 'verifier',
                'frozen_failed_publisher'
            )) {
            if (-not (Test-JsonEquivalent -Left $Plan.epoch_4.$anchorName `
                    -Right $epochFivePlan.epoch_4.$anchorName)) {
                $errors.Add("epoch-4 anchor differs across epoch-5/6: $anchorName")
            }
        }
        foreach ($anchorName in @(
                'control_manifest', 'control_digest', 'runtime_plan',
                'runtime_self_test'
            )) {
            if (-not (Test-JsonEquivalent -Left $Plan.epoch_3.$anchorName `
                    -Right $epochFivePlan.epoch_3.$anchorName)) {
                $errors.Add("epoch-3 anchor differs across epoch-5/6: $anchorName")
            }
        }
        if ([string]$Plan.epoch_4.control_manifest_digest_line -cne
                [string]$epochFivePlan.epoch_4.control_manifest_digest_line -or
            [string]$Plan.epoch_3.control_manifest_digest_line -cne
                [string]$epochFivePlan.epoch_3.control_manifest_digest_line) {
            $errors.Add('transitive digest-line anchor differs across epoch-5/6')
        }
        foreach ($anchorName in @('runtime_common', 'verifier')) {
            $check = Test-EpochSixFileAnchor -RepositoryRoot $RepositoryRoot `
                -Anchor $Plan.epoch_4.$anchorName -Label "epoch-4 $anchorName"
            foreach ($message in @($check.errors)) { $errors.Add([string]$message) }
        }
        $transitive = Test-EpochFourFrozenDependencySet `
            -RepositoryRoot $RepositoryRoot -EpochFivePlan $epochFivePlan `
            -ExpectedHead $ExpectedHead
        foreach ($message in @($transitive.errors)) {
            $errors.Add([string]$message)
        }
        $epochFourStaticChecked = [int]$transitive.static_controls_checked
        $transitiveEpochThreeChecked =
            [int]$transitive.transitive_epoch_3_controls_checked
        if (-not [bool]$transitive.passed -or
            $epochFourStaticChecked -ne 12 -or
            $transitiveEpochThreeChecked -ne 20) {
            $errors.Add('epoch-4/epoch-3 frozen dependency traversal differs')
        }
    }
    catch { $errors.Add("frozen dependency traversal failed: $($_.Exception.Message)") }
    [pscustomobject][ordered]@{
        passed = ($errors.Count -eq 0)
        epoch_5_static_controls_checked = $epochFiveStaticChecked
        epoch_4_static_controls_checked = $epochFourStaticChecked
        transitive_epoch_3_controls_checked = $transitiveEpochThreeChecked
        errors = @($errors)
    }
}

function Write-EpochSixJsonAtomic {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value,
        [ValidateRange(2, 100)][int]$Depth = 64
    )

    $resolvedPath = [System.IO.Path]::GetFullPath($Path)
    $parent = Split-Path -Parent $resolvedPath
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw 'atomic JSON parent is absent'
    }
    $parentItem = Get-Item -LiteralPath $parent -Force
    if ($parentItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        throw 'atomic JSON parent is a reparse point'
    }
    if (Test-Path -LiteralPath $resolvedPath) {
        throw 'atomic JSON destination already exists'
    }
    $tempName = ".epoch6-$([guid]::NewGuid().ToString('N')).tmp"
    $tempPath = Join-Path $parent $tempName
    try {
        Write-JsonLf -Path $tempPath -Value $Value -Depth $Depth
        if (Test-Path -LiteralPath $resolvedPath) {
            throw 'atomic JSON destination appeared before publication'
        }
        [System.IO.File]::Move($tempPath, $resolvedPath, $false)
    }
    finally {
        if ((Test-Path -LiteralPath $tempPath -PathType Leaf) -and
            (Split-Path -Parent ([System.IO.Path]::GetFullPath($tempPath))) -ceq
                [System.IO.Path]::GetFullPath($parent) -and
            (Split-Path -Leaf $tempPath) -cmatch
                '^\.epoch6-[0-9a-f]{32}\.tmp$') {
            [System.IO.File]::Delete($tempPath)
        }
    }
}
