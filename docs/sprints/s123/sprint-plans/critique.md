# Plan Critique — Sprint 123

## Concerns

### C-001: The binary-identity check shifts from compile-time `env!` to a runtime set-once value
- **Where:** `build-plan.md` T-12301 / `bin_identity.rs` + `tailscale_localapi.rs:219`
- **Quote:** "replaces the library-invalid `env!("CARGO_BIN_NAME")` with a process-set-once identity (`run` calls `set_binary_name` first)"
- **Failure mode:** hidden-dep
- **Why it matters:** the change is *required* — `env!("CARGO_BIN_NAME")` does not compile in a library — but it moves the gate from a compile-time constant to a value read after `run` sets it. A code path that read the identity *before* `run` would silently get the default (`false`).
- **Suggested response:** fix-in-plan / accept. `run` calls `set_binary_name` as its first statement, so no production path reaches the gate before it is set; the only pre-set readers are library unit tests, which never had a `CARGO_BIN_NAME` anyway and correctly get `false`. The existing `tests/server_lifecycle_fixture.rs` — which spawns the real `ferric-lifecycle-test` binary and exercises the fixture transport — is the load-bearing proof the real paths are unchanged, and the pure `name_is_lifecycle_fixture` test pins the predicate. Not a `block`.

### C-002: One atomic task is coarse, but justified
- **Where:** `build-plan.md` T-12301
- **Quote:** "atomic — the crate must compile as a unit, so this is one task"
- **Failure mode:** granularity (screened, accepted)
- **Why it matters:** a single task spanning a new lib, two shims, an identity module, a gate change, and Cargo edits is large.
- **Suggested response:** accept. The library boundary genuinely cannot compile in pieces (the shims need the lib; the lib needs the identity seam to compile at all). Splitting would produce non-compiling intermediate commits, which is worse. The change is behavior-preserving and coherent (one boundary), and the whole workspace suite gates it.

## Confidence
proceed-with-caveats

Every INT-0009 AC-1 EARS clause maps to a named test; the highest-risk element (the CI lifecycle fixture) is covered by an existing integration test that must pass unchanged. No acceptance criterion is weakened; INT-0009 AC-2/AC-4 and the server/serving-crate splits remain explicitly deferred to later increments.
