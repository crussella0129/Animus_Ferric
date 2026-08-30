# Sprint 116 Integration Test Results

**Status:** passed on the final tree.

The full workspace all-features test gate passed. Specifically observed
integration suites included:

| Suite | Result |
| --- | --- |
| Ferric CLI integration tests | 69/69 passed |
| Server lifecycle fixture | 3/3 passed |
| Benchmark mock integration tests | 6/6 passed |
| Template-hygiene integration tests | 3/3 passed |

The lifecycle fixture is feature-gated and model-free. Its three tests exercise
the real CLI and isolated temporary state:

- `model_free_server_lifecycle_fixture_e2e` covers ordinary
  `server up/status/down`, complete registration publication, process/listener
  ownership, and cleanup without a GGUF.
- `tailscale_refusal_has_zero_external_effects` proves the blocked Tailscale
  mode refuses before child, proxy, or registration mutation.
- `legacy_adoption_then_down_cli_e2e` proves a live schema-v1 record cannot
  authorize teardown until explicit non-destructive adoption creates exact
  schema-v2 identity.

The test-only helper serialization described in the unit results is part of
the final harness used by this successful workspace gate. No user model,
operator registration, fixed port, or durable proxy state was used.
