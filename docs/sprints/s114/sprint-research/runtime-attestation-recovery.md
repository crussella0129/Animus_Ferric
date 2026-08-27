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
