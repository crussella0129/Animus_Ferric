param (
    [string]$Iterations = "10"
)

$ErrorActionPreference = "Continue"

# In-process mistral.rs (GGUF) runs the TextXml protocol (no constraint, no
# server). Llama-3.2-1B is the NANO smoke model.
Write-Host "Running Mistral backend (TextXml) with Llama-3.2-1B..."
$MistralLog = "toolbench_mistral.log"
cargo run --release -p ferric-cli --features backend-mistralrs,backend-openai -- toolbench --backend mistral --model-dir "D:\Models" --model-file "Llama-3.2-1B-Instruct-Q4_K_M.gguf" --iterations $Iterations > $MistralLog 2>&1

# The constrained-JSON thesis runs through the OpenAI-compatible HTTP valve
# (the ADR-001 escape valve), which enforces the action schema server-side.
# Gemma-4-e4b — formerly the crashing embedded-PyTorch path (removed, ADR-021) —
# is reached this way. Start a server first (e.g. `ollama serve` with the model
# pulled), then:
#   cargo run --release -p ferric-cli --features backend-openai -- `
#     toolbench --backend openai --model gemma4:e4b --api-base http://localhost:11434/v1 `
#     --iterations $Iterations
# (Requires a running server; exercised by the E2E acceptance step.)

Write-Host "Benchmark complete!"
