Finalized - DO NOT EDIT

# Sprint 32 Build Plan — Ornstein increment 3: the Tailnet/NAS-FS retriever (Tailscale SSH)

The second source plane behind the `Retriever` keystone: search a *remote* device's filesystem
over Tailscale SSH, feed matches to the same quarantine. Per the user, build the deterministic
core + unit tests now; defer the live E2E (no SSH target currently reachable). Mirrors
`server.rs`'s tested-`command()`-builder / separate-spawn split. Rationale:
`sprints/s32/sprint-research/research-report.md`.

## Schema Tree
- **Goal:** the tailnet plane (pure core tested; live spawn deferred), recorded.
  - **A. pure argv/escaping/parse core** — T-3201
  - **B. `TailnetFsRetriever`** — T-3202
  - **C. ADR + docs** — T-3203

## Execution Sequence

### T-3201: `SshTransport` + pure helpers (escaping / argv / status-parse)
- **Touches:** `crates/ferric-research/src/retriever.rs`
- **Depends on:** —
- **Description:** `enum SshTransport { Tailscale, Plain { port: u16 } }`; `shell_single_quote(s)` (POSIX `'…'`, embedded `'` → `'\''`); `ssh_search_argv(transport, host, query, remote_root, max_files)` (remote cmd `grep -rIl -- 'Q' 'ROOT' | head -n N`, escaped; Tailscale=`tailscale ssh host -- cmd`, Plain=`ssh -p P -o BatchMode=yes -o ConnectTimeout=8 host -- cmd`); `ssh_cat_argv(transport, host, remote_path)` (escaped); `parse_status_devices(stdout) -> Vec<TailnetDevice{name, ip, online}>`.
- **Success (EARS):**
  - WHEN the query has shell metacharacters THEN the built remote command SHALL carry them safely single-quoted (no injection).
  - WHEN built for `Tailscale` vs `Plain` THEN the argv SHALL use the `tailscale ssh` vs `ssh -p … BatchMode` form.
  - WHEN `tailscale status` output is parsed THEN per-device online/offline + IP SHALL be correct.

### T-3202: `TailnetFsRetriever`
- **Touches:** `crates/ferric-research/src/retriever.rs`, `crates/ferric-research/src/lib.rs`
- **Depends on:** T-3201
- **Description:** `TailnetFsRetriever { host, remote_root, transport, max_files, max_bytes_per_file }` impl `Retriever`: `plane()="tailnet"`; `available()` = host online in `tailscale status`; `retrieve()` spawns `ssh_search_argv` → parse file list (cap) → spawn `ssh_cat_argv` per file (byte-cap) → `RetrievedChunk{source:"host:relpath", content}`. `RetrieveError` gains an exec/ssh variant. Re-export from `lib.rs`.
- **Success (EARS):**
  - WHEN `host` is offline/absent THEN `available()` SHALL be false (so `research()` is a no-op).
  - WHEN a remote search returns files THEN each SHALL become a `RetrievedChunk` tagged `host:relpath`.

### T-3203: ADR-042 + docs
- **Touches:** `decisions.md`, `docs/ornstein.md`, `README.md`, `agent-tasks/*`
- **Depends on:** T-3202
- **Description:** ADR-042 (tailnet plane; `tailscale ssh` vs plain-`ssh` transports; remote-shell injection risk + escaping defense; the live-probe finding + deferred-E2E; Web is next). docs/ornstein.md tailnet section; README Status 32.
- **Success (EARS):** WHEN the sprint closes THEN `decisions.md` SHALL contain ADR-042 and README SHALL show Sprint 32.

## Post-build (test)
- `cargo test -p ferric-research` (new pure-helper tests + the existing 11) + `cargo test --workspace` green; clippy `-D warnings`; fmt. The injection-escaping tests are the load-bearing proof; live E2E deferred + documented.
