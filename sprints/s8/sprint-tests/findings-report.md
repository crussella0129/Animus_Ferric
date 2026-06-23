# Sprint 8 — Real-Model Test Findings (the heartbeat, run)

**Date:** 2026-06-23 · **Run by:** Claude Opus 4.8, on the dev machine.
This is the real-model E2E acceptance that earlier sprints had parked as
"needs a human heartbeat." It needed no deployment — ollama was already running —
so it was run directly. It validates the *entire* sprint 7 + sprint 8 stack.

## Environment
- **Engine:** ollama 0.30.10 (already serving on `:11434`). `llama-server` is NOT installed.
- **Models (ollama):** `qwen2.5-coder:7b`, `llama3.1:8b`.
- **Models (GGUF, `D:\Models`):** Llama-3.2-1B, Phi-3-mini-4k, Qwen2.5-Coder-7B/14B, Qwen3-VL-8B, c4ai-command-r7b, gemma-4-e4b (Gemma 3n E4B), gemma-4-12B, qwen1_5-0_5b, …
- **Binary:** `cargo build -p ferric-cli --features backend-openai` (debug, 15 s — ferric only talks HTTP; ollama does the inference).

## Headline result: the constrained-decoding thesis, proven on a real model

Same model (`qwen2.5-coder:7b`), same 5 tools, 5 iterations each, two protocols:

| Protocol | Result | Verdict |
|---|---|---|
| `grammar` (**ConstrainedJson** — harness sends a JSON-Schema via `response_format`, ollama enforces it) | **25 / 25 = 100.0%** | **solid** (every tool 5/5) |
| `native` (trust ollama's tool-calling) | **0 / 25 = 0.0%** | **unreliable** (`no_action×5` on every tool) |

**The diagnostic earned its keep — it named the cause, not just the number.** The
native failures were classified `no_action`, and a raw probe confirmed why:

```
POST /v1/chat/completions  (model qwen2.5-coder:7b, tools=[read_file], tool_choice=auto)
→ message.content    = '{"name": "read_file", "arguments": {"path": "a.txt"}}'
→ message.tool_calls = null
```

Ollama returns the tool call as **text in `content`** with the OpenAI
`tool_calls` field **null** — and even the shape (`{name, arguments}`) differs
from the harness's `{tool, args}`. So native tool-calling is silently unreliable
through ollama's OpenAI-compatible endpoint; the harness-owned constraint
sidesteps it entirely by *forcing* the exact action schema → 100%.

## Launcher: full lifecycle validated against real ollama
`ferric server up --engine ollama --port 18080`:
- spawned a second ollama (pid 59744), polled it ready, wrote `.ferric/server.json`. ✅
- `ferric server status` → `engine=Ollama pid=59744 base_url=http://127.0.0.1:18080/v1 (reachable)`. ✅
- `ferric toolbench --backend openai --protocol grammar` **with no `--api-base`** → auto-discovered `:18080` from the runfile → **10/10 = 100% solid**. ✅ (T-805 proven)
- `ferric server down` → killed pid 59744, removed the runfile; `status` → "no server registered"; process count back to 1. ✅

(Note: ollama is a singleton daemon, so launching a *second* instance is an edge
case — it worked here. The launcher's primary home is `llama-server`, or starting
ollama when it isn't already up; against an already-running ollama you just point
at it.)

## What this validates
- The sprint-7 OpenAI backend `response_format` wiring works against a real server.
- Ollama **does** honor `response_format` json_schema (the load-bearing E2E-1 unknown — resolved: it enforces it).
- The sprint-8 diagnostic toolbench (taxonomy + report + verdict) works end-to-end and is genuinely diagnostic.
- The sprint-8 `ferric server` launcher + auto-discovery work end-to-end.
- `ferric query --backend openai` defaults (via `select_protocol`) to `ConstrainedJson`, i.e. the 100% path — so the 0% native result is only reachable by explicit `--protocol native`. The default is correct.

## Implications for next steps (input to sprint 9 research)
1. **The foundation is solid — build up, not sideways.** Constrained tool-calling
   works on a real small/mid model. The natural next feature is the deferred one
   (ADR-023): **multimodal "any file" input** — Gemma 3n E4B is sitting in
   `D:\Models`, and the whole point of "process any file" needs the `Message`
   content-parts + capability-gating work.
2. **Native-path robustness (minor).** Ollama-returns-tool-call-as-text is a real
   gap, but ConstrainedJson is the default for the HTTP valve and works, so this
   is low priority — a possible small task: a native fallback that scrapes a
   tool-call-shaped `content` when `tool_calls` is null.
3. **Fleet calibration (now unlocked).** The testbench works — running it across
   `D:\Models` (0.5B → 14B) would produce the real "which model is good enough"
   capability table, feeding `measured_level` (ADR-019). High-value, cheap.
4. **mistral.rs 0.8.15 viability (still pending).** The grammar-hang probe against
   the bumped dep — the ADR-023 decision gate for mistral.rs's future.
