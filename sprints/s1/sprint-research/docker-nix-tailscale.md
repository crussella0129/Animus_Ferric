# Artifact: Docker / Nix / Ornstein / Tailscale Substrate (mid-2026)

> Source: web-research agent, 2026-06-10.

## Docker from Rust

- **bollard** (v0.21.x) is the choice: async, tracks Moby API 1.52, version negotiation, Windows named pipes, **first-class rootless Podman** (auto socket discovery).
- Pattern to copy: **OpenHands' action-execution-server** — harness launches a runtime container running a small server; loop = send Action → receive Observation; workspace bind-mounted. Beats raw `docker exec` for streaming/PTY/file ops.
- Isolation ladder consensus: runc → gVisor (runsc, systrap platform — works in WSL2 without nested virt) → Firecracker microVM. Hardened runc is the pragmatic floor for own-code-own-repo; runsc opportunistically (`HostConfig.runtime`); note Docker Desktop fights custom runtimes — plain Docker-CE in a WSL2 distro is more reliable.
- Minimal throwaway pattern: create-not-run; pinned image digest; `network_mode: none` default; `cap_drop ALL`; `no-new-privileges`; read-only rootfs + tmpfs /tmp noexec; memory/cpu/pids limits all set; non-root UID; workspace `:ro` by default with explicit `:rw` escalation; stream logs with timeout → kill → auto-remove. One container per trust boundary.

## Nix

- Flakes stable-in-practice; **dockerTools.buildLayeredImage = OCI images without Dockerfiles or a daemon**, reproducible, 10–30 MB — the agent composes sandbox images declaratively and bollard-loads them (Modal's "image defined in code" idea, hermetically).
- **Windows: WSL2-only** (no native port coming). NixOS-WSL is active. Same WSL2 world as the Docker engine — convenient.
- Settled composition: **Docker-from-Nix** (Nix = environment compiler, Docker = executor). Nix-inside-Docker only as fallback for Nix-less hosts.
- Design rule for Ferric: "Nix available?" is a capability probe, not an assumption.

## The Ornstein pattern (quarantined retrieval)

State of the art = **dual-LLM quarantine + CaMeL-style information-flow control**:
1. Research/browse in its own hardened container; egress only via allowlist proxy; planner's container has no direct internet.
2. Retrieved content → **quarantined summarizer model** (no tools, no memory — a local small model is perfect) → typed, schema-validated output (claims/URLs/quotes), never free-form instructions.
3. Results cross as **provenance-tagged data variables**; tool calls with tainted-derived arguments require policy approval (CaMeL-lite: tainted-string tracking + sink policy table; full interpreter not required).
4. Retrieved text NEVER writes to agent memory/config without an explicit gate (OpenClaw memory-poisoning lesson: time-shifted triggers).
- Willison's lethal trifecta (private data + untrusted content + exfil channel — remove a leg) is the design frame. Container isolation handles *code* escape; quarantine handles *semantic* escape; both are required.

## Tailscale from Rust

- **LocalAPI** over safesocket: Linux unix socket; Windows named pipe `\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled`. Key endpoints: `/localapi/v0/status` (peer map), `/localapi/v0/whois?addr=` (inbound connection → tailnet identity — the auth primitive).
- Crates: `tailscale-localapi` (typed client, works today); **`tailscale-rs`** (official native-Rust tsnet, announced Apr 2026 — EXPERIMENTAL: `TS_RS_EXPERIMENT` flag, DERP-only, not for production; watch, don't build on); `tailscale status --json` subprocess as zero-dep fallback.
- **Tailscale SSH**: tailscaled owns :22 on the tailnet IP; identity + ACL `ssh` rules; no key management.
- Verdict: (a) enumerate devices via LocalAPI /status; (b) reachability via Tailscale SSH + `tailscale serve` (tailnet-only; NEVER funnel for an agent), with LocalAPI `whois` for identity-based authz inside Ferric; (c) call peers via plain TCP through host tailscaled. ACL pattern: `tag:animus`, restrict to your devices, narrow outbound dst list.

## Sources

1. https://tailscale.com/blog/tailscale-rs-rust-tsnet-library-preview
2. https://github.com/fussybeaver/bollard
3. https://simonwillison.net/2025/Jun/13/prompt-injection-design-patterns/
4. https://docs.openhands.dev/openhands/usage/architecture/runtime
5. https://gvisor.dev/docs/architecture_guide/platforms/
Extras: simonw.substack.com (CaMeL); infoq.com/news/2025/04/deepmind-camel-promt-injection; tailscale.com/docs (ssh, serve); github.com/jtdowney/tailscale-localapi; mitchellh.com/writing/nix-with-dockerfiles; numtide.com/blog/nix-docker-or-both; nix.dev dockerTools tutorial; github.com/nix-community/NixOS-WSL; northflank.com sandboxing guides; OpenClaw incident analyses.
