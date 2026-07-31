# Chatting, and `/do`

`ferric query` is one-shot. `ferric chat` is the interactive counterpart: a REPL
where you can talk with the model freely and, when you actually want it to *act*,
escalate a single turn into the constrained agentic loop with `/do`.

```sh
ferric chat
```

## Two kinds of turn, one conversation

Chat deliberately separates **talking** from **doing**, and the separation is
structural, not a matter of prompting:

- **Talk (the default).** Anything you type is a plain, unconstrained completion:
  the model replies as text. This path has **no tools and no constraint** — its
  output is printed and added to the conversation history, and that is all. It is
  never parsed for a tool call and never dispatched. The model literally cannot
  act from a talk turn, because a talk turn does not go through the dispatching
  loop.

- **Do (`/do <request>`).** Prefixing a message with `/do` promotes *that one
  turn* into the same constrained agentic loop `ferric query` runs — tools, the
  guard, the loop-hardening guards, and a fresh trace file — seeded with the
  conversation so far as context. When it finishes, you are back in talk mode.

This is why chat is safe by default: **only you** can escalate into acting. The
model can suggest, explain, and plan in talk mode, but it can never promote itself
into doing something. That boundary is a deliberate security property (a model
that could self-escalate into tool use would be a model that could act on
injected instructions).

## A session, end to end

```
you> what does the loop guard family protect against?
model> (a plain explanation — no tools ran)

you> /do add a doc-comment to ProgressGuard summarizing that
▸ calling read_file...
✓ read_file: crates/ferric-loop/src/progress.rs
▸ calling edit_file...
✓ edit_file: added the doc comment
[task_complete after 2 turn(s); trace: .ferric/trace/q-...jsonl]

you> thanks
model> (talk again)
```

Each `/do` writes its **own** agentic trace file, exactly as a standalone
`ferric query` would; the talk turns are logged separately as a chat-session log.

## REPL commands

| Command | Effect |
|---|---|
| `/do <request>` | escalate this turn into the constrained agentic loop |
| `/help` | list the available commands |
| `/exit` or `/quit` | leave the REPL |

## Launch-time-fixed settings

Like `ferric mcp`, chat fixes its workspace, model, and protocol **once** at
launch — they are ordinary CLI flags held for the whole session, not something a
turn can change mid-conversation. That, again, is containment by construction: no
turn can redirect the workspace or swap the model, because there is no channel for
it to do so.

---

Next: [Loading a Skill](basics-skills.md) — giving the model a reusable
instruction set for a run.
