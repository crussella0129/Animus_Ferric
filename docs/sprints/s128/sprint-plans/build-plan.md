# Sprint 128 — Build plan

The owner's goal (2026-10-22): "something as easy to set up and use as Hermes,
Claude Code or Codex: you do one or two things and boom, it's ready."

Owner decisions, recorded the same day:

- **Surface.** The standalone `ferric`.
- **Downloads.** Yes, after one consent prompt, pinned and SHA-256-verified.
- **Install.** Prebuilt binaries and a one-line installer. The first release
  tag needs the owner's approval.

## The friction removed

| Before | After |
|---|---|
| Install Rust, clone, `cargo install` (minutes) | `curl … \| sh` or `irm … \| iex` (seconds, prebuilt, verified) |
| Find the right llama.cpp asset for your OS and GPU, unzip it, put it on PATH | Offered and installed on the first run |
| Pick a GGUF and quantization for your RAM, download it | Offered (sized to RAM) and installed on the first run |
| Set `FERRIC_MODELS_DIR` | Not needed: Ferric's data folder is the default |
| Run `ferric` | Run `ferric` |

## Execution sequence

| Task | What | Acceptance criteria |
|---|---|---|
| T-12801 | `ferric-setup` crate: catalog, hardware, fetch, engine, paths | INT-0016 AC-4–AC-6 |
| T-12802 | `ferric setup` and the first-run offer, managed engine and models lookup | AC-3, AC-6, AC-7 |
| T-12803 | Release workflow, `install.sh`, `install.ps1` | AC-1, AC-2 |
| T-12804 | Fresh-machine proof, docs, records | AC-8, AC-9 |

## Design decisions

- **One prompt, Enter accepts.** The offer names the source, size, license
  and destination before any byte moves. Non-interactive runs never
  download.
- **Pins live in source.** Model revision, SHA-256 and size come from the
  Hugging Face API. The engine tag is pinned, and its asset digest comes from
  GitHub's release record at install time. An asset without a digest is
  refused. GitHub release metadata was not reachable from the build sandbox,
  so engine digests are read at install time instead of being compiled in.
- **Verified before rename.** Nothing reaches a path Ferric will load until
  its hash matches. Part files resume.
- **The system `tar`, not an archive crate.** It reads `.tar.gz` everywhere
  and `.zip` on Windows 10 and later, so ADR-004 gains no dependency.
- **CPU builds only for now.** Startup launches engines CPU-only
  (`gpu_layers 0`), so GPU builds wait for measured offload (T-12806).
- **Respect what exists.** A PATH `llama-server` and existing GGUFs are used
  as they are. `./models` still wins when it exists.
