# Why Animus?

**Animus Ferric** is a local-first, agentic coding harness built for *small*
models — the kind that run on a laptop, a Jetson, or a Raspberry Pi, with no API
key and no data leaving the machine. This book is its manual, and also, by
design, a record of where it is going (see [How this book works](#how-this-book-works)
below).

Before you invest in it, the honest question is: **why this, and not something
bigger?**

## When Animus is the right tool — and when it isn't

There is a class of coding agents you already know: large, cloud-hosted harnesses
driving frontier models through an API. They are excellent. For what they solve,
they are genuinely more sophisticated than Animus, and this book will not pretend
otherwise. If you have a capable model and the budget to call it, reach for one
of those.

Animus is built for the *other* situation — and the two do not substitute for
each other in either direction:

- **A big harness would crush a small model.** The large agents assume a model
  that can hold a sprawling context, recover from its own malformed output, and
  reason across many loosely-specified tool calls. Hand that machinery a 1B model
  and it flails. Animus assumes the opposite: the model is small and will not
  reliably emit a correct tool call on its own — so the *harness* guarantees
  correctness instead (see [harness-owned decoding](#the-thesis-the-harness-owns-decoding)).

- **Animus is not the token-efficient choice for a big model.** The same
  guardrails that make a 1B model usable — constrained grammars, tight tool
  rings, structural containment — are overhead a frontier model does not need and
  would arguably be *slower* and more token-hungry running under them than under a
  harness designed around its strengths. The larger agents appear to run as
  ReAct-style loops operating at the level of the operating system itself; that is
  a fundamentally different design, and (we should be candid) one Animus has not
  benchmarked head-to-head. Different tools, different problems.

So: choose Animus when the model is small, the machine is yours, the data must
stay local, or the target is the edge. Choose a bigger agent when you have a
capable model, an API budget, and a task that rewards raw model sophistication.
Neither choice is a compromise; they are answers to different questions.

## The philosophy

The Animus Project operates on a single principle:

> **The simplest, most efficient design *is* the most sophisticated one.**

This is not a concession — "small, therefore humble." It is the claim itself.
Sophistication is not measured in parameters or in the sprawl of the machinery
around a model. It is measured in how much reliable capability you extract per
unit of complexity. A harness that makes a 1B model complete real multi-turn
tasks — by *removing* the ways it can fail rather than by piling on more
inference — is doing the more sophisticated thing, not the lesser one.

### Spine thinkers

A useful picture. Dinosaurs had famously tiny brains, yet they operated enormous,
capable bodies with precision. The work was not all done in the head: their
massive, elaborate spinal architecture carried an enormous share of the
coordination. They were, in a sense, *spine thinkers*.

Animus is built on the same division of labor. **The model is the brain; the
harness is the spine.** The brain supplies intent — what to do next. The spine
supplies structure, reflex, and safety — it constrains what a malformed impulse
can become, it holds the body's boundaries, it turns a small signal into a
reliable action. Give a modest brain a sophisticated enough spine and it drives a
body far larger than its size would suggest.

Everything in this book is, ultimately, spine.

## The thesis: the harness owns decoding

Concretely, the spine's central mechanism is **harness-owned decoding**. Rather
than trusting the model to emit a well-formed tool call and hoping for the best,
Animus constrains generation to a JSON grammar it controls, enforced server-side.
The model cannot emit an invalid action because the invalid tokens are never
offered to it. This is what extends the usable floor down to a 1B model, where an
unconstrained model's own tool-calling collapses.

From that one idea the rest follows: [tool rings](advanced-tool-rings.md) that
widen only as a model *proves* it can drive them; a [structural
quarantine](ornstein.md) that lets untrusted research reach the model as data but
never as an action; [containment](icm.md) that lives in the filesystem rather than
a coordination framework. Small pieces, each doing one thing, each removing a way
to fail.

## How this book works

This book is built with [mdBook](https://rust-lang.github.io/mdBook/), the same
tool behind *The Rust Programming Language*. Its source is this repository's
`docs/` folder, so every page is also browsable directly on GitHub.

It serves two purposes at once, and it tells you which is which:

- **Documentation of what exists.** Most chapters describe shipped, tested
  behavior. Commands shown here work today.
- **A record of stated intent.** Some sections — most explicitly [Animus Swarming
  with K8s](swarming-k8s.md), and parts of [Animus By Example](by-example.md) —
  describe where the project is *going*. These are marked clearly as intent, not
  as current fact. The book is meant to be the project's durable statement of
  direction, the place a future sprint reads to know what it was asked to build.

Where a chapter mixes the two, it says so inline.

## Where to go next

- New here? [Installation & First Run](getting-started.md) gets you from `git
  clone` to a first query.
- Want the basics? [Your First Query](basics-query.md), [Chatting and
  `/do`](basics-chat.md), and [Loading a Skill](basics-skills.md).
- Ready to go deep? The [Advanced Features](advanced-tool-rings.md) part is the
  long one.
- Just need a flag? The [Command Reference](commands.md).
