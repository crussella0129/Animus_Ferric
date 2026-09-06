# Existing WSL qualification boundary

After the Windows qualification round, the owner's WSL note prompted inspection
of the existing Ubuntu WSL2 distribution. It already has a non-root identity,
Rust/Cargo 1.96.1, Python and the namespace tools. No distribution or toolchain
was installed, and no sudoers or root-test bypass was introduced.

At source `4eded51eaa6e0681f513ae5f8a1891de841a5a8b`, Ubuntu's Cargo workspace
format check and explicit included-human-fixture rustfmt check passed.

The canonical isolated Linux workspace/lifecycle runner still cannot run
locally: its `sudo -n` prerequisite requires interactive authentication on this
installation. The unchanged GitHub Linux namespace jobs remain the canonical
runtime evidence. Ordinary-host Linux authority and abrupt nested-engine
ownership are separate open boundaries (T-11707/T-11904); WSL's presence does
not satisfy them.

## Process-free core test run

The small core crate's tests do not launch runtime children, so they can run
under the existing non-root identity without the namespace gate. A first
`--offline` attempt stopped during dependency resolution because the local
registry lacked `rustpython-ruff_python_ast` metadata required by the locked
workspace graph. No test executed; this is not a pass.

The subsequent ordinary locked Cargo invocation retrieved missing metadata
and built only the selected core targets in ignored `target/wsl-s121`:

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/wsl-s121 CARGO_BUILD_JOBS=2 CARGO_HTTP_TIMEOUT=30 CARGO_NET_RETRY=0 cargo test -p ferric-core --locked -- --test-threads=1
```

Passed **35 tests**, zero failures/ignores: 34 core units in 0.01 s, one
`tier_table_snapshot` integration in 0.00 s, zero doc tests. Compilation took
12.63 s. Named confirmations include `output_budget_default_matrix`,
`output_budget_invalid_matrix`, `legacy_budget_metadata_is_unknown`,
`policy_for_is_unchanged_without_an_override`, and the unchanged authority/tier
and prompt-budget cases. Core source and the locked dependency graph remained
identical to `4eded51`; concurrent later human-fixture edits are not attributed
to this process-free core run. No model, engine or runtime fixture was started.

The same environment then passed `cargo clippy -p ferric-core --all-targets
--locked -- -D warnings` in 13.33 s. The pre-existing duplicate CLI target
declaration warning remains T-12028; it is not a core Clippy warning or test
failure. This small WSL result does not substitute for the full CI matrix.
