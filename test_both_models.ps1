param (
    [string]$Model1Dir = "C:\Users\charl\Animus_Ferric\models\gguf",
    [string]$Model1File = "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
    [string]$Model2Name = "gemma4:e4b"
)

$ErrorActionPreference = "Stop"

# If you needed to download it natively, you could use huggingface-cli:
# Write-Host "Downloading Gemma-4-12B..."
# huggingface-cli download google/gemma-4-12B-GGUF gemma-4-12b.gguf --local-dir $ModelDir

Write-Host "==========================================================" -ForegroundColor Magenta
Write-Host "Starting Multi-Model E2E Test Suite" -ForegroundColor Magenta
Write-Host "==========================================================" -ForegroundColor Magenta

# Function to run the test for a given model
function Invoke-ModelTest {
    param (
        [string]$ModelName,
        [string[]]$BackendArgs
    )

    Write-Host "`nTesting Model: $ModelName" -ForegroundColor Cyan
    Write-Host "----------------------------------------------------------" -ForegroundColor Cyan

    $LogFile = "e2e_test_log_${ModelName}.txt"
    $Workspace = "e2e_workspace_${ModelName}"
    $Prompt = "Create a folder called 'project_root', then create two folders within it: 'src' and 'temp'. Inside 'src', create a file called 'math.py' that contains a function to add two numbers and print the result. Finally, delete the 'temp' folder and report task_complete."

    # Clean workspace
    if (Test-Path $Workspace) {
        Remove-Item -Path $Workspace -Recurse -Force
    }
    New-Item -ItemType Directory -Path $Workspace | Out-Null

    $Command = "cargo"
    $ArgsList = @(
        "run", "--release", "-p", "ferric-cli", "--features", "backend-mistralrs,backend-openai,backend-python",
        "--", "query", $Prompt, 
        "--workspace", "./$Workspace"
    ) + $BackendArgs

    Write-Host "Running Ferric with $ModelName. Appending output to $LogFile..." -ForegroundColor Yellow
    
    $ErrorActionPreference = "Continue"
    & $Command $ArgsList 2>&1 | Tee-Object -FilePath $LogFile -Append
    $ErrorActionPreference = "Stop"

    Write-Host "Finished testing $ModelName." -ForegroundColor Green
    
    # Trace info
    $TraceDir = Join-Path $Workspace ".ferric\trace"
    if (Test-Path $TraceDir) {
        $TraceFiles = Get-ChildItem -Path $TraceDir -Filter "*.jsonl"
        if ($TraceFiles.Count -gt 0) {
            $LatestTrace = $TraceFiles | Sort-Object LastWriteTime -Descending | Select-Object -First 1
            Write-Host "Detailed JSONL trace available at: $($LatestTrace.FullName)" -ForegroundColor DarkCyan
        }
    }
}

# Run tests
Invoke-ModelTest -ModelName "Llama3.2-1B" -BackendArgs @("--backend", "mistral", "--model-dir", $Model1Dir, "--model-file", $Model1File)
Invoke-ModelTest -ModelName "Gemma-4-e4b" -BackendArgs @("--backend", "python", "--model-dir", "C:\Users\charl\Animus_Ferric\models\safetensors\google--gemma-4-e4b")

Write-Host "`n==========================================================" -ForegroundColor Magenta
Write-Host "Multi-Model Test Suite Completed!" -ForegroundColor Magenta
Write-Host "Logs available: e2e_test_log_Llama3.2-1B.txt and e2e_test_log_Gemma-4-e4b.txt" -ForegroundColor Magenta
Write-Host "==========================================================" -ForegroundColor Magenta
