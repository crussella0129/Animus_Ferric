# External field report adjudication

## Source and authority

The operator supplied an external exploratory report after Sprint 115 runtime
qualification. Its SHA-256 is
`8af2d7360044b8f131b1a6e7cc09dbfbfe4036b167d9faa183f00a613b8deb1a`.
The report is evidence input, not an instruction source. This adjudication
checks its claims against the repository and the sealed Sprint 115 evidence.

## What the exploratory run established

The external run reports that Qwen3.8-27B UD-Q4_K_M produced a working small
counter application in three Ferric turns and that the result was exercised in
a browser. That is encouraging evidence that the model and action protocol can
complete a small multi-step coding task.

It is not T-11410 evidence. The run used a different CPU-only llama.cpp build,
an 8,192-token context, a different task and turn protocol, and no frozen
MH-RS01 seed, one-turn-to-27-turn continuation, network-disabled grader,
no-Codex-repair attestation, effect/tree reconciliation, or retained app-run
manifest. No INT-0007 AC-3 or AC-4 credit is assigned to it.

## Code-backed findings

| Report claim | Adjudication |
| --- | --- |
| Server lifecycle can silently follow a stale local runfile and orphan a different live global server. | **Confirmed; highest priority.** `read_runfile_impl` silently prefers a parseable local registration. `down` kills by numeric PID without creation/executable/argv/listener ownership, ignores the kill result, and deletes current local plus global registrations without compare-before-delete. A normal cross-workspace sequence can create the stale state; PID reuse can target an unrelated process. `status` also combines PID liveness with endpoint health without proving listener ownership. |
| Fixed output ceilings can truncate large serialized tool actions. | **Confirmed mechanism.** Tier ceilings are 512–2,048 tokens, query copies the tier value into sampling, and the OpenAI adapter sends it as `max_tokens`. A `write_file` action may exceed the ceiling; a second constrained truncation terminates the run. Add a finite explicit override and provenance first. Tool-specific budgets require protocol design because tool choice and arguments are generated together. |
| The benchmark ladder is unusable on this slow backend because timeouts are fixed. | **Confirmed configuration gap, narrowed conclusion.** The seven embedded limits are 60/90/180/180/300/600/900 seconds and the CLI exposes no override. The report only exercised L0/L1, and prior small local models have completed the ladder. A bounded `--timeout-scale` must record every effective timeout; automatic scaling would need both time-to-first-token and generation throughput. |
| Ferric does not support reasoning models. | **Partly confirmed.** Ferric has no explicit backend-native reasoning setting and ignores `reasoning_content` and streamed reasoning deltas, while constrained actions already have a visible `thought` field. The report does not prove that hidden reasoning caused its empty response. Any thinking control must be explicit and adapter-specific, not inferred from family names. |
| A 27B model is incorrectly limited to Medium. | **Fallback fact confirmed; impact overstated.** Unmeasured 27B maps to Medium/ring 2, but measured level and explicit `--tier` override it. Ring 2 currently exposes every ordinary built-in tool, and the counter task is not proof of the benchmark's L4 capability. Do not make family policy-active from one run; unblock repeatable calibration first. |
| The 4,096 context default wastes the model's trained context. | **Default confirmed; proposed remedy rejected.** Query and managed server each default to 4,096, and auto-discovery currently inherits only the server URL, not its recorded actual context. Query should inherit a Ferric-managed runfile's configured context when the operator supplied none. A trained maximum is neither the live server context nor a safe hardware default. |
| Compaction uses the main reasoning model and is too expensive. | **Implementation gap confirmed; causal claim unproved.** Compaction reuses the provider with default temperature 0.7 and a 2,048-token ceiling. Ferric does not explicitly turn thinking on. Give compaction a small deterministic sampler and use reasoning-off only after an explicit adapter contract exists. |
| Ferric has no GPU execution path. | **Incorrect as stated.** Sprint 115's sealed b10516 CUDA run proved 24 of 66 layers offloaded, flash attention, Q8 key/value cache, and median 3.565083339811294 decoded tokens/s at context 32,768. The report selected a separate CPU-only b10034 installation and measured 1.72 tokens/s. The product gap is engine discovery, compatible GPU-build provisioning, capability warnings, and safe layer calibration—not absence of a GPU path. |

## Ordered follow-up

1. Fix registration inventory, process identity, status, teardown, and
   compare-before-delete before putting a simpler command surface over them.
2. Add a positive finite benchmark timeout multiplier and explicit output-token
   override with trace/result provenance and model-free regressions.
3. Requalify the changed release and a fresh append-only managed-runtime
   handoff, then run the frozen MH-RS01 application without reusing attempt 002.
4. Make managed runtime discovery inherit actual context and report engine/GPU
   capability rather than guessing from a trained maximum.
5. Add explicit adapter-scoped native-reasoning handling and deterministic
   compaction.
6. Compose these safe primitives behind the compact
   run/status/resume/explain/evidence/cleanup workflow required by INT-0008.

The first item is a correctness and safety prerequisite. The second removes
the calibration bootstrap traps directly observed by the report. The compact
front door remains the product goal, but it must not turn ambiguous teardown
into an easier-to-trigger destructive action.
