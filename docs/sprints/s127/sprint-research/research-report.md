# Sprint 127 — Research

## The incidents (as of 2026-10-22)

- **opencode CVE-2026-22812 (January 2026).** Versions before 1.0.216
  started an unauthenticated HTTP server with permissive CORS. Local
  processes and web sites could run shell commands through it. Fixed in
  1.0.216.
- **opencode GHSA-c83v-7274-4vgp (January 2026).** An XSS in the web UI
  reached command execution through the `/pty/` endpoints. Fixed in 1.1.10.
- **opencode 1.14.30–1.18.21 (reported about late September 2026).** A
  crafted HTML form POST from any page reached the local API and ran
  commands. Exposure required `serve`/`web` without password protection, or
  basic-auth credentials cached in the browser. Fixed in 1.18.22.
- **PHP2Shell, CVE-2026-109618 and CVE-2026-109619 (disclosed 2026-10-21).**
  Unauthenticated RCE in the PHP interpreter, CVSS v4 9.2. Amazon's honeypot
  recorded an exploitation attempt on 2026-10-17, four days before
  disclosure. Other vendors disagree on how widespread exploitation is.

The shared lesson for an agent harness: a local HTTP listener that can drive
tools is a remote-code-execution endpoint unless it authenticates every
request, and refuses both browsers and foreign `Host` names. Separately,
dependencies will have zero-days, so no single layer may be load-bearing.

Sources: SentinelOne and cvereports.com on CVE-2026-22812; the GitHub
advisory GHSA-c83v-7274-4vgp; cybersecuritynews.com and thehackerwire.com on
the opencode server RCE; Help Net Security, The Hacker News, Rapid7 and
watchTowr on PHP2Shell.

## GotaTun

GotaTun is Mullvad's WireGuard implementation in Rust, a fork of
Cloudflare's BoringTun, announced in December 2025. On crates.io it is
`gotatun` and `gotatun-cli` 0.10.2, MPL-2.0. The CLI creates a TUN device
and a UAPI socket at `/var/run/wireguard/<if>.sock`, then daemonizes.
Building with `--no-default-features --features ring` avoids aws-lc-rs's C
toolchain.

As plain root without a terminal, its privilege drop fails (`getlogin`
returns NULL). `--disable-drop-privileges` is the documented flag for that
case; under `sudo` the drop works and is kept.

## Tailscale versus a self-hosted plan

Ferric already integrates Tailscale serve. Tailscale's coordination server
distributes node keys and ACLs, which makes it a third party in the trust
path and a single point of control. A two-peer WireGuard plan with locally
generated keys has neither, at the cost of manual endpoint configuration and
no NAT traversal. That is acceptable for "my laptop to my GPU box".
