[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\..')).Path
$preflightRoot = Join-Path $PSScriptRoot 'preflight'
$resultPath = Join-Path $preflightRoot 'result.json'
$resultHashPath = Join-Path $preflightRoot 'result.sha256'

if (Test-Path -LiteralPath $preflightRoot) {
    throw "preflight evidence already exists: $preflightRoot"
}
New-Item -ItemType Directory -Path $preflightRoot | Out-Null

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Invoke-Captured([string]$FilePath, [string[]]$ArgumentList) {
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $FilePath
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $ArgumentList) {
        $start.ArgumentList.Add($argument)
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    if (-not $process.Start()) {
        throw "failed to start $FilePath"
    }
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [pscustomobject]@{
        exit_code = $process.ExitCode
        stdout = $stdout
        stderr = $stderr
    }
}

$querySource = Join-Path $repoRoot 'crates\ferric-cli\src\query.rs'
$runCheckSource = Join-Path $repoRoot 'crates\ferric-tools\src\builtin\run_check.rs'
$graderSource = Join-Path $repoRoot 'docs\sprints\s114\control-artifacts\app-harness\grader\src\lib.rs'
$frozenManifest = Join-Path $repoRoot 'docs\sprints\s114\control-artifacts\app-harness\frozen-inputs.json'
$frozenManifestHash = Join-Path $repoRoot 'docs\sprints\s114\control-artifacts\app-harness\frozen-inputs.sha256'
$runtimePreflightPath = Join-Path $repoRoot 'docs\sprints\s114\control-artifacts\runtime\epoch-3\attempts\e03-01-q4-32768\preflight.json'
$ferricPath = Join-Path $repoRoot 'target\release\ferric.exe'
$candidatePath = Join-Path $repoRoot 'target\s114-experiment\app-workspace'

foreach ($required in @(
    $querySource,
    $runCheckSource,
    $graderSource,
    $frozenManifest,
    $frozenManifestHash,
    $runtimePreflightPath,
    $ferricPath
)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "required input is missing: $required"
    }
}

$runtimePreflight = Get-Content -Raw -LiteralPath $runtimePreflightPath | ConvertFrom-Json
$ferricHash = Get-Sha256 $ferricPath
if ($ferricHash -ne [string]$runtimePreflight.ferric.sha256) {
    throw 'release Ferric binary no longer matches the T-11409-calibrated identity'
}

$help = Invoke-Captured $ferricPath @('query', '--help')
if ($help.exit_code -ne 0) {
    throw "ferric query --help failed with exit code $($help.exit_code)"
}
$helpPath = Join-Path $preflightRoot 'query-help.stdout.txt'
$helpErrorPath = Join-Path $preflightRoot 'query-help.stderr.txt'
[System.IO.File]::WriteAllText($helpPath, $help.stdout, $utf8NoBom)
[System.IO.File]::WriteAllText($helpErrorPath, $help.stderr, $utf8NoBom)

$queryText = Get-Content -Raw -LiteralPath $querySource
$runCheckText = Get-Content -Raw -LiteralPath $runCheckSource
$graderText = Get-Content -Raw -LiteralPath $graderSource
$traceIsHardcoded = $queryText.Contains('workspace_root.join(".ferric").join("trace")')
$externalTraceOptionAbsent = -not [regex]::IsMatch($help.stdout, '(?m)^\s*--trace-dir(?:\s|$)')
$runCheckUsesWorkspace = $runCheckText.Contains('.current_dir(workspace)')
$graderAllowsOnlySourceDirectories = $graderText.Contains('BTreeSet::from(["src", "tests"])')
$graderRejectsOtherDirectories = $graderText.Contains('EntryKind::Directory if !allowed_directories.contains(entry.relative.as_str())')

$listeners = @(Get-NetTCPConnection -State Listen -LocalPort 8080 -ErrorAction SilentlyContinue)
$localRunfile = Join-Path $repoRoot '.ferric\server.json'
$repositoryCommit = (& git -C $repoRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) {
    throw 'could not resolve repository HEAD'
}

$result = [ordered]@{
    schema = 'animus-ferric-s114-app-preflight-v1'
    task = 'T-11410'
    result = 'blocked_pre_inference'
    captured_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    reason = 'query_trace_directory_collides_with_frozen_candidate_path_policy'
    repository = [ordered]@{
        commit_at_capture = $repositoryCommit
        calibrated_source_commit = [string]$runtimePreflight.repository_commit
    }
    ferric = [ordered]@{
        display_path = 'target/release/ferric.exe'
        bytes = (Get-Item -LiteralPath $ferricPath).Length
        sha256 = $ferricHash
        matches_t11409_calibrated_binary = $true
        query_help_exit_code = $help.exit_code
        query_help_stdout_sha256 = Get-Sha256 $helpPath
        query_help_stderr_sha256 = Get-Sha256 $helpErrorPath
        external_trace_option_absent = $externalTraceOptionAbsent
    }
    source_findings = [ordered]@{
        query_source = 'crates/ferric-cli/src/query.rs'
        query_source_sha256 = Get-Sha256 $querySource
        hardcoded_workspace_trace = $traceIsHardcoded
        hardcoded_workspace_trace_line = 935
        trace_sink_accepts_directory_line = 1128
        run_check_source = 'crates/ferric-tools/src/builtin/run_check.rs'
        run_check_source_sha256 = Get-Sha256 $runCheckSource
        run_check_uses_exact_workspace = $runCheckUsesWorkspace
        run_check_current_directory_line = 239
    }
    frozen_grader = [ordered]@{
        source = 'docs/sprints/s114/control-artifacts/app-harness/grader/src/lib.rs'
        source_sha256 = Get-Sha256 $graderSource
        frozen_inputs_sha256 = Get-Sha256 $frozenManifest
        frozen_inputs_hash_record = (Get-Content -Raw -LiteralPath $frozenManifestHash).Trim()
        allowed_directories_are_only_src_and_tests = $graderAllowsOnlySourceDirectories
        other_directories_fail_path_policy = $graderRejectsOtherDirectories
        path_policy_lines = '307-321'
    }
    collision = [ordered]@{
        query_creates_candidate_dot_ferric_before_inference = $traceIsHardcoded
        model_visible_check_runs_from_candidate = $runCheckUsesWorkspace
        dot_ferric_is_forbidden_by_frozen_grader = ($graderAllowsOnlySourceDirectories -and $graderRejectsOtherDirectories)
        accepted_in_session_check_possible = $false
        post_run_deletion_repairs_in_session_check = $false
    }
    execution_state = [ordered]@{
        candidate_display_path = 'target/s114-experiment/app-workspace'
        candidate_exists = Test-Path -LiteralPath $candidatePath
        model_inference_started = $false
        candidate_mutation_started = $false
        managed_server_runfile_absent = -not (Test-Path -LiteralPath $localRunfile)
        listener_8080_count_at_capture = $listeners.Count
    }
    disposition = [ordered]@{
        frozen_harness_edited = $false
        calibrated_binary_rebuilt = $false
        task_completed = $false
        next_requirement = 'operator_only_external_trace_root_plus_independent_binary_requalification'
    }
}

$requiredTruths = @(
    $externalTraceOptionAbsent,
    $traceIsHardcoded,
    $runCheckUsesWorkspace,
    $graderAllowsOnlySourceDirectories,
    $graderRejectsOtherDirectories,
    (-not $result.execution_state.candidate_exists),
    $result.execution_state.managed_server_runfile_absent
)
if ($requiredTruths -contains $false) {
    throw 'preflight evidence did not satisfy the expected cold blocked state'
}

$json = $result | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText($resultPath, $json + "`n", $utf8NoBom)
$resultHash = Get-Sha256 $resultPath
[System.IO.File]::WriteAllText($resultHashPath, "$resultHash  result.json`n", $utf8NoBom)
Write-Output $resultPath

