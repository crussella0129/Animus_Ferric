# Sprint 120 Research Report

## Intents Reviewed

- [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) — revised: human-first zero-argument launch, bounded decision cost, explicit authority, readiness separate from capability qualification.
- [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) — revised: present invalid configuration must not silently change authority; selected workspace and effective settings must agree.
- [INT-0005](../../../intents/INT-0005-safe-multilanguage-syntax-admission.md) — selected for existing Python parser compatibility maintenance only; no new-language acceptance claim.
- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) — reviewed dependency, not advanced by a readiness-only front door.

## 1. Sprint Goal

Review the repository and implement a prepared-host first-run experience whose
normal command is `cargo r` (installed: `ferric`). Automate technical setup and
owned cleanup, expose only meaningful model/folder-authority choices, preserve
the expert interfaces, and retain the wider review findings as ordered work.
First repair the owner-merged dependency update that currently prevents launch.
This is one bounded sprint and one owner-approved dev-to-main PR, not completion
of every local-model or repository-wide finding.

## 2. Existing Code Survey

The following 51 unique source/control files were inspected by the primary
agent and three bounded read-only reviewers. Some were targeted sections rather
than complete-file audits; filename discovery is not counted as code review.

| File | Relevance |
|---|---|
| `Cargo.toml` | workspace/default launch |
| `crates/ferric-cli/Cargo.toml` | feature/default binary |
| `crates/ferric-cli/src/main.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/backend.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/config.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/chat.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/server.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/server_process.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/autonomy_cmd.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/bench_cmd.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/query.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/mcp.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/api.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/src/icm.rs` | CLI/config/model/lifecycle review |
| `crates/ferric-cli/tests/cli.rs` | old no-argument contract |
| `crates/ferric-bench/src/calibrate.rs` | qualification and process provenance |
| `crates/ferric-bench/src/runner.rs` | qualification and process provenance |
| `crates/ferric-bench/src/process.rs` | qualification and process provenance |
| `crates/ferric-bench/src/verify.rs` | qualification and process provenance |
| `crates/ferric-bench/src/summary.rs` | qualification and process provenance |
| `crates/ferric-provider/src/lib.rs` | provider I/O |
| `crates/ferric-provider/src/openai.rs` | provider I/O |
| `crates/ferric-provider/src/traits.rs` | provider I/O |
| `crates/ferric-core/src/scale.rs` | policy consumers |
| `crates/ferric-core/src/harness.rs` | policy consumers |
| `crates/ferric-loop/src/lib.rs` | turn execution and compaction |
| `crates/ferric-loop/src/run.rs` | turn execution and compaction |
| `crates/ferric-loop/src/compact.rs` | turn execution and compaction |
| `crates/ferric-guard/src/lib.rs` | authority and paths |
| `crates/ferric-guard/src/checker.rs` | authority and paths |
| `crates/ferric-guard/src/workspace.rs` | authority and paths |
| `crates/ferric-icm/src/lib.rs` | delegation filesystem boundary |
| `crates/ferric-prompt/src/lib.rs` | prompt composition |
| `crates/ferric-tools/src/lib.rs` | tool authority and source admission |
| `crates/ferric-tools/src/registry.rs` | tool authority and source admission |
| `crates/ferric-tools/src/builtin/shell_exec.rs` | tool authority and source admission |
| `crates/ferric-tools/src/builtin/check_syntax.rs` | tool authority and source admission |
| `crates/ferric-tools/tests/controlled_mutations.rs` | affected Python fixture |
| `crates/ferric-trace/src/lib.rs` | trace publication |
| `crates/ferric-trace/src/sink.rs` | trace publication |
| `crates/ferric-vcs/src/lib.rs` | Git snapshots |
| `crates/ferric-skills/src/lib.rs` | skill discovery |
| `crates/ferric-research/src/lib.rs` | research I/O |
| `crates/ferric-research/src/web.rs` | research I/O |
| `crates/animus-launch/src/lib.rs` | project bootstrap |
| `crates/ferric-cron/src/lib.rs` | schedule persistence |
| `crates/ferric-process/src/lib.rs` | owned foreground scope |
| `tools/install.ps1` | installation |
| `tools/install.sh` | installation |
| `docs/sprints/s114/control-artifacts/model/acquire-model.ps1` | historical acquisition evidence only |
| `docs/sprints/s114/control-artifacts/model/verify-model.ps1` | historical validation evidence only |

## 3. External Sources

- [CLI Guidelines](https://clig.dev/) — primary community guidance on human-first defaults, concise help, terminal detection, progress and explicit interaction.
- [Cargo run](https://doc.rust-lang.org/cargo/commands/cargo-run.html) — primary default package/binary behavior.
- [Clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) — optional subcommands and derived parser structure.
- [llama.cpp server README](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) — model metadata and engine options; current upstream options are not assumed available in the installed engine.
- [Rust File locking](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock) — OS-held file-lock API for crash-released coordination.

Accessed 2026-09-05 UTC. Cached RustPython 0.5 compiler/parser/AST sources were
also inspected for the exact dependency migration; these are local primary
dependency sources, not an unverified recollection of a web API.

## 4. Risks, Unknowns, Dependencies

- [Repository review](repository-review.md) records R01–R21, baseline references,
  strengths, disposition, model-evidence limits, and Python migration details.
- The baseline `17fc166bc8143ef85f3f3859f6a156902e0a68dd` includes merged PRs
  106 and 107. RustPython 0.5 breaks compilation before Ferric can start.
- Auto-start must never create an unowned daemon, stop a borrowed server,
  bypass ambiguous managed ownership, or treat a listening port as exact
  process identity. Use source-owned scopes and explicit cleanup proof.
- Human defaults cannot silently increase tool authority, reveal lower-layer
  hooks after a parse error, send a credential to a different discovered host,
  reuse a stale capability profile, or infer context capacity from trained context.
- There is no product-grade clean-host downloader/hardware calibration API to
  simply call. Historical scripts are evidence, not a cross-platform product.
  Provisioning, full fit qualification, native parity and medium-horizon success
  remain explicit later work. Model-backed success must be recorded separately
  from deterministic fixture success.
- No new engine is downloaded in this sprint proposal. Available local model
  filenames and installed engine presence are not readiness or acceleration proof.
- The unrelated Sprint 114 acquisition evidence edit is preserved separately
  with its exact preexisting hash and must be restored before handoff.
- The installed skill is 0.22.0 (requested 0.21.0 absent); its Claude-specific
  Plan Mode tools are unavailable here. Source remains frozen during proposal
  review, and explicit approval is required before the plan is locked.

## 5. Recommended Approach

Repair the pinned Python compiler adapter with admission tests. Introduce typed,
credential-safe configuration failure and common effective-setting validation;
fix workspace-bound provider selection and configured streaming. Compose an
owned foreground startup session from existing closed engine command generation,
managed-discovery authority and the shared process scope. Keep a small separate,
versioned preference record so remembering a model never rewrites expert config
or persists blanket write permission. Add the default human entry point, compact
help and explicit advanced access. Use short bounded readiness/model probes,
not the L0-L6 ladder as a conversation admission check.

A prepared interactive session should ask at most three meaningful decisions.
An empty non-TTY invocation must return a useful welcome with success and no
side effects. Missing resources and uncertain ownership must have short honest
next actions. Borrow existing servers; reap only newly owned engines on every
return/cancellation path. Preserve script semantics for original commands.
Validate the entire journey with source-defined fixtures and an attributable,
bounded real-model attempt when the host supports it. An unsuccessful live
attempt is retained as failure, not repaired by manually killing leftovers.

Broader R03/R04 security/responsiveness repairs and remaining R08–R21 debt are
explicit prioritized follow-ups. None is represented as fixed merely because
the human front door is simpler.

## Artifacts

- [Repository-wide findings and disposition](repository-review.md).
- [Sprint metadata](../sprint-meta.md): merged baseline, protected user edit and phase boundary.
- Stable intent revisions above own the semantic changes; this report is provenance.

## Budget Override

The owner explicitly requested a repository-wide review/refactor, and a normal
launch crosses CLI, policy, provider, tools, persistence and process ownership.
A 20-file cap would omit material authority and lifetime boundaries. The
51-file cross-crate survey is therefore intentional, distributed across
three independent bounded reviewers plus the primary entrypoint trace; it is
not an exhaustive line-by-line audit. Five external sources were used.
Research intake began around 02:14 UTC and substantive investigation stopped
around 02:40 UTC on 2026-09-05; report recording and later Plan work are separate.
No further open-ended research or model benchmarking is included in this budget.
