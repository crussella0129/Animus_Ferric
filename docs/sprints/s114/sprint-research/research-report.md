# Sprint 114 Research Report

## Intents Reviewed

- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) — created and planned; relevance: stable authority for the hardware-calibrated model selection, Ferric-built app trial, README cleanup, evidence boundary, and layered Sprint Loops verdict; current state: `planned`.
- [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) — selected for terminal context, not reopened; relevance: Sprint 113's 0/3 long-horizon result and rejected planner constrain this trial; current state: `abandoned`.
- [INT-0002](../../../intents/INT-0002-operator-authorized-default-verification.md) — selected as a dependency boundary; relevance: the trial must pass an explicit operator-authored checks file because default verification is not implemented; current state: `proposed`.
- [INT-0003](../../../intents/INT-0003-requirement-evidenced-completion.md) — selected as a diagnostic lens, not implementation scope; relevance: the grader will score contract obligations independently because Ferric has no requirement ledger; current state: `proposed`.
- [INT-0004](../../../intents/INT-0004-auditable-session-provenance.md) — selected as an evidence dependency; relevance: complete prompt, trace, model, binary, workspace, check, and teardown provenance is required even though ordinary traces lack an integrity chain; current state: `proposed`.
- [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) — selected as a reporting boundary; relevance: `--skill` and dormant orchestration vocabulary must not be represented as capabilities that Ferric does not execute; current state: `proposed`.

## 1. Sprint Goal

Use current primary sources and measured host capacity to select and acquire the
best practical local GGUF for long-horizon coding, then use Animus Ferric—not
Codex—to build a bounded multi-file Rust application while preserving the full
causal record. In the same sprint, install and probe a pinned Animus Sprint
Loops open-harness adapter in isolation and report exactly which Book-v2 layers
Ferric can use today. No Ferric product change is assumed or required for a
truthful evaluation result.

## 2. Existing Code Survey

| File | Relevance | Notes |
| --- | --- | --- |
| `docs/intents/INT-0001-evidence-bound-autonomous-recovery.md` | high | Terminal 0/3 Evidence result; forbids reopening the same intervention or claiming the planner exists. |
| `docs/intents/INT-0002-operator-authorized-default-verification.md` | high | Confirms persistent default checks are desired but not implemented. |
| `docs/intents/INT-0003-requirement-evidenced-completion.md` | medium | Motivates independent contract grading rather than trusting `task_complete`. |
| `docs/intents/INT-0004-auditable-session-provenance.md` | high | Defines the provenance and integrity gaps this experiment must compensate for manually. |
| `docs/intents/INT-0006-truthful-policy-contract.md` | high | Prevents skill injection or dormant fields from being labeled orchestration. |
| `docs/sprints/s113/sprint-research/research-report.md` | high | Freezes the old 7B model, control conditions, failure mechanisms, and wider-field gaps. |
| `docs/sprints/s113/sprint-tests/development-screen.md` | high | Records all three Evidence screens at 0/3 and the exhausted revision budget. |
| `docs/sprints/s113/sprint-tests/test-report.md` | high | Separates structurally passing implementation from falsified model performance. |
| `docs/basics-skills.md` | high | Documents operator-only skill authorization and no model self-authorization path. |
| `crates/ferric-bench/autonomy/v1.toml` | medium | Shows the existing frozen task-corpus pattern; Sprint 114 stays separate. |
| `crates/ferric-bench/src/autonomy.rs` | medium | Informs trace/result capture and process-cold experiment discipline. |
| `crates/ferric-skills/src/lib.rs` | high | Discovers only top-level `SKILL.md` bodies with strict frontmatter/name matching. |
| `crates/ferric-cli/src/query.rs` | high | Owns `--skill`, checks-file, policy, protocol, context, tier, and ring wiring. |
| `crates/ferric-tools/src/builtin/shell_exec.rs` | high | Defines an unsandboxed host-shell tool, requiring the registration audit that shows it is human-only rather than model-facing. |
| `crates/ferric-tools/src/builtin/mod.rs` | high | Proves `shell_exec`/`manage_task` are human-only registrations and absent from the `ferric query` model registry. |
| `crates/ferric-tools/src/control.rs` | high | Evidence policy refuses opaque mutation tools, constraining helper-based Book writes. |
| `crates/ferric-tools/src/builtin/git_write.rs` | high | Native local Git lacks fetch, merge, push, and PR operations. |
| `docs/SUMMARY.md` | low | Navigation surface checked before adding INT-0007 and Sprint 114 links. |
| `README.md` | medium | The user explicitly requested removing low-value sprint history from the landing page while retaining durable records in the Book. |

No Sprint 113 `failure-report.md` exists. Its terminal negative result is
recorded in the development screen and test report instead.

## 3. External Sources

- [Official Qwen3.8-27B model card](https://huggingface.co/Qwen/Qwen3.8-27B) — primary architecture, license, context, thinking controls, recommended sampling, and publisher benchmark results.
- [Pinned third-party Qwen3.8-27B GGUF tree](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/tree/313447f257f7ebde0b968e4778feef774546ed81) — Unsloth conversion revision and available quantization artifacts.
- [Pinned Q4_K_M GGUF file](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q4_K_M.gguf) — quality-first primary artifact size and converter-published SHA-256.
- [Pinned Q3_K_XL GGUF file](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q3_K_XL.gguf) — conditional throughput fallback size and converter-published SHA-256.
- [Animus Sprint Loops](https://github.com/crussella0129/Animus_Sprint_Loops) — current Book-v2 workflow, open-harness routing contract, two-branch checkpoint model, and claimed Ferric compatibility.

## 4. Risks, Unknowns, Dependencies

- **Runtime compatibility:** the installed llama.cpp build is recent but has not
  yet proved that it accepts Qwen3.8 architecture, chat template, thinking
  output, and Ferric's constrained OpenAI route. A failed smoke is an engine
  incompatibility until isolated.
- **Usable versus trained context:** the model advertises 262K native context
  and extension to 1M, while the local plan begins at `32768`. The 16.46 GB Q4 weights
  require hybrid CPU/GPU offload; only measured startup allocation and
  inference can establish the operating point.
- **Memory pressure:** plan-freeze free resources were about 15.04 GiB RAM and
  8.57 GiB VRAM. Q8 K/V, 24 offloaded layers, and mmap make Q4 plausible, not
  guaranteed. One `16384` memory retry and one predeclared Q3 quant fallback bound
  calibration without pretending that a 27B model is a native 11 GiB fit.
- **Throughput:** a model that merely loads may still be unusable over 28 agent
  turns. A 2.0 decoded-token/s viability floor is measured after one warm-up
  by the median of three fixed requests that each decode at least 128 tokens.
  Per-request and whole-session elapsed caps are frozen before inference.
- **Sampling mismatch:** Qwen's precise-coding recommendation includes knobs
  Ferric does not expose directly. Exact effective settings must be retained;
  they cannot be silently described as publisher-recommended sampling.
- **Model-authored execution:** Rust tests are executable untrusted code. Docker
  is unavailable, but WSL Bubblewrap successfully unshares network and can
  expose a read-only source plus isolated writable target/temp areas. The
  grader must fail closed if that sandbox cannot be recreated.
- **Disk pressure:** the selected artifact needs 16.46 GB while the volume had
  84.14 GB free at inventory time. Build artifacts and duplicate evidence must
  be bounded; the GGUF remains ignored and is never copied into tracked docs.
- **Skill packaging:** the current upstream adapter tree/frontmatter and helper
  paths require a pinned live installation test. Repository prose is not proof
  of Ferric parser compatibility.
- **Skill resources:** current Ferric injects only one top-level body and denies
  model access to `.ferric`; linked phase resources are the predicted first
  orchestration gap.
- **Helpers and remote authority:** the model-facing query registry excludes
  `shell_exec` and `manage_task` under every policy and ring. Ordinary typed
  file tools can attempt an operator-materialized phase, but cannot run the
  distribution's helpers. Native Git lacks remote checkpoint operations. These
  outcomes must stay separate from instruction-loading success.
- **Trace integrity:** ordinary trace validation is structural, not a hash
  chain. Sprint artifacts will record hashes and Git-track retained evidence,
  but that compensating procedure is not a product capability.

## 5. Recommended Approach

Primary: download the pinned `Qwen3.8-27B-UD-Q4_K_M.gguf` into `models/`,
verify exact bytes and SHA-256, and smoke the managed llama.cpp server at
context `32768`
with 24-layer hybrid offload, Q8 K/V, flash attention, one slot, and retained
effective settings. If and only if the frozen viability rule fails, test the
pinned Q3_K_XL artifact once under the same functional measurement and select
it only by the precommitted rule. Then run the frozen `MH-RS01` release-plan
task through Ferric in a disposable Git workspace. Use an explicit checks
file, grammar protocol, a smoke-calibrated tier/turn budget, and at most one
genuine resume. Run the trusted grader through network-disabled Bubblewrap,
retain all model output without Codex repair, and grade planning, build, tests,
iteration, persistence, safety, and trace separately.

In parallel but isolated from the app coordinate, pin the current Sprint Loops
upstream commit, install its open-harness adapter as the operator, and execute
the smallest layer-by-layer protocol in
`sprint-loops-capability-audit.md`. Use `--no-config` negative/authorized
loading controls, a no-hint resource-resolution probe, effective-grammar proof
that helper execution is unavailable, and only then a clearly labeled
operator-materialized typed-tool Book-write probe. Validate its result outside
the model with upstream helpers. Do not authorize remote mutation or call
repeated operator invocations autonomous re-entry.

Alternatives considered: the smaller Qwen3.8 2-bit/IQ2 artifacts fit more
easily but risk exact syntax and long-horizon decision quality, so they are not
in the fallback ladder. Qwen3.5-9B Q5_K_M is outside the frozen ladder and
would require a new retained plan deviation. The existing Qwen2.5-Coder-7B,
with its directly relevant 0/3 history, is the sole frozen fallback simulation
and skill-audit control when Qwen3.8 is non-viable.

Scan the complete `README.md` and remove every numerically named sprint-result
narrative and direct sprint-result link. The current file has one such block,
the Sprint 113 status paragraph. Leave the landing page pointing at the
authoritative Book, intents, current work, and completed work; the evidence
itself remains untouched in Sprint 113.

Rationale: this isolates the highest-leverage changed factor—the current model—
while keeping task, grader, execution authority, and evidence semantics fixed.
It answers the user's practical questions without converting a demo into an
unbounded benchmark or an upstream compatibility claim into fact.

## Artifacts

- [Hardware inventory](hardware-inventory.md) — measured host, runtime, disk,
  sandbox, existing-model, and skill state.
- [Model selection](model-selection.md) — exact artifact, hash, fit estimate,
  alternatives, and runtime caveats.
- [Sprint Loops capability audit](sprint-loops-capability-audit.md) — static
  layer matrix and minimum live-test protocol.
- [Medium-horizon experiment design](experiment-design.md) — frozen app task,
  grader contract, isolation rule, resume coordinate, and pass vector.
