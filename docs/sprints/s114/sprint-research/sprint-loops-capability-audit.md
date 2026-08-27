# Sprint 114 Animus Sprint Loops Capability Audit

## Research verdict

Ferric can discover and inject one operator-installed, Ferric-formatted
top-level skill. It cannot yet be called an autonomous Book-v2 Sprint Loops
runtime. An operator-supervised Legacy-policy experiment in a disposable
workspace is feasible and will be performed; the full-loop claim remains
unproven until the live test.

The upstream repository's statement that it is a drop-in skill for Ferric is
treated as a packaging hypothesis, not execution authority or proof.

## Capability layers

| Layer | Research status | Reason |
| --- | --- | --- |
| Install/discover | Partial | Only `.ferric/skills/<name>/SKILL.md` is discovered; the operator must install it, frontmatter must parse, and the directory/name must match. |
| Explicit authorization | Supported | `--skill <name>` or an operator-owned allowlist injects the selected body; the model cannot authorize itself. |
| Linked resources | Unsupported natively | Ferric injects the selected `SKILL.md` body only. It has no scoped loader for phase files or other resources under `.ferric`. |
| Nested/re-entrant skill calls | Unsupported | There is no model-callable skill loader or scheduler. A new operator invocation is required for another phase. |
| Router/helper scripts | Unsupported to the model | `shell_exec` and `manage_task` are registered only on explicit human surfaces; `ferric query` registers neither at any ring. The model has no native skill-resource identity or script executor. |
| Book writes | Partial | Ordinary guarded file tools can write `docs/`, but the model cannot execute the distribution's router or mutation helpers. Operator-materialized phase instructions may test manual typed-tool advancement, which is not native helper support. |
| Local Git | Partial | Native `git_write` permits add, commit, checkout, and branch; it does not provide fetch, merge, push, or PR operations. |
| Remote checkpoint | Unsupported natively | Push and provider-aware PR creation require separately authorized host shell commands and credentials; Ferric does not enforce the Sprint Loops remote profile. |
| Cross-run continuation | Manual/partial | Book state persists on disk, but the operator must invoke Ferric again and re-authorize the skill for each phase. |

## Minimum live test

1. Pin the upstream commit and install only the open-harness adapter in a
   disposable Git workspace; record the tree and file hashes.
2. Record exact `ferric skills list` discovery or the exact unmodified
   adapter parse/name/layout failure. On packaging failure, behavioral arms are
   `not-runnable-after-packaging-failure`; registry and remote-boundary facts
   still complete.
3. Run identical-prompt/model/seed `--no-config` arms without and with
   `--skill sprint-loop`; require absent/present captured CLI diagnostics,
   absent/present exact skill content in `SessionPrompt.system`, and a unique
   content marker. Do not use pre-skill `PromptComposed` as injection proof.
4. In the authorized arm, first require the skill to resolve its linked phase
   and router with no resource path, environment shim, or operator hint. Score
   this native-resolution attempt before a separately labeled operator-
   materialized readable-resource arm.
5. Treat source registration as the static expectation that `shell_exec` and
   `manage_task` are absent, but do not infer runtime inventory from trace
   silence. Ask for the exact read-only router and mutation helper and record
   inability to execute rather than inventing a controller refusal.
6. Use an operator-owned capture stub to retain/hash the actual provider
   request and constrained schema for explicit-Ultra Evidence/Ring-1 and
   Legacy/Ring-1. Then,
   for a separately labeled assisted arm, copy only the required phase resource
   byte-for-byte from the pinned tree to an ordinary readable workspace path,
   record both hashes and the path hint, and run one explicit-Ultra
   Legacy/Ring-1 typed-tool Book attempt without a helper executor.
7. Independently, outside the model run, execute `check-book.sh` and the router
   from that same pinned tree at the disposable project root. Require no
   writable legacy/split-brain state and router output supported by actual
   phase-exit artifacts. This is operator validation, not Ferric execution.
8. Repeat a fresh explicitly authorized query for any subsequent phase; never
   claim nested invocation. Under separate explicit-Ultra Evidence/Ring-2 and
   Legacy/Ring-2 arms, score `git_write` as registered, offered, attempted, and
   succeeded; no shell-helper Git path exists on the model surface.
9. Stop before remote mutation. Record provider-aware push/PR as unsupported
   unless separately implemented; an ad hoc approved `gh` shell command is not
   native Sprint Loops support.

Final results will use the labels `discovered`, `authorized`, `resource
accessible natively`, `resource accessible after operator materialization`,
`helper tool exposed`, `Book advanced with typed tools`, `Book operator-
validated`, `cross-run resumed`, `git_write registered`, `git_write offered`,
`git_write attempted`, `git_write succeeded`, and `remote checkpoint`
independently. If Qwen3.8 is non-viable, behavioral arms use only the existing
7B after exact hash reverification and a fresh smoke; failure produces
`fallback_control_unavailable`, never a substitute download.

Source: <https://github.com/crussella0129/Animus_Sprint_Loops>
