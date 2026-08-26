Finalized - DO NOT EDIT

# Sprint 18 Test Plan — Round out Ring 1

## Unit (`ferric-tools`, default CI)
- **`find_files`:** a tree with `config.toml`, `src/config.rs`, `notes.md` → `find_files {pattern:"config"}` returns both config paths (name-sorted), not `notes.md`; `path:"src"` scopes to the subtree; `max_results` caps; a `.git/` entry is skipped; empty `pattern` → error.
- **`copy_file`:** copies `a.txt` → `b/a.txt` (parent created, original kept); `copy_file` into `.ferric/` → `Denied`; a directory `from` → error.
- **`rings_gate_builtins_by_tier`:** Nano → exactly the 6 core (no `find_files`/`copy_file`/`search_files`/`move_path`); Small → **10** tools including `find_files` + `copy_file`.

## Build / Lint
- `cargo test --workspace` green; `clippy --workspace --all-targets -- -D warnings` clean; `fmt --check` clean.

## End-to-End — RUN it
- `ferric toolbench --backend openai --api-base http://localhost:11434/v1 --models qwen2.5-coder:7b,llama3.2:1b --protocol grammar --iterations 10 --calibrate-rings` → ring 1 now lists **8 tools** (was 6 at ring 0; +`search_files,move_path,find_files,copy_file`), and both models still calibrate to **`--max-ring 1` at solid** — proving the wider Ring 1 didn't cost reliability.

## Notes
- The builtin units + the rings-gate count are the AI-verifiable core; the ollama re-bench is the "growing the ring kept it 100%" confirmation.
