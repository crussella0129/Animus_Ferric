# Sprint 128 — Integration tests

`crates/ferric-setup/tests/mirror.rs`, against a loopback axum mirror that
speaks the GitHub release API and HTTP ranges:

- **A verified download lands.** A matching download reaches its final path,
  leaves no part file, and reports full progress.
- **A mismatch is discarded.** A mismatched download returns `Checksum`, and
  neither the final file nor the part file remains.
- **An interrupted download resumes.** It re-hashes the half already on disk
  and sends exactly `bytes=<half>-`.
- **An existing verified file is kept.** It is not fetched again; the URL
  points at a dead port to prove it.
- **The engine installs.** It installs from a release whose asset carries a
  matching digest. The archive is a `.tar.gz` with a `llama-<tag>/` root,
  like llama.cpp's. The server's `--version` is smoke-run, the install is
  recorded in `engine.json`, and the archive is removed.
- **A wrong engine digest is refused.** No engine is recorded.

CLI guards:

- **`human_cli`.** The primary help lists `run`, `setup`, `status`,
  `explain` and `advanced` (without the backend, the old four).
- **`getting_started_doc`.** The guide must show the one-liners,
  `ferric setup` and SHA-256.
- **`charter_docs`.** INT-0016 is a live intent.
