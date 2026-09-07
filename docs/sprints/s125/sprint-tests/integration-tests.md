# Sprint 125 — Integration Test Results

Scope: discovery against a real external directory (decoupled from the working
folder) and the backward-compatibility proof that the default
`<workspace>/models` path is unchanged. Runner: `cargo test -p ferric-cli
--locked`.

## T-12501 — discovery confined to the resolved external directory

These exercise the real cap-std confinement over an on-disk `tempfile`
directory that is **not** a `<workspace>/models`, proving the run-from-anywhere
path enumerates GGUFs while every safety check ports over.

| Test | EARS clause | Assertion | Result |
|------|-------------|-----------|--------|
| `startup::models::tests::scan_dir_discovers_gguf_in_an_external_directory` | WHEN the resolved dir is outside the workspace THEN enumerate its GGUFs | A temp dir holding two `.gguf` files plus a `.txt` → `scan_dir` returns exactly the two models; the non-GGUF is ignored. | ok |
| `startup::models::tests::scan_dir_rejects_a_non_file_gguf_entry` | …AND still reject non-file entries | A directory named `a-directory.gguf` inside the resolved dir → `scan_dir(...).is_err()` (not admitted as a model). | ok |
| `startup::models::tests::scan_dir_rejects_a_symlinked_gguf_in_an_external_directory` | …AND still reject symlinks | A `link.gguf` symlink inside the resolved dir → `scan_dir(...).is_err()`, rejected at enumeration by the same `is_link` check (`scan_binding`, `models.rs:240`). | ok |

Negative paths (non-file and symlink) are both directly exercised, so the "reject
symlinks/non-file entries" half of the clause is proven for the external dir, not
inferred from the default path.

## Backward-compatibility proof (default `<workspace>/models`)

The historical `scan(workspace, explicit)` is now a thin
`scan_dir(&workspace.join("models"), explicit)` wrapper, so every pre-existing
`startup::models::tests` case runs unchanged on the default path. Confirmed
green in the same run:

- `automatic_symlink_root_is_refused_but_explicit_external_model_is_allowed`
- `discovered_models_directory_swap_cannot_admit_external_model`
- `discovered_binding_is_retained_for_later_model_validation`
- and the rest of the `startup::models` and `startup` suites.

This is the safety argument for the refactor: the external-dir path and the
default path share one enumeration + validation body (`scan_binding` →
`LocalModel::open_in`), so the same identity/symlink/non-file/entry-cap checks
apply to both. The default path opening `(workspace, "models")` is byte-for-byte
the prior behavior.

## Determinism

Every fixture is a `tempfile::tempdir()` created and torn down per test; no
shared directory, no clock or randomness. Symlink creation succeeds on the
Windows and Linux CI runners (the pre-existing `symlink_directory` tests already
rely on it), so the new file-symlink case is not a new platform dependency.
