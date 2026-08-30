[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$EvidenceRoot,
    [switch]$CheckLiveBinary
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'The T-11501 verifier requires PowerShell 7 or newer.'
}

$root = (Resolve-Path -LiteralPath $EvidenceRoot).Path
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$resultPath = Join-Path $root 'result.json'
$resultHashPath = Join-Path $root 'result.sha256'
$manifestPath = Join-Path $root 'files.sha256'
$journalPath = Join-Path $root 'journal.jsonl'

function Get-Sha256 {
    param([Parameter(Mandatory)][string]$LiteralPath)
    (Get-FileHash -Algorithm SHA256 -LiteralPath $LiteralPath).Hash.ToLowerInvariant()
}

function Assert-OrdinaryTree {
    param([Parameter(Mandatory)][string]$LiteralPath)
    foreach ($item in @(
        Get-Item -Force -LiteralPath $LiteralPath
        Get-ChildItem -Force -Recurse -LiteralPath $LiteralPath
    )) {
        if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
            throw "evidence tree contains a reparse point: $($item.FullName)"
        }
    }
}

function Get-SafeEvidencePath {
    param([Parameter(Mandatory)][string]$RelativePath)
    if ([System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath.Contains('\') -or
        @($RelativePath.Split('/') | Where-Object { $_ -in @('', '.', '..') }).Count -ne 0) {
        throw "manifest path is not a normalized slash-relative path: $RelativePath"
    }
    $candidate = [System.IO.Path]::GetFullPath((Join-Path $root $RelativePath))
    if (-not $candidate.StartsWith(
            $root.TrimEnd('\') + '\',
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "manifest path escapes evidence root: $RelativePath"
    }
    $candidate
}

function Read-TraceIdentity {
    param([Parameter(Mandatory)][string]$LiteralPath)
    $events = @(
        Get-Content -LiteralPath $LiteralPath |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
    $start = @($events | Where-Object { $_.event.type -eq 'session_start' })
    $end = @($events | Where-Object { $_.event.type -eq 'session_end' })
    if ($start.Count -ne 1 -or $end.Count -ne 1) {
        throw "retained trace does not contain one start and one end: $LiteralPath"
    }
    $session = [string]$start[0].session
    if ([string]::IsNullOrWhiteSpace($session) -or
        [string]$end[0].session -cne $session -or
        @($events | Where-Object { [string]$_.session -cne $session }).Count -ne 0) {
        throw "retained trace contains inconsistent session identities: $LiteralPath"
    }
    $resumedProperty = $start[0].event.PSObject.Properties['resumed_from']
    [pscustomobject]@{
        session = $session
        workspace = [string]$start[0].event.workspace
        resumed_from = if ($null -eq $resumedProperty -or $null -eq $resumedProperty.Value) {
            $null
        }
        else {
            [string]$resumedProperty.Value
        }
        reason = [string]$end[0].event.reason
    }
}

function Get-VerbatimCanonicalPath {
    param([Parameter(Mandatory)][string]$LiteralPath)
    $resolved = [System.IO.Path]::GetFullPath($LiteralPath)
    if ($resolved.StartsWith('\\?\')) {
        return $resolved
    }
    if ($resolved.StartsWith('\\')) {
        return '\\?\UNC\' + $resolved.TrimStart('\')
    }
    '\\?\' + $resolved
}

function Get-ResumeElements {
    param([Parameter(Mandatory)][string]$StandardError)
    $lines = @($StandardError -split "`r?`n" | Where-Object { $_.StartsWith('Resume: ') })
    if ($lines.Count -ne 1) {
        throw "expected one public Resume line, observed $($lines.Count)"
    }
    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput(
        $lines[0].Substring('Resume: '.Length),
        [ref]$tokens,
        [ref]$parseErrors
    )
    if ($parseErrors.Count -ne 0) {
        throw "captured Resume command is not valid PowerShell: $($parseErrors -join '; ')"
    }
    $commands = @($ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst]
            }, $true))
    if ($commands.Count -ne 1 -or $commands[0].Redirections.Count -ne 0) {
        throw 'captured Resume command must contain one command and no redirections.'
    }
    @($commands[0].CommandElements | ForEach-Object {
            if ($_ -isnot [System.Management.Automation.Language.StringConstantExpressionAst]) {
                throw "captured Resume command contains a non-literal element: $($_.Extent.Text)"
            }
            $_.Value
        })
}

function Assert-StringArrayExact {
    param(
        [Parameter(Mandatory)][object[]]$Actual,
        [Parameter(Mandatory)][string[]]$Expected,
        [Parameter(Mandatory)][string]$Label
    )
    if ($Actual.Count -ne $Expected.Count) {
        throw "$Label argv count mismatch: expected $($Expected.Count), observed $($Actual.Count)"
    }
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        if ([string]$Actual[$index] -cne $Expected[$index]) {
            throw "$Label argv[$index] mismatch."
        }
    }
}

foreach ($required in @($resultPath, $resultHashPath, $manifestPath, $journalPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "required evidence file is missing: $required"
    }
}
Assert-OrdinaryTree -LiteralPath $root

$resultHashLine = (Get-Content -Raw -LiteralPath $resultHashPath).Trim()
if ($resultHashLine -notmatch '^([0-9a-f]{64})  result\.json$') {
    throw 'result.sha256 has an invalid format.'
}
$expectedResultHash = $Matches[1]
if ((Get-Sha256 -LiteralPath $resultPath) -cne $expectedResultHash) {
    throw 'result.json does not match result.sha256.'
}

$manifestEntries = [ordered]@{}
foreach ($line in @(Get-Content -LiteralPath $manifestPath)) {
    if ($line -notmatch '^([0-9a-f]{64})  (.+)$') {
        throw "invalid files.sha256 line: $line"
    }
    $expectedHash = $Matches[1]
    $relative = $Matches[2]
    if ($manifestEntries.Contains($relative)) {
        throw "duplicate files.sha256 path: $relative"
    }
    $path = Get-SafeEvidencePath -RelativePath $relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "manifest payload is missing: $relative"
    }
    if ((Get-Sha256 -LiteralPath $path) -cne $expectedHash) {
        throw "manifest payload hash mismatch: $relative"
    }
    $manifestEntries[$relative] = $expectedHash
}

$actualFiles = @(
    Get-ChildItem -File -Recurse -LiteralPath $root |
        Where-Object { $_.FullName -cne $manifestPath } |
        ForEach-Object {
            [System.IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/')
        } |
        Sort-Object
)
$manifestFiles = @($manifestEntries.Keys | Sort-Object)
if ($actualFiles.Count -ne $manifestFiles.Count) {
    throw 'files.sha256 does not enumerate the exact evidence payload set.'
}
for ($index = 0; $index -lt $actualFiles.Count; $index++) {
    if ($actualFiles[$index] -cne $manifestFiles[$index]) {
        throw "files.sha256 set mismatch: '$($actualFiles[$index])' vs '$($manifestFiles[$index])'"
    }
}

$result = Get-Content -Raw -LiteralPath $resultPath | ConvertFrom-Json
if ($result.schema -cne 'animus-ferric-s115-release-qualification-v1' -or
    $result.task -cne 'T-11501' -or
    -not $result.passed) {
    throw 'result.json is not a passing T-11501 release qualification.'
}
if ($result.repository.branch -cne 'dev' -or
    [string]$result.repository.commit -notmatch '^[0-9a-f]{40}$' -or
    -not $result.repository.qualified_source_clean -or
    (@($result.repository.qualified_source_scope) -join ',') -cne 'Cargo.toml,Cargo.lock,crates') {
    throw 'repository provenance is incomplete or unclean.'
}
if ($result.repository.known_unrelated_edit.path -cne
    'docs/sprints/s114/control-artifacts/model/acquisition-tests.json' -or
    $result.repository.known_unrelated_edit.tolerated_status -cne ' M') {
    throw 'known unrelated edit policy drifted.'
}
$attemptNumber = [int]$result.attempt
if ($attemptNumber -lt 1 -or $attemptNumber -gt 999) {
    throw 'qualification attempt number is outside the supported range.'
}
$attemptLabel = '{0:D3}' -f $attemptNumber
$expectedTransientRelative = "target/s115-release-qualification/attempts/$attemptLabel"
$expectedRetainedRelative = "docs/sprints/s115/control-artifacts/release/attempts/$attemptLabel"
if ($result.publication.transient_root -cne $expectedTransientRelative -or
    $result.publication.retained_root -cne $expectedRetainedRelative -or
    -not $result.publication.staged_then_verified) {
    throw 'qualification publication identity drifted.'
}
$transientRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $repositoryRoot $expectedTransientRelative.Replace('/', '\'))
)
$cargoBuildRoot = Join-Path $transientRoot 'cargo-target'
$builtBinaryPath = Join-Path $cargoBuildRoot 'release\ferric.exe'
$binaryPath = Join-Path $repositoryRoot 'target\release\ferric.exe'

$expectedGates = @(
    'source-status',
    'known-unrelated-status',
    'source-head',
    'source-branch',
    'fmt',
    'clippy-default',
    'clippy-backend',
    'query-unit-tests',
    'cli-integration-tests',
    'workspace-tests',
    'release-build',
    'post-source-status',
    'post-known-unrelated-status',
    'post-source-head',
    'binary-version',
    'query-help',
    'probe-default-fresh',
    'probe-default-resume',
    'probe-external-fresh',
    'probe-external-resume'
)
$gates = @($result.gates)
if ($gates.Count -ne $expectedGates.Count) {
    throw "gate count mismatch: expected $($expectedGates.Count), observed $($gates.Count)"
}
$journal = @(
    Get-Content -LiteralPath $journalPath |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        ForEach-Object { $_ | ConvertFrom-Json }
)
if ($journal.Count -ne $gates.Count) {
    throw 'journal and result gate counts differ.'
}

$gitPath = (Get-Command git -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$cargoPath = (Get-Command cargo -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$knownPath = 'docs/sprints/s114/control-artifacts/model/acquisition-tests.json'
$fixedGateSpecs = [ordered]@{
    'source-status' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', 'Cargo.toml', 'Cargo.lock', 'crates')
    }
    'known-unrelated-status' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', $knownPath)
    }
    'source-head' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'rev-parse', 'HEAD')
    }
    'source-branch' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'branch', '--show-current')
    }
    'fmt' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'fmt', '--all', '--', '--check')
    }
    'clippy-default' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
    }
    'clippy-backend' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'clippy', '-p', 'ferric-cli', '--all-targets', '--features', 'backend-openai', '--', '-D', 'warnings')
    }
    'query-unit-tests' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'test', '-p', 'ferric-cli', '--features', 'backend-openai', '--bin', 'ferric', 'query::tests')
    }
    'cli-integration-tests' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'test', '-p', 'ferric-cli', '--features', 'backend-openai', '--test', 'cli')
    }
    'workspace-tests' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'test', '--workspace', '--all-targets')
    }
    'release-build' = [pscustomobject]@{
        executable = $cargoPath
        argv = @('--locked', 'build', '--release', '-p', 'ferric-cli', '--features', 'backend-openai', '--target-dir', $cargoBuildRoot)
    }
    'post-source-status' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', 'Cargo.toml', 'Cargo.lock', 'crates')
    }
    'post-known-unrelated-status' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'status', '--porcelain=v1', '--untracked-files=all', '--', $knownPath)
    }
    'post-source-head' = [pscustomobject]@{
        executable = $gitPath
        argv = @('-C', $repositoryRoot, 'rev-parse', 'HEAD')
    }
    'binary-version' = [pscustomobject]@{
        executable = $binaryPath
        argv = @('--version')
    }
    'query-help' = [pscustomobject]@{
        executable = $binaryPath
        argv = @('query', '--help')
    }
}
$expectedTimeouts = [ordered]@{
    'source-status' = 120
    'known-unrelated-status' = 120
    'source-head' = 120
    'source-branch' = 120
    'fmt' = 600
    'clippy-default' = 3600
    'clippy-backend' = 3600
    'query-unit-tests' = 3600
    'cli-integration-tests' = 3600
    'workspace-tests' = 7200
    'release-build' = 3600
    'post-source-status' = 120
    'post-known-unrelated-status' = 120
    'post-source-head' = 120
    'binary-version' = 120
    'query-help' = 120
    'probe-default-fresh' = 300
    'probe-default-resume' = 300
    'probe-external-fresh' = 300
    'probe-external-resume' = 300
}

for ($index = 0; $index -lt $gates.Count; $index++) {
    $gate = $gates[$index]
    $entry = $journal[$index]
    if ($gate.name -cne $expectedGates[$index] -or
        $gate.name -cne $entry.name -or
        [int]$gate.ordinal -ne ($index + 1) -or
        [int]$entry.ordinal -ne ($index + 1) -or
        -not $gate.passed -or
        [int]$gate.exit_code -ne [int]$entry.exit_code) {
        throw "gate/journal mismatch at ordinal $($index + 1)."
    }
    if (($gate | ConvertTo-Json -Depth 15 -Compress) -cne
        ($entry | ConvertTo-Json -Depth 15 -Compress)) {
        throw "gate and journal records differ at ordinal $($index + 1)."
    }
    $expectedExit = if ($gate.name -in @('probe-default-fresh', 'probe-external-fresh')) { 1 } else { 0 }
    if (@($gate.expected_exit_codes).Count -ne 1 -or
        [int]$gate.expected_exit_codes[0] -ne $expectedExit -or
        [int]$gate.exit_code -ne $expectedExit -or
        [string]$gate.working_directory -cne $repositoryRoot -or
        [bool]$gate.timed_out -or
        [int]$gate.timeout_seconds -ne [int]$expectedTimeouts[$gate.name] -or
        [int]$gate.process_id -le 0) {
        throw "gate execution contract drifted: $($gate.name)"
    }
    if ($fixedGateSpecs.Contains($gate.name)) {
        $spec = $fixedGateSpecs[$gate.name]
        if ([string]$gate.executable -cne [string]$spec.executable) {
            throw "gate executable drifted: $($gate.name)"
        }
        Assert-StringArrayExact -Actual @($gate.argv) -Expected @($spec.argv) -Label $gate.name
    }
    foreach ($stream in @('stdout', 'stderr')) {
        $relative = [string]$gate.$stream
        $path = Get-SafeEvidencePath -RelativePath $relative
        if ((Get-Sha256 -LiteralPath $path) -cne [string]$gate."${stream}_sha256") {
            throw "gate $($gate.name) $stream hash mismatch."
        }
    }
}

$sourceStatusText = Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[0].stdout))
if (-not [string]::IsNullOrWhiteSpace($sourceStatusText)) {
    throw 'qualified source-status output is not clean.'
}
$knownStatusText = ([string](Get-Content -Raw -LiteralPath (
            Get-SafeEvidencePath -RelativePath ([string]$gates[1].stdout)
        ))).TrimEnd([char[]]"`r`n")
if ($knownStatusText -notin @('', " M $knownPath")) {
    throw 'known unrelated status output has an unexpected value.'
}
$headText = (Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[2].stdout))).Trim()
$branchText = (Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[3].stdout))).Trim()
if ($headText -cne $result.repository.commit -or $branchText -cne 'dev') {
    throw 'captured Git identity does not match result provenance.'
}
$postStatusText = Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[11].stdout))
$postKnownStatusText = ([string](Get-Content -Raw -LiteralPath (
            Get-SafeEvidencePath -RelativePath ([string]$gates[12].stdout)
        ))).TrimEnd([char[]]"`r`n")
$postHeadText = (Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[13].stdout))).Trim()
if (-not [string]::IsNullOrWhiteSpace($postStatusText) -or
    $postKnownStatusText -cne $knownStatusText -or
    $postHeadText -cne $headText -or
    [bool]$result.repository.known_unrelated_edit.present -ne (-not [string]::IsNullOrEmpty($knownStatusText))) {
    throw 'post-build Git status or known-edit report drifted.'
}

$expectedBuildDisplayPath = "$expectedTransientRelative/cargo-target/release/ferric.exe"
if ($result.binary.display_path -cne 'target/release/ferric.exe' -or
    [string]$result.binary.sha256 -notmatch '^[0-9a-f]{64}$' -or
    [long]$result.binary.bytes -le 0 -or
    -not ([string]$result.binary.version).StartsWith('ferric ') -or
    $result.binary.source_commit -cne $result.repository.commit -or
    -not $result.binary.backend_openai -or
    -not $result.binary.published_from_exact_build_output -or
    $result.binary.build_output.display_path -cne $expectedBuildDisplayPath -or
    [long]$result.binary.build_output.bytes -ne [long]$result.binary.bytes -or
    [string]$result.binary.build_output.sha256 -cne [string]$result.binary.sha256) {
    throw 'binary attestation is incomplete.'
}
$versionText = (Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$gates[14].stdout))).Trim()
if ($versionText -cne $result.binary.version) {
    throw 'binary version claim does not match captured stdout.'
}
$helpGate = $gates[15]
if ([string]$result.query_help.gate -cne [string]$helpGate.name -or
    [string]$result.query_help.stdout -cne [string]$helpGate.stdout -or
    [string]$result.query_help.stdout_sha256 -cne [string]$helpGate.stdout_sha256 -or
    [string]$result.query_help.stderr -cne [string]$helpGate.stderr -or
    [string]$result.query_help.stderr_sha256 -cne [string]$helpGate.stderr_sha256) {
    throw 'query-help capture identity does not match the validated gate record.'
}
$helpText = Get-Content -Raw -LiteralPath (Get-SafeEvidencePath -RelativePath ([string]$helpGate.stdout))
$derivedHelpChecks = [ordered]@{
    trace_dir = $helpText.Contains('--trace-dir')
    default_root = $helpText.Contains('<workspace>/.ferric/trace')
    disjoint = $helpText.Contains('disjoint')
    reparse = $helpText.Contains('reparse')
    explicit_resume = $helpText.Contains('explicit') -and $helpText.Contains('resume')
    powershell = $helpText.Contains('PowerShell')
}
foreach ($check in $derivedHelpChecks.Keys) {
    if (-not $derivedHelpChecks[$check] -or
        -not $result.query_help.checks.$check) {
        throw "query-help contract check failed: $check"
    }
}

$expectedProbes = @('default_fresh', 'default_resume', 'external_fresh', 'external_resume')
$probes = @($result.probes)
if ($probes.Count -ne $expectedProbes.Count) {
    throw 'probe count mismatch.'
}
$probeByName = [ordered]@{}
for ($index = 0; $index -lt $probes.Count; $index++) {
    if ($probes[$index].name -cne $expectedProbes[$index]) {
        throw "probe order drifted at index $index."
    }
    $probeByName[$probes[$index].name] = $probes[$index]
}
$gateByName = [ordered]@{}
foreach ($gate in $gates) {
    $gateByName[$gate.name] = $gate
}

foreach ($kind in @('default', 'external')) {
    $external = $kind -ceq 'external'
    $freshProbe = $probeByName["${kind}_fresh"]
    $resumeProbe = $probeByName["${kind}_resume"]
    $freshGateName = "probe-$kind-fresh"
    $resumeGateName = "probe-$kind-resume"
    $freshGate = $gateByName[$freshGateName]
    $resumeGate = $gateByName[$resumeGateName]
    $pairRoot = Join-Path (Join-Path $transientRoot 'probes') $kind
    $workspace = Join-Path $pairRoot 'workspace'
    $traceRoot = if ($external) {
        Join-Path $pairRoot 'traces'
    }
    else {
        Join-Path $workspace '.ferric\trace'
    }
    $workspaceRelative = [System.IO.Path]::GetRelativePath(
        $repositoryRoot,
        $workspace
    ).Replace('\', '/')
    $traceRootRelative = [System.IO.Path]::GetRelativePath(
        $repositoryRoot,
        $traceRoot
    ).Replace('\', '/')

    $freshArguments = @(
        'query', '--mock', '--no-config', '--max-turns', '1',
        '--workspace', $workspace
    )
    if ($external) {
        $freshArguments += @('--trace-dir', $traceRoot)
    }
    $freshArguments += 'do a release qualification mock task'
    if ([string]$freshGate.executable -cne $binaryPath -or
        [string]$resumeGate.executable -cne $binaryPath) {
        throw "$kind probe did not execute the published release binary."
    }
    Assert-StringArrayExact `
        -Actual @($freshGate.argv) `
        -Expected $freshArguments `
        -Label $freshGateName

    $resumeActual = @($resumeGate.argv)
    $expectedResumeCount = if ($external) { 11 } else { 9 }
    if ($resumeActual.Count -ne $expectedResumeCount) {
        throw "$resumeGateName argv count drifted."
    }
    $sourceTrace = [string]$resumeActual[4]
    if ([System.IO.Path]::GetFullPath($sourceTrace) -cne $sourceTrace -or
        [System.IO.Path]::GetDirectoryName($sourceTrace) -cne $traceRoot -or
        [System.IO.Path]::GetFileName($sourceTrace) -notmatch '^q-.+\.jsonl$') {
        throw "$resumeGateName source trace escaped its exact trace root."
    }
    $resumeArguments = @(
        'query', '--mock', '--no-config', '--resume', $sourceTrace,
        '--workspace', $workspace, '--max-turns', '3'
    )
    if ($external) {
        $resumeArguments += @('--trace-dir', $traceRoot)
    }
    Assert-StringArrayExact `
        -Actual $resumeActual `
        -Expected $resumeArguments `
        -Label $resumeGateName

    foreach ($probeSpec in @(
            [pscustomobject]@{
                probe = $freshProbe
                gate = $freshGate
                retained = "probes/$kind-fresh.jsonl"
                reason = 'max_turns'
                fresh = $true
            },
            [pscustomobject]@{
                probe = $resumeProbe
                gate = $resumeGate
                retained = "probes/$kind-resume.jsonl"
                reason = 'task_complete'
                fresh = $false
            }
        )) {
        $probe = $probeSpec.probe
        if (-not $probe.passed -or
            [bool]$probe.external -ne $external -or
            $probe.gate -cne $probeSpec.gate.name -or
            $probe.workspace -cne $workspaceRelative -or
            $probe.trace_root -cne $traceRootRelative -or
            -not $probe.trace_location_exact -or
            -not $probe.trace_workspace_exact -or
            [bool]$probe.workspace_dot_ferric_absent -ne $external -or
            $probe.trace.retained_path -cne $probeSpec.retained) {
            throw "probe location or verdict drifted: $($probe.name)"
        }
        $tracePath = Get-SafeEvidencePath -RelativePath ([string]$probe.trace.retained_path)
        if ((Get-Sha256 -LiteralPath $tracePath) -cne [string]$probe.trace.sha256 -or
            (Get-Item -LiteralPath $tracePath).Length -ne [long]$probe.trace.bytes) {
            throw "retained probe trace mismatch: $($probe.name)"
        }
        $traceIdentity = Read-TraceIdentity -LiteralPath $tracePath
        if ($traceIdentity.session -cne $probe.session -or
            $traceIdentity.reason -cne $probeSpec.reason -or
            $probe.stop_reason -cne $probeSpec.reason -or
            $traceIdentity.workspace -cne $probe.observed_trace_workspace -or
            $traceIdentity.workspace -cne (Get-VerbatimCanonicalPath -LiteralPath $workspace)) {
            throw "retained probe trace identity mismatch: $($probe.name)"
        }
    }

    $freshIdentity = Read-TraceIdentity -LiteralPath (
        Get-SafeEvidencePath -RelativePath ([string]$freshProbe.trace.retained_path)
    )
    $resumeIdentity = Read-TraceIdentity -LiteralPath (
        Get-SafeEvidencePath -RelativePath ([string]$resumeProbe.trace.retained_path)
    )
    if ($null -ne $freshIdentity.resumed_from -or
        -not $freshProbe.resume_argv_exact -or
        -not $freshProbe.answer_absent -or
        $resumeIdentity.resumed_from -cne $freshIdentity.session -or
        $resumeProbe.resumed_from -cne $freshIdentity.session -or
        $resumeProbe.expected_resumed_from -cne $freshIdentity.session -or
        -not $resumeProbe.resumed_from_matches -or
        -not $resumeProbe.terminal_resume_hint_absent) {
        throw "$kind probe fresh/resume cross-link failed."
    }

    $freshStderr = [string](Get-Content -Raw -LiteralPath (
            Get-SafeEvidencePath -RelativePath ([string]$freshGate.stderr)
        ))
    $capturedResumeElements = @(Get-ResumeElements -StandardError $freshStderr)
    $expectedResumeElements = @(
        'ferric', 'query', '--resume', (Get-VerbatimCanonicalPath -LiteralPath $sourceTrace),
        '--workspace', (Get-VerbatimCanonicalPath -LiteralPath $workspace)
    )
    if ($external) {
        $expectedResumeElements += @(
            '--trace-dir', (Get-VerbatimCanonicalPath -LiteralPath $traceRoot)
        )
    }
    Assert-StringArrayExact `
        -Actual $capturedResumeElements `
        -Expected $expectedResumeElements `
        -Label "$kind captured Resume"
    Assert-StringArrayExact `
        -Actual @($freshProbe.resume_argv) `
        -Expected $capturedResumeElements `
        -Label "$kind result Resume"
    if ($capturedResumeElements -ccontains '--answer') {
        throw "$kind ordinary max_turns Resume unexpectedly contains --answer."
    }
    $resumeStderr = [string](Get-Content -Raw -LiteralPath (
            Get-SafeEvidencePath -RelativePath ([string]$resumeGate.stderr)
        ))
    if ($resumeStderr -match '(?m)^Resume: ') {
        throw "$kind task_complete resume emitted an inapplicable Resume command."
    }

    if ($CheckLiveBinary) {
        if (-not (Test-Path -LiteralPath $workspace -PathType Container) -or
            -not (Test-Path -LiteralPath (Join-Path $workspace 'ferric-mock.txt') -PathType Leaf) -or
            -not (Test-Path -LiteralPath $traceRoot -PathType Container) -or
            ($external -and (Test-Path -LiteralPath (Join-Path $workspace '.ferric')))) {
            throw "$kind live probe filesystem contract failed."
        }
        Assert-OrdinaryTree -LiteralPath $pairRoot
        $runtimeTraces = @(Get-ChildItem -File -LiteralPath $traceRoot -Filter 'q-*.jsonl')
        if ($runtimeTraces.Count -ne 2) {
            throw "$kind live trace root does not contain exactly source and continuation."
        }
        $freshMatches = @($runtimeTraces | Where-Object {
                (Get-Sha256 -LiteralPath $_.FullName) -ceq [string]$freshProbe.trace.sha256
            })
        $resumeMatches = @($runtimeTraces | Where-Object {
                (Get-Sha256 -LiteralPath $_.FullName) -ceq [string]$resumeProbe.trace.sha256
            })
        if ($freshMatches.Count -ne 1 -or
            $resumeMatches.Count -ne 1 -or
            $freshMatches[0].FullName -cne $sourceTrace) {
            throw "$kind retained traces do not map exactly onto the live trace root."
        }
    }
}

if ($CheckLiveBinary) {
    if (-not (Test-Path -LiteralPath $builtBinaryPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
        throw 'fresh Cargo output or published release binary is missing.'
    }
    $builtItem = Get-Item -Force -LiteralPath $builtBinaryPath
    $publishedItem = Get-Item -Force -LiteralPath $binaryPath
    if ($builtItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        $publishedItem.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint) -or
        $builtItem.Length -ne $publishedItem.Length -or
        $builtItem.Length -ne [long]$result.binary.build_output.bytes -or
        $publishedItem.Length -ne [long]$result.binary.bytes -or
        (Get-Sha256 -LiteralPath $builtBinaryPath) -cne [string]$result.binary.build_output.sha256 -or
        (Get-Sha256 -LiteralPath $binaryPath) -cne [string]$result.binary.sha256 -or
        (Get-Sha256 -LiteralPath $builtBinaryPath) -cne (Get-Sha256 -LiteralPath $binaryPath)) {
        throw 'fresh Cargo output and published release binary do not match the attested identity.'
    }
}

[pscustomobject][ordered]@{
    schema = 'animus-ferric-s115-release-verifier-v1'
    passed = $true
    evidence_root = $root
    result_sha256 = Get-Sha256 -LiteralPath $resultPath
    files = $manifestEntries.Count
    gates = $gates.Count
    probes = $probes.Count
    live_binary_checked = [bool]$CheckLiveBinary
} | ConvertTo-Json -Depth 5
