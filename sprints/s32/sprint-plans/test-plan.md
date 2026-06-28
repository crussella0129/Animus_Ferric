Finalized - DO NOT EDIT

# Sprint 32 Test Plan — Ornstein increment 3 (Tailnet/NAS-FS retriever)

## Unit — `retriever.rs` (`ferric-research`; pure, deterministic, no network)
- **`shell_single_quote` (the security core):** plain text → `'text'`; a value with an embedded
  `'` → the `'\''` splice; `; rm -rf /`, `$(whoami)`, and backtick payloads → each fully enclosed
  in single quotes so the remote shell treats them as one literal argument (no injection).
- **`ssh_search_argv` — Tailscale:** argv = `tailscale` + `["ssh", host, "--", cmd]`; `cmd`
  contains `grep -rIl --` + the **escaped** query + escaped root + `| head -n N`.
- **`ssh_search_argv` — Plain{8022}:** argv = `ssh` + `["-p","8022","-o","BatchMode=yes","-o","ConnectTimeout=8", host,"--",cmd]`; same escaped remote command.
- **`ssh_cat_argv`:** both transports; the remote path is escaped; `cat --` form.
- **`parse_status_devices`:** the captured real `tailscale status` sample (pixel online @
  `100.100.225.71`; `switchblade … offline`) → `pixel-10-pro-xl` `online=true` with that IP,
  `switchblade` `online=false`.

## Build / Lint (default CI)
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean;
  `fmt --check` clean.

## E2E (deferred — documented, not run this sprint)
- No SSH target is reachable now (Pixel has no sshd; switchblade offline). The live run —
  `research(&TailnetFsRetriever{ host, remote_root, transport }, provider, query)` against a live
  device (Termux `Plain{8022}` on the Pixel, or `Tailscale` on switchblade when back) → quarantined
  `host:path` digests — is recorded as the follow-up. The deterministic core (escaping, argv,
  status-parse) is fully covered now; the spawn is a thin shell over it (the `server.rs` precedent:
  `command()` tested, spawn not).
