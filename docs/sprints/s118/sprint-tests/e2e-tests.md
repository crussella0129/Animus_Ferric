# Sprint 118 End-to-End Test Results

- **Tested code head:** `0145e45cb3ab8ab74ae71981d0851525eef2eb1c`
- **Command:** `cargo test -p ferric-cli --all-features --test server_lifecycle_fixture`
- **Result:** 5 passed, 0 failed, 0 ignored; 3 tests exercise the new Tailscale
  fixture contract.

## Model-free real-CLI lifecycle

`tailscale_cli_lifecycle_preserves_unrelated_state` passed. The real `ferric`
binary ran `server doctor --tailscale`, `server up --tailscale`, `server
status`, and `server down` against copied fake engine and Tailscale executables
in isolated workspace/config roots. Assertions proved:

- local/global schema-v2 ownership journals were byte-identical and matched
  the exact token, mount, loopback target, local base, canonical FQDN, status
  digest, and tokenized remote `/v1` base before apply;
- an unrelated same-host handler and unknown future state survived both launch
  and down;
- active status and operator output reported the exact remote base;
- successful scoped off required the recorded loopback target to answer HTTP
  health, corroborating proxy-before-process teardown;
- the retained process exited, its listener closed, both owned registrations
  disappeared, unrelated sentinels remained, and final fake Serve JSON was
  semantically equal to its initial state.
- a second real `ferric server down` then reported no registered server while
  the listener stayed closed, journals stayed absent, unrelated state stayed
  byte/semantically unchanged, and the complete Tailscale command ledger gained
  no entry.

`tailscale_command_log_contains_no_broad_mutation` passed. Every logged argv in
its full lifecycle was one of exact `whoami --json`, exact `serve status
--json`, exact token-path apply, or matching token-path `off`; it saw every
required shape and rejected reset, set-config, root path, unscoped off, or any
unexpected argv. The adapter's closed `Command::new` implementation and unit
API proof carry the separate no-shell claim: a child argv log cannot prove the
identity of its parent process by itself.

`tailscale_fixture_rejects_apply_without_journals` passed. A direct otherwise
valid scoped apply with both journal variables removed exited nonzero, logged
`journals_ready_on_apply = false`, and left the complete Serve state unchanged.

The two pre-existing lifecycle E2Es also remained green: the ordinary
model-free server lifecycle and legacy adoption/down flow.

## Operator smoke and protected evidence

- `cargo run -p ferric-cli -- server up --help`: passed after preserving
  `ferric` as the package default run target.
- `cargo run -p ferric-cli -- server doctor --help`: passed.
- Protected Sprint 114 acquisition artifact SHA-256 remained
  `8ECF94878E7AD745AEA28A9365AF58EE111C80B26D21A15A0F434EDB2BEB75DB`
  and the file remained unstaged.

## Explicit limits and unlocks

This is deterministic model-free evidence, not a live tailnet acceptance run.
It does not prove Tailscale certificate issuance, MagicDNS, ACL policy, daemon
compatibility, or remote reachability. Those remain required by
[INT-0008 AC-9](../../../intents/INT-0008-unified-local-model-workflow.md#acceptance-criteria)
in a separately authorized live-tailnet environment. Hostile takeover of the
unguessable exact path inside the native CLI compare/off window remains an
INT-0008 AC-6 LocalAPI `If-Match` follow-up. macOS and cross-platform parity
remain outside this sprint and AC-8; T-11707 owns the next Linux authority
work. No live-tailnet, hostile-CAS, or macOS acceptance is claimed.
