param (
    [string]$Iterations = "10"
)

$ErrorActionPreference = "Continue"

Write-Host "Running Mistral backend with Llama-3.2-1B..."
$MistralLog = "toolbench_mistral.log"
cargo run --release -p ferric-cli --features backend-mistralrs,backend-openai,backend-python -- toolbench --backend mistral --model-dir "D:\Models" --model-file "Llama-3.2-1B-Instruct-Q4_K_M.gguf" --iterations $Iterations > $MistralLog 2>&1

Write-Host "Running Python backend with Gemma-4-e4b..."
$PythonLog = "toolbench_python.log"
cargo run --release -p ferric-cli --features backend-mistralrs,backend-openai,backend-python -- toolbench --backend python --model-dir "D:\Models\google--gemma-4-e4b" --iterations $Iterations > $PythonLog 2>&1

Write-Host "Benchmark complete!"
