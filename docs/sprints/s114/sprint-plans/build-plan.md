Finalized - DO NOT EDIT

# Sprint 114 Build Plan

## Intents

- [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md) — state: `planned`; acceptance criteria covered: AC-1 through AC-7.
- [INT-0001](../../../intents/INT-0001-evidence-bound-autonomous-recovery.md) — terminal constraint only; acceptance criteria changed: none; Sprint 114 does not reopen or promote the falsified Evidence intervention.
- [INT-0002](../../../intents/INT-0002-operator-authorized-default-verification.md) — dependency boundary only; acceptance criteria changed: none; the app trial uses an explicit checks file and does not implement defaults.
- [INT-0003](../../../intents/INT-0003-requirement-evidenced-completion.md) — diagnostic boundary only; acceptance criteria changed: none; an external grader scores obligations without implementing a requirement ledger.
- [INT-0004](../../../intents/INT-0004-auditable-session-provenance.md) — dependency boundary only; acceptance criteria changed: none; manual hashes compensate for, but do not implement, product trace integrity.
- [INT-0006](../../../intents/INT-0006-truthful-policy-contract.md) — reporting boundary only; acceptance criteria changed: none; no unavailable orchestration is relabeled.

## Planning Adapter Caveat

The current Codex adapter did not expose Sprint Loops' `EnterPlanMode` and
`ExitPlanMode` actions. The user explicitly authorized continuing this bounded
sprint. Planning therefore kept implementation source unchanged, wrote only
Book planning/work metadata after that authorization, and retained this caveat
instead of claiming native Plan Mode evidence. This is a deliberate
adapter-level protocol deviation and the sprint can proceed only with that
caveat; `finalize-plan.sh` validates the files but cannot retroactively make
the Plan phase fully Book-v2 conformant.

## Schema Tree

- Hardware-calibrated autonomous development
  - Model and runtime
    - T-11407: acquire and attest the selected GGUF
    - T-11409: calibrate the managed-server/Ferric coordinate
  - Reproducible app experiment
    - T-11408: freeze and self-test the seed, grader, and sandbox
    - T-11410: run and grade the no-repair medium-horizon task
  - Sprint Loops compatibility
    - T-11411: pin, install, and probe every capability layer
  - Evidence and closeout
    - T-11413: remove sprint-history detail from the landing README
    - T-11412: archive, verify, tear down, and report truthfully

## Execution Sequence

### T-11407: Acquire and attest the selected GGUF

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** ignored `models/Qwen3.8-27B-UD-Q4_K_M.gguf`; conditionally ignored `models/Qwen3.8-27B-UD-Q3_K_XL.gguf`; `docs/sprints/s114/control-artifacts/model/`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0007 AC-1
- **Success criterion (EARS):**
  - **E07-A — WHEN** at least 25 GB remains free and the primary artifact is absent, **THEN** acquisition **SHALL** stream third-party conversion repository `unsloth/Qwen3.8-27B-GGUF` revision `313447f257f7ebde0b968e4778feef774546ed81` artifact `Qwen3.8-27B-UD-Q4_K_M.gguf` from `https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q4_K_M.gguf?download=true` to a temporary file under `models/`, verify exact size `16,464,440,224` and SHA-256 `322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482`, and publish the final filename only after verification.
  - **E07-B — WHEN** storage, transport, size, or hash validation fails, **THEN** acquisition **SHALL** fail closed before inference, preserve an exact failure record, and leave no artifact represented as verified.
  - **E07-C — WHEN** an artifact is attested, **THEN** Git ignore verification **SHALL** prove that the GGUF is outside the tracked sprint diff while one machine-readable record asserts its conversion publisher, official upstream `Qwen/Qwen3.8-27B`, Apache-2.0 license, full revision, pinned URL, filename, exact byte size, and full SHA-256 together.
  - **E07-D — WHEN** T-11409's frozen viability rule authorizes the sole quant fallback, **THEN** acquisition **SHALL** apply the same temporary-file and fail-closed checks to revision `313447f257f7ebde0b968e4778feef774546ed81` file `Qwen3.8-27B-UD-Q3_K_XL.gguf` from `https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/313447f257f7ebde0b968e4778feef774546ed81/Qwen3.8-27B-UD-Q3_K_XL.gguf?download=true`, exact size `13,146,393,504` and SHA-256 `8c2a45ff85e7674ca185ec8eb6cdeab0e617ed9d8018caed0b64380eb2a67a5e`, without downloading a 2-bit quant.
- **Notes:** Unsloth is the third-party conversion publisher; Qwen is the official upstream model publisher. Do not download the optional vision projector or MTP head. Never commit or duplicate the model into `docs/`.

### T-11408: Freeze and self-test the app seed, grader, and sandbox

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `docs/sprints/s114/control-artifacts/app-harness/`; ignored `target/s114-experiment/`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0007 AC-3, AC-4
- **Success criterion (EARS):**
  - **E08-A — WHEN** the MH-RS01 seed, prompt, explicit checks profile, and hidden grader are frozen, **THEN** the harness **SHALL** record their hashes, demonstrate that the untouched seed fails for the intended missing implementation, and prevent the model from changing operator-owned inputs.
  - **E08-B — WHEN** the grader executes candidate Rust, **THEN** Bubblewrap **SHALL** unshare the network, expose candidate source read-only, hide unrelated host/user data, provide only isolated writable target/temp paths, and enforce bounded wall time and resources.
  - **E08-C — WHEN** the grader is self-tested against known-good and deliberately invalid fixtures, **THEN** it **SHALL** pass the complete fixture, reject every seeded contract/safety violation, and emit deterministic dimension-level results.
- **Notes:** Model-authored code is never executed if containment preflight fails. The grader may vary examples but may not add undisclosed requirements.

### T-11409: Prove and calibrate the managed-server coordinate

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `docs/sprints/s114/control-artifacts/runtime/`; ignored `target/s114-experiment/smoke/`; project-local server runfile during the live process
- **Depends on:** T-11407
- **Acceptance criterion:** INT-0007 AC-2, AC-6
- **Success criterion (EARS):**
  - **E09-A — WHEN** any verified Qwen3.8 candidate, context retry, or quant fallback is launched through `ferric server up`, **THEN** that exact coordinate **SHALL** retain the complete command/environment, Ferric/llama.cpp versions, startup allocation/offload log, effective context/cache/reasoning/timeout, memory snapshots, listener owner, `/health`, and `/v1/models` identity. The initial Q4 coordinate **SHALL** request context `32768`, exactly 24 GPU layers, Q8 K/V cache, flash attention, a 1,024 MiB device/VRAM fit target, one slot, twelve threads, batch 512, thinking enabled with a 1,024-token budget and preservation, a 720-second server read/write timeout, and fixed query seed 42; every retry **SHALL** record its explicitly allowed changed fields.
  - **E09-B — WHEN** the healthy managed endpoint receives the real grammar-protocol Ferric nonce task at thinking-mode temperature 1.0, **THEN** the model **SHALL** reach a structurally verified terminal trace without workspace mutation or clarification; after that functional smoke, throughput **SHALL** use one unscored warm-up plus exactly three identical 256-token timed requests with no replacements, retain every counter/rate, and classify any request error, timeout, or result below 128 decoded tokens as coordinate non-viability because no valid median exists.
  - **E09-C — WHEN** context `32768` allocation fails specifically from memory pressure, **THEN** calibration **SHALL** retain the diagnostic and try at most one declared context `16384` reduction; architecture, chat-template, or constrained-decoding failures do not authorize that context retry.
  - **E09-D — WHEN** Q4 is non-viable because its functional smoke fails, any of its three timed samples errors, times out, or decodes fewer than 128 tokens, or its valid three-sample median is below 2.0 decoded tokens/second, **THEN** calibration **SHALL** authorize the single pinned Q3_K_XL acquisition and run the same context/cache/sampling/measurement protocol with exactly 32 requested GPU layers. Q3 **SHALL** be selected only if it completes the functional smoke, all three timed samples are valid, and their median reaches at least 2.0 tokens/second; otherwise the result is `no_viable_qwen38_coordinate`, with no undeclared model substitution.
- **Notes:** Each quant calibration has a 90-minute wall cap. Publisher benchmarks and the 262K trained window remain external evidence, not local results. Effective settings and inherited llama.cpp environment are authoritative.

### T-11410: Run and grade the no-repair MH-RS01 app task

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** ignored `target/s114-experiment/app-workspace/`; `docs/sprints/s114/control-artifacts/app-run/`
- **Depends on:** T-11408, T-11409
- **Acceptance criterion:** INT-0007 AC-3, AC-4, AC-6
- **Success criterion (EARS):**
  - **E10-A — WHEN** the seed, grader, trial model, server, prompt, initial file manifest, and invocation are sealed, **THEN** Ferric **SHALL** receive the exact MH-RS01 task under explicit Ultra tier, Legacy policy, grammar protocol, explicit checks, Ring 1 ceiling, and a one-turn first segment. The retained active-tool inventory **SHALL** exclude `git_write`; the query registry's model surface excludes `shell_exec` and `manage_task`; candidate execution **SHALL** occur only through the fixed sandboxed `run_check`; and Codex **SHALL** issue no candidate-workspace mutation after inference begins.
  - **E10-B — WHEN** the one-turn segment begins on the intentionally failing seed, **THEN** its required fresh passing check makes accepted completion impossible in that segment; the run **SHALL** end with `max_turns`, then exactly one resume **SHALL** inherit the original trace policy, workspace, and objective without added guidance and receive 27 further turns, for 28 total. Clarification, provider failure, or another terminal result before `max_turns` is a failed persistence outcome, not `not-observed`; no second resume or prompt repair is permitted.
  - **E10-C — WHEN** execution terminates or the six-hour session wall cap expires, **THEN** the independent grader **SHALL** report planning, build, model-authored tests, verification-driven iteration, persistence, safety, and trace results separately, preserve the exact final workspace, and classify infrastructure failures outside the model score. Every final changed path and before/after hash **SHALL** reconcile to a committed Ferric trace effect and the sealed command journal; any unexplained mutation invalidates no-repair and safety rather than being attributed by assumption.
  - **E10-D — WHEN** T-11409 records `no_viable_qwen38_coordinate`, **THEN** the same sealed MH-RS01 protocol **SHALL** run once as an explicitly labeled fallback simulation against the existing `qwen2.5-coder-7b-instruct-q4_k_m.gguf` only after its recorded SHA-256 is reverified; its result **SHALL NOT** be reported as Qwen3.8 performance.
- **Notes:** The server's 720-second read/write timeout bounds a generation and an external supervisor enforces the six-hour session cap. Codex never repairs and reruns the candidate.

### T-11411: Pin, install, and probe Animus Sprint Loops

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** ignored `target/s114-experiment/sprint-loop-source/`; ignored `target/s114-experiment/sprint-loop-workspace/`; `docs/sprints/s114/control-artifacts/sprint-loop-run/`
- **Depends on:** T-11409 outcome; a non-viable Qwen3.8 result activates E11-F rather than blocking this task
- **Acceptance criterion:** INT-0007 AC-5, AC-6
- **Success criterion (EARS):**
  - **E11-A — WHEN** the pinned open-harness adapter is operator-installed unmodified, **THEN** the audit **SHALL** retain commit/tree/file hashes and record either exact `ferric skills list` discovery or the exact parse/name/layout failure. Packaging failure **SHALL** mark B through E `not-runnable-after-packaging-failure` while model-independent registry/helper/remote facts still complete.
  - **E11-B — WHEN** injection is runnable and tested, **THEN** negative and authorized arms **SHALL** use identical prompt/model/seed and `--no-config`, differ only by `--skill sprint-loop`, and require three distinct observations: absent/present captured CLI diagnostic `skill: sprint-loop (UserRequested)`, absent/present exact skill section in `SessionPrompt.system` with retained bytes/hash, and the marker as behavioral corroboration. `PromptComposed` **SHALL NOT** be used as skill-injection proof.
  - **E11-C — WHEN** the authorized skill first follows a linked phase/router reference, **THEN** it **SHALL** receive no resource path, environment shim, or operator hint; that native-resolution result **SHALL** be scored before an explicitly assisted arm copies the minimum required phase resource to an ordinary readable path. The copied bytes/hash **SHALL** equal the corresponding file in the pinned source tree, and the path hint/copy **SHALL** be labeled operator materialization.
  - **E11-D — WHEN** helper and Book behavior are probed, **THEN** an operator-owned capture stub **SHALL** retain and hash the actual provider request/constrained schema for otherwise identical explicit-Ultra Evidence/Ring-1 and Legacy/Ring-1 invocations, proving which tools are offered rather than inferring from traces or source alone. Exact helper requests **SHALL** not be mislabeled controller refusals when the tools are absent. Only afterward may the operator-materialized resource drive one explicit-Ultra Legacy/Ring-1 typed-file-tool Book attempt; no helper executor is supplied.
  - **E11-E — WHEN** the local probe ends, **THEN** the operator **SHALL** run `check-book.sh` and the router from the disposable project root using the same pinned source tree/hash, require no writable legacy/split-brain state and artifact-supported phase output, and test fresh-invocation resumption if advancement occurred. A separate explicit-Ultra policy/ring Git matrix **SHALL** score `git_write` as registered, offered, attempted, and succeeded under Evidence/Ring-2 and Legacy/Ring-2; the audit stops before remote mutation and scores every capability layer independently.
  - **E11-F — WHEN** T-11409 yields no viable Qwen3.8 coordinate, **THEN** the skill audit **SHALL** reverify SHA-256 `509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c` and freshly smoke the existing Qwen2.5-Coder-7B Q4_K_M control. Missing file, hash mismatch, or failed smoke **SHALL** produce `fallback_control_unavailable` with no alternate model or silent redownload; packaging, model-registry, and remote-authority conclusions still complete, and any behavioral results retain their separate model identity.
- **Notes:** A raw host-shell `gh` command does not become native remote-profile support. No real project or remote is mutated by this probe.

### T-11413: Remove low-value sprint history from README

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `README.md`
- **Depends on:** (none)
- **Acceptance criterion:** INT-0007 AC-7
- **Success criterion (EARS):**
  - **E13-A — WHEN** the landing README is edited, **THEN** it **SHALL** remove every numerically named sprint-result narrative and every `docs/sprints/sN/` result link from the entire landing page while preserving concise current capability and policy semantics.
  - **E13-B — WHEN** the cleanup is reviewed, **THEN** every retained README documentation link **SHALL** resolve locally and the canonical Sprint Book, intents, current-work, and completed-work links **SHALL** remain the route to history.
  - **E13-C — WHEN** the README diff is compared with durable evidence, **THEN** no historical sprint artifact **SHALL** be deleted, rewritten, or moved merely to support the cleanup.
- **Notes:** This is landing-page curation, not provenance deletion or a Git-history rewrite.

### T-11412: Archive, verify, tear down, and publish the verdict

- **Intent:** [INT-0007](../../../intents/INT-0007-hardware-calibrated-autonomous-development.md)
- **Touches:** `docs/sprints/s114/`; `docs/work/tasks.md`; `docs/work/completed-tasks.md`; `docs/intents/INT-0007-hardware-calibrated-autonomous-development.md`
- **Depends on:** T-11410, T-11411, T-11413
- **Acceptance criterion:** INT-0007 AC-1 through AC-6
- **Success criterion (EARS):**
  - **E12-A — WHEN** both live coordinates finish, **THEN** archival **SHALL** retain a timestamped command journal, source/binary/model/prompt/seed/grader/workspace/trace hashes, stdout/stderr, server attestations, final file inventory and diff, structured grades, and capability matrix without reconstructing missing evidence.
  - **E12-B — WHEN** teardown begins, **THEN** verification **SHALL** validate every retained trace side-effect-free, prove artifact hashes after copying, stop the managed server, and independently confirm absent listener, process, runfile, and untracked non-evidence residue.
  - **E12-C — WHEN** Sprint 114 closes, **THEN** the report and Book state **SHALL** distinguish trained/usable context, publisher/local results, model/infrastructure failures, prompt injection/orchestration, manual/native operations, and structural/application success; task and intent transitions shall match the evidence.
- **Notes:** A negative or partial capability result can realize INT-0007 if all bounded questions are answered with intact evidence.
