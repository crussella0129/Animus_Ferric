# Sprint 127 — End-to-end tests

## Offline attack replay (`ferric secure demo`)

```
Ferric attack replay: one agent endpoint, served two ways on this machine

  naive  http://127.0.0.1:<port>  no credential, CORS open (the pattern behind the 2026 opencode advisories)
  gated  http://127.0.0.1:<port>  the same endpoint behind ferric-gate

  Attack                                                                   naive localhost            ferric-gate
  ----------------------------------------------------------------------------------------------------------------------------
  Local process, no credential (malware, npm postinstall, another user)    REACHED APP 200 +CORS      blocked 401 missing-token
  Local process with a guessed token                                       REACHED APP 200 +CORS      blocked 401 bad-token
  Web page you visit: cross-site HTML form POST (opencode 2026 class)      REACHED APP 200 +CORS      blocked 403 browser-origin
  Web page you visit: no-cors fetch with a text/plain body                 REACHED APP 200 +CORS      blocked 403 browser-origin
  Web page via DNS rebinding (attacker.example -> 127.0.0.1)               REACHED APP 200 +CORS      blocked 403 foreign-host
  DNS rebinding that also holds the real token                             REACHED APP 200 +CORS      blocked 403 foreign-host
  Web page that holds the real token (leaked into a page)                  REACHED APP 200 +CORS      blocked 403 browser-origin
  Real token, but a text/plain body (CSRF-shaped request)                  REACHED APP 200 +CORS      blocked 415 not-json
  Legitimate local client with the token (control)                         admitted (app 200) +CORS   admitted (app 200)

  naive localhost: 8/8 attacks reached the agent
  ferric-gate:     0/8 attacks reached the agent
  legitimate client through the gate: admitted
```

## Remote access over GotaTun (two network namespaces)

The setup, as root in a Linux container with `/dev/net/tun`:

- **Network.** A veth pair joins the host namespace (10.99.0.1) and a
  client namespace `fcli` (10.99.0.2).
- **Data plane.** GotaTun 0.10.2 (`cargo install gotatun-cli
  --no-default-features --features ring`).
- **Tunnel.** `ferric tunnel init --endpoint 10.99.0.1:51820`, then
  `ferric tunnel up` on the host, and
  `ip netns exec fcli ferric tunnel up --conf client-1.conf --interface ferric1`
  on the client.
- **Service.** `ferric api --mock --tunnel --port 3581`, which listens on
  10.77.0.1 only.

`$STATE` stands for the state directory used in the run.

```
$ ip -br addr show ferric0                      # host namespace
ferric0          UNKNOWN        10.77.0.1/24 
$ ip netns exec fcli ip -br addr show ferric1   # client namespace
ferric1          UNKNOWN        10.77.0.2/32 
$ ip netns exec fcli curl -s http://10.77.0.1:3581/health
{"status":"ok","service":"ferric-api","version":"0.1.0"}
$ ip netns exec fcli curl -s -X POST http://10.77.0.1:3581/v1/query -H "authorization: Bearer $(cat $STATE/gate/api.token)" -H "content-type: application/json" -d '{"prompt":"say hi"}'
{"text":"mock run complete","turns":2,"stop_reason":"task_complete"}
$ ip netns exec fcli curl -s -m 3 http://10.99.0.1:3581/health   # the underlying veth address, not the tunnel
curl exit 7
$ ip netns exec fcli ferric secure probe http://10.77.0.1:3581 --token-file $STATE/gate/api.token
Attack replay against http://10.77.0.1:3581/v1/query

  PASS Local process, no credential (malware, npm postinstall, another user)    blocked 401 missing-token
  PASS Local process with a guessed token                                       blocked 401 bad-token
  PASS Web page you visit: cross-site HTML form POST (opencode 2026 class)      blocked 403 browser-origin
  PASS Web page you visit: no-cors fetch with a text/plain body                 blocked 403 browser-origin
  PASS Web page via DNS rebinding (attacker.example -> 127.0.0.1)               blocked 403 foreign-host
  PASS DNS rebinding that also holds the real token                             blocked 403 foreign-host
  PASS Web page that holds the real token (leaked into a page)                  blocked 403 browser-origin
  PASS Real token, but a text/plain body (CSRF-shaped request)                  blocked 415 not-json
  PASS Legitimate local client with the token (control)                         admitted (app 400)

Every attack was refused before reaching the application.
probe exit 0
$ ferric tunnel status
ferric0: 1 peer(s)
  peer <client public key>  endpoint 10.99.0.2:55548  handshake 177s ago  rx <n> B  tx <n> B
```

Two problems showed up on the way and were fixed:

- **GotaTun would not start as plain root.** Its privilege drop calls
  `getlogin()`, which returns NULL without a controlling terminal. `ferric
  tunnel up` now passes `--disable-drop-privileges` only when no `SUDO_UID`
  is set, because there is no user to drop to.
- **The client had no route to the host.** The handshake completed, but
  TCP failed, because the client's /32 address gives no route to 10.77.0.1.
  `ferric tunnel up` now installs a route for each peer's AllowedIPs, as
  `wg-quick` does.
