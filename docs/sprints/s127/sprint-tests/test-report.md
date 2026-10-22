# Sprint 127 — Test report

- **Format:** `cargo fmt --all --check` and the included-fixture `rustfmt`
  check are clean.
- **Lint:** `cargo clippy --workspace --all-targets --locked -- -D warnings`
  and `cargo clippy -p ferric-cli --no-default-features --all-targets
  --locked -- -D warnings` are clean.
- **Tests:** `cargo test --workspace --locked --no-fail-fast`:
  - 95 test binaries pass, including every new test: 21 unit and 4 layer
    tests in `ferric-gate`, 3 valve gate tests, 3 `secure_cmd` tests, and 1
    `tunnel_cmd` test. The `charter_docs` and `template_hygiene` guards pass
    after the doc changes.
  - One binary, the `ferric-cli` lib, fails 19 process-lifecycle tests in
    this container. They are the `startup::*`, `human::enabled::*`,
    `live_budget_*` and `linux_pidfd_process_handle_smoke` tests, failing
    with "selected port is not exclusively owned by the new engine" and a
    pidfd smoke-helper failure.
  - **These failures predate this sprint.** The same tests fail on an
    untouched export of the pre-sprint tree (merge `1821132`), built in a
    separate target directory in the same container
    (`startup_owned_runtime_reaches_ready` and
    `linux_pidfd_process_handle_smoke`: both FAILED). CI on GitHub runners
    is the authority for them.
- **End to end:** see [e2e-tests.md](e2e-tests.md). The offline demo shows
  naive 8/8 against gate 0/8. Over the GotaTun tunnel, the probe passes 9/9
  from the client namespace, and the underlying network address is
  unreachable.
- **CI fix (Windows):** the first CI run failed Windows clippy on an
  unused-variable warning in a `ferric-gate` tunnel test (the variable was
  used only inside a `cfg(unix)` block). The test now uses it on every
  platform. `cargo clippy -p ferric-cli -p ferric-valve -p ferric-gate
  --all-targets --target x86_64-pc-windows-gnu -- -D warnings` is clean
  locally.
- **Verdict:** proceed. Every new check is independently tested, and the
  remote path is proven on real WireGuard.
