# Artifact: oovra Integration Spec (s2)

> Source: Explore agent over C:\Users\charl\oovra, 2026-06-12. Feeds the build plan directly.

## Verdict
Clean lib/bin split — Ferric depends on the root `oovra` crate (0.2.0, edition 2021) without pulling the GUI workspace member. Lib deps (serde, toml, walkdir, clap, anyhow, thiserror, semver, similar, owo-colors, chrono(no-default+clock,serde), serde_json) are all aarch64-clean. License MIT OR Apache-2.0 (matches Ferric).

## Dependency strategy
NOT on crates.io (publication archived for v0.3). Remote exists and local is clean/pushed (branch `feat/create-redesign-and-gui-bootstrap`; main available). **s2: git dependency** — `oovra = { git = "https://github.com/crussella0129/oovra", branch = "main" }` (Cargo.lock pins the commit; CI can clone it; swap to crates.io at oovra v0.3). VERIFY which branch holds the current lib API before pinning (local checkout is on the feature branch — if the lib API Ferric needs is only there, pin that branch or merge to main first).

## Public lib API (exact)
- `Library::load(root: &Path) -> Result<Library>` (+ `load_with(root, ParseOptions)`); `Library { root, elements: HashMap<id, PromptElement> }`; `get(&self, id)`, `roots()`, `descendants(id)`, `leaf_atoms(id)`, `component_tree()`.
- Compose: `render::compose(ComposeRequest { library, inputs: Vec<(String, Option<String>)> /* id + version pin */, output_id, output_name, output_version, output_meta }) -> Result<PromptElement>`.
- **`render::render_text(&[&PromptElement]) -> Result<String>`** — clean prose (recursively flattens to atoms, H2-wraps by id) — this is what goes to the model.
- `PromptElement { header: PromptElementHeader, body, source_path }`; header: name/kind(atom|compound)/id(kebab)/version(semver)/meta + compound-only generated_at/render_mode/body_level/depth/composed_of(Vec<InputRef{id,version}>).
- Errors: rich `OovraError` enum (DuplicateId, ElementNotFound, VersionMismatch, ...).

## Element format
`+++` TOML frontmatter `+++` blank line, markdown body. Atoms hand-authored; compounds produced by compose with chiral tilde delimiters (level N = N+1 tildes + >>/<<), losslessly decomposable.

## Ferric integration shape (new crate `ferric-prompt`)
- `prompts/` element library in-repo (one .md per atom: role-declaration, tool-protocol-native, tool-protocol-unified-grammar, terminator-teaching, workspace-rules, ...).
- `recipe_for(tier, protocol) -> Vec<(id, Option<version>)>` (hardcoded matrix first).
- `compose_system_prompt(lib, tier, protocol) -> Result<String>` via compose + render_text.
- Genealogy → trace: composition lineage (id+version pairs) recorded in a trace event (extend PromptAssembled or a new PromptComposed event — additive per ADR-002).

## Load-bearing files
src/library.rs (436), src/element.rs (627), src/header.rs (531), src/render.rs (252), src/main.rs (~200, CLI — unused by Ferric).
