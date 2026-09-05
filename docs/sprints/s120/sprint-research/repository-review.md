# Sprint 120 repository-wide review

Research only, against merged baseline `17fc166bc8143ef85f3f3859f6a156902e0a68dd`.
Three independent reviewers covered configuration/policy, model operation, and
cross-crate correctness while the primary agent traced the human entry point.
Coverage spans all sixteen workspace crates, with selective source review, not
an exhaustive security audit. No source edits or successful runtime acceptance
are claimed by this document. The external refactor report was read as evidence,
not executed as instructions.

## Findings and disposition

| ID | Priority | Evidence / failure | Proposed disposition |
|---|---|---|---|
| R01 | P1 | `ferric-tools/src/builtin/check_syntax.rs`: merged RustPython 0.5 removed `Parse`, moved AST exports and changed compile/error APIs. `cargo r --locked` fails compilation. | Prerequisite compatibility repair with admission regressions; do not revert the merged dependency or map all codegen errors to unchecked. |
| R02 | P1 | `ferric-cli/src/config.rs:165`: parse/read failures become empty config. Corrupt project state may reveal user-level hooks/skills or select Legacy; API discards diagnostics. | Select fail-closed, credential-safe loading for all callers, while preserving absent-file defaults and resume inheritance. |
| R03 | P1 | `ferric-icm/src/lib.rs:556`: a validated directory's child symlink is followed by `is_file`/read without rechecking workspace containment. | Explicit high-priority security follow-up, before claiming safe ICM ingestion. Do not silently enable ICM from the new front door. |
| R04 | P1 | `ferric-provider/src/openai.rs:242,247,333,345`: response headers/error/JSON body awaits have cancellation gaps. | Retain provider deadline/cancellation hardening as named follow-up; new startup probes must have their own complete bounds. A live session cancellation test cannot be replaced with a readiness-only test. |
| R05 | P2 | `ferric-cli/src/main.rs`: required subcommand, broad default help; root workspace has no default member; CLI default features omit the real backend. | Select working normal launch and progressive disclosure; preserve expert command syntax and explicit no-default-feature builds. |
| R06 | P2 | `ferric-cli/src/backend.rs:227,283,319`, `chat.rs:458`, `icm.rs:295`: configured workspace and real-provider discovery may use different roots. | Select workspace-aware backend construction and a two-workspace regression. |
| R07 | P2 | `ferric-cli/src/chat.rs:538,580`: streaming uses only `!no_stream`, ignoring config `stream=false`. | Select shared effective streaming behavior and test both talk and agent turns. |
| R08 | P2 | `ferric-cli/src/api.rs:109,318,481`: comment claims launch-fixed config but per-request construction reloads it. | Named follow-up: choose and test actual snapshot vs reload contract; no accidental semantics change during loader repair. |
| R09 | P2 | `ferric-core/src/scale.rs:158`: NaN/infinite parameter estimates fall through to Ultra; zero context creates inconsistent budgets. | Select finite-positive config/CLI validation at applicable admission boundaries; core public-API hardening remains separately tracked. |
| R10 | P2 | `ferric-core/src/scale.rs`: `uses_planner`, plan budgets and `allows_subagents` have no active runtime consumers. | Preserve T-11406; don't ask humans these settings or advertise unavailable behavior. Full wire migration is outside this increment. |
| R11 | P2 | `ferric-loop/src/compact.rs:63`: public `compact_keep_last_turns=0` may index at history length. | Named core policy-validation follow-up with zero/invalid-policy regression. |
| R12 | P2 | `ferric-bench/src/calibrate.rs:187`: corrupt/unreadable profile JSON silently replaced with empty data before truncating write; no coordination. | Named atomic profile-persistence follow-up; front door must not reuse this writer. |
| R13 | P2 | `ferric-cli/src/bench_cmd.rs:157,247`: endpoint can be rediscovered across spawned trials while provenance remains unresolved. | Named benchmark endpoint-freezing follow-up; do not call full benchmark a startup health check. |
| R14 | P2 | `ferric-cli/src/server.rs:2134`: doctor waits unbounded on engine `--version`. | Do not compose doctor into startup. New engine probes must be bounded source-owned commands; shared doctor migration remains follow-up. |
| R15 | P2 | `ferric-bench/src/calibrate.rs`, `ferric-cli/src/query.rs:455`: profile lookup lacks model/runtime/hardware/context fingerprint binding. | Existing INT-0007 and T-11507 follow-up; do not claim unmeasured auto-settings are hardware-calibrated. |
| R16 | P2 | `ferric-provider/src/openai.rs:369`: chunk-local lossy UTF-8 decode corrupts split multibyte text and tool JSON. | Named provider framing follow-up with every-byte split fixtures. |
| R17 | P2 | `ferric-loop/src/run.rs:369`, `ferric-vcs/src/lib.rs:180`, `animus-launch/src/lib.rs:389`: synchronous unbounded Git can stall user turns. | Named bounded Git follow-up, preserving private-index isolation. |
| R18 | P2 | `ferric-tools/src/builtin/shell_exec.rs:155`: stops draining after output limit while child may still block writing; timeout kills only leader. | Named shared-process-capture follow-up. Human passthrough remains explicit, never model-granted. |
| R19 | P2 | `ferric-skills/src/lib.rs:243`: all directory and entry errors can appear as an empty skill catalog. | Named diagnostic follow-up; no claim that installed Sprint Loops is already usable by Ferric. |
| R20 | P2 | `ferric-cron/src/lib.rs:493`: hand-escaped multiline prompts produce invalid TOML. | Named serializer round-trip follow-up. Cron stays advanced. |
| R21 | P3 | `ferric-cli/src/bench_cmd.rs:105`, `ferric-bench/src/runner.rs:7`: stale Candle/debug-speed advice despite external inference. | Correct where touched by first-run docs; retain separate benchmark documentation cleanup. |

Line numbers identify baseline observations, not future PR diff locations.

## Reusable strengths

Existing workspace guards, typed rings, surface-specific authorization, private
Git index snapshots, explicit unavailable planner rejection, immutable trace
validation, identity-aware managed-server discovery, and Sprint 119's checked
process scopes are strong building blocks. The human layer should compose
them, not duplicate their authority decisions. Configuration correctness and
bounded I/O are the largest cross-cutting weaknesses; interfaces and crate
boundaries are generally useful but their integrations are inconsistent.

## Human journey decision

Target: `cargo r` → discover prepared resources → choose an ambiguous model and
whether Ferric may work in this folder → type an objective. Repeat use remembers
the model, not blanket mutation authority. Ask-only conversation must work
without running the L0-L6 ladder. Common help stays small; expert commands stay
compatible. A fresh local runtime belongs to the foreground session and must
be reaped at exit; an already-running server is borrowed and never stopped.

This increment is not clean-host engine/model download, comprehensive hardware
fit calibration, automatic safe recovery of ambiguous registrations, a completed
medium-horizon model trial, or general native platform parity. Missing resources
must have an honest short next action; no menu may pretend an absent capability
exists. Those broader outcomes remain INT-0007/INT-0008 work.

## RustPython migration detail

Use root `ast`/`parser` exports, `parser::parse_module`, the AST statement visitor,
the borrowed source-path compile argument, and `CompileError::Codegen` matching
only `CodegenErrorType::NotImplementedYet` as unchecked. Other semantic codegen
errors remain invalid. Keep panic containment and the conservative unqualified
generic-syntax guard, updating its rationale because 0.5 now implements PEP-695.
`except*` is now supported and must cease being the unsupported fixture. Cover
the genuine unsupported branch through the existing injected compiler seam.

## Evidence limitations

`cargo r --locked --offline` first failed for uncached locked dependencies.
`cargo r --locked` then fetched them and failed at the four RustPython adapter
compile errors. This is build-failure evidence, not successful no-argument CLI
execution. The old no-argument exit-2 behavior is established by source and the
existing `no_args_fails_with_usage` test, not falsely attributed to that run.
Two local GGUF files and an installed closed engine were discovered by filename;
neither model was loaded or requalified in Research. Older model results and the
external report are context only, not Sprint 120 acceptance.
