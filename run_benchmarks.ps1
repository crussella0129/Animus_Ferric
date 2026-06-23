param (
    [string]$Iterations = "10",
    # Constrained path: a GGUF for `ferric server up` (llama-server) to load.
    [string]$LlamaGguf = "D:\Models\gguf\Llama-3.2-1B-Instruct-Q4_K_M.gguf",
    [string]$OpenAiModel = "Llama-3.2-1B-Instruct"
)

$ErrorActionPreference = "Continue"
$Features = "backend-mistralrs,backend-openai"

Write-Host "Building ferric ($Features)..." -ForegroundColor Cyan
cargo build --release -p ferric-cli --features $Features
$Ferric = ".\target\release\ferric.exe"

# 1. In-process mistral.rs (TextXml protocol — no constraint, no server).
Write-Host "`nMistral backend (TextXml) — Llama-3.2-1B..." -ForegroundColor Cyan
& $Ferric toolbench --backend mistral --model-dir "D:\Models" --model-file "Llama-3.2-1B-Instruct-Q4_K_M.gguf" --iterations $Iterations --report toolbench_mistral.md

# 2. Constrained-JSON thesis via the OpenAI-compatible HTTP valve. The launcher
# brings up llama-server, toolbench auto-discovers it (.ferric/server.json),
# then we stop it. Gemma-4-e4b (the removed PyTorch path, ADR-021) is reached the
# same way — just point --model/--mmproj at it.
Write-Host "`nOpenAI valve (ConstrainedJson) via ferric server..." -ForegroundColor Cyan
& $Ferric server up --engine llama-server --model $LlamaGguf
& $Ferric server status
& $Ferric toolbench --backend openai --model $OpenAiModel --protocol grammar --iterations $Iterations --report toolbench_openai.md
& $Ferric server down

Write-Host "`nReports: toolbench_mistral.md / toolbench_openai.md (+ .jsonl). Read the verdict bands." -ForegroundColor Green
