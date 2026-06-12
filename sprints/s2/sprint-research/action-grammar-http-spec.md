# Artifact: Unified Action Grammar + HTTP Escape Valve Spec (s2)

> Source: web+source research agent (verified against the lockfile-resolved llguidance 1.7.6 and mistralrs-core 0.8.1 sources, and llama.cpp master), 2026-06-12.

## A. Unified action grammar (llguidance via Constraint::JsonSchema)

### Support matrix (llguidance 1.7.6 — the version our build resolves)
- **`anyOf` fully supported; `oneOf` REJECTED by default** (errors unless coerce_one_of/lenient) → use anyOf.
- `const`/`enum`/`required`/`additionalProperties:false`/`$defs`+in-doc `$ref`/pattern/lengths: supported.
- Properties emitted in declaration order → put `"tool"` FIRST in each branch (early branch commitment).
- `x-guidance` root extension: `{"whitespace_flexible": false}` saves tokens, more deterministic.

### mistralrs 0.8.1 mechanics (source-confirmed)
- `Constraint::JsonSchema(value)` → `TopLevelGrammar::from_json_schema` → llguidance token mask over the ENTIRE assistant completion from token zero. The whole raw completion IS the action JSON; parse with serde_json.
- Prompt still flows through the chat template; the system prompt must teach tool semantics (the grammar shapes, the prompt teaches).
- Caveats: thinking-model templates fight a from-token-zero mask; grammar-accepting state forces clean EOS; **`finish_reason == "length"` (max_tokens truncation) is the one malformed-action case the grammar cannot prevent — must be handled as a failed action**.
- **mistralrs does NOT guard constraint×tools coexistence (independent NormalRequest fields)** — exclusivity is the harness's job (ADR-010 vindicated). llama.cpp's server enforces it server-side ("Cannot use custom grammar constraints with tools.").

### Schema shape (verified-supported constructs)
Top-level `{"x-guidance":{"whitespace_flexible":false}, "type":"object", "anyOf":[ per-tool branches ]}` where each branch = `{"properties": {"tool": {"const": NAME}, "args": TOOL_SCHEMA(additionalProperties:false)}, "required":["tool","args"], "additionalProperties":false}`. Generated mechanically from ToolDescriptors + the task_complete descriptor. Optional constrained `"reasoning"` scratchpad property (before "tool") can improve small-model action choice at token cost.

### Protocol switch recommendation
Model as an enum so the invalid state is unrepresentable:
```rust
enum ActionProtocol { NativeTools, UnifiedGrammar }
```
- NativeTools: tools+tool_choice set, constraint None (today's s1 behavior).
- UnifiedGrammar: constraint = generated schema, tools empty; tool docs in system prompt; whole completion parsed as `Action { tool, args }`.
- Both normalize into the same internal action before dispatch (executor/trace protocol-agnostic).
- Default UnifiedGrammar for small models (RunPolicy.protocol == ConstrainedJson drives it); NativeTools per-model where the template has a trained tool format.
- Tool results in UnifiedGrammar mode: prefer user-role framed `[tool_result for X] ...` messages (template tool-role may misbehave without tools in context).

## B. HTTP escape valve (llama-server, verified against master source)

- `POST /v1/chat/completions`: `response_format: {"type":"json_schema","json_schema":{"schema":{...}}}` (server reads `.json_schema.schema`; name/strict ignored; legacy `{"type":"json_object","schema":{...}}` also works). Server converts schema→GBNF internally (`json_schema_to_grammar`) — no client-side GBNF needed. Top-level `"grammar"` (GBNF) extension exists; mutually exclusive with json_schema AND with tools (server throws).
- Tools require `--jinja`; `tool_choice` ∈ auto|none|required only; response `tool_calls[].function.arguments` is a JSON-encoded STRING (serde_json::from_str); finish_reason "tool_calls"; native grammar-constrained handlers for Llama 3.x/Qwen 2.5/Hermes/etc., generic fallback otherwise.
- Usage block: `usage.{prompt_tokens, completion_tokens, total_tokens}` (+ llama.cpp `timings`).
- Health: `GET /health` 200 ok / 503 loading (poll before first turn). `GET /v1/models` (id, n_ctx_train), `GET /props` (model_path, active chat_template, slots).
- Rust client pattern: reqwest with connect_timeout 5s + total timeout (generation is slow, e.g. 300s), Content-Length guard AND capped `bytes_stream()` accumulation (8 MB) — Content-Length can be absent/lying; retry classification: 429/500/502/503/504 + timeout/connect/reset → RetryableBackend (honor Retry-After); 400/401/403/404/422 → permanent; 200 with finish_reason "length" → harness-level truncated action, not transport retry.

## Sources
1. github.com/guidance-ai/llguidance docs/json_schema.md
2. Local lockfile-resolved sources: llguidance-1.7.6 src/json/{compiler,schema}.rs; mistralrs-core-0.8.1 src/{request.rs, pipeline/llg.rs, engine/add_request.rs}
3. github.com/ggml-org/llama.cpp tools/server/README.md
4. llama.cpp tools/server/server-common.cpp (oaicompat_chat_params_parse)
5. llama.cpp docs/function-calling.md (+ common/chat.cpp; mistral.rs PR #899)
