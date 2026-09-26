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
