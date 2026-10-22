# Sprint 127 — Unit tests

All pass (`cargo test -p ferric-gate`, and the `secure_cmd` and `tunnel_cmd`
modules in `ferric-cli`).

| Module | Tests |
|---|---|
| `ferric-gate::token` | generated tokens are well formed and unique; `Debug` never prints the secret; `parse` rejects wrong shapes; `load_or_create` mints once then reuses; the file is 0600 in a 0700 directory, and a 0644 file is refused |
| `ferric-gate::guard` | a legitimate local client passes; DNS rebinding is refused even with the token (wrong port and missing Host too); browsers are refused even with the token (`Origin`, `Sec-Fetch-Site`, `Origin: null`); the token is required and must match (`Basic` is not a bearer token); open paths skip only the token and only for GET; preflight-free body types are refused; `allow_host` admits a tunnel authority |
| `ferric-gate::bind` | loopback is always allowed; wildcards and public addresses are refused; only the private tunnel address is allowed |
| `ferric-gate::tunnel` | the public key matches the RFC 7748 §6.1 vector; keys are clamped and distinct; the client reaches only the Ferric host; host.conf round-trips into a UAPI request; the plan is written 0600 and never overwritten; status parses peers from a UAPI `get` |
| `ferric-cli::secure_cmd` | the demo blocks every attack and admits the client; every attack reaches a naive server; the gate refuses each attack with its own check |
| `ferric-cli::tunnel_cmd` | AllowedIPs are collected from every peer |
