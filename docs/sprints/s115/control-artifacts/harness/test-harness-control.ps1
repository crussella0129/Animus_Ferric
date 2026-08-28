[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$qualifierPath = Join-Path $PSScriptRoot 'qualify-harness.ps1'
$verifierPath = Join-Path $PSScriptRoot 'verify-harness.ps1'

function Get-ParsedScript([string]$Path) {
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $Path,
        [ref]$tokens,
        [ref]$errors
    )
    if ($errors.Count -ne 0) {
        $messages = @($errors | ForEach-Object { $_.Message }) -join '; '
        throw "PowerShell parser rejected $Path`: $messages"
    }
    return [pscustomobject]@{
        ast = $ast
        tokens = $tokens
        source = Get-Content -Raw -LiteralPath $Path
    }
}

function Assert-ContainsAll([string]$Source, [string[]]$Required, [string]$Label) {
    foreach ($needle in $Required) {
        if (-not $Source.Contains($needle)) {
            throw "$Label is missing required control text: $needle"
        }
    }
}

function Assert-NoDestructiveCommands([object]$Parsed, [string]$Label) {
    $commands = @($Parsed.ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.CommandAst]
            }, $true))
    foreach ($command in $commands) {
        $name = $command.GetCommandName()
        if ($name -in @(
                'Remove-Item', 'Clear-Content', 'del', 'erase', 'rd', 'ri',
                'rmdir', 'rm'
            )) {
            throw "$Label contains forbidden destructive command: $name"
        }
        if ($name -in @('git', 'git.exe') -and
            $command.Extent.Text -match '(?i)(?:\s|["''])((clean)|(reset)|(checkout))(?=\s|["''])') {
            throw "$Label contains a forbidden mutating Git command"
        }
        if ($name -eq 'Copy-Item') {
            $recursive = @($command.CommandElements | Where-Object {
                    $_ -is [System.Management.Automation.Language.CommandParameterAst] -and
                    $_.ParameterName -eq 'Recurse'
                })
            if ($recursive.Count -gt 0) {
                throw "$Label contains a recursive Copy-Item operation"
            }
        }
    }
    $deleteMembers = @($Parsed.ast.FindAll({
                param($node)
                $node -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -and
                [string]$node.Member.Value -eq 'Delete' -and
                $node.Expression.Extent.Text -match '(?i)(System\.IO\.(File|Directory)|FileInfo|DirectoryInfo)'
            }, $true))
    if ($deleteMembers.Count -gt 0) {
        throw "$Label contains a forbidden .NET file or directory delete call"
    }
}

$qualifier = Get-ParsedScript $qualifierPath
$verifier = Get-ParsedScript $verifierPath
Assert-NoDestructiveCommands $qualifier 'qualifier'
Assert-NoDestructiveCommands $verifier 'verifier'

Assert-ContainsAll $qualifier.source @(
    "'app-harness' = Join-Path `$experimentRoot 'app-harness'",
    "'self-test-workspaces' = Join-Path `$experimentRoot 'self-test-workspaces'",
    "'app-workspace' = Join-Path `$experimentRoot 'app-workspace'",
    "'launcher-attestation-probe' = Join-Path `$experimentRoot 'launcher-attestation-probe'",
    "target\s115-preserved-preflight",
    'attempt-$name',
    "'001-pre-selftest'",
    "'002-frozen-copy'",
    "'003-post-selftest'",
    'finally {',
    'Move-Item -LiteralPath',
    'GetPathRoot',
    'immediately before move',
    'byte_identical_manifests',
    'EnumerateFileSystemEntries',
    'ReparsePoint',
    'link_target',
    'WaitForExitAsync',
    'ReadToEndAsync',
    'Kill($true)',
    "'GIT_DIR'",
    "'GIT_WORK_TREE'",
    "'GIT_INDEX_FILE'",
    "'GIT_OBJECT_DIRECTORY'",
    "'GIT_ALTERNATE_OBJECT_DIRECTORIES'",
    "'GIT_COMMON_DIR'",
    'GIT_CEILING_DIRECTORIES',
    '-CandidateRoot $canonicalRoots[''app-workspace'']',
    '-MetadataRoot $canonicalRoots[''launcher-attestation-probe'']',
    "'--unshare-net'",
    '/proc/net/dev',
    '/dev/tcp/198.51.100.1/9',
    'Invoke-LiveHarnessJournalAudit',
    'tracked-harness.before.entries.jsonl',
    'known_unrelated_edit_sha256_before',
    'repository_tree',
    'frozen_seed_file_count = 5',
    'frozen_file_count = 30',
    'depth_components_to_repo = 6',
    "@('--exec', 'bash', `$selfTestRelative)",
    'recursive_delete_used = $false',
    '[System.IO.FileShare]::None',
    'qualification.lock',
    'Capture-ControlProvenance',
    'Write-TerminalFailureRecord',
    "@(`$gitRoots + @('init'))"
) 'qualifier'

Assert-ContainsAll $verifier.source @(
    '[switch]$CheckQuarantine',
    'Get-TreeEntriesText',
    'ignored quarantine rewalk differs',
    'files.sha256 does not describe the exact retained evidence file set',
    'self-test summary values differ from the frozen contract',
    'qualification command journal gate set differs from the fixed control',
    'git-ambient-discovery-blocked',
    'target/s114-experiment/app-workspace',
    'target/s114-experiment/launcher-attestation-probe',
    'live harness journal audit is incomplete',
    'launcher attestation differs on quarantine rewalk',
    'canonical root is not absent at live handoff',
    'Assert-ControlSourceStatic',
    'Invoke-IndependentLiveJournalVerification',
    'EvidenceRoot basename/location is not bound to result.attempt',
    'independent journal enumeration differs from retained qualifier audit',
    'control provenance does not name the exact four control files'
) 'verifier'

if ($qualifier.source.Contains('--separate-git-dir') -or
    $qualifier.source -match '(?i)\bgit(?:\.exe)?\b[^\r\n]*(?:\bclean\b|\breset\b|\bcheckout\b)') {
    throw 'qualifier contains a forbidden Git setup or mutation surface'
}
if (($qualifier.source | Select-String -Pattern 'Move-Item -LiteralPath' -AllMatches).Matches.Count -ne 2) {
    throw 'qualifier must have exactly the manifested-root move and compact-evidence publication moves'
}
if (-not $qualifier.source.Contains(
        "`$transientHarnessRoot = Join-Path `$attemptRoot 'frozen\app-harness'"
    ) -or -not $qualifier.source.Contains(
        "Join-Path `$transientScriptsRoot '..\..\..\..\..\..'"
    )) {
    throw 'depth-preserving copy layout or six-parent host proof differs'
}

[pscustomobject]@{
    schema = 's115-harness-control-selftest-v1'
    status = 'pass'
    parsed_scripts = 2
    attempt_created = $false
    preservation_move_run = $false
    harness_selftest_run = $false
    destructive_commands_found = 0
} | ConvertTo-Json -Depth 3
