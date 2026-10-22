# ferric-valve

An OpenAI-compatible chat-completions service that puts Ferric's
harness-owned constrained decoding between **Hermes Agent** (Animus Amalgam)
and a **llama.cpp** backend. Hermes keeps its loop, tool validation,
authorization, execution and history. The valve changes only how the next
action is decoded. It is stateless per request (INT-0012).

## Request contract (Hermes to upstream)

**Mode selection.** A request is **constrained** when it has a non-empty
`tools` array, a `tool_choice` other than `"none"`, and no `response_format`.
Anything else is **pass-through**: the original bytes go upstream unchanged.
Hermes's auxiliary calls (titles, compression, structured output) take the
pass-through path.

| Field | Constrained mode | Pass-through |
|---|---|---|
| `messages` | Rewritten deterministically (see below) | Unchanged |
| `tools` | Removed; rendered into the system message and compiled into the schema | Unchanged |
| `tool_choice` | Removed. `auto` or absent admits every tool plus `task_complete`; `required` admits the tools only; `{"type":"function","function":{"name":X}}` admits only X. Any other value is a 400 | Unchanged |
| `parallel_tool_calls` | Removed (one action per turn) | Unchanged |
| `stream` | Forced to `true` upstream; the client's value selects the downstream format | Unchanged |
| `response_format` | Set to `{"type":"json_schema","json_schema":{"name":"ferric_action","schema":<action schema>,"strict":true}}` | Present means pass-through |
| Everything else (`model`, sampling, `max_tokens`, `chat_template_kwargs`, …) | Forwarded unchanged | Unchanged |

**History projection** follows the convention Ferric's own constrained loop
replays:

- **The first `system` message** gets the constrained-protocol teaching text
  and the tool listing appended (`\n\nAvailable tools:\n- name: description`).
  If there is no system message, one is inserted first.
- **An assistant message with `tool_calls`** becomes one assistant message per
  call. Its content is the compact action
  `{"thought":<reasoning_content or "">,"tool":<name>,"args":<arguments>}`.
- **An assistant message without `tool_calls`** becomes
  `{"thought":<reasoning_content or "">,"tool":"task_complete","args":{"summary":<content>}}`.
- **A `tool` message** becomes a user message
  `[tool_result for NAME] <content>`, where NAME is resolved through
  `tool_call_id`. An unknown id is a 400; the valve never guesses.
- **Any other message** keeps its role and content.

The final-answer control is `task_complete`, with `summary` described as the
complete reply to the user. The same transform always produces the same
bytes, and appending turns only appends projected messages, so a backend
prefix cache survives from request to request.

## Response contract (upstream to Hermes)

The upstream request always streams. How the finished constrained action
reaches the client:

| Upstream outcome | Final message | `finish_reason` |
|---|---|---|
| Offered tool | `content: null`, `reasoning_content` = thought, one `tool_calls` entry (arguments as compact JSON) | `tool_calls` |
| `task_complete` | `content` = `summary`, `reasoning_content` = thought | `stop` |
| Upstream `finish_reason: length` | no content, no `tool_calls` (the partial action is never parsed) | `length` |
| Unparsable output, or a tool that was not offered | Non-streaming: HTTP 502 with an OpenAI `error` body. Streaming: an in-band `error` event and no `[DONE]` | none |
| Upstream non-2xx or unreachable | HTTP 502 with the upstream status and a bounded excerpt of its error body; no retry | none |

Streaming order:

1. a role chunk;
2. `reasoning_content` deltas as the thought decodes;
3. the tool name as soon as the scanner commits to an offered tool;
4. the arguments, or the final answer as `content`, once the action parses;
5. a finish chunk, then `data: [DONE]`.

The final answer and the arguments are held until the action parses, so a
truncated completion never reaches the client as a partial answer. If a
completion is truncated after the tool name has streamed, its arguments are
never sent. An empty-delta heartbeat is sent only when upstream bytes have
arrived since the last chunk, so a wedged upstream still looks stale to the
client.

When the client disconnects, the upstream response is dropped and its
connection closes, which is how llama.cpp stops generating.
