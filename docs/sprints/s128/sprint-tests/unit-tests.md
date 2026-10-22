# Sprint 128 — Unit tests

| Module | Tests |
|---|---|
| `ferric-setup::catalog` | every entry is pinned and well formed (40-hex revision, 64-hex SHA-256, `.gguf`, size, Apache-2.0); the URL names the pinned revision; unknown ids are rejected |
| `ferric-setup::hardware` | small machines get the small model (8 and 11 GiB → 1.5B); typical and unknown machines get 7B (12 and 64 GiB, and unknown) |
| `ferric-setup::paths` | the explicit override wins; Unix uses XDG then HOME, and blank values are ignored |
| `ferric-setup::fetch` | only https, or loopback http, is accepted (including `127.0.0.1.attacker.example`, `ftp:` and `file:` refusals); the part file sits beside the destination |
| `ferric-setup::engine` | each supported platform maps to one CPU asset, and unsupported OS or CPU combinations are refused; an asset needs a `sha256:` digest; no record (or a missing server) means no managed engine |
| `ferric-cli::setup_cmd` | Enter and yes accept, anything else declines; a folder with a GGUF needs no model, and a request overrides that; the offer names source, size, license, destination and SHA-256, and says "as requested" for a requested model |
| `ferric-cli::startup` | the existing precedence holds; the default prefers `./models`, then Ferric's data folder |
