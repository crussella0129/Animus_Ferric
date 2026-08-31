Finalized - DO NOT EDIT

# Sprint 118 Build Plan

## Intents

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — state: active; acceptance criteria covered: AC-3 explicit partial/stale state, AC-4 truthful status, AC-6 exact ownership and atomic evidence, AC-7 scoped/idempotent cleanup, and enabling AC-9 lifecycle E2E evidence. AC-8 platform parity is expressly not claimed.

## Schema Tree

- Sprint goal: restore ownership-safe positive Tailscale Serve lifecycle
  - Typed external-state boundary
    - T-11801: fixed-command adapter, exact status projection, ownership schema
  - Launch lifecycle
    - T-11802: write-ahead publication, apply/verify, failure compensation
  - Read and teardown lifecycle
    - T-11803: truthful status and proxy-first exact down
  - Operator surface
    - T-11804: read-only doctor and concise endpoint/recovery guidance
  - Cross-boundary proof
    - T-11805: stateful fake CLI and model-free lifecycle matrices

## Execution Sequence

### T-11801: Add a closed Tailscale Serve adapter and additive ownership record

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `Cargo.toml`, `Cargo.lock`, `crates/ferric-cli/Cargo.toml`, `crates/ferric-cli/src/tailscale_serve.rs` (new), `crates/ferric-cli/src/main.rs`, `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/server_resolution.rs`
- **Depends on:** (none)
- **Acceptance criterion:** AC-3, AC-4, AC-6, AC-7
- **Success criterion (EARS):**
  - **T-11801-E01 — WHEN** documented `tailscale serve status --json` contains zero or one handler at a validated Ferric token path, **THEN** the adapter **SHALL** return `Absent` or the exact host/path/handler projection while ignoring but retaining the identity of unrelated handlers.
  - **T-11801-E02 — WHEN** status execution fails, exceeds its bound, exceeds bounded output, is malformed, contains an ambiguous duplicate token path, or contains a non-proxy handler at the owned coordinate, **THEN** the adapter **SHALL** return a non-authorizing error without invoking any mutating command.
  - **T-11801-E03 — WHEN** Ferric applies or removes an owned endpoint, **THEN** the adapter **SHALL** invoke only fixed `serve --bg --https=443 --set-path=<token-path> --yes <loopback-target>` or matching endpoint-scoped `off` argv and **SHALL NOT** expose or invoke `reset`, `set-config`, or a shell.
  - **T-11801-E04 — WHEN** ownership is prepared for a nonzero loopback port, **THEN** Ferric **SHALL** obtain exactly 128 bits from `getrandom::fill`, produce a validated 32-hex token, `/_ferric/<token>` mount, exact `http://127.0.0.1:<port>` target, canonical self FQDN, canonical whole-status provenance digest, and tokenized remote `/v1` base; entropy or identity failure **SHALL** precede engine spawn and every Serve mutation.
  - **T-11801-E05 — WHEN** a schema-v2 runfile has `tailscale: true`, **THEN** Ferric **SHALL** authorize positive lifecycle behavior only if its additive ownership object is structurally valid; boolean-only historical records **SHALL** remain fail-closed and byte-compatible.
- **Notes:** The immutable registration is the write-ahead journal. The adapter owns a unique path handler, not HTTPS 443, the node certificate, the whole `Web` map, or unrelated Serve state. The whole-status digest is provenance only; authority compares the exact token path, proxy target, and compatible web-port mode. Unrelated handler drift is tolerated and preserved.

### T-11802: Journal, apply, verify, and compensate the positive launch

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/server_registration.rs`
- **Depends on:** T-11801
- **Acceptance criterion:** AC-3, AC-6, AC-7
- **Success criterion (EARS):**
  - **T-11802-E01 — WHEN** `server up --tailscale` passes ordinary preflight and the token path is absent on a compatible web port, **THEN** Ferric **SHALL** health-check and retain the exact engine generation, publish byte-identical ownership-bearing local/global registrations, re-observe that exact path as absent while tolerating unrelated handler drift, apply the exact path, verify its exact target and remote host, revalidate process/listener/registration authority, and only then report ready.
  - **T-11802-E02 — WHEN** capture, collision checking, engine readiness, exact process/listener inspection, or mirrored journal publication fails before Serve mutation, **THEN** Ferric **SHALL** perform no Serve mutation and **SHALL** preserve the existing exact-child publication-compensation contract.
  - **T-11802-E03 — WHEN** apply returns failure or ambiguity, post-apply observation fails, the observed handler differs, or final process/listener/registration authority fails after journal publication, **THEN** Ferric **SHALL** immediately compare and remove only the exact unchanged token path (or prove it absent), independently stop/reap only the exact child, and compare-remove registrations only if both resources are resolved.
  - **T-11802-E04 — WHEN** post-journal proxy comparison, endpoint-scoped `off`, or absence verification cannot authorize external cleanup, **THEN** Ferric **SHALL NOT** mutate the ambiguous path, **SHALL** still stop/reap the independently proven exact child, and **SHALL** retain every ownership-bearing registration with an exact recovery diagnostic.
- **Notes:** Reuse the accepted Sprint 117 process/listener and mirrored-publication seams. A crash after journal publication but before mutation is represented by an absent owned path and is safely resumable by `down`. Native CLI cleanup is an immediate compare then token-path `off`: the CLI's internal config CAS plus an unguessable 128-bit coordinate covers ordinary non-hostile concurrency, but cannot atomically bind Ferric's earlier target comparison. Hostile takeover of that exact token during the narrow window remains owned by INT-0008 AC-6 and requires a future LocalAPI `If-Match` adapter; this sprint does not claim that stronger guarantee.

### T-11803: Make status truthful and down proxy-first, scoped, and retryable

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`, `crates/ferric-cli/src/server_resolution.rs`, `crates/ferric-cli/src/tailscale_serve.rs`
- **Depends on:** T-11801, T-11802
- **Acceptance criterion:** AC-3, AC-4, AC-6, AC-7
- **Success criterion (EARS):**
  - **T-11803-E01 — WHEN** status resolves a valid typed Tailscale registration, **THEN** it **SHALL** report the proxy as active, pending/absent, replaced, or uninspectable with the tokenized remote base and one safe next action, and **SHALL** succeed only for an exact active handler plus the existing Ready native process state.
  - **T-11803-E02 — WHEN** down resolves unchanged typed registrations whose exact token path is active or absent, **THEN** it **SHALL** revalidate registration revisions, immediately compare/remove and verify only an exact active path (or accept absence), independently reuse exact process teardown, revalidate revisions, and conditionally remove registrations only after both resources resolve.
  - **T-11803-E03 — WHEN** the token path is replaced, duplicated, malformed, unreadable, an endpoint-scoped `off` fails, or post-`off` absence cannot be proved, **THEN** down **SHALL NOT** mutate the ambiguous external path, **SHALL** still stop/reap an independently authorized exact process, and **SHALL** preserve every registration with actionable coordinate-specific output.
  - **T-11803-E04 — WHEN** down is repeated after the exact proxy is already absent or after the process has already exited, **THEN** it **SHALL** converge idempotently through the existing absent/stale cleanup rules without a node-wide mutation.
  - **T-11803-E05 — WHEN** a legacy `tailscale: true` record lacks valid ownership, **THEN** status and down **SHALL** retain the current pre-process, non-mutating manual-recovery block and **SHALL NOT** invoke Tailscale.
- **Notes:** Backend auto-discovery may continue selecting the healthy local loopback endpoint; status and destructive lifecycle authority additionally require the external proxy observation.

### T-11804: Restore read-only doctor checks and document the compact operator path

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/server.rs`, `docs/server-configuration.md`, `docs/commands.md`, `README.md`
- **Depends on:** T-11801, T-11803
- **Acceptance criterion:** AC-4, AC-7, enabling AC-9
- **Success criterion (EARS):**
  - **T-11804-E01 — WHEN** `server doctor --tailscale` receives valid static arguments and an unblocked registration inventory, **THEN** it **SHALL** run the existing engine/model probes plus bounded read-only `whoami --json` and `serve status --json` probes and report success or a precise Tailscale blocker without mutation.
  - **T-11804-E02 — WHEN** static arguments or registration state already block doctor, **THEN** doctor **SHALL** return before engine, model, network, or Tailscale probes, preserving the accepted probe-order contract.
  - **T-11804-E03 — WHEN** launch, status, or down reports Tailscale state, **THEN** operator output and docs **SHALL** present the local base, tokenized remote `/v1` base when known, exact scoped cleanup behavior, retained-evidence recovery path, model-free limitations, and an explicit prohibition on blind reset.
- **Notes:** README receives only concise current behavior; sprint history remains in the Book.

### T-11805: Prove the lifecycle with a stateful fake Tailscale executable

- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- **Touches:** `crates/ferric-cli/src/bin/ferric_lifecycle_fixture.rs`, `crates/ferric-cli/tests/server_lifecycle_fixture.rs`, `docs/sprints/s118/sprint-tests/*.md`
- **Depends on:** T-11801, T-11802, T-11803, T-11804
- **Acceptance criterion:** AC-6, AC-7, enabling AC-9
- **Success criterion (EARS):**
  - **T-11805-E01 — WHEN** the real Ferric CLI runs `doctor`, `up`, `status`, and `down` against isolated fake engine and Tailscale executables, **THEN** the fixture **SHALL** prove write-ahead ordering, exact remote base, exact argv, proxy-before-process cleanup, registration removal, and preservation of unrelated Serve handlers.
  - **T-11805-E02 — WHEN** deterministic unit/composition fault seams inject capture, publication, apply, verification, replacement, `off`, post-`off`, child, listener, or registration-revision failures, **THEN** named tests **SHALL** prove the EARS-specific mutation, signal, evidence-retention, output, and retry results without a model or live tailnet.
  - **T-11805-E03 — WHEN** the command log for every Tailscale test is inspected, **THEN** it **SHALL** contain only exact read-only `whoami --json`/`serve status --json`, exact token-path apply, and exact token-path `off` invocations and **SHALL NOT** contain `reset`, `set-config`, or an unscoped `off`.
- **Notes:** The test fixture is feature-gated and excluded from normal/release builds. Live tailnet, certificate, ACL, and macOS acceptance remain outside this sprint.
