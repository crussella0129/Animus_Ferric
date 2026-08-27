# Runtime attestation recovery

## Finding

The epoch-1 `01-q4-32768` result is an attestation-design failure, not a Q4
model failure. The pinned llama.cpp b10516 server loaded the exact model at
the requested context and emitted the expected allocation, cache, flash, and
thinking evidence. Its `/props` response omitted `default_template_kwargs`,
so the frozen verifier could not satisfy a predicate that the endpoint does
not expose.

That omission matches the pinned upstream implementation. During model load,
llama.cpp copies `params_base.default_template_kwargs` into the server's
reused chat parameters; the `/props` implementation publishes the chat
template and capability map, but not that defaults map. The server then
merges request `chat_template_kwargs` over the launch defaults before applying
the template. `/apply-template` follows that same parsing/rendering path and
returns the prompt without performing inference.

Primary sources at the exact runtime tag:

- [server context initialization and `/props`](https://github.com/ggml-org/llama.cpp/blob/b10516/tools/server/server-context.cpp)
- [request/default merge before template application](https://github.com/ggml-org/llama.cpp/blob/b10516/tools/server/server-common.cpp)
- [reasoning-preservation alias behavior](https://github.com/ggml-org/llama.cpp/blob/b10516/common/jinja/caps.cpp)
- [`/apply-template` endpoint contract](https://github.com/ggml-org/llama.cpp/blob/b10516/tools/server/README.md#post-apply-template-apply-chat-template-to-a-conversation)

## Epoch-2 recovery protocol

Epoch 2 preserves epoch 1 byte-for-byte and freezes a separate control set.
Its non-inference attestation sends the same three-message sentinel history
through four `/apply-template` arms:

1. no request override;
2. `preserve_reasoning: false` only;
3. both `preserve_reasoning: false` and `preserve_thinking: false`;
4. both values true.

The positive arms must be byte-identical and contain the prior assistant
reasoning exactly once in the template-added `<think>` structure. The
all-false arm must equal a positive arm with only that exact block removed.
Every arm must end in the enabled generation `<think>` prefix. The retained
`/props.chat_template` must also hash to the precommitted source identity.

The second epoch-1 defect was archive portability: verification compared the
recorded child `PATH` with the later verifier process's `PATH`. Epoch 2 records
the parent `PATH` in the launch declaration, derives the child value as the
pinned runtime prepend plus that recorded parent value, and never consults the
verifier environment.

The recovery also hardens provenance without changing model-selection or
measurement semantics. Attempt manifests reject unlisted files; an anchored
`.gitattributes` rule prevents Git from normalizing retained log bytes; parsed
llama device identity is frozen separately from volatile free-memory text; and
final coverage spans both runtime epochs.

No context retry, Q3 acquisition, viability gate, smoke, or throughput result
is authorized by epoch 1. Those transitions remained gated on the first valid
Q4 attempt.

## Epoch-2 live result

Epoch 2 proved the replacement template protocol, but exposed a separate
Windows process-identity false negative. Ferric deliberately invokes the
declared PATH token `llama-server`; Windows retained that basename as command
line argument zero while independently reporting the resolved absolute
`ExecutablePath`. The epoch-2 online attestor and offline verifier both
replaced the declared basename with the absolute path before comparing the
argument vectors. Their only failed core predicate was therefore
`llama-server` versus the already verified absolute executable path.

That distinction is part of the Windows process contract: `Win32_Process`
reports `ExecutablePath` and the launch `CommandLine` separately, and
`CreateProcess` does not require the executable spelling in the retained
command line to equal the resolved image path:

- [Win32_Process](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process)
- [CreateProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessa)
- [CommandLineToArgvW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-commandlinetoargvw)

The archived `e02-01-q4-32768` evidence remains trustworthy as a control
incident. It records healthy startup of the exact executable/runtime/model,
the requested 32,768 context and 24-layer offload, matching runfiles and
listener ownership, successful health/model/property endpoints, a passing
four-arm template differential, and clean teardown. It is not model-viability
evidence: the attestation false negative correctly stopped smoke and
throughput, produced `infrastructure_blocked`, and left `evidence_complete`
false. It authorizes neither the 16,384 context retry nor Q3 acquisition.

## Epoch-3 recovery protocol

Epoch 3 preserves epochs 1 and 2 byte-for-byte and changes only process-command
attestation plus epoch/provenance identities. A shared validator binds the
resolved executable path and recomputed SHA-256 to the frozen runtime first,
parses the retained command line with Windows quote/backslash semantics, then
accepts only the exact declared bare argument-zero token or the exact frozen
absolute executable. It compares every remaining argument individually and in
order; it never resolves argument zero through PATH or accepts an arbitrary
same-basename path.

The self-test base fixture must use the observed live basename form. Separate
regressions cover the exact absolute form, wrong and path-like aliases,
executable path/hash tampering, missing/extra/reordered or boundary-shifted
arguments, malformed command lines, measurement-contract drift, disjoint epoch
paths, and exact prior-epoch trees. Only a newly frozen `e03-01-q4-32768`
attempt may resume calibration.

## Epoch-3 live result

The epoch-3 coordinate completed the previously blocked calibration without a
context or quant fallback. `e03-01-q4-32768` loaded the exact
`Qwen3.8-27B-UD-Q4_K_M.gguf` identity
`322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482`
at context 32,768 with 24 requested and effective GPU layers. Managed-server
identity, listener ownership, health, served model, effective cache/reasoning
settings, the four-arm template differential, and teardown all passed under
the corrected process-command contract.

The real grammar-protocol nonce smoke reached a structurally verified terminal
trace without workspace mutation or clarification. It read the exact nonce,
reached `task_complete` in two turns, and retained trace SHA-256
`d7b373edc31b52b11096d715cd3802298514b63e3a83fa389705bb1cc23d8942`.
The whole coordinate completed in 528.057 seconds, within its 5,400-second
cap. One unscored warm-up and all three fixed 256-token timed requests
completed with valid counters and minimum lengths. Their decoded-token rates
were `3.2064850358228254`, `3.2287513446141283`, and
`3.173189287397254`; the retained median was
`3.2064850358228254` decoded tokens per second, above the frozen 2.0-token
floor. The coordinate was therefore `viable`; context 16,384 and the Q3_K_XL
fallback were never authorized.

The sealed attempt contains 49 manifest entries and 437,140 payload bytes.
Its manifest SHA-256 is
`4ba753e79f59d2441eade7d7e7bab7131f7f6cfeed6a702bcf719faf8fde430a`;
the terminal attempt and attestation SHA-256 values are respectively
`167a964e471fec93bc7e58ff0ec76bbba45f3025f18c2fe84060248732b4fae4`
and `792ae02c6323deafcaae9b89b247b43fccdc07cabb3ff470a0b7edfee78b0a99`.

## Publication recovery and final selection

The measurement completed once, but its publication crossed three separately
frozen recovery boundaries. Its initial post-measurement publisher coerced RFC
3339 JSON strings through local `DateTime` values and failed verification.
Epoch 4 corrected that with string-preserving date handling, then its publisher
failed closed because the wrapper read attestation protocols from the recovery
plan instead of the anchored epoch-3 source plan. Epoch 5 used the correct
source binding and published the exact 49-entry destination, then failed closed
before writing envelopes because its exact-property check received an
`OrderedDictionary` rather than the JSON-object contract it validated. Neither
incident repeated model execution or altered an epoch-3 measurement byte.

Epoch 6 preserved both failed publishers and the first failed pre-control
self-test as immutable incident evidence. That report is 39,692 bytes at
SHA-256
`de7ce31a000cd78abe55455db5d6ed5b6931ef00e76c18a2c5e25b03822e27ce`:
21 of 22 checks passed, the sole failure was an ambiguous first-occurrence
harness search, and no controls or official outputs existed. The corrected
self-test is 40,822 bytes at SHA-256
`69b98fc9bb81be32759b079c16eed71aa05530df5ad613bf10b742f5d6319844`;
it passed 23 of 23 checks with exactly one live model hash and froze control
manifest
`a63f9dff611974a2c5bdf3e63e6bd09853ff29fab0684bd4b3f5af662d8b6930`,
materialized the legacy epoch-4 envelope and epoch-5 correction as JSON-native
objects, and re-entered the frozen epoch-5 publisher only through its
already-complete validation path. A second materializer invocation changed no
bytes or timestamps.

The final Q4 gate independently reverified the viable result. The atomic final
bundle selected `e03-01-q4-32768`, `Q4_K_M`, context 32,768, and 24 effective
GPU layers. Gate, selection, and final runtime-verification SHA-256 values are
`8901d1374a137e7a48e3e30a2c4237348fa46c02918fad92155108d7b46d9fd9`,
`1af2c036e26f244e273c4378cae948b1d9c13bc105e2c9db34fbd85af7ef45dc`,
and `a4b256cca32260d9497537b90cb8e36d100f9aac6dccfac55a0105020141486b`.
All four named runtime verification groups and final cold-state teardown
passed. Artifact manifest
`60cac4761f5276d05fe9f1be296925e4d9e0e7e667706564b8478bb792167f40`
covers all 213 non-self runtime files and 3,361,653 payload bytes with no
mismatch or residual staging directory.
