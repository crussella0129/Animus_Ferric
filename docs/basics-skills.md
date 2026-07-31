# Loading a Skill

A **skill** is a reusable instruction set — a named block of guidance you can pull
into a run's system prompt on demand, instead of retyping it or baking it into
every prompt. Skills live per-workspace under `.ferric/skills/`.

## Seeing what's installed

```sh
ferric skills list
```

This lists every skill in `.ferric/skills/` and shows how each one would be
authorized — whether it is standing-authorized for the workspace, or must be named
explicitly per run.

## Using a skill for a run

Name the skill on the invocation with `--skill` (repeatable):

```sh
ferric query "refactor the parser" --skill rust-review --skill terse-commits
```

**Naming the skill *is* the authorization.** It is you, on this invocation, asking
for that guidance to be in scope. This matters: a skill sitting in
`.ferric/skills/` is visible to `ferric skills list` but contributes *nothing* to
a prompt until you say so. There is no channel through which the model can enable
a skill for itself — the authorization only ever comes from your flags or your
config, never from anything the model emits.

If you name a skill that isn't installed, Ferric tells you rather than silently
running without it:

```
skill: no skill named `rust-reviw` is installed in .ferric/skills/
```

(A typo that quietly runs nothing is exactly the failure this guard prevents.)

## Standing authorization

If you always want certain skills available without naming them every time, list
them in `allowed_skills` in `.ferric/config.toml`:

```toml
allowed_skills = ["rust-review", "terse-commits"]
```

A project allowlist **replaces** the user's rather than extending it, so a repo
cannot silently inherit skills you enabled globally. And because config lives
under `.ferric/` — which the guard write-denies to the model — the allowlist is
something only *you* can edit. An allowlist the agent could rewrite would authorize
nothing.

## Why skills are a security surface, not just a convenience

Everything above is the same principle in a different place: authorization flows
in one direction, from you to the run, and never back from the model. A skill is
just trusted instructions you chose to add — like the project's `Animus.md` file,
and unlike anything Ornstein retrieves, which is quarantined as untrusted data.

---

That's the basics. From here, the [Advanced Features](advanced-tool-rings.md)
part covers tool rings, research, delegation, MCP, and more.
