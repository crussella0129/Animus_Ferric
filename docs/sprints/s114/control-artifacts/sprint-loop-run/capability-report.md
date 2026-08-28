# Sprint 114 Animus Sprint Loops capability report

## Verdict

Animus Ferric cannot yet use the pinned Animus Sprint Loops runtime-neutral
distribution as a native skill. The upstream Open Harnesses installer works,
but it installs operator helper scripts at the project root and provides no
Ferric-formatted `.ferric/skills/<name>/SKILL.md`. Consequently
`ferric skills list` reports no installed skills and no prompt-injection or
behavioral arm is authorized to run.

This is a packaging and orchestration boundary, not a Qwen3.8 model result.
The selected Qwen3.8 coordinate remains viable, but no model request was made
for this probe because T-11411 requires all downstream arms to stop after an
unmodified-adapter packaging failure.

## Pinned inputs

- Upstream repository: exact remote URL retained in the structured evidence
- Commit: `4acc1fd6e0b964ea4bcbedd17c44cb2ca8ca0066`
- Git tree: `3420c3d9858b6d3049b81f2334ca21a9d1fdaade`
- Open Harnesses source manifest: 52 files, SHA-256
  `07dfa0e4dd1713aea691e982f47fdc53f235bcac26ff0c85c688c4968c66fbb7`
- Installed helper manifest: 28 files, SHA-256
  `773525fbdf3b8ba0faaeda0ed81df86737e15da5d56c1a8496f3c68bc597da3c`
- Disposable workspace manifest: 28 files, SHA-256
  `cb96aa08d9d52dc445a80555ec5f5be3f9741ccd50d8a4917d3ca7f88573b690`
- Ferric binary: SHA-256
  `af75612b3498a1721e5b5f1b2f6309bf851d65b9bd13ad45e76cf8e370cf10f2`

The installed 28-script tree is byte-identical to the pinned source scripts,
and an idempotent reinstall leaves the isolated Git workspace clean.

## Capability matrix

| Capability layer | Observed result |
| --- | --- |
| Operator installation | Yes, helper scripts only |
| Ferric discovery | No |
| Explicit authorization | Not runnable after packaging failure |
| Top-level instruction injection | Not runnable after packaging failure |
| Native linked-resource access | Not runnable after packaging failure |
| Assisted resource access | Not runnable after packaging failure |
| Helper tool exposure | Not runnable after packaging failure |
| Book advancement with typed tools | Not runnable after packaging failure |
| Cross-run resume | Not runnable after packaging failure |
| `git_write` registration | Yes, statically registered at Ring 2 |
| `git_write` offered/attempted/succeeded | Not runnable after packaging failure |
| Native remote checkpoint | No native remote tool registered |

Ferric query statically registers `git_write` but does not register
`shell_exec` or `manage_task`; those are reserved for explicit human surfaces.
Thus installed shell helpers do not become model-callable merely by existing
in the workspace.

## Operator-only validation

Running the pinned `check-book.sh` in the disposable project correctly reports
that no Book is initialized. Running the pinned router returns
`uninitialized`. These results establish that the helpers themselves execute;
they do not establish native Ferric orchestration. No remote mutation was
attempted.

The structured verdict is SHA-256
`501e8494accd951262caccb9351e765bee6bfd3859a9897be42a9a33296754fe`.
The evidence manifest covers exactly 38 non-self evidence payloads: the
verdict, 34 command streams, and three manifests.

## Missing integration

A native path needs a Ferric-compatible top-level skill package, scoped loading
of the linked phase/schema/prompt resources, and an explicit orchestration
decision for deterministic helper execution and re-entry. Local Git and remote
checkpoint authority must remain separate capabilities rather than being
inferred from the presence of shell scripts.
