# Sprint 114 Hardware Inventory

- Captured: 2026-08-26 22:48 EDT
- Operating system: Windows 11 Pro, build 26200
- CPU: AMD Ryzen 9 5900X, 12 physical cores / 24 logical processors
- Installed RAM: 34,281,484,288 bytes (31.93 GiB)
- GPU: NVIDIA GeForce RTX 2080 Ti, compute capability 7.5
- GPU VRAM: 11,264 MiB total; 8,667 MiB free at capture
- NVIDIA driver: 596.49
- Repository volume: 999,199,973,376 bytes total;
  84,141,854,720 bytes free at capture
- Windows Rust: `rustc 1.96.0`, `cargo 1.96.0`
- WSL Rust: `rustc 1.96.1`, `cargo 1.96.1`
- WSL Bash: GNU Bash 5.3.9
- `llama-server`: build 10034 (`505b1ed15`), Windows x86_64
- `ferric`: 0.1.0
- Docker daemon: unavailable at capture
- WSL Bubblewrap: available and a read-only, network-unshared smoke command
  returned `sandbox-ok`

## Existing local model

- File: `models/qwen2.5-coder-7b-instruct-q4_k_m.gguf`
- Size: 4,683,073,536 bytes
- SHA-256:
  `509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c`
- Ferric server state at capture: no registered server and no project
  `.ferric/server.json`
- Installed Ferric skills at capture: none

The inventory deliberately omits hostname, account identity, and absolute home
paths from tracked prose. Command output used for these values was observed
directly on the test host during Sprint 114 research.

## Qwen3.8 plan-freeze preflight

- Captured: 2026-08-26 23:15:22 EDT
- Free RAM: 16,151,130,112 bytes (15.04 GiB)
- Repository-volume free space: 84,543,565,824 bytes (78.74 GiB)
- GPU VRAM: 11,264 MiB total; 8,772 MiB free

These are availability observations, not reservations. Acquisition and server
startup recheck them because concurrent applications can change both RAM and
VRAM before inference.
