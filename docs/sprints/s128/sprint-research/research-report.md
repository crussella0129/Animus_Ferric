# Sprint 128 — Research

- **The setup bar.** Claude Code, Codex and Hermes Agent each install with
  one command (a shell or npm one-liner), then start with one command. Any
  further setup is an in-app first-run flow. Ferric needed six manual steps,
  three of which (engine asset, model and quantization, models folder)
  require knowledge the harness already has.
- **llama.cpp release assets** (read from `.github/workflows/release.yml` and
  `release-publish.yml` at commit 970b51c):
  - Linux `llama-<tag>-bin-ubuntu-{x64,arm64}.tar.gz`
  - macOS `llama-<tag>-bin-macos-{arm64,x64}.tar.gz`
  - Windows `llama-<tag>-bin-win-cpu-{x64,arm64}.zip`

  Tarballs have a `llama-<tag>/` root, and Windows zips are flat. The
  Windows CUDA and Vulkan zips have the CPU toolset injected; the CUDA
  runtime ships separately (`cudart-llama-bin-win-cuda-*.zip`). The pinned
  tag is `b11780` (2026-10-21).
- **Models.** These were read from the Hugging Face API (`?blobs=true`) at
  the pinned revisions. All are Apache-2.0, single-file Q4_K_M.

  | Model | Size (bytes) | SHA-256 |
  |---|---|---|
  | Qwen2.5-Coder 0.5B | 491,400,064 | `1d96…ff32` |
  | Qwen2.5-Coder 1.5B | 1,117,320,768 | `cc32…b046` |
  | Qwen2.5-Coder 7B | 4,683,073,536 | `5092…4d3c` |
  | Qwen2.5-Coder 14B | 8,988,110,272 | `c1e6…d940` |

  Qwen2.5-Coder-7B Q4_K_M is the model most used across recent sprints'
  evidence.
- **Sandbox limits found.**
  - github.com release downloads are a policy 403, and llama.cpp's REST API
    needs a push attachment, which was not requested. The engine path was
    therefore proven through a local mirror of the release API, serving a
    static `llama-server` built from source.
  - Hugging Face, and its CDN with ranges, are reachable.
  - The managed engine launch is refused by Ferric's port-ownership check
    in this sandbox. That is pre-existing; see T-12810.
