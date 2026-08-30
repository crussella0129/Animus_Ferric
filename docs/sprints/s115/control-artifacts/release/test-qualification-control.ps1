[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'The qualification control self-test requires PowerShell 7 or newer.'
}

function Read-ParsedScript {
    param([Parameter(Mandatory)][string]$LiteralPath)
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $LiteralPath,
        [ref]$tokens,
        [ref]$errors
    )
    if ($errors.Count -ne 0) {
        throw "PowerShell parse errors in $LiteralPath`: $($errors -join '; ')"
    }
    [pscustomobject]@{
        ast = $ast
        text = Get-Content -Raw -LiteralPath $LiteralPath
    }
}

function Assert-ContainsAll {
    param(
        [Parameter(Mandatory)][string]$Text,
        [Parameter(Mandatory)][string[]]$Needles,
        [Parameter(Mandatory)][string]$Label
    )
    foreach ($needle in $Needles) {
        if (-not $Text.Contains($needle)) {
            throw "$Label is missing required control text: $needle"
        }
    }
}

$mainPath = Join-Path $PSScriptRoot 'qualify-release.ps1'
$verifierPath = Join-Path $PSScriptRoot 'verify-release.ps1'
$readmePath = Join-Path $PSScriptRoot 'README.md'
foreach ($required in @($mainPath, $verifierPath, $readmePath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "qualification control file is missing: $required"
    }
}

$main = Read-ParsedScript -LiteralPath $mainPath
$verifier = Read-ParsedScript -LiteralPath $verifierPath
$readme = Get-Content -Raw -LiteralPath $readmePath

$mainNeedles = @(
    's115-release-qualification\attempts',
    'Get-ExistingAttemptNumbers',
    'allocation.lock',
    'Assert-SafePathTail',
    "'Cargo.toml', 'Cargo.lock', 'crates'",
    'docs/sprints/s114/control-artifacts/model/acquisition-tests.json',
    'ReadToEndAsync',
    'WaitForExit($TimeoutSeconds * 1000)',
    'ArgumentList.Add',
    'rustup intentionally installs cargo/rustc proxy symlinks',
    "-Name 'fmt'",
    "-Name 'clippy-default'",
    "-Name 'clippy-backend'",
    "-Name 'query-unit-tests'",
    "-Name 'cli-integration-tests'",
    "-Name 'workspace-tests'",
    "'--bin', 'ferric', 'query::tests'",
    "'--locked', 'build', '--release', '-p', 'ferric-cli', '--features', 'backend-openai'",
    "'--target-dir', `$cargoBuildRoot",
    'published release binary does not exactly match the fresh Cargo output',
    "'query', '--mock', '--no-config', '--max-turns', '1'",
    'HashSet[string]',
    "'target/release/ferric.exe'",
    "'query', '--help'",
    'result.sha256',
    'files.sha256',
    'Move-Item -LiteralPath $publicationStage -Destination $retainedEvidence'
)
Assert-ContainsAll -Text $main.text -Label 'qualification entrypoint' -Needles $mainNeedles

$verifierNeedles = @(
    'animus-ferric-s115-release-qualification-v1',
    "'clippy-default'",
    "'clippy-backend'",
    "'default_fresh', 'default_resume', 'external_fresh', 'external_resume'",
    'captured Resume command must contain one command and no redirections',
    'fresh Cargo output and published release binary do not match',
    'retained traces do not map exactly onto the live trace root',
    'query-help capture identity does not match the validated gate record',
    '.TrimEnd([char[]]"`r`n")',
    'resumed_from_matches',
    'files.sha256',
    'journal and result gate counts differ'
)
Assert-ContainsAll -Text $verifier.text -Label 'release verifier' -Needles $verifierNeedles

$readmeNeedles = @(
    'qualify-release.ps1',
    'attempts/001',
    'next numeric',
    'fresh per-attempt Cargo target',
    'normal host PowerShell 7',
    'acquisition-tests.json'
)
Assert-ContainsAll -Text $readme -Label 'release README' -Needles $readmeNeedles

$knownPath = 'docs/sprints/s114/control-artifacts/model/acquisition-tests.json'
$porcelainSample = " M $knownPath`r`n"
$normalizedPorcelain = $porcelainSample.TrimEnd([char[]]"`r`n")
if ($normalizedPorcelain -cne " M $knownPath") {
    throw 'porcelain status normalization did not preserve its leading status column.'
}

$emptyStatusFunctions = @('Get-StatusLines', 'Assert-AllowedRepositoryStatus', 'Assert-KnownUnrelatedStatus')
foreach ($functionName in $emptyStatusFunctions) {
    $functionAst = @($main.ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -ceq $functionName
            }, $true))
    if ($functionAst.Count -ne 1 -or
        -not $functionAst[0].Extent.Text.Contains('[AllowEmptyString()]')) {
        throw "$functionName must accept clean Git status output as an empty string."
    }
}

$forbiddenCommands = @('Remove-Item', 'Start-Process', 'Invoke-Expression')
$commands = @($main.ast.FindAll({ param($node) $node -is [System.Management.Automation.Language.CommandAst] }, $true))
foreach ($command in $commands) {
    if ($forbiddenCommands -ccontains $command.GetCommandName()) {
        throw "qualification entrypoint contains forbidden command: $($command.GetCommandName())"
    }
}

[pscustomobject][ordered]@{
    schema = 'animus-ferric-s115-release-control-self-test-v1'
    passed = $true
    parsed_scripts = 2
    required_main_controls = $mainNeedles.Count
    required_verifier_controls = $verifierNeedles.Count
    required_readme_controls = $readmeNeedles.Count
    forbidden_commands_absent = $true
    porcelain_leading_column_preserved = $true
    empty_status_capture_accepted = $true
    full_qualification_executed = $false
} | ConvertTo-Json -Depth 4
