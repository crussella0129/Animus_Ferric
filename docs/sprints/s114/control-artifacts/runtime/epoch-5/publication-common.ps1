function Get-EpochFiveStaticControlNames {
    [CmdletBinding()]
    param()

    @(
        '.gitattributes'
        'README.md'
        'incident.json'
        'runtime-plan.json'
        'publication-common.ps1'
        'test-publication.ps1'
        'freeze-publication.ps1'
        'publish-e04-correction.ps1'
    )
}

function Test-EpochFiveAnchorShape {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Anchor)

    if ($null -eq $Anchor) {
        return $false
    }
    try {
        $relativePath = [string]$Anchor.relative_path
        -not [string]::IsNullOrWhiteSpace($relativePath) -and
            -not [System.IO.Path]::IsPathRooted($relativePath) -and
            $relativePath.IndexOf([char]0) -lt 0 -and
            $relativePath -notmatch '(^|[\\/])\.{1,2}([\\/]|$)' -and
            $relativePath.IndexOf(':') -lt 0 -and
            [UInt64]$Anchor.bytes -gt 0 -and
            [string]$Anchor.sha256 -cmatch '^[0-9a-f]{64}$'
    }
    catch {
        $false
    }
}

function Test-EpochFivePlanIdentity {
    [CmdletBinding()]
    param([AllowNull()][Parameter(Mandatory = $true)]$Plan)

    if ($null -eq $Plan) {
        return $false
    }
    try {
        $operation = $Plan.operation
        $terminal = $operation.expected_terminal
        $anchors = @(
            $Plan.model
            $operation.manifest
            $operation.attempt
            $operation.attestation
            $Plan.epoch_4.runtime_plan
            $Plan.epoch_4.raw_source_anchor
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
                -not (Test-EpochFiveAnchorShape -Anchor $_)
            }).Count -eq 0

        $Plan.schema -ceq
            'animus-ferric-runtime-publication-correction-plan-v5' -and
            $Plan.task -ceq 'T-11409' -and
            [int]$Plan.execution_epoch -eq 3 -and
            [int]$Plan.failed_publication_epoch -eq 4 -and
            [int]$Plan.correction_epoch -eq 5 -and
            $Plan.timestamp_protocol -ceq
                'powershell-json-datekind-string-rfc3339-v1' -and
            $Plan.repository_commit_before_epoch_5_controls -ceq
                'a1306e5191591600551ef7c2c8676f061e8d554f' -and
            $Plan.source_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-3' -and
            $Plan.failed_publication_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-4' -and
            $Plan.correction_artifact_relative_path -ceq
                'docs/sprints/s114/control-artifacts/runtime/epoch-5' -and
            $Plan.model.relative_path -ceq
                'models/Qwen3.8-27B-UD-Q4_K_M.gguf' -and
            [UInt64]$Plan.model.bytes -eq 16464440224 -and
            $Plan.model.sha256 -ceq
                '322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482' -and
            $operation.id -ceq
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
            $Plan.epoch_4.control_manifest_digest_line -cmatch
                '^[0-9a-f]{64}  control-inputs\.json$' -and
            $Plan.epoch_4.control_manifest_digest_line -ceq
                "$($Plan.epoch_4.control_manifest.sha256)  control-inputs.json" -and
            $Plan.epoch_3.control_manifest_digest_line -cmatch
                '^[0-9a-f]{64}  control-inputs\.json$' -and
            $Plan.epoch_3.control_manifest_digest_line -ceq
                "$($Plan.epoch_3.control_manifest.sha256)  control-inputs.json" -and
            $anchorsPassed
    }
    catch {
        $false
    }
}

function Resolve-EpochFiveRepoRelativePath {
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
        throw "unsafe epoch-5 repository-relative path: $RelativePath"
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
        throw "epoch-5 path escapes the repository: $RelativePath"
    }
    $resolved
}

function Test-EpochFiveFileAnchor {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [AllowNull()][Parameter(Mandatory = $true)]$Anchor,
        [string]$Label = 'anchored file'
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $path = $null
    if (-not (Test-EpochFiveAnchorShape -Anchor $Anchor)) {
        $errors.Add("$Label anchor shape is invalid")
    }
    else {
        try {
            $path = Resolve-EpochFiveRepoRelativePath `
                -RepositoryRoot $RepositoryRoot `
                -RelativePath ([string]$Anchor.relative_path)
        }
        catch {
            $errors.Add("$Label path is unsafe: $($_.Exception.Message)")
        }
    }
    if ($null -ne $path) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $errors.Add("$Label is absent")
        }
        else {
            $item = Get-Item -LiteralPath $path -Force
            if ($item.Attributes.HasFlag(
                    [System.IO.FileAttributes]::ReparsePoint
                )) {
                $errors.Add("$Label is a reparse point")
            }
            if ([UInt64]$item.Length -ne [UInt64]$Anchor.bytes) {
                $errors.Add("$Label byte length differs")
            }
            if ((Get-Sha256Lower -Path $path) -cne [string]$Anchor.sha256) {
                $errors.Add("$Label SHA-256 differs")
            }
        }
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        label = $Label
        relative_path = if ($null -ne $Anchor) {
            [string](Get-OptionalProperty -Value $Anchor -Name 'relative_path')
        }
        else { $null }
        resolved_path = $path
        bytes = if ($null -ne $path -and
            (Test-Path -LiteralPath $path -PathType Leaf)) {
            [UInt64](Get-Item -LiteralPath $path).Length
        }
        else { $null }
        sha256 = if ($null -ne $path -and
            (Test-Path -LiteralPath $path -PathType Leaf)) {
            Get-Sha256Lower -Path $path
        }
        else { $null }
        errors = @($errors)
    }
}

function Test-EpochFiveExactTree {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [AllowNull()][Parameter(Mandatory = $true)]$ManifestAnchor,
        [int]$ExpectedEntries = 49
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $resolvedRoot = [System.IO.Path]::GetFullPath($Root)
    $manifestPath = $null
    $manifestCheck = $null
    $payloadBytes = [UInt64]0
    $actualEntryCount = 0

    if ($null -eq $ManifestAnchor) {
        $errors.Add('exact-tree anchor is absent')
    }
    if (-not (Test-Path -LiteralPath $resolvedRoot -PathType Container)) {
        $errors.Add('exact-tree root is absent')
    }
    else {
        $rootItem = Get-Item -LiteralPath $resolvedRoot -Force
        if ($rootItem.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            )) {
            $errors.Add('exact-tree root is a reparse point')
        }
        $reparseEntries = @(Get-ChildItem -LiteralPath $resolvedRoot `
                -Recurse -Force | Where-Object {
                $_.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)
            })
        if ($reparseEntries.Count -ne 0) {
            $errors.Add('exact-tree contains a reparse point')
        }
    }

    if ($null -ne $ManifestAnchor) {
        try {
            $manifest = $ManifestAnchor.manifest
            $files = @($ManifestAnchor.files)
            $manifestRelative = [string]$manifest.path
            if ([string]::IsNullOrWhiteSpace($manifestRelative) -or
                [System.IO.Path]::IsPathRooted($manifestRelative) -or
                $manifestRelative.IndexOf([char]0) -ge 0 -or
                $manifestRelative.IndexOf(':') -ge 0 -or
                $manifestRelative -match '(^|[\\/])\.{1,2}([\\/]|$)') {
                $errors.Add('exact-tree manifest path is unsafe')
            }
            else {
                $manifestPath = [System.IO.Path]::GetFullPath(
                    (Join-Path $resolvedRoot $manifestRelative)
                )
                $rootPrefix = $resolvedRoot.TrimEnd(
                    [System.IO.Path]::DirectorySeparatorChar,
                    [System.IO.Path]::AltDirectorySeparatorChar
                ) + [System.IO.Path]::DirectorySeparatorChar
                if (-not $manifestPath.StartsWith(
                        $rootPrefix,
                        [System.StringComparison]::OrdinalIgnoreCase
                    )) {
                    $errors.Add('exact-tree manifest escapes its root')
                }
                elseif (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
                    $errors.Add('exact-tree manifest is absent')
                }
                else {
                    $manifestItem = Get-Item -LiteralPath $manifestPath -Force
                    if ($manifestItem.Attributes.HasFlag(
                            [System.IO.FileAttributes]::ReparsePoint
                        )) {
                        $errors.Add('exact-tree manifest is a reparse point')
                    }
                    if ([UInt64]$manifestItem.Length -ne [UInt64]$manifest.bytes -or
                        (Get-Sha256Lower -Path $manifestPath) -cne
                            [string]$manifest.sha256) {
                        $errors.Add('exact-tree manifest differs from its anchor')
                    }
                    $manifestCheck = Test-HashManifest -Root $resolvedRoot `
                        -ManifestPath $manifestPath -RejectUnlistedFiles
                    if (-not [bool]$manifestCheck.passed) {
                        foreach ($message in @($manifestCheck.errors)) {
                            $errors.Add("exact-tree manifest: $message")
                        }
                    }
                }
            }

            if ([int]$manifest.entry_count -ne $ExpectedEntries -or
                $files.Count -ne $ExpectedEntries) {
                $errors.Add('exact-tree frozen entry count differs')
            }
            $actualEntryCount = $files.Count
            $seen = [System.Collections.Generic.HashSet[string]]::new(
                [System.StringComparer]::Ordinal
            )
            foreach ($entry in $files) {
                $relative = [string]$entry.path
                if ([string]::IsNullOrWhiteSpace($relative) -or
                    [System.IO.Path]::IsPathRooted($relative) -or
                    $relative.IndexOf([char]0) -ge 0 -or
                    $relative.IndexOf(':') -ge 0 -or
                    $relative -match '(^|[\\/])\.{1,2}([\\/]|$)' -or
                    -not $seen.Add($relative)) {
                    $errors.Add("unsafe or duplicate exact-tree entry: $relative")
                    continue
                }
                $path = [System.IO.Path]::GetFullPath((Join-Path $resolvedRoot (
                            $relative.Replace('/',
                                [System.IO.Path]::DirectorySeparatorChar)
                        )))
                $rootPrefix = $resolvedRoot.TrimEnd(
                    [System.IO.Path]::DirectorySeparatorChar,
                    [System.IO.Path]::AltDirectorySeparatorChar
                ) + [System.IO.Path]::DirectorySeparatorChar
                if (-not $path.StartsWith(
                        $rootPrefix,
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    $errors.Add("exact-tree file is absent or outside root: $relative")
                    continue
                }
                $item = Get-Item -LiteralPath $path -Force
                if ($item.Attributes.HasFlag(
                        [System.IO.FileAttributes]::ReparsePoint
                    ) -or [UInt64]$item.Length -ne [UInt64]$entry.bytes -or
                    (Get-Sha256Lower -Path $path) -cne [string]$entry.sha256) {
                    $errors.Add("exact-tree file differs: $relative")
                    continue
                }
                $payloadBytes += [UInt64]$item.Length
            }
            $allFiles = if (Test-Path -LiteralPath $resolvedRoot -PathType Container) {
                @(Get-ChildItem -LiteralPath $resolvedRoot -File -Recurse -Force)
            }
            else { @() }
            if ($allFiles.Count -ne ($ExpectedEntries + 1)) {
                $errors.Add('exact-tree total file count differs')
            }
            if ($payloadBytes -ne [UInt64]$manifest.payload_bytes) {
                $errors.Add('exact-tree payload byte total differs')
            }
        }
        catch {
            $errors.Add("exact-tree anchor is malformed: $($_.Exception.Message)")
        }
    }

    [ordered]@{
        passed = ($errors.Count -eq 0)
        root = $resolvedRoot
        manifest_path = $manifestPath
        manifest_sha256 = if ($null -ne $manifestPath -and
            (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            Get-Sha256Lower -Path $manifestPath
        }
        else { $null }
        entries = if ($null -ne $manifestCheck) {
            [int]$manifestCheck.entries
        }
        else { $actualEntryCount }
        payload_bytes = $payloadBytes
        errors = @($errors)
    }
}

function Test-EpochFiveStagePathPolicy {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)][string]$StageRoot,
        [Parameter(Mandatory = $true)][string]$Coordinate
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $expectedParent = Resolve-EpochFiveRepoRelativePath `
        -RepositoryRoot $RepositoryRoot `
        -RelativePath 'target/s114-experiment/recovery-stage'
    $resolvedStage = [System.IO.Path]::GetFullPath($StageRoot)
    $ownerPath = Split-Path -Parent $resolvedStage
    $stageParent = Split-Path -Parent $ownerPath
    $ownerName = Split-Path -Leaf $ownerPath
    $leaf = Split-Path -Leaf $resolvedStage
    if ($stageParent -cne $expectedParent) {
        $errors.Add('stage path is outside the exact recovery-stage parent')
    }
    if ($ownerName -cnotmatch '^[0-9a-f]{32}$') {
        $errors.Add('stage owner is not a lowercase 32-hex GUID')
    }
    if ($leaf -cne $Coordinate -or $Coordinate -cne 'e03-01-q4-32768') {
        $errors.Add('stage leaf is not the exact recovery coordinate')
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        stage_parent = $expectedParent
        owner_path = $ownerPath
        stage_path = $resolvedStage
        errors = @($errors)
    }
}

function Test-EpochFivePublicationState {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][bool]$DestinationExists,
        [Parameter(Mandatory = $true)][bool]$DestinationExact,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeExists,
        [Parameter(Mandatory = $true)][bool]$LegacyEnvelopeExact,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceExists,
        [Parameter(Mandatory = $true)][bool]$CorrectionEvidenceExact
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $action = $null
    if ($DestinationExists -and -not $DestinationExact) {
        $errors.Add('existing destination is not exact')
    }
    if ($LegacyEnvelopeExists -and -not $LegacyEnvelopeExact) {
        $errors.Add('existing legacy envelope is not exact')
    }
    if ($CorrectionEvidenceExists -and -not $CorrectionEvidenceExact) {
        $errors.Add('existing correction evidence is not exact')
    }
    if (($LegacyEnvelopeExists -or $CorrectionEvidenceExists) -and
        -not $DestinationExists) {
        $errors.Add('publication evidence exists without its destination')
    }
    if ($CorrectionEvidenceExists -and -not $LegacyEnvelopeExists) {
        $errors.Add('correction evidence exists without its legacy envelope')
    }
    if ($errors.Count -eq 0) {
        if (-not $DestinationExists -and -not $LegacyEnvelopeExists -and
            -not $CorrectionEvidenceExists) {
            $action = 'publish_fresh'
        }
        elseif ($DestinationExists -and -not $LegacyEnvelopeExists -and
            -not $CorrectionEvidenceExists) {
            $action = 'resume_exact_destination'
        }
        elseif ($DestinationExists -and $LegacyEnvelopeExists -and
            -not $CorrectionEvidenceExists) {
            $action = 'complete_correction_evidence'
        }
        elseif ($DestinationExists -and $LegacyEnvelopeExists -and
            $CorrectionEvidenceExists) {
            $action = 'already_complete'
        }
        else {
            $errors.Add('publication state is not an authorized transition')
        }
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        action = $action
        errors = @($errors)
    }
}

function Test-EpochFourFrozenDependencySet {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$RepositoryRoot,
        [Parameter(Mandatory = $true)]$EpochFivePlan,
        [Parameter(Mandatory = $true)][string]$ExpectedHead
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    $staticChecked = 0
    $transitiveChecked = 0
    try {
        $epochFourDir = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.failed_publication_artifact_relative_path)
        $epochThreeDir = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.source_artifact_relative_path)
        $epochFourControlPath = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.epoch_4.control_manifest.relative_path)
        $epochFourDigestPath = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.epoch_4.control_digest.relative_path)
        $epochFourControls = Get-Content -Raw -LiteralPath $epochFourControlPath |
            ConvertFrom-Json -DateKind String
        $epochFourDigest = (Get-Content -Raw `
                -LiteralPath $epochFourDigestPath).TrimEnd("`r", "`n")
        if ($epochFourDigest -cne
                [string]$EpochFivePlan.epoch_4.control_manifest_digest_line -or
            $epochFourDigest -cne
                "$((Get-Sha256Lower -Path $epochFourControlPath))  control-inputs.json") {
            $errors.Add('epoch-4 frozen control digest differs')
        }
        if ([string]$epochFourControls.schema -cne
                'animus-ferric-runtime-recovery-control-inputs-v4' -or
            [string]$epochFourControls.task -cne 'T-11409' -or
            [string]$epochFourControls.operation_id -cne
                [string]$EpochFivePlan.operation.failed_operation_id -or
            [int]$epochFourControls.execution_epoch -ne 3 -or
            [int]$epochFourControls.publication_epoch -ne 4 -or
            [string]$epochFourControls.timestamp_protocol -cne
                [string]$EpochFivePlan.timestamp_protocol -or
            [string]$epochFourControls.repository.head_at_freeze -cne
                $ExpectedHead -or
            [string]$epochFourControls.runtime_plan_sha256 -cne
                [string]$EpochFivePlan.epoch_4.runtime_plan.sha256 -or
            [string]$epochFourControls.raw_source_anchor_sha256 -cne
                [string]$EpochFivePlan.epoch_4.raw_source_anchor.sha256 -or
            -not [bool]$epochFourControls.epoch_3.passed -or
            -not [bool]$epochFourControls.runtime_self_test.passed -or
            -not [bool]$epochFourControls.source_verification.passed -or
            [bool]$epochFourControls.source_verification.hash_deferral_used -or
            -not [bool]$epochFourControls.model.passed -or
            -not [bool]$epochFourControls.model.independently_rehashed) {
            $errors.Add('epoch-4 frozen control identity differs')
        }

        $expectedStaticNames = @(Get-EpochFourStaticControlNames)
        $staticEntries = @($epochFourControls.static_controls)
        if ($expectedStaticNames.Count -ne 12 -or $staticEntries.Count -ne 12) {
            $errors.Add('epoch-4 static control count differs')
        }
        else {
            for ($index = 0; $index -lt $expectedStaticNames.Count; $index++) {
                $name = [string]$expectedStaticNames[$index]
                $entry = $staticEntries[$index]
                if ([string]$entry.path -cne $name) {
                    $errors.Add("epoch-4 static order differs at: $name")
                    continue
                }
                $path = Join-Path $epochFourDir $name
                if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    $errors.Add("epoch-4 static control is absent: $name")
                    continue
                }
                $item = Get-Item -LiteralPath $path -Force
                if ($item.Attributes.HasFlag(
                        [System.IO.FileAttributes]::ReparsePoint
                    ) -or [UInt64]$item.Length -ne [UInt64]$entry.bytes -or
                    (Get-Sha256Lower -Path $path) -cne [string]$entry.sha256) {
                    $errors.Add("epoch-4 static control differs: $name")
                    continue
                }
                $staticChecked++
            }
        }
        $epochFourSelfTestPath = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.epoch_4.runtime_self_test.relative_path)
        $epochFourSelfTest = Get-Item -LiteralPath $epochFourSelfTestPath -Force
        if ($epochFourSelfTest.Attributes.HasFlag(
                [System.IO.FileAttributes]::ReparsePoint
            ) -or [UInt64]$epochFourSelfTest.Length -ne
                [UInt64]$EpochFivePlan.epoch_4.runtime_self_test.bytes -or
            (Get-Sha256Lower -Path $epochFourSelfTestPath) -cne
                [string]$EpochFivePlan.epoch_4.runtime_self_test.sha256 -or
            [UInt64]$epochFourSelfTest.Length -ne
                [UInt64]$epochFourControls.runtime_self_test.bytes -or
            (Get-Sha256Lower -Path $epochFourSelfTestPath) -cne
                [string]$epochFourControls.runtime_self_test.sha256) {
            $errors.Add('epoch-4 runtime self-test differs')
        }

        foreach ($name in @(
                'control_manifest',
                'control_digest',
                'runtime_plan',
                'runtime_self_test'
            )) {
            if (-not (Test-JsonEquivalent `
                    -Left $epochFourControls.epoch_3.$name `
                    -Right $EpochFivePlan.epoch_3.$name)) {
                $errors.Add("epoch-4/epoch-5 epoch-3 anchor differs: $name")
            }
        }
        if ([string]$epochFourControls.epoch_3.control_manifest_digest_line -cne
            [string]$EpochFivePlan.epoch_3.control_manifest_digest_line) {
            $errors.Add('epoch-3 digest-line anchor differs across epochs')
        }

        $epochThreeControlPath = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.epoch_3.control_manifest.relative_path)
        $epochThreeDigestPath = Resolve-EpochFiveRepoRelativePath `
            -RepositoryRoot $RepositoryRoot `
            -RelativePath ([string]$EpochFivePlan.epoch_3.control_digest.relative_path)
        $epochThreeDigest = (Get-Content -Raw `
                -LiteralPath $epochThreeDigestPath).TrimEnd("`r", "`n")
        if ($epochThreeDigest -cne
                [string]$EpochFivePlan.epoch_3.control_manifest_digest_line -or
            $epochThreeDigest -cne
                "$((Get-Sha256Lower -Path $epochThreeControlPath))  control-inputs.json") {
            $errors.Add('epoch-3 frozen control digest differs')
        }
        $epochThreeControls = Get-Content -Raw -LiteralPath $epochThreeControlPath |
            ConvertFrom-Json -DateKind String
        if ([string]$epochThreeControls.schema -cne
                'animus-ferric-runtime-control-inputs-v3' -or
            [string]$epochThreeControls.task -cne 'T-11409' -or
            [int]$epochThreeControls.control_epoch -ne 3 -or
            [string]$epochThreeControls.repository.head_at_freeze -cne
                $ExpectedHead) {
            $errors.Add('epoch-3 frozen control identity differs')
        }
        $transitiveEntries = @($epochThreeControls.controls)
        if ($transitiveEntries.Count -ne 20) {
            $errors.Add('epoch-3 transitive control count differs')
        }
        foreach ($entry in $transitiveEntries) {
            $relative = [string]$entry.path
            try {
                if ([string]::IsNullOrWhiteSpace($relative) -or
                    [System.IO.Path]::IsPathRooted($relative) -or
                    $relative.IndexOf([char]0) -ge 0 -or
                    $relative.IndexOf(':') -ge 0 -or
                    $relative -match '(^|[\\/])\.{1,2}([\\/]|$)') {
                    throw 'unsafe relative control path'
                }
                $path = [System.IO.Path]::GetFullPath(
                    (Join-Path $epochThreeDir $relative)
                )
                $prefix = $epochThreeDir.TrimEnd(
                    [System.IO.Path]::DirectorySeparatorChar,
                    [System.IO.Path]::AltDirectorySeparatorChar
                ) + [System.IO.Path]::DirectorySeparatorChar
                if (-not $path.StartsWith(
                        $prefix,
                        [System.StringComparison]::OrdinalIgnoreCase
                    ) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
                    throw 'control path is absent or outside epoch 3'
                }
                $item = Get-Item -LiteralPath $path -Force
                if ($item.Attributes.HasFlag(
                        [System.IO.FileAttributes]::ReparsePoint
                    ) -or [UInt64]$item.Length -ne [UInt64]$entry.bytes -or
                    (Get-Sha256Lower -Path $path) -cne [string]$entry.sha256) {
                    throw 'control identity differs'
                }
                $transitiveChecked++
            }
            catch {
                $errors.Add(
                    "epoch-3 transitive control differs: ${relative}: $($_.Exception.Message)"
                )
            }
        }
    }
    catch {
        $errors.Add("frozen dependency traversal failed: $($_.Exception.Message)")
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        static_controls_checked = $staticChecked
        transitive_epoch_3_controls_checked = $transitiveChecked
        errors = @($errors)
    }
}

function Test-EpochFourVerificationReport {
    [CmdletBinding()]
    param(
        [AllowNull()][Parameter(Mandatory = $true)]$Report,
        [AllowNull()][Parameter(Mandatory = $true)]$RecoveryPlan,
        [AllowNull()][Parameter(Mandatory = $true)]$SourcePlan,
        [Parameter(Mandatory = $true)][string]$ExpectedAttemptPath,
        [Parameter(Mandatory = $true)]
        [ValidateSet(
            'epoch_4_frozen_publication_stage',
            'epoch_4_frozen_recovery'
        )]
        [string]$ExpectedAnchorMode
    )

    $errors = [System.Collections.Generic.List[string]]::new()
    if ($null -eq $Report) { $errors.Add('epoch-4 verifier report is absent') }
    if ($null -eq $RecoveryPlan) { $errors.Add('epoch-4 recovery plan is absent') }
    if ($null -eq $SourcePlan) { $errors.Add('epoch-3 source plan is absent') }
    if ($errors.Count -eq 0) {
        try {
            $expectedPath = [System.IO.Path]::GetFullPath($ExpectedAttemptPath)
            if ([string]$Report.schema -cne
                    'animus-ferric-runtime-recovery-verification-v4') {
                $errors.Add('verifier schema is not recovery-qualified v4')
            }
            if ([string]$Report.task -cne 'T-11409') {
                $errors.Add('verifier task differs')
            }
            if ([string]$Report.operation_id -cne
                    [string]$RecoveryPlan.operation.id) {
                $errors.Add('verifier operation differs')
            }
            if ([int]$Report.execution_epoch -ne 3 -or
                [int]$Report.publication_epoch -ne 4 -or
                [int]$Report.control_epoch -ne 3) {
                $errors.Add('verifier epoch tuple differs')
            }
            if ([string]$Report.source_attempt_schema -cne
                    [string]$RecoveryPlan.operation.source_attempt_schema) {
                $errors.Add('verifier source schema differs')
            }
            if ([string]$Report.timestamp_protocol -cne
                    [string]$RecoveryPlan.timestamp_protocol) {
                $errors.Add('verifier timestamp protocol differs')
            }
            if ([string]$Report.attestation_protocol -cne
                    [string]$SourcePlan.template_attestation.protocol) {
                $errors.Add('verifier template-attestation protocol differs')
            }
            if ([string]$Report.process_command_protocol -cne
                    [string]$SourcePlan.process_command_attestation.protocol) {
                $errors.Add('verifier process-command protocol differs')
            }
            if (-not [System.IO.Path]::GetFullPath(
                    [string]$Report.attempt_path
                ).Equals(
                    $expectedPath,
                    [System.StringComparison]::OrdinalIgnoreCase
                )) {
                $errors.Add('verifier attempt path differs')
            }
            if ([string]$Report.coordinate -cne
                    [string]$RecoveryPlan.operation.coordinate -or
                [string]$Report.verdict -cne
                    [string]$RecoveryPlan.operation.expected_terminal.verdict) {
                $errors.Add('verifier coordinate or verdict differs')
            }
            if ([string]$Report.control_anchor_mode -cne $ExpectedAnchorMode) {
                $errors.Add('verifier control-anchor mode differs')
            }
            if (-not [bool]$Report.live_model_identity.checked -or
                [string]$Report.live_model_identity.mode -cne
                    'checked_in_verifier' -or
                [string]$Report.live_model_identity.sha256 -cne
                    [string]$RecoveryPlan.model.sha256) {
                $errors.Add('verifier did not check the exact live model hash')
            }
            if (-not [bool]$Report.manifest.passed -or
                [int]$Report.manifest.entries -ne 49 -or
                [int]$Report.manifest.entries -ne
                    [int]$RecoveryPlan.operation.exact_manifest_entries) {
                $errors.Add('verifier exact manifest result differs')
            }
            if (-not [bool]$Report.recovery_anchor.applicable -or
                -not [bool]$Report.recovery_anchor.passed -or
                [int]$Report.recovery_anchor.expected_entries -ne 49 -or
                [int]$Report.recovery_anchor.observed_entries -ne 49) {
                $errors.Add('verifier recovery anchor is not applicable and exact')
            }
            if (-not [bool]$Report.passed -or @($Report.errors).Count -ne 0) {
                $errors.Add('verifier report is not a clean pass')
            }
        }
        catch {
            $errors.Add("verifier report is malformed: $($_.Exception.Message)")
        }
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        errors = @($errors)
    }
}

function Invoke-EpochFourVerification {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$VerifierPath,
        [Parameter(Mandatory = $true)][string]$AttemptPath,
        [Parameter(Mandatory = $true)]$RecoveryPlan,
        [Parameter(Mandatory = $true)]$SourcePlan,
        [switch]$RecoveryPublicationStage
    )

    $arguments = @('-AttemptPath', $AttemptPath)
    $expectedAnchorMode = 'epoch_4_frozen_recovery'
    if ($RecoveryPublicationStage) {
        $arguments += '-RecoveryPublicationStage'
        $expectedAnchorMode = 'epoch_4_frozen_publication_stage'
    }
    $process = Invoke-PowerShellFileBounded -ScriptPath $VerifierPath `
        -Arguments $arguments
    $report = try {
        $process.stdout | ConvertFrom-Json -DateKind String
    }
    catch { $null }
    $validation = Test-EpochFourVerificationReport -Report $report `
        -RecoveryPlan $RecoveryPlan -SourcePlan $SourcePlan `
        -ExpectedAttemptPath $AttemptPath `
        -ExpectedAnchorMode $expectedAnchorMode
    if ($process.exit_code -ne 0 -or -not [bool]$validation.passed) {
        $messages = @(
            @($validation.errors)
            if (-not [string]::IsNullOrWhiteSpace([string]$process.stderr)) {
                [string]$process.stderr
            }
        )
        throw "epoch-4 verification failed: $($messages -join '; ')"
    }
    $report
}

function Write-EpochFiveJsonAtomic {
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
    $tempName = ".epoch5-$([guid]::NewGuid().ToString('N')).tmp"
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
                '^\.epoch5-[0-9a-f]{32}\.tmp$') {
            [System.IO.File]::Delete($tempPath)
        }
    }
}

function Get-EpochFiveColdState {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$RepositoryRoot)

    $errors = [System.Collections.Generic.List[string]]::new()
    $localRunfile = Join-Path $RepositoryRoot '.ferric/server.json'
    $globalRunfile = if ([string]::IsNullOrWhiteSpace([string]$env:APPDATA)) {
        $null
    }
    else { Join-Path $env:APPDATA 'ferric/server.json' }
    $listeners = try {
        @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
                Where-Object { [int]$_.LocalPort -eq 8080 })
    }
    catch {
        $errors.Add("listener query failed: $($_.Exception.Message)")
        @('query_failed')
    }
    $llamaProcesses = try {
        @(Get-CimInstance Win32_Process -Filter "Name = 'llama-server.exe'" `
                -ErrorAction Stop)
    }
    catch {
        $errors.Add("llama-server process query failed: $($_.Exception.Message)")
        @('query_failed')
    }
    $state = [ordered]@{
        local_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        global_runfile_absent = $null -ne $globalRunfile -and
            -not (Test-Path -LiteralPath $globalRunfile)
        listener_absent = (@($listeners).Count -eq 0)
        llama_server_process_absent = (@($llamaProcesses).Count -eq 0)
    }
    if (-not $state.local_runfile_absent) { $errors.Add('local runfile exists') }
    if (-not $state.global_runfile_absent) { $errors.Add('global runfile exists') }
    if (-not $state.listener_absent) { $errors.Add('listener exists on port 8080') }
    if (-not $state.llama_server_process_absent) {
        $errors.Add('llama-server process exists')
    }
    [ordered]@{
        passed = ($errors.Count -eq 0)
        state = $state
        errors = @($errors)
    }
}
