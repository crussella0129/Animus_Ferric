param (
    [string]$Model,
    [switch]$Mock
)

$ErrorActionPreference = "Stop"

$Workspace = "e2e_workspace"
$LogFile = "e2e_test_log.txt"
$Prompt = "Create a folder called 'project_root', then create two folders within it: 'src' and 'temp'. Inside 'src', create a file called 'math.py' that contains a function to add two numbers and print the result. Finally, delete the 'temp' folder and report task_complete."

# 1. Clean up old test runs
if (Test-Path $Workspace) {
    Write-Host "Cleaning up old workspace..." -ForegroundColor Yellow
    Remove-Item -Path $Workspace -Recurse -Force
}
New-Item -ItemType Directory -Path $Workspace | Out-Null
Write-Host "Created fresh workspace: $Workspace" -ForegroundColor Green

# 2. Build the command
$Command = "cargo"
$ArgsList = @(
    "run", "--release", "-p", "ferric-cli", "--features", "backend-openai",
    "--", "query", $Prompt, "--workspace", "./$Workspace"
)

if ($Mock) {
    Write-Host "Running in MOCK mode (no real model)..." -ForegroundColor Cyan
    $ArgsList += "--mock"
} else {
    if (-not $Model) {
        Write-Host "ERROR: You must provide -Model when not using -Mock." -ForegroundColor Red
        Write-Host "Example: .\tools\e2e_test.ps1 -Model `"gpt-4o`"" -ForegroundColor Red
        exit 1
    }
    Write-Host "Running with REAL model: $Model" -ForegroundColor Cyan
    $ArgsList += "--model"
    $ArgsList += $Model
}

Write-Host "Starting E2E Test. Output is being appended to $LogFile..." -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green

# 3. Execute and tee output to log file
# Combine stdout and stderr (2>&1), pipe to ForEach to extract text, and append via Out-File or Tee-Object
# To preserve formatting in terminal and file, Tee-Object is ideal.
$ErrorActionPreference = "Continue"
& $Command $ArgsList 2>&1 | Tee-Object -FilePath $LogFile -Append
$ErrorActionPreference = "Stop"

Write-Host "==========================================================" -ForegroundColor Green
Write-Host "Test completed." -ForegroundColor Green

# 4. Point user to the trace file
$TraceDir = Join-Path $Workspace ".ferric\trace"
if (Test-Path $TraceDir) {
    $TraceFiles = Get-ChildItem -Path $TraceDir -Filter "*.jsonl"
    if ($TraceFiles.Count -gt 0) {
        $LatestTrace = $TraceFiles | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        Write-Host ""
        Write-Host "Detailed backend JSONL trace (with tool call arguments, timings, and tokens) is available at:" -ForegroundColor Cyan
        Write-Host $LatestTrace.FullName -ForegroundColor Cyan
    }
}
