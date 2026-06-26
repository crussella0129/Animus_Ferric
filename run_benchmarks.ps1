param (
    [string]$Iterations = "10",
    # Constrained path: a GGUF for `ferric server up` (llama-server) to load.
    [string]$LlamaGguf = "D:\Models\gguf\Llama-3.2-1B-Instruct-Q4_K_M.gguf",
    [string]$OpenAiModel = "Llama-3.2-1B-Instruct",
    # Fleet calibration: ollama models to sweep into one leaderboard (must be
    # `ollama pull`-ed). For a GGUF fleet instead, sweep `--backend mistral`
    # with `--models file1.gguf,file2.gguf`.
    [string]$OllamaFleet = "qwen2.5-coder:7b,llama3.1:8b"
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

# 3. Fleet calibration across installed ollama models — one leaderboard, sorted
# best->worst, so you can pick the smallest model that's still "solid". Targets a
# running ollama directly (default :11434); skipped silently if ollama isn't up.
Write-Host "`nFleet calibration (ollama) — $OllamaFleet ..." -ForegroundColor Cyan
& $Ferric toolbench --backend openai --api-base "http://localhost:11434/v1" --models $OllamaFleet --protocol grammar --iterations $Iterations --report toolbench_fleet.md

# 4. Ring calibration — for each ollama model, sweep rings 0,1,... and report the
# highest ring it reliably drives (the recommended --max-ring).
Write-Host "`nRing calibration (ollama) — $OllamaFleet ..." -ForegroundColor Cyan
& $Ferric toolbench --backend openai --api-base "http://localhost:11434/v1" --models $OllamaFleet --protocol grammar --iterations $Iterations --calibrate-rings --report toolbench_calib.md

# 5. Full agentic loop (L0-L6) — multi-turn task completion, not just single tool
# calls. Sets measured_level in benchmarks/model_profiles.json, which `ferric query
# --profile-dir benchmarks` then auto-applies. Targets the first ollama model.
$BenchModel = ($OllamaFleet -split ',')[0].Trim()
Write-Host "`nFull agentic loop L0-L6 (ollama) — $BenchModel ..." -ForegroundColor Cyan
& $Ferric bench --backend openai --api-base "http://localhost:11434/v1" --model $BenchModel --params-b 7 --protocol grammar --results-dir benchmarks

Write-Host "`nReports: toolbench_mistral.md / toolbench_openai.md / toolbench_fleet.md / toolbench_calib.md (+ .jsonl); benchmarks/model_profiles.json (measured_level). Read the verdict bands + the recommended --max-ring." -ForegroundColor Green
