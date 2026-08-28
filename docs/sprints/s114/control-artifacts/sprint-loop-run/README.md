# Sprint 114 Animus Sprint Loops probe

This bundle tests the pinned upstream runtime-neutral `open-harnesses` adapter
without modifying it. The source checkout is pinned by Git commit and tree;
the installer output and every installed helper are hashed before Ferric
discovery is evaluated.

The probe stops behavioral arms on packaging failure, as required by T-11411.
That keeps “helpers can be installed by an operator” distinct from “Ferric can
discover and authorize a skill.” Model-independent registry, helper, router,
local-Git, and remote-authority facts are still recorded. No remote mutation is
performed.

The live result and layer-by-layer interpretation are in
[`capability-report.md`](capability-report.md). The structured source of truth
is `evidence/capability-verdict.json`, whose hash is retained in
`evidence/files.sha256`.

Run `verify-probe.ps1` to recheck the retained bundle. Run
`verify-probe.tests.ps1` to exercise seven disposable adversarial mutations;
the test harness writes only below `target/` and removes its fixtures.
Verification intentionally binds the retained command arguments to this exact
checkout location and rehashes the ignored release Ferric executable. A moved
or fresh clone must first restore the same checkout path and byte-identical
binary; the retained JSON and hash manifest remain inspectable without them.

Run `capture-probe.ps1` only against the isolated source and workspace under
`target/s114-experiment/`. The evidence directory is fail-once and is never
overwritten. Capture writes to a uniquely named staging directory and publishes
`evidence/` only after every check succeeds; a failed capture removes its
staging directory so a clean retry remains possible.

## Reproduction preconditions

The capture deliberately does not clone, initialize Git, or choose an upstream
revision. Before running it:

1. Clone the upstream repository named by the retained structured evidence to
   `target/s114-experiment/sprint-loop-source/`, check out the pinned commit
   named in `capture-probe.ps1`, and leave the checkout clean.
2. Create `target/s114-experiment/sprint-loop-workspace/` as a standalone local
   Git repository on `dev`. Use documentation-only local identity values such
   as `Documentation Operator` and `operator@example.invalid`.
3. Run the pinned `open-harnesses/install.sh` into that workspace once, add only
   the resulting `scripts/` tree, and create a local baseline commit. Do not add
   a remote.
4. Build the release Ferric executable and confirm its SHA-256 matches the pin
   in `capture-probe.ps1`.

The capture then proves the source manifest against the pinned Git tree,
proves the full non-`.git` workspace against its local Git tree both before and
after an idempotent reinstall, and records both identities. A reconstructed
workspace commit may differ because Git metadata differs; its file tree must
remain equivalent.
