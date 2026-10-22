# Sprint 128 — Test report

- **Format:** `cargo fmt --all --check` and the included-fixture `rustfmt`
  check are clean.
- **Lint:** clean for:
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo clippy -p ferric-cli --no-default-features --all-targets --locked -- -D warnings`
  - `cargo clippy -p ferric-cli -p ferric-setup --all-targets --target x86_64-pc-windows-gnu -- -D warnings`
- **New tests (all pass):**
  - `ferric-setup`: 11 unit and 6 mirror integration tests.
  - `setup_cmd`: 3 unit tests.
  - Startup: 1 models-folder fallback test.
- **Guards (all pass, in both the backend and no-backend lanes):**
  `human_cli`, `human_docs`, `budget_docs`, `charter_docs`,
  `getting_started_doc` and `template_hygiene`.
- **Front-door guards updated deliberately.** `human_docs` and `budget_docs`
  pinned the first README and command-reference example to `cargo r`, and
  the primary actions to run/status/explain/advanced. By the owner's
  direction (INT-0016), they now pin the one-line installer as that single
  command, and add `setup` as a primary action with its documented flags
  (`--list`, `--model`, `--models-dir`, `--yes`). Both are feature-aware, so
  the no-backend lane still expects the old four.
- **Full workspace:** `cargo test --workspace --locked --no-fail-fast`. Every
  test binary passes except the `ferric-cli` lib, which has the same 19
  process-lifecycle failures as the untouched pre-sprint tree in this
  sandbox: `startup::*`, `human::enabled::*`, `live_budget_*` and
  `linux_pidfd_process_handle_smoke`, all "selected port is not exclusively
  owned". GitHub CI runs those lifecycle tests green on Ubuntu and Windows
  (sprint 127's PR #123). T-12810 records the sandbox investigation.
- **End to end:** see [e2e-tests.md](e2e-tests.md).
  - An empty home went through install, a non-interactive refusal, and
    `ferric setup --yes`: the engine was verified through a release-API
    mirror and the real 1.1 GB model from Hugging Face was verified.
  - The installed engine and model answered a real prompt through
    `ferric run`.
  - The interactive first-run offer under a pseudo-terminal (Enter accepts)
    installed the engine and continued into the session.
  - The installer refused a tampered checksum.
- **Not run here:**
  - `install.ps1`: no PowerShell in the sandbox. A new CI job parses it under
    Windows PowerShell 5.1 and PowerShell 7, and `sh -n install.sh` runs on
    Linux and macOS.
  - The release workflow: it can only be dispatched from `main`, and the
    first tag is T-12805.
  - The managed engine launch in this sandbox: T-12810.
- **Verdict:** proceed with caveats.
