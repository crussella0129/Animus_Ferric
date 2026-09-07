# Sprint 125 — Research Report

## Sprint Goal

Make `ferric` a **deployable, run-from-anywhere** front door that finds models in
a **configured location** (decoupled from the working folder) and selects by the
owner's rule — **0 models → tell the user to drop a GGUF in the models folder;
1 → auto-pick; >1 → show the picker (with fit warnings)** — then starts the
engine and begins the session. **No auto-download** (owner's decision: acquiring
a GGUF stays the human's job). Advances INT-0008 (the unified human entry point).

This is the fix for the second human use test: `cargo r` had to run *inside the
repo* because model discovery only scans the current folder's `models/`, the
picker silently reused a stale saved 27B, and the PATH `ferric` was a months-old
`cargo install`.

## Existing Code Survey

- **Model discovery is hard-coupled to the working folder.** `startup/models.rs::scan(workspace, …)`
  → `DiscoveredDirectory::open(workspace)` canonicalizes the workspace and opens
  its `models` subdir under a cap-std confined root (`open_plain_directory(&root, Path::new("models"))`,
  then `root_path.join("models").join(name)`). So models are always
  `<cwd>/models` — which is exactly why the tool only works when run from the repo.
- **The picker silently reuses / auto-selects.** `human.rs::choose_model` returns
  `start.preferred_index` if a saved preference exists, and auto-selects when
  `models.len() == 1 && !requires_model_choice`, both **without listing**. A saved
  preference (`.ferric/startup-preference.json`) therefore skips the picker *and*
  the sprint-122 fit warning — so a too-big model is reused with no caution. The
  fit annotation + won't-fit confirm (`fit_annotation`, `wontfit_confirm_prompt`)
  only run on the interactive list path.
- **Config already supports what's needed.** `Config` (`config.rs:15`) has a
  layered project+user load (`load_layered`), a `profile_dir: Option<PathBuf>`
  field, and the codebase uses `FERRIC_*` env vars (`FERRIC_PROMPTS_DIR`,
  `FERRIC_LIVE_MODEL`, …). A `models_dir: Option<PathBuf>` config + a
  `FERRIC_MODELS_DIR` env fits the existing pattern exactly.
- **The workspace stays the project.** `human.rs::run` resolves `root =
  args.workspace | cwd`; that stays the file-work workspace (guarded against the
  ferric source tree already). Only *model discovery* moves to the configured dir.
- **Deploy is undocumented.** `welcome()` says "Run cargo r in a terminal"; there
  is no installed-binary story, and `docs/getting-started.md` assumes running in
  the repo. The PATH binary drifts because nobody re-runs `cargo install`.

## External Sources

- cap-std: opening the configured models directory as **its own** ambient-authority
  root (canonicalize → `Dir::open_ambient_dir`) keeps discovery confined to that
  directory only; discovery is read-only (enumerate + open GGUF leaves), so a
  models dir outside the workspace is a safe, bounded read boundary — not a write
  escape. Template hygiene (ADR-096): the path must come from config/env, never a
  hardcoded machine path in tracked source.

## Risks / Unknowns / Dependencies

- **Security boundary move.** Discovery's confined root moves from `<workspace>`
  to the resolved `models_dir`. It must stay read-only, canonicalized, symlink/
  non-file rejecting (the existing checks port over unchanged), and the default
  (`<workspace>/models`) must behave byte-identically to today so nothing
  regresses.
- **Backward compatibility.** With no `--models-dir` / `FERRIC_MODELS_DIR` /
  config, the resolved dir is `<workspace>/models` — the current behavior; the
  existing model-discovery tests must pass unchanged.
- **Picker-rule change is owner-specified.** Replace the silent `preferred_index`
  auto-select: >1 model always lists; 1 auto-picks (still surfacing its fit /
  won't-fit confirm); 0 gives an actionable "add a GGUF to `<models_dir>`" message
  naming the resolved directory. A saved preference becomes at most the default
  highlight, never a silent skip.
- **Template hygiene.** No machine path in source — `FERRIC_MODELS_DIR` / config
  is how the owner points at `Animus_Ferric/models`.

## Recommended Approach

1. **`models_dir` resolution** (new): `--models-dir` flag > `FERRIC_MODELS_DIR`
   env > config `models_dir` > default `<workspace>/models`. Resolve once in the
   run/startup path and thread it into discovery.
2. **Decouple discovery**: `DiscoveredDirectory::open` (and `scan`) take the
   resolved models directory and open **it** as the confined root (enumerate its
   GGUFs directly), preserving every existing safety check; the default path
   reproduces today's `<workspace>/models` behavior exactly.
3. **Picker rules** (`choose_model`): 0 → actionable message naming `<models_dir>`;
   1 → auto-pick with its fit surfaced; >1 → list with fit annotations; drop the
   silent saved-preference skip.
4. **Deploy**: document `cargo install --path crates/ferric-cli --force`; update
   `welcome()` + `docs/getting-started.md` to "install once, run `ferric` in your
   project folder, point `FERRIC_MODELS_DIR` at your GGUF folder" — and keep the
   ferric-source-tree Work guard.
5. Verify: existing model-discovery + human tests pass unchanged on the default;
   new tests cover the resolution order, the external-dir discovery, and the
   0/1/>1 picker outcomes; full workspace + feature shapes + aarch64 green.

## Intents Reviewed

- [INT-0008 — Unified local model workflow](../../intents/INT-0008-unified-local-model-workflow.md)
  — **selected** (active). This advances the human entry point: run-from-anywhere
  + configured model discovery + honest model selection, without the deferred
  acquisition clause of AC-13 (owner has ruled auto-download out of scope).

## Referenced Artifacts

- This report: `docs/sprints/s125/sprint-research/research-report.md`
- Direction plan (context): `docs/plans/2026-09-06-direction-and-refactor.md`
