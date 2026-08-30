[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$artifact = $PSScriptRoot
$commonPath = Join-Path $artifact 'runtime-common.ps1'
. $commonPath
$context = Get-S115Context

function Get-AttemptSnapshot {
    param([string[]]$Roots)
    @(
        foreach ($root in $Roots) {
            if (Test-Path -LiteralPath $root -PathType Container) {
                Get-ChildItem -LiteralPath $root -Directory -Force |
                    ForEach-Object {
                        [ordered]@{
                            root = [System.IO.Path]::GetFullPath($root)
                            name = $_.Name
                            creation_utc = $_.CreationTimeUtc.ToString('o')
                        }
                    }
            }
        }
    ) | ConvertTo-Json -Depth 8 -Compress
}

$attemptRoots = @($context.tracked_attempt_root, $context.raw_attempt_root)
$before = Get-AttemptSnapshot -Roots $attemptRoots
$checks = [System.Collections.Generic.List[object]]::new()
function Assert-Check {
    param([bool]$Condition, [string]$Name)
    if (-not $Condition) { throw "static control check failed: $Name" }
    $checks.Add([ordered]@{ name = $Name; passed = $true })
}

$powershellFiles = @(Get-ChildItem -LiteralPath $artifact -Filter '*.ps1' -File)
foreach ($file in $powershellFiles) {
    $tokens = $null
    $parseErrors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile(
        $file.FullName,
        [ref]$tokens,
        [ref]$parseErrors
    )
    Assert-Check -Condition (@($parseErrors).Count -eq 0) `
        -Name "PowerShell parses: $($file.Name)"
}

$control = Assert-S115ControlInputs -Context $context
Assert-Check -Condition ($control.entries -eq 15) `
    -Name 'control-inputs freezes all fifteen root controls'
Assert-Check -Condition ((Get-S115RawSha256 -Path $script:S115FrozenCommonPath) `
    -ceq $script:S115FrozenCommonSha256) `
    -Name 'Sprint 114 helper is hash-bound before dot-source'

$plan = $context.plan
Assert-Check -Condition (
    $plan.schema -ceq 'animus-ferric-s115-runtime-plan-v1' -and
    $plan.task -ceq 'T-11503' -and
    [int]$plan.policy.attempt_wall_seconds -eq 5400 -and
    [bool]$plan.policy.no_qualification_attempt_retry -and
    [string]$plan.policy.provider_retry_policy -ceq
        'ferric-built-in-transient-backoff-not-disabled-or-claimed-zero' -and
    [bool]$plan.policy.no_fallback -and
    [bool]$plan.policy.no_download -and
    [int]$plan.coordinate.context -eq 32768 -and
    [int]$plan.coordinate.gpu_layers -eq 24 -and
    [int]$plan.coordinate.threads -eq 12 -and
    [int]$plan.coordinate.batch_size -eq 512 -and
    [int]$plan.coordinate.seed -eq 42 -and
    [int]$plan.coordinate.parallel_slots -eq 1 -and
    [int]$plan.coordinate.port -eq 8080 -and
    [string]$plan.wsl.bubblewrap_version -ceq '0.11.1' -and
    [UInt64]$plan.model.bytes -eq 16464440224 -and
    [string]$plan.model.sha256 -ceq
        '322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482'
) -Name 'plan freezes the sole Qwen Q4 coordinate and policy'
Assert-Check -Condition (
    (@($plan.throughput.sequence) -join ',') -ceq
        'warmup,trial-01,trial-02,trial-03' -and
    [int]$plan.throughput.scored_samples -eq 3 -and
    [int]$plan.throughput.replacement_samples -eq 0
) -Name 'throughput is exactly one warmup and three trials'
Assert-Check -Condition (
    (@($plan.forbidden_inherited_environment) -join ',') -ceq
        'LLAMA_ARG_*,FERRIC_*,GGML_*,CUDA_*,OMP_*,MKL_*,OPENAI_API_KEY,HTTP_PROXY,HTTPS_PROXY,ALL_PROXY,NO_PROXY'
) -Name 'undeclared inherited runtime tuning is rejected'
$sourceManifest = Get-Content -Raw -LiteralPath $context.source_manifest_path |
    ConvertFrom-Json
Assert-Check -Condition (
    @($sourceManifest.binaries.llama_runtime.files).Count -eq 55
) -Name 'runtime identity covers the exact 55-file tree'
$syntheticLog = @'
load_tensors: offloaded 24 / 65 layers to GPU
kv cache: K (q8_0) : 1024 MiB, V (q8_0) : 1024 MiB
server: flash_attn = on
server: chat template, thinking = 1
'@
$syntheticLogFacts = Get-S115ServerLogFacts -Text $syntheticLog `
    -Context $context
Assert-Check -Condition (
    $syntheticLogFacts.passed -and
    [int]$syntheticLogFacts.value.effective_gpu_layers -eq 24 -and
    [int]$syntheticLogFacts.value.preserve_warning_count -eq 0 -and
    [string]$syntheticLogFacts.sha256 -match '^[0-9a-f]{64}$'
) -Name 'shared server-log parser derives the frozen effective runtime facts'
$retainedBubblewrapFixture = @'
Linux 6.6.114.1-microsoft-standard-WSL2 x86_64 GNU/Linux
bubblewrap 0.11.1
S115_NETWORK_NAMESPACE_ONLY_LOOPBACK=1
'@
$bubblewrapFacts = Get-S115BubblewrapVersionFacts `
    -Output $retainedBubblewrapFixture -ExpectedVersion '0.11.1'
$deceptiveBubblewrapFixtures = @(
    '',
    'bwrap 0.11.1',
    'bubblewrap 0.11',
    'bubblewrap 0.11.1 extra',
    "bubblewrap 0.11.1`nbubblewrap 0.11.1",
    'not-bubblewrap 0.11.1',
    'bubblewrap 0.11.2'
)
$deceptiveAccepted = @($deceptiveBubblewrapFixtures | Where-Object {
    (Get-S115BubblewrapVersionFacts -Output $_ `
        -ExpectedVersion '0.11.1').passed
})
Assert-Check -Condition (
    $bubblewrapFacts.passed -and
    [string]$bubblewrapFacts.observed_version -ceq '0.11.1' -and
    [string]$bubblewrapFacts.exact_line -ceq 'bubblewrap 0.11.1' -and
    $deceptiveAccepted.Count -eq 0
) -Name 'Bubblewrap parser accepts retained exact output and rejects deceptive forms'

$attempt002Timestamp = '2026-08-28T08:57:26.6784000+00:00'
$attempt002HandoffPath = Join-Path $context.tracked_attempt_root `
    '002/handoff.json'
$attempt002Handoff = Read-S115EvidenceJson -Path $attempt002HandoffPath
Assert-Check -Condition (
    $attempt002Handoff.process.creation_utc -is [string] -and
    $attempt002Handoff.process.creation_utc -ceq $attempt002Timestamp -and
    (ConvertTo-S115CanonicalUtcInstant `
        -Value $attempt002Handoff.process.creation_utc) -ceq $attempt002Timestamp
) -Name 'attempt 002 ISO creation instant survives retained JSON round trip'

$originalCulture = [System.Threading.Thread]::CurrentThread.CurrentCulture
$originalUiCulture = [System.Threading.Thread]::CurrentThread.CurrentUICulture
try {
    $nonDefaultCulture = [System.Globalization.CultureInfo]::GetCultureInfo(
        'fr-FR'
    )
    [System.Threading.Thread]::CurrentThread.CurrentCulture = $nonDefaultCulture
    [System.Threading.Thread]::CurrentThread.CurrentUICulture = $nonDefaultCulture
    $legacyDate = ('{"creation_utc":"' + $attempt002Timestamp + '"}' |
        ConvertFrom-Json).creation_utc
    $legacyCanonical = ConvertTo-S115CanonicalUtcInstant -Value $legacyDate
}
finally {
    [System.Threading.Thread]::CurrentThread.CurrentCulture = $originalCulture
    [System.Threading.Thread]::CurrentThread.CurrentUICulture = $originalUiCulture
}
$differentInstant = Test-S115UtcInstantEquivalent `
    -Left $attempt002Timestamp `
    -Right '2026-08-28T08:57:26.6784001+00:00'
Assert-Check -Condition (
    $legacyDate -is [DateTime] -and
    $legacyCanonical -ceq $attempt002Timestamp -and
    -not $differentInstant.passed
) -Name 'legacy DateTime canonicalization is culture invariant and rejects a different instant'

$currentManifestSha256 = [string]$control.manifest_sha256
$currentCompatibility = Test-S115VerifierControlManifestCompatibility `
    -AttemptId '999' `
    -AttemptSourceManifestSha256 $currentManifestSha256 `
    -CurrentManifestSha256 $currentManifestSha256
$predecessorCompatibility = Test-S115VerifierControlManifestCompatibility `
    -AttemptId '002' `
    -AttemptSourceManifestSha256 `
        $script:S115VerifierPredecessorControlManifestSha256 `
    -CurrentManifestSha256 $currentManifestSha256
$arbitraryCompatibility = Test-S115VerifierControlManifestCompatibility `
    -AttemptId '002' `
    -AttemptSourceManifestSha256 ('a' * 64) `
    -CurrentManifestSha256 $currentManifestSha256
$uppercaseCompatibility = Test-S115VerifierControlManifestCompatibility `
    -AttemptId '002' `
    -AttemptSourceManifestSha256 `
        $script:S115VerifierPredecessorControlManifestSha256.ToUpperInvariant() `
    -CurrentManifestSha256 $currentManifestSha256
$wrongAttemptCompatibility = Test-S115VerifierControlManifestCompatibility `
    -AttemptId '003' `
    -AttemptSourceManifestSha256 `
        $script:S115VerifierPredecessorControlManifestSha256 `
    -CurrentManifestSha256 $currentManifestSha256
Assert-Check -Condition (
    $currentCompatibility.passed -and
    $predecessorCompatibility.passed -and
    -not $arbitraryCompatibility.passed -and
    -not $uppercaseCompatibility.passed -and
    -not $wrongAttemptCompatibility.passed
) -Name 'verifier admits the exact predecessor only for attempt 002'

$heldThreeTicks = Test-S115HeldHandleCreationInstant `
    -Observed '2026-08-28T08:57:26.6784003+00:00' `
    -Expected $attempt002Timestamp
$heldTenTicks = Test-S115HeldHandleCreationInstant `
    -Observed '2026-08-28T08:57:26.6784010+00:00' `
    -Expected $attempt002Timestamp
$heldElevenTicks = Test-S115HeldHandleCreationInstant `
    -Observed '2026-08-28T08:57:26.6784011+00:00' `
    -Expected $attempt002Timestamp
Assert-Check -Condition (
    $heldThreeTicks.passed -and [Int64]$heldThreeTicks.delta_ticks -eq 3 -and
    $heldTenTicks.passed -and [Int64]$heldTenTicks.delta_ticks -eq 10 -and
    -not $heldElevenTicks.passed -and
    [Int64]$heldElevenTicks.delta_ticks -eq 11
) -Name 'held-handle cleanup accepts at most the fixed ten-tick cross-API delta'

$attributes = (Get-Content -Raw -LiteralPath (Join-Path $artifact `.gitattributes)).Replace("`r", '')
Assert-Check -Condition ($attributes -ceq "* text eol=lf`nattempts/** -text`n") `
    -Name 'control LF and evidence raw-byte checkout rules are frozen'

$qualifierPath = Join-Path $artifact 'qualify-runtime.ps1'
$qualifierText = Get-Content -Raw -LiteralPath $qualifierPath
$qualifierTokens = $null
$qualifierErrors = $null
$qualifierAst = [System.Management.Automation.Language.Parser]::ParseFile(
    $qualifierPath,
    [ref]$qualifierTokens,
    [ref]$qualifierErrors
)
$fileRedirectCalls = @($qualifierAst.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'Invoke-FileRedirectedProcess'
}, $true))
Assert-Check -Condition ($fileRedirectCalls.Count -eq 1) `
    -Name 'qualifier contains exactly one managed server launch callsite'
Assert-Check -Condition (
    $qualifierText.Contains("launch_ordinal = 1") -and
    $qualifierText.Contains("'server', 'up'") -and
    $qualifierText.Contains('Open-S115RuntimeLock') -and
    $qualifierText.IndexOf('New-S115SafeDirectory') -lt
        $qualifierText.IndexOf('Open-S115RuntimeLock') -and
    $qualifierText.Contains('Get-S115NextAttemptId') -and
    $qualifierText.Contains('Invoke-S115WslIsolationProbe') -and
    $qualifierText.Contains('inherited_forbidden_environment_names') -and
    $qualifierText.Contains("Join-Path `$raw 'launch-live.stdout.log'") -and
    $qualifierText.Contains('external trace root must be absent') -and
    $qualifierText.Contains('Get-S115RuntimeIdentity -Context $context') -and
    $qualifierText.Contains('Test-S115LiveHandoff') -and
    ([regex]::Matches($qualifierText,
        'Get-S115ServerLogFacts').Count -eq 2) -and
    $qualifierText.Contains('Get-S115BareEngineResolutionProof') -and
    $qualifierText.Contains('parent-process-scoped-inheritance-restored') -and
    ([regex]::Matches($qualifierText,
        '\$context\.artifact_directory').Count -eq 3) -and
    -not $qualifierText.Contains('Invoke-WebRequest') -and
    -not $qualifierText.Contains('Start-BitsTransfer') -and
    -not $qualifierText.Contains('huggingface.co')
) -Name 'qualifier encodes lock/no-download/external-trace/final-live controls'

. $qualifierPath
. (Join-Path $artifact 'verify-runtime.ps1')
. (Join-Path $artifact 'verify-handoff.ps1')
$liveFlagPropagates = & {
    $Attempt = '007'
    $CheckLive = $false
    . (Join-Path $artifact 'verify-handoff.ps1') `
        -Attempt $Attempt -CheckLive
    [bool]$CheckLive
}
Assert-Check -Condition $liveFlagPropagates `
    -Name 'handoff verifier preserves an explicit CheckLive switch'
$qualifierCommand = Get-Command Invoke-S115RuntimeQualification
$nonCommonParameters = @($qualifierCommand.Parameters.Keys | Where-Object {
    $_ -notin @('Verbose', 'Debug', 'ErrorAction', 'WarningAction',
        'InformationAction', 'ProgressAction', 'ErrorVariable', 'WarningVariable',
        'InformationVariable', 'OutVariable', 'OutBuffer', 'PipelineVariable')
})
Assert-Check -Condition ($nonCommonParameters.Count -eq 0) `
    -Name 'qualification command accepts no override parameters'

foreach ($name in @('verify-runtime.ps1', 'verify-handoff.ps1')) {
    $text = Get-Content -Raw -LiteralPath (Join-Path $artifact $name)
    Assert-Check -Condition (
        $text -notmatch '(?m)\b(?:Write-JsonLf|Write-Utf8Lf|Copy-Item|New-Item|Remove-Item|Invoke-CapturedProcess|Invoke-FileRedirectedProcess)\b'
    ) -Name "$name is read-only"
}
$runtimeVerifierText = Get-Content -Raw -LiteralPath (Join-Path `
    $artifact 'verify-runtime.ps1')
Assert-Check -Condition (
    $runtimeVerifierText.Contains('environmentPassed') -and
    $runtimeVerifierText.Contains('throughput-request.json is absent') -and
    $runtimeVerifierText.Contains('single_ferric_server_up') -and
    $runtimeVerifierText.Contains('expectedJournalSequence') -and
    $runtimeVerifierText.Contains('attempt_wall_seconds') -and
    $runtimeVerifierText.Contains('engine-resolution.json') -and
    $runtimeVerifierText.Contains('model-inventory.before.json') -and
    $runtimeVerifierText.Contains('launch-stream-prefixes.json')
) -Name 'offline verifier derives environment, journal, request, and hash links'
$commonText = Get-Content -Raw -LiteralPath $commonPath
Assert-Check -Condition (
    $commonText.Contains('[System.Diagnostics.Process]::GetProcessById') -and
    $commonText.Contains('$process.Kill()') -and
    $commonText.Contains('function Get-S115ServerLogFacts') -and
    $commonText.Contains('function Get-S115BubblewrapVersionFacts') -and
    $commonText.Contains('function Read-S115EvidenceJson') -and
    $commonText.Contains('ConvertFrom-Json -DateKind String') -and
    $commonText.Contains('function ConvertTo-S115CanonicalUtcInstant') -and
    $commonText.Contains('function Test-S115HeldHandleCreationInstant') -and
    $commonText.Contains('$bubblewrapVersion.passed') -and
    -not $commonText.Contains("Contains('bwrap ')") -and
    -not $runtimeVerifierText.Contains("Contains('bwrap ')") -and
    $commonText.Contains('live server-log prefix effective facts changed') -and
    $commonText.Contains('retained_process_or_listener_exit_unconfirmed') -and
    $commonText.Contains('Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue') -and
    -not $commonText.Contains("@('server', 'down')") -and
    -not $qualifierText.Contains("@('server', 'down')")
) -Name 'cleanup uses held exact process and lock paths reject reparse entries'

$temporaryBase = [System.IO.Path]::GetTempPath()
$temporary = [System.IO.Path]::Combine(
    $temporaryBase,
    "animus-ferric-s115-static-$([Guid]::NewGuid().ToString('N'))"
)
try {
    [System.IO.Directory]::CreateDirectory($temporary) | Out-Null
    $safeNested = Join-Path $temporary 'safe/nested'
    $null = New-S115SafeDirectory -Root $temporary -Path $safeNested
    $safeCheck = Test-S115SafeDirectoryTraversal -Root $temporary `
        -Path $safeNested -RequireTarget
    Assert-Check -Condition $safeCheck.passed `
        -Name 'one-command allocation safely creates ordinary missing parents'
    $trackedTest = Join-Path $temporary 'tracked'
    $rawTest = Join-Path $temporary 'raw'
    [System.IO.Directory]::CreateDirectory((Join-Path $trackedTest '001')) |
        Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $rawTest '003')) |
        Out-Null
    Assert-Check -Condition ((Get-S115NextAttemptId -TrackedRoot $trackedTest `
        -RawRoot $rawTest) -ceq '004') `
        -Name 'numeric attempt allocator advances across tracked and raw roots'
    $lockPath = Join-Path $temporary 'runtime.lock'
    $firstLock = Open-S115RuntimeLock -Path $lockPath
    try {
        $secondRejected = $false
        try { $second = Open-S115RuntimeLock -Path $lockPath; $second.Dispose() }
        catch { $secondRejected = $true }
        Assert-Check -Condition $secondRejected `
            -Name 'exclusive attempt lock rejects a concurrent owner'
    }
    finally { $firstLock.Dispose() }
    $reopened = Open-S115RuntimeLock -Path $lockPath
    $reopened.Dispose()
    Assert-Check -Condition $true -Name 'exclusive lock is reusable after release'
}
finally {
    $temporaryFull = [System.IO.Path]::GetFullPath($temporary)
    $temporaryRoot = [System.IO.Path]::GetFullPath($temporaryBase).TrimEnd('\') + '\'
    if (-not $temporaryFull.StartsWith(
        $temporaryRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    ) -or -not (Split-Path -Leaf $temporaryFull).StartsWith(
        'animus-ferric-s115-static-',
        [System.StringComparison]::Ordinal
    )) { throw 'refusing to remove an unverified static-test temporary path' }
    if (Test-Path -LiteralPath $temporaryFull) {
        [System.IO.Directory]::Delete($temporaryFull, $true)
    }
}

$after = Get-AttemptSnapshot -Roots $attemptRoots
Assert-Check -Condition ($after -ceq $before) `
    -Name 'static self-test creates no tracked or raw runtime attempt'

[pscustomobject][ordered]@{
    schema = 'animus-ferric-s115-runtime-control-static-test-v1'
    passed = $true
    checks = $checks.Count
    parsed_scripts = @($powershellFiles.Name | Sort-Object)
    attempt_snapshot_unchanged = $true
} | ConvertTo-Json -Depth 16
