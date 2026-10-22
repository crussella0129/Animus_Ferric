# Security: localhost is not a trust boundary

Sprint 127 ([INT-0015](intents/INT-0015-localhost-is-not-a-trust-boundary.md)).

Local agent harnesses tend to serve an HTTP API on `127.0.0.1` and treat
"only this machine can reach it" as authentication. That assumption fails in
two ways:

- **Every process on the machine can reach loopback.** That includes a
  compromised dependency's install script, another user's session, and any
  container sharing the host network.
- **Every web page the user opens can reach loopback through the browser.**
  A cross-site HTML form POST and a `no-cors` `fetch` with a `text/plain`
  body are "simple requests" that need no CORS preflight. DNS rebinding goes
  further: it makes `attacker.example` resolve to `127.0.0.1`, so the
  browser treats the local server as the attacker's own origin and lets the
  page read the responses.

In 2026 this went from theory to repeated incidents in opencode. In January,
CVE-2026-22812: an unauthenticated local server with permissive CORS that let
local processes and web sites run shell commands. Also in January,
GHSA-c83v-7274-4vgp: an XSS-to-command-execution chain through the web UI's
`/pty/` endpoints. Then a later advisory for 1.14.30–1.18.21: a crafted HTML
form POST from any page reached the local API and ran commands. Separately,
PHP2Shell (CVE-2026-109618 and CVE-2026-109619, October 2026) showed how much
lead time attackers can have: a honeypot recorded exploitation four days
before disclosure. A harness has to assume its dependencies will have
zero-days and make sure one hole does not hand over the machine.

## What Ferric does about it

Ferric's design already keeps the model away from security decisions:

- The harness owns the action grammar, so the model can only emit a
  well-formed call to an offered tool.
- The guard's denylists are compiled in (ADR-005), and the model is never
  consulted on a security decision.
- Workspaces are contained, and `.ferricignore` can only add restrictions.
- `--accept-edits` puts a human in front of every write.
- Research is quarantined behind a taint boundary.

Sprint 127 applies the same idea to the network: no decision is left to the
request.

### The gate (`ferric-gate`)

Every Ferric HTTP service (`ferric api` and `ferric-valve`) passes each
request through four checks, in a fixed order. Each check stands on its own,
so passing one never excuses another:

| # | Check | Refuses | Stops |
|---|---|---|---|
| 1 | `Host` must name this listener (`127.0.0.1:<port>`, `localhost:<port>`, `[::1]:<port>`, or the tunnel address) | `403 foreign-host` | DNS rebinding, even when the page somehow holds the token |
| 2 | No browser: any `Origin` or `Sec-Fetch-*` header | `403 browser-origin` / `browser-fetch` | Drive-by form POSTs and `no-cors` fetches, even with a leaked token |
| 3 | `Authorization: Bearer <token>`, compared as SHA-256 digests in constant time | `401 missing-token` / `bad-token` | Any local process without the token |
| 4 | `POST`/`PUT`/`PATCH` bodies must be `application/json` | `415 not-json` | CSRF-shaped bodies that need no preflight |

The gate also guarantees:

- No CORS header is ever sent, so a browser cannot read a response even if
  a request got through.
- Every refusal carries `x-ferric-gate: <code>` and a JSON error.
- An admitted request has its `Authorization` header removed before any
  handler runs, so the gate token is never forwarded to llama.cpp.
- `GET /health` on the API skips only the token check.

**Tokens.** Each service mints a 256-bit token from the OS CSPRNG on first
start. The token is `ferric_` followed by 64 hex digits; the prefix makes a
leak easy to find with secret scanners. It is written to a file created mode
0600 in a mode-0700 directory:

| Service | Token file |
|---|---|
| `ferric api` | `$FERRIC_STATE_DIR/gate/api.token`, else `~/.local/state/ferric/gate/api.token` (`%LOCALAPPDATA%\ferric\gate\api.token` on Windows) |
| `ferric-valve` | the same directory, `valve.token` |

The file is outside every workspace, so the model's file tools cannot read
it. A token file that group or others can read is refused at startup. Use
`--token-file` to choose the path.

**Binding.** A service binds loopback, or the host address of a Ferric
tunnel. `0.0.0.0`, `::` and any other address are refused before the socket
opens.

### Remote access without a coordination server

Tailscale is convenient, but its coordination server is a third party in the
key-distribution path and a single point of control. Ferric's alternative is
a two-peer WireGuard plan that you own end to end, run on Mullvad's
[GotaTun](https://github.com/mullvad/gotatun): a userspace WireGuard written
in Rust and forked from Cloudflare's BoringTun.

- `ferric tunnel init` generates X25519 key pairs for the host and one
  client, plus a preshared key, which adds a symmetric layer to the
  handshake. No root is needed.
- The client's `AllowedIPs` is the host's single tunnel address
  (`10.77.0.1/32`), so the tunnel reaches Ferric and nothing else on your
  network.
- Every file is written mode 0600, and an existing plan is never
  overwritten.
- The files are standard WireGuard configs: any client works, including the
  official mobile apps and `wg-quick`.
- Only the step that creates a network interface needs root. Ferric itself
  never does.

The `ferric tunnel` commands are a Ferric feature built in sprint 127. The
GotaTun data plane is a separate binary from Mullvad; Ferric configures it
over the standard WireGuard UAPI socket.

## Demo script

The first demo is offline and needs no model and no network:

```sh
cargo build --release -p ferric-cli
./target/release/ferric secure demo
```

It serves one agent endpoint twice on loopback. The first copy is naive: no
credential and CORS open to every origin, the pattern behind the opencode
advisories. The second copy is the same endpoint behind the gate. The demo
replays eight attacks against both, plus a legitimate client:

```
  naive localhost: 8/8 attacks reached the agent
  ferric-gate:     0/8 attacks reached the agent
  legitimate client through the gate: admitted
```

Next, attack a real Ferric API. This uses the mock model, so no engine is
needed:

```sh
ferric api --mock --port 3581 &
ferric secure probe http://127.0.0.1:3581 --token-file ~/.local/state/ferric/gate/api.token
curl -s http://127.0.0.1:3581/health                                     # open
curl -s -X POST http://127.0.0.1:3581/v1/query \
     -H 'content-type: application/json' -d '{"prompt":"hi"}'           # 401
curl -s -X POST http://127.0.0.1:3581/v1/query \
     -H "authorization: Bearer $(cat ~/.local/state/ferric/gate/api.token)" \
     -H 'content-type: application/json' -d '{"prompt":"hi"}'           # runs
ferric api --mock --host 0.0.0.0 --port 3582                             # refused at startup
```

`probe` exits non-zero if any attack reaches the application, so it can run
in CI or a cron job.

Finally, remote access over your own WireGuard tunnel. This is Linux, and
`up` needs root:

```sh
cargo install gotatun-cli                       # Mullvad's userspace WireGuard
ferric tunnel init --endpoint <this-host>:51820 # keys + configs, mode 0600
sudo ferric tunnel up                           # ferric0 at 10.77.0.1, via GotaTun
ferric api --tunnel                             # listens on 10.77.0.1 only, token still required
ferric tunnel status                            # peers, last handshake, bytes
```

Import the client file (`~/.local/state/ferric/tunnel/client-1.conf`) on the
other device. Any WireGuard client works; on a Linux client,
`sudo ferric tunnel up --conf client-1.conf --interface ferric1` does it. From
there, `http://10.77.0.1:3581` is reachable with the token. Nothing is exposed
on the LAN address, and `ferric secure probe` run across the tunnel passes
every row. Run `sudo ferric tunnel down` to remove the interface.

## Hermes and the valve

```sh
llama-server --api-key-file ~/llama.key ... &      # lock the engine too
ferric-valve --upstream http://127.0.0.1:8080 --upstream-key-file ~/llama.key
```

Point Hermes's custom provider at `http://127.0.0.1:8090/v1` and set its
`api_key` to the contents of `valve.token`. The valve presents the llama.cpp
key upstream and never forwards Hermes's credential. This includes the
pass-through paths: `/slots`, `/props` and the rest of llama.cpp's surface are
gated like chat completions.

## Limits, stated plainly

- **The token is a bearer secret.** Any process running as your user can
  read your token file, just as it can read your SSH keys. The gate protects
  against other users, other machines, containers and web pages. It cannot
  protect you from malware already running as you. Containing that is a
  sandbox's job (see the follow-ups).
- **`ferric server up` does not yet start llama-server with a key.** Pass
  `--api-key-file` yourself, as above, until T-12705 lands.
- **`ferric tunnel up` drives GotaTun on Linux only.** On macOS and Windows,
  import `host.conf` into the WireGuard app.
- **Browsers are refused outright.** A web UI would need an explicit
  per-origin allow-list, and none exists yet (T-12708).
- **The MCP surface is stdio.** It has no network listener, so the gate does
  not apply to it.
