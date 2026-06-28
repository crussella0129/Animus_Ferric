# Sprint 32 Research Report — Ornstein increment 3: the Tailnet/NAS-FS retriever (Tailscale SSH)

## Sprint goal (in my words)
Add the **second source plane** behind the `Retriever` keystone: a **Tailnet/NAS-FS retriever**
that searches a *remote* device's filesystem over **Tailscale SSH**, returning matching files as
untrusted chunks → the same quarantine. User-chosen build order (Local FS ✅ → **Tailnet/NAS** →
Web). Per the user: **build the deterministic core + unit tests now, defer the live E2E** until a
tailnet SSH target is reachable.

## Live tailnet probe (evidence, this machine — 2026-06-28)
- `tailscale status`: **`pixel-10-pro-xl` 100.100.225.71 android — online**; `switchblade`
  100.98.104.44 linux — **offline (last seen 1h ago)**.
- `tailscale ping pixel-10-pro-xl` → **pong** (833 ms via LAN 192.168.86.31) — reachable.
- `tailscale ssh pixel-10-pro-xl -- echo` → **502 / connection refused on :22** — Android has no
  Tailscale SSH *server* (that's a Linux feature).
- plain `ssh -p 8022` (Termux default) → **connection refused** — no Termux `sshd` up right now.
- **Conclusion:** no SSH target is currently reachable (the Linux box that supports `tailscale
  ssh` is offline; the Pixel has no sshd up). So the **live E2E is deferred** (user's call); this
  sprint builds + unit-tests the deterministic core, exactly like `server.rs`'s `command()` is
  tested without a live server.

## Decisions Reviewed
- **ADR-041 (s31)** — the `Retriever` keystone + the Local-FS plane. This adds the tailnet plane
  behind the *same* trait; `research()` is unchanged (it already quarantines every chunk).
- **ADR-005 (boundaries) + the s1 `docker-nix-tailscale.md` verdict** — Tailscale reach via
  `tailscale ssh` (keyless, identity-based, tailnet-only — *never* funnel) for Linux devices;
  plain `ssh -p` for Termux-style sshd. The **remote command must be shell-safe**: ssh always runs
  the command through the remote shell, so the research query (caller-supplied) must be
  single-quote-escaped or it's **remote command injection**. This is the security-critical,
  deterministically-testable core.

## Existing Code Survey
| File | Role / relevance |
|---|---|
| `crates/ferric-cli/src/server.rs` | The subprocess pattern to mirror: a **pure `command()` argv builder** (`LaunchCommand{program,args}`, unit-tested) separated from the spawn (`Command::new().output()/.status()`). The tailnet retriever copies this split: pure argv builders (tested) + a live spawn (deferred E2E). |
| `crates/ferric-research/src/retriever.rs` | The `Retriever` trait + `LocalFsRetriever` to mirror; `RetrievedChunk{source,content}`; `research()` already feeds the quarantine. `RetrieveError` extended for ssh failures. |
| live `tailscale status` output | Format: `IP  name  owner  os  …`; `offline` marker present on down devices → `parse_status_devices` keys off it for `available()`. |

## External Sources
- The live probe above (`tailscale status/ping/ssh`). `tailscale ssh` semantics + the plain-ssh
  fallback are from the s1 artifact + the probe; no new external fetch.

## Risks / unknowns / dependencies
- **Remote command injection (the load-bearing risk):** ssh runs `cmd` via the remote `$SHELL`,
  so the query + remote root **must** be POSIX single-quote-escaped. A dedicated
  `shell_single_quote` + tests (a query with `'`, `;`, `$(...)`, backticks → safely quoted) is the
  core deliverable.
- **Transport variance:** Linux tailnet devices → `tailscale ssh <host> -- cmd` (keyless);
  Termux/other → `ssh -p <port> -o BatchMode=yes <host> -- cmd`. Model both with an
  `SshTransport` enum so the argv builder serves the whole fleet.
- **Live E2E deferred:** `available()` + `retrieve()` spawn real processes — not unit-tested this
  sprint (no reachable target). The *pure* builders/parsers are fully tested; the spawn is a thin
  shell over them (the `server.rs` precedent).
- **Remote tool assumptions:** the remote search uses `grep -rIl` + `cat` (POSIX/busybox-safe).
  Noted; verified at live-E2E time.

## Recommended approach
Extend `crates/ferric-research/src/retriever.rs`:
- **`SshTransport`**: `Tailscale` (`tailscale ssh <host> -- <cmd>`) | `Plain { port: u16 }`
  (`ssh -p <port> -o BatchMode=yes -o ConnectTimeout=8 <host> -- <cmd>`).
- **Pure builders (unit-tested):** `shell_single_quote(s)`; `ssh_search_argv(transport, host,
  query, remote_root, max_files)` → the argv whose remote command is `grep -rIl -- 'Q' 'ROOT' |
  head -n N` with `Q`/`ROOT` escaped; `ssh_cat_argv(transport, host, remote_path)` (path escaped);
  `parse_status_devices(stdout) -> Vec<TailnetDevice{name, ip, online}>`.
- **`TailnetFsRetriever { host, remote_root, transport, max_files, max_bytes_per_file }`** impl
  `Retriever`: `plane()="tailnet"`; `available()` = the host appears **online** in `tailscale
  status` (live, via `parse_status_devices`); `retrieve()` spawns `ssh_search_argv`, parses the
  newline-separated file list (cap `max_files`), spawns `ssh_cat_argv` per file (byte-cap),
  `source = "host:relpath"`. (Spawn = the deferred-E2E part.)
- **Tests (deterministic):** `shell_single_quote` escapes injections; `ssh_search_argv`/`ssh_cat_argv`
  for **both** transports contain the escaped query/path + the right ssh flags; `parse_status_devices`
  parses the real `tailscale status` sample (pixel online, switchblade offline) → correct
  `online` flags + IPs. The live `research(&tailnet, provider, query)` E2E is documented as the
  follow-up once a target's sshd is up.

### Alternative considered — mount the remote share + reuse `LocalFsRetriever` (deferred)
If a device exposes an SMB/NFS share mounted as a path, `LocalFsRetriever::new(mount)` already
works. Rejected as the *first* tailnet mechanism per the user's "tailscale ssh" choice (keyless,
no mount needed, works on headless boxes); the mount path remains available for NAS shares later.
