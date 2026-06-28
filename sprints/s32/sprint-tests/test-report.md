# Sprint 32 Test Report — Ornstein increment 3 (Tailnet/NAS-FS retriever, ADR-042)

**Date:** 2026-06-28. Added the second source plane behind the keystone — a remote-FS retriever
over Tailscale SSH. The security-critical core (remote-shell escaping) + the argv builders +
status parsing ship and are fully unit-tested; the live SSH E2E is deferred (no target reachable).

## Build / Lint (green)
- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean.

## Unit — `ferric-research::retriever` (pure, deterministic, no network) — 6 new / 17 total pass
- **`shell_single_quote_neutralizes_injections`** (the security headline): `'`→`'\''` splice;
  `; rm -rf /`, `$(whoami)`, `` `id` ``, `&& curl evil` each land inside one single-quoted literal
  — the remote shell can't interpret them. Untrusted research input cannot become a remote command.
- `ssh_search_argv_tailscale_form`: `tailscale` + `["ssh", host, "--", cmd]`; `cmd` = `grep -rIl --`
  + escaped query + escaped root + `head -n N`.
- `ssh_search_argv_plain_form`: `ssh` + `-p 8022 -o BatchMode=yes -o ConnectTimeout=8 host -- cmd`.
- `ssh_cat_argv_escapes_path`: `cat -- 'PATH'` with the path single-quoted.
- `parse_status_devices_reads_online_offline`: the **captured real** `tailscale status` sample →
  `pixel-10-pro-xl` online @ `100.100.225.71`, `switchblade` offline.
- `tailnet_retriever_plane_label`: `plane() == "tailnet"`.

## Live tailnet probe (research evidence)
`tailscale ping pixel-10-pro-xl` → pong; `tailscale ssh …:22` → refused (Android has no Tailscale
SSH server); plain `ssh :8022` → refused (Termux sshd not up); `switchblade` offline. **No SSH
target reachable**, so the live E2E is the documented follow-up (run when a target's sshd is up).

## Verdict
**Ornstein increment 3 validated (deterministic core).** The tailnet plane plugs into the same
`Retriever`/`research()` pipeline with zero pipeline change — the funnel is source-agnostic. The
load-bearing risk (remote command injection via the research query) is closed by
`shell_single_quote`, proven against `;`/`$()`/backtick payloads; defense in depth, the fetched
content still flows through the quarantine. The live spawn is a thin shell over the tested builders
(the `server.rs` precedent: `command()` tested, spawn not). Next: the live SSH E2E (once a target's
sshd is up) + the Web plane + container/proxy. ADR-042.
