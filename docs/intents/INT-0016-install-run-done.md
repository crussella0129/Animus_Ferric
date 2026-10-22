# INT-0016 — Install, run, done

<!-- sprint-loop-intent-v2 -->
- **Intent ID:** INT-0016
- **State:** active
- **Work evidence:** [Sprint 128 build plan](../sprints/s128/sprint-plans/build-plan.md); [T-12801–T-12804](../work/completed-tasks.md#t-12801-sprint-128); [open follow-ups](../work/tasks.md#install-run-done-int-0016)
- **Completion evidence:** none
- **Code evidence:** `crates/ferric-setup` (catalog, hardware, fetch, engine, paths); `crates/ferric-cli/src/{setup_cmd,startup,human}.rs`; `install.sh`, `install.ps1`, `.github/workflows/release.yml`
- **Test evidence:** [Sprint 128 test report](../sprints/s128/sprint-tests/test-report.md)
- **Documentation evidence:** [Getting started §0](../getting-started.md); README "Install and run"

## Intent

Getting Ferric working should be as short as with Hermes, Claude Code or
Codex. A new user runs two things:

1. **One install line.** `curl … | sh`, or `irm … | iex` on Windows, puts a
   prebuilt, checksum-verified `ferric` on the PATH.
2. **`ferric` in a project folder.** When the machine has no engine or no
   model, Ferric says exactly what it would download, from where, at what
   size and under which license. It asks once, fetches pinned files,
   verifies each SHA-256, and opens the session.

Every later `ferric` opens the session directly.

The owner directed this on 2026-10-22. It reverses Sprint 125's "no
auto-download" deferral (INT-0008 AC-13). Downloads now happen, but only after
one consent, only from pinned sources, and only after verification. It is
also an explicit exception to INT-0010's maintenance status for the
standalone surface: the install and first-run path is active. The rest of
the standalone surface stays maintenance-only.

## Acceptance criteria

1. A tag builds prebuilt `ferric` binaries for Linux x64 and arm64, macOS
   arm64 and x64, and Windows x64. The release assets have stable names, and
   each has a SHA-256 file.
2. `install.sh` and `install.ps1` install the latest release for the
   machine, verify the SHA-256 (refusing on mismatch), and place `ferric` on
   the user's PATH. Before any release exists, they build from source with
   cargo.
3. When the engine or every model is missing, an interactive `ferric` run
   describes the download and asks once (Enter accepts). A non-interactive
   run downloads nothing and names the command that would.
4. Models are pinned to a Hugging Face revision, with SHA-256, size and
   license recorded in source. The engine is pinned to a llama.cpp release
   tag, and its asset is verified against the digest GitHub records for that
   release. An asset without a digest is refused.
5. A download reaches its final path only after its SHA-256 matches.
   Interrupted downloads resume with a range request. Only `https`, or
   loopback `http` for a local mirror, is fetched, including after
   redirects.
6. The default model fits the machine. Under 12 GiB of RAM the 1.5B model is
   offered, otherwise the 7B. Other sizes are one flag away (`ferric setup
   --model`, `--list`).
7. What the person already has is used without downloading: `llama-server`
   on PATH, and GGUFs in `./models`, `FERRIC_MODELS_DIR` or a configured
   `models_dir`. Downloads go to a per-user data folder, so one download
   serves every project.
8. A fresh-machine run is proven end to end: install into an empty home,
   then setup, then a real model answering a real prompt.
9. Follow-ups are recorded: GPU builds and offload, the first published
   release, package managers, self-update, and macOS memory detection.

## Rationale

Every step a newcomer must research is a place they give up: which llama.cpp
asset, which GGUF quantization, where to put it, which variable to set. The
harness already knows the answers, so it should do them, while keeping the
person in control (one explicit yes) and keeping supply-chain integrity
(pinned and verified) at the level the rest of Ferric holds.

## Alternatives

- **Keep acquisition manual.** This is the status quo that made the flow six
  steps long.
- **Fetch the newest model or engine at run time.** It is unpinned, so
  upstream can change underneath the user. Rejected.
- **An archive library for extraction.** Rejected in favour of the system
  `tar`, which reads `.tar.gz` everywhere and `.zip` on Windows 10 and later.
  That means no new dependency (ADR-004).
- **cargo-only installation.** It needs Rust and minutes of compiling. It is
  kept only as the installer's fallback.

## Consequences

- The first release tag must be pushed (with the owner's approval) before
  the one-liners stop falling back to cargo.
- The catalog's pins need deliberate updates. A new model or engine version
  is a reviewed source change with new hashes.
