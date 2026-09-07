# Sprint 125 Test Plan

## Intent Traceability
| Intent | Acceptance criterion | Build task / EARS clause | Verification |
|--------|----------------------|--------------------------|--------------|
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | run-from-anywhere: configured discovery, default preserved | T-12501 / WHEN no config THEN `<workspace>/models`; WHEN set THEN precedence | `resolve_models_dir_precedence`, existing `startup/models.rs` tests |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | discovery confined to the resolved external dir | T-12501 / WHEN external dir THEN enumerate + reject symlink/non-file | `discovers_gguf_in_external_dir`, `rejects_symlink_in_external_dir` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | honest 0/1/>1 selection | T-12502 / WHEN 0 / 1 / >1 THEN message / auto / picker | `no_model_names_the_dir`, `single_model_auto_picks`, `multiple_models_always_list` |
| [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md) | documented entry point is install+run-anywhere | T-12503 / WHEN docs read THEN install/project/FERRIC_MODELS_DIR flow | `getting_started_describes_installed_flow` (doc-content check) |

## Unit Tests
### T-12501
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `resolve_models_dir_precedence`: flag > `FERRIC_MODELS_DIR` > config > default `<workspace>/models`, each layer overriding the ones below.
- Stubs: an injected env accessor (the `user_config_path_from`-style `&impl Fn(&str)->Option<String>` pattern) so no real env is mutated.

### T-12502
- **Intent:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `no_model_names_the_dir`: 0 GGUFs → error/message containing the resolved directory path.
- `single_model_auto_picks`: 1 GGUF → selected index 0, no picker prompt.
- `multiple_models_always_list`: 2 GGUFs (with a saved preference present) → the picker is shown (each line carries a fit annotation) and a choice is required.
- Stubs: existing `ScriptedIo` + an injected model list / `Startup`-shaped input.

## Integration Tests
### Discovery against a real external directory
- **Intents:** [INT-0008](../../../intents/INT-0008-unified-local-model-workflow.md)
- `discovers_gguf_in_external_dir`: a `tempfile` dir (outside the workspace) holding `a.gguf`/`b.gguf` → both discovered when it is the resolved models dir.
- `rejects_symlink_in_external_dir`: a symlinked GGUF in that dir is still rejected (the safety check ported over).
- The existing `startup/models.rs` discovery tests run unchanged on the default `<workspace>/models` path — the backward-compat proof.

## End-to-End Tests
- **Status:** possible
- `models_dir_env_end_to_end`: with `FERRIC_MODELS_DIR` set to a temp dir of two fixture GGUFs, a scripted run reaches the picker; with one GGUF it auto-picks; with zero it prints the add-a-GGUF message naming the dir. Workspace (file work) stays the run folder, distinct from the models dir.
- Behavior preservation: full `cargo test --workspace` green, plus the `lifecycle-fixture` / `no-default-features` shapes and the aarch64 `cargo check`.
