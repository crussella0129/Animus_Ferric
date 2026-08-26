# Sprint 111 Walkthrough — Live Monday Path

The acceptance path now starts an actual local model server through Ferric and
runs a real agent task. From the repository root, use the backend-enabled
`target/release/ferric.exe`, preflight the exact GGUF with `server doctor`, and
launch it with `server up`. In a second terminal, require `server status`, HTTP
health, and model metadata before running:

```powershell
.\tools\e2e_test.ps1 -Model qwen2.5-coder-7b
```

The run is accepted only when the script prints `E2E PASS`; it validates both
filesystem effects and the terminal trace. Finish with `server down` and check
that `server status` reports no registration. The exact copy-paste sequence is
in `docs/demo-guide.md`.

On the validated host, the 7B task took about 58 seconds because the installed
engine distribution appeared CPU-only. Leave that latency in the presentation
budget or install a GPU-enabled llama.cpp build before rehearsing again.
