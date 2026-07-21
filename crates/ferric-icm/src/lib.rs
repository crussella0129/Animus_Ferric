//! `ferric-icm` — Interpretable Context Methodology (ICM): filesystem as agent
//! architecture (ADR-064, sprint 73).
//!
//! ICM (Van Clief & McDermott, 2026) replaces framework-level orchestration
//! with **folder structure**. Numbered stage folders (`stages/01_research/`,
//! `stages/02_script/`, …) encode execution order; each stage's `CONTEXT.md`
//! is a *contract* (Inputs / Process / Outputs); a five-layer context hierarchy
//! scopes exactly what each stage sees:
//!
//! - **Layer 0** — root identity (`Animus.md`, or `CLAUDE.md`): "where am I?"
//! - **Layer 1** — workspace routing (`CONTEXT.md`): "where do I go?"
//! - **Layer 2** — the stage contract (`stages/NN_*/CONTEXT.md`): "what do I do?"
//! - **Layer 3** — reference material (`references/`, `_config/`), stable across
//!   runs — the *factory*: internalized as constraints.
//! - **Layer 4** — working artifacts (a prior stage's `output/`), per-run — the
//!   *product*: processed as input.
//!
//! One agent runs every stage; the folder structure IS the delegation logic
//! (the same files are the human's control surface and the model's orchestration
//! spec). This is Ferric-native: each stage is a workspace-scoped run, so
//! `ferric-guard`'s containment applies unchanged — every referenced path is
//! resolved through the workspace boundary, so a contract can never pull context
//! from outside the workspace.
//!
//! **Increment 1 (this crate)** is the pure model: discover a workspace, parse
//! contracts, compose each stage's scoped context (guard-checked) into an
//! [`OrchestrationPlan`], and scaffold a new workspace skeleton (LLM-free,
//! refuse-to-clobber). Feeding each composed stage prompt into the constrained
//! loop with human review gates is increment 2 — the plan produced here is
//! exactly what inc 2 will execute.

use std::path::{Path, PathBuf};

use ferric_guard::Workspace;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IcmError {
    #[error("not an ICM workspace: {0} (expected a `stages/` directory)")]
    NotAWorkspace(PathBuf),
    #[error("workspace has no stages under {0}/stages")]
    NoStages(PathBuf),
    #[error("stage index {0} out of range (workspace has {1} stages)")]
    StageOutOfRange(usize, usize),
    #[error("refuse to clobber: {0} exists and is not an empty directory")]
    TargetNotEmpty(PathBuf),
    #[error("a stage input escapes the workspace boundary: {0}")]
    Boundary(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Which content layer a stage input belongs to. Layer 3 is stable reference
/// material (the factory); Layer 4 is per-run working artifacts (the product).
/// They warrant different attention: reference is internalized as constraints,
/// working material is processed as input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Layer 3 — reference material, stable across runs.
    Reference,
    /// Layer 4 — working artifacts, per-run.
    Working,
}

impl Layer {
    fn number(self) -> u8 {
        match self {
            Layer::Reference => 3,
            Layer::Working => 4,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Layer::Reference => "Layer 3 (reference)",
            Layer::Working => "Layer 4 (working)",
        }
    }
}

/// One declared input in a stage contract's `## Inputs` table. `path` is
/// relative to the stage's own folder (matching the paper's `../01_research/
/// output/`, `references/structure.md`, `../../_config/voice.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRef {
    pub layer: Layer,
    pub path: String,
}

/// One declared output in a stage contract's `## Outputs` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRef {
    /// The artifact filename the stage produces, e.g. `script_draft.md`.
    pub name: String,
    /// The destination folder, e.g. `output/`.
    pub dest: String,
}

/// A stage's parsed contract: what it reads, what it does, what it writes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageContract {
    pub inputs: Vec<InputRef>,
    pub process: String,
    pub outputs: Vec<OutputRef>,
}

/// Parse a stage `CONTEXT.md` into a contract. Tolerant by design (the file is a
/// human edit surface): recognizes `## Inputs` / `## Process` / `## Outputs`
/// sections case-insensitively, classifies each input bullet by a
/// `Layer 3`/`reference` vs `Layer 4`/`working` marker, and treats the process
/// section as free prose. Unknown sections are ignored; a missing section yields
/// an empty part rather than an error.
pub fn parse_contract(md: &str) -> StageContract {
    #[derive(PartialEq)]
    enum Section {
        None,
        Inputs,
        Process,
        Outputs,
    }
    let mut section = Section::None;
    let mut contract = StageContract::default();
    let mut process_lines: Vec<String> = Vec::new();

    for raw in md.lines() {
        let line = raw.trim();
        if let Some(heading) = line.strip_prefix("##") {
            let h = heading.trim().to_ascii_lowercase();
            section = if h.starts_with("input") {
                Section::Inputs
            } else if h.starts_with("process") {
                Section::Process
            } else if h.starts_with("output") {
                Section::Outputs
            } else {
                Section::None
            };
            continue;
        }
        match section {
            Section::Inputs => {
                if let Some(item) = bullet_body(line)
                    && let Some(input) = parse_input_bullet(&item)
                {
                    contract.inputs.push(input);
                }
            }
            Section::Outputs => {
                if let Some(item) = bullet_body(line)
                    && let Some(output) = parse_output_bullet(&item)
                {
                    contract.outputs.push(output);
                }
            }
            Section::Process => {
                if !line.is_empty() {
                    process_lines.push(line.to_string());
                }
            }
            Section::None => {}
        }
    }
    contract.process = process_lines.join("\n");
    contract
}

/// Strip a `-` / `*` list marker, returning the bullet body, or `None` for a
/// non-bullet line.
fn bullet_body(line: &str) -> Option<String> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .map(|s| s.trim().to_string())
}

/// Parse an input bullet like `Layer 4 (working): ../01_research/output/` or
/// `Layer 3 (reference): references/structure.md`. The path is everything after
/// the final colon; the layer is inferred from a `3`/`reference` vs
/// `4`/`working` marker before it.
fn parse_input_bullet(item: &str) -> Option<InputRef> {
    // A `tag: path` bullet splits on the final colon; a bare bullet (no colon)
    // is treated entirely as the path, unlabeled.
    let (tag, path) = item.rsplit_once(':').unwrap_or(("", item));
    let path = path.trim().trim_matches('`').trim();
    if path.is_empty() {
        return None;
    }
    let tag_l = tag.to_ascii_lowercase();
    let layer = if tag_l.contains("layer 4") || tag_l.contains("working") {
        Layer::Working
    } else if tag_l.contains("layer 3") || tag_l.contains("reference") {
        Layer::Reference
    } else {
        // Unlabeled: default to reference (the conservative "stable rule" read).
        Layer::Reference
    };
    Some(InputRef {
        layer,
        path: path.to_string(),
    })
}

/// Parse an output bullet like `script_draft.md -> output/`. A bullet with no
/// `->` treats the whole body as the name and `output/` as the destination.
fn parse_output_bullet(item: &str) -> Option<OutputRef> {
    let item = item.trim().trim_matches('`');
    if item.is_empty() {
        return None;
    }
    if let Some((name, dest)) = item.split_once("->") {
        Some(OutputRef {
            name: name.trim().trim_matches('`').trim().to_string(),
            dest: dest.trim().trim_matches('`').trim().to_string(),
        })
    } else {
        Some(OutputRef {
            name: item.trim().to_string(),
            dest: "output/".to_string(),
        })
    }
}

/// One discovered stage: its numeric order, folder name, directory, and parsed
/// contract.
#[derive(Debug, Clone)]
pub struct Stage {
    pub index: u32,
    pub name: String,
    pub dir: PathBuf,
    pub contract: StageContract,
}

/// A discovered ICM workspace.
#[derive(Debug, Clone)]
pub struct IcmWorkspace {
    pub root: PathBuf,
    /// Layer 0 identity text (`Animus.md` preferred, else `CLAUDE.md`).
    pub identity: Option<String>,
    /// The filename Layer 0 was read from, for provenance.
    pub identity_file: Option<String>,
    /// Layer 1 routing text (root `CONTEXT.md`).
    pub routing: Option<String>,
    pub stages: Vec<Stage>,
}

impl IcmWorkspace {
    /// Discover the workspace rooted at `root`: read Layer 0/1, then enumerate
    /// `stages/NN_*/` folders in numeric order, parsing each `CONTEXT.md`.
    pub fn discover(root: &Path) -> Result<Self, IcmError> {
        let stages_dir = root.join("stages");
        if !stages_dir.is_dir() {
            return Err(IcmError::NotAWorkspace(root.to_path_buf()));
        }

        // Layer 0: Animus.md (Ferric-native, ADR-048) preferred, else CLAUDE.md.
        let (identity, identity_file) = ["Animus.md", "CLAUDE.md"]
            .iter()
            .find_map(|name| {
                let p = root.join(name);
                read_if_present(&p).map(|text| (Some(text), Some((*name).to_string())))
            })
            .unwrap_or((None, None));

        let routing = read_if_present(&root.join("CONTEXT.md"));

        let mut entries: Vec<(u32, String, PathBuf)> = Vec::new();
        for entry in read_dir(&stages_dir)? {
            let entry = entry.map_err(|e| IcmError::Io {
                path: stages_dir.clone(),
                source: e,
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(index) = stage_index(&name) {
                entries.push((index, name, path));
            }
        }
        // Numeric order is execution order; break ties by name for determinism.
        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        if entries.is_empty() {
            return Err(IcmError::NoStages(root.to_path_buf()));
        }

        let mut stages = Vec::with_capacity(entries.len());
        for (index, name, dir) in entries {
            let contract = read_if_present(&dir.join("CONTEXT.md"))
                .map(|md| parse_contract(&md))
                .unwrap_or_default();
            stages.push(Stage {
                index,
                name,
                dir,
                contract,
            });
        }

        Ok(IcmWorkspace {
            root: root.to_path_buf(),
            identity,
            identity_file,
            routing,
            stages,
        })
    }
}

/// Parse a leading numeric prefix from a stage folder name (`01_research` → 1,
/// `2-script` → 2). Returns `None` when there is no leading number.
fn stage_index(name: &str) -> Option<u32> {
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// One source folded into a stage's composed context, recorded for inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Context layer 0–4.
    pub layer: u8,
    /// Human label, e.g. `Layer 3 (reference)`.
    pub label: String,
    /// Source path relative to the workspace root, or a synthetic name.
    pub source: String,
    pub bytes: usize,
    /// `false` when a declared input file/dir is absent (recorded, not fatal).
    pub present: bool,
}

/// A stage's composed, scoped context: the prompt an agent would receive, plus
/// the genealogy of every layer that built it (mirrors `ferric-prompt`'s
/// text-plus-lineage shape).
#[derive(Debug, Clone)]
pub struct ComposedStage {
    pub index: u32,
    pub name: String,
    pub prompt: String,
    pub provenance: Vec<Provenance>,
}

/// The full delegation plan: one composed context per stage, in execution order.
#[derive(Debug, Clone)]
pub struct OrchestrationPlan {
    pub stages: Vec<ComposedStage>,
}

/// Compose the scoped context for a single stage (0-based `index`). Assembles
/// Layers 0–2 (identity, routing, contract) followed by the contract's declared
/// Layer 3/4 inputs, each resolved through the workspace boundary so nothing
/// outside the workspace can be pulled in. A declared input that is missing is
/// recorded in provenance (`present: false`) rather than erroring, so a
/// scaffolded-but-not-yet-run workspace still plans cleanly.
pub fn compose_stage(ws: &IcmWorkspace, index: usize) -> Result<ComposedStage, IcmError> {
    let stage = ws
        .stages
        .get(index)
        .ok_or(IcmError::StageOutOfRange(index, ws.stages.len()))?;
    let guard = Workspace::new(&ws.root).map_err(|_| IcmError::NotAWorkspace(ws.root.clone()))?;

    let mut prompt = String::new();
    let mut provenance = Vec::new();

    prompt.push_str(&format!("# ICM stage {}: {}\n", stage.index, stage.name));

    // Layer 0 — identity.
    if let (Some(text), Some(file)) = (&ws.identity, &ws.identity_file) {
        section(&mut prompt, "Identity (Layer 0)", text);
        provenance.push(Provenance {
            layer: 0,
            label: "Layer 0 (identity)".into(),
            source: file.clone(),
            bytes: text.len(),
            present: true,
        });
    }

    // Layer 1 — workspace routing.
    if let Some(text) = &ws.routing {
        section(&mut prompt, "Workspace routing (Layer 1)", text);
        provenance.push(Provenance {
            layer: 1,
            label: "Layer 1 (routing)".into(),
            source: "CONTEXT.md".into(),
            bytes: text.len(),
            present: true,
        });
    }

    // Layer 2 — the stage contract (its process prose).
    let contract_src = rel_to_root(&ws.root, &stage.dir.join("CONTEXT.md"));
    if !stage.contract.process.is_empty() {
        section(
            &mut prompt,
            "Stage contract (Layer 2)",
            &stage.contract.process,
        );
    }
    provenance.push(Provenance {
        layer: 2,
        label: "Layer 2 (contract)".into(),
        source: contract_src,
        bytes: stage.contract.process.len(),
        present: true,
    });

    // Layers 3 & 4 — the contract's declared inputs, boundary-checked.
    for input in &stage.contract.inputs {
        // Inputs are relative to the stage's own folder.
        let candidate = stage.dir.join(&input.path);
        let resolved = match guard.resolve(&candidate) {
            Ok(p) => p,
            Err(_) => return Err(IcmError::Boundary(input.path.clone())),
        };
        for (src, content, present) in read_input(&resolved)? {
            let rel = rel_to_root(&ws.root, &src);
            let header = format!("{}: {}", input.layer.label(), rel);
            if present {
                section(&mut prompt, &header, &content);
            }
            provenance.push(Provenance {
                layer: input.layer.number(),
                label: input.layer.label().into(),
                source: rel,
                bytes: content.len(),
                present,
            });
        }
    }

    // The output contract: tell the agent exactly where to write its
    // deliverable(s). Live execution (inc 2) runs each stage contained to its
    // own folder, so `output/` resolves inside the stage — the agent produces
    // its product there and the next stage reads it as Layer 4.
    if !stage.contract.outputs.is_empty() {
        let mut out = String::new();
        for o in &stage.contract.outputs {
            out.push_str(&format!("- Write `{}` to `{}`\n", o.name, o.dest));
        }
        section(&mut prompt, "Outputs (write your deliverables here)", &out);
    }

    Ok(ComposedStage {
        index: stage.index,
        name: stage.name.clone(),
        prompt,
        provenance,
    })
}

/// Compose every stage in execution order into a full [`OrchestrationPlan`].
pub fn plan(ws: &IcmWorkspace) -> Result<OrchestrationPlan, IcmError> {
    let stages = (0..ws.stages.len())
        .map(|i| compose_stage(ws, i))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OrchestrationPlan { stages })
}

/// Read a resolved input path into `(source, content, present)` tuples. A file
/// yields one tuple; a directory yields one per contained file (sorted, one
/// level deep) — the Layer-4 `output/` handoff. A missing path yields a single
/// `present: false` marker so the gap is visible in provenance.
fn read_input(resolved: &Path) -> Result<Vec<(PathBuf, String, bool)>, IcmError> {
    if !resolved.exists() {
        return Ok(vec![(resolved.to_path_buf(), String::new(), false)]);
    }
    if resolved.is_dir() {
        let mut files: Vec<PathBuf> = read_dir(resolved)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            // Skip hidden placeholders (`.gitkeep` and friends): they hold empty
            // scaffold dirs open in git but are not working artifacts.
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| !n.starts_with('.'))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        if files.is_empty() {
            return Ok(vec![(resolved.to_path_buf(), String::new(), false)]);
        }
        let mut out = Vec::with_capacity(files.len());
        for f in files {
            let content = read_file(&f)?;
            out.push((f, content, true));
        }
        Ok(out)
    } else {
        let content = read_file(resolved)?;
        Ok(vec![(resolved.to_path_buf(), content, true)])
    }
}

/// A `## header` + body block appended to the composed prompt.
fn section(prompt: &mut String, header: &str, body: &str) {
    prompt.push_str("\n## ");
    prompt.push_str(header);
    prompt.push('\n');
    prompt.push_str(body.trim_end());
    prompt.push('\n');
}

fn rel_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_if_present(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn read_file(path: &Path) -> Result<String, IcmError> {
    std::fs::read_to_string(path).map_err(|e| IcmError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

fn read_dir(path: &Path) -> Result<std::fs::ReadDir, IcmError> {
    std::fs::read_dir(path).map_err(|e| IcmError::Io {
        path: path.to_path_buf(),
        source: e,
    })
}

/// The result of scaffolding a new ICM workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldReport {
    pub root: PathBuf,
    pub dirs_created: Vec<String>,
    pub files_created: Vec<String>,
}

/// Scaffold a new ICM workspace skeleton at `root` — LLM-free and
/// **refuse-to-clobber** (the sole safety property, mirroring `animus-launch`,
/// ADR-053): proceeds only if `root` does not exist, or is an empty directory.
/// Creates Layer 0/1 files and a three-stage `research → script → production`
/// pipeline whose `CONTEXT.md` contracts round-trip through [`parse_contract`].
pub fn scaffold_workspace(root: &Path) -> Result<ScaffoldReport, IcmError> {
    if !clobber_safe(root)? {
        return Err(IcmError::TargetNotEmpty(root.to_path_buf()));
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    let mkdir = |rel: &str, dirs: &mut Vec<String>| -> Result<(), IcmError> {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).map_err(|e| IcmError::Io { path: p, source: e })?;
        dirs.push(rel.to_string());
        Ok(())
    };
    let write = |rel: &str, body: &str, files: &mut Vec<String>| -> Result<(), IcmError> {
        let p = root.join(rel);
        std::fs::write(&p, body).map_err(|e| IcmError::Io { path: p, source: e })?;
        files.push(rel.to_string());
        Ok(())
    };

    mkdir("", &mut dirs)?;
    mkdir("_config", &mut dirs)?;
    mkdir("shared", &mut dirs)?;

    write("Animus.md", LAYER0_TEMPLATE, &mut files)?;
    write("CONTEXT.md", LAYER1_TEMPLATE, &mut files)?;

    for (folder, contract) in [
        ("01_research", RESEARCH_CONTRACT),
        ("02_script", SCRIPT_CONTRACT),
        ("03_production", PRODUCTION_CONTRACT),
    ] {
        mkdir(&format!("stages/{folder}"), &mut dirs)?;
        mkdir(&format!("stages/{folder}/references"), &mut dirs)?;
        mkdir(&format!("stages/{folder}/output"), &mut dirs)?;
        write(&format!("stages/{folder}/CONTEXT.md"), contract, &mut files)?;
        // Keep the empty scaffold dirs in git (they carry no artifacts yet).
        write(&format!("stages/{folder}/output/.gitkeep"), "", &mut files)?;
    }

    Ok(ScaffoldReport {
        root: root.to_path_buf(),
        dirs_created: dirs,
        files_created: files,
    })
}

/// Refuse-to-clobber: safe if the target does not exist, or is an empty
/// directory (hidden entries count as non-empty). A non-directory is unsafe.
fn clobber_safe(path: &Path) -> Result<bool, IcmError> {
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = read_dir(path)?;
    Ok(entries.next().is_none())
}

const LAYER0_TEMPLATE: &str = "# Workspace identity (Layer 0)\n\n\
This is an ICM (Interpretable Context Methodology) workspace. Stages live under\n\
`stages/NN_name/` and run in numeric order. Each stage reads the previous\n\
stage's `output/`, follows its own `CONTEXT.md` contract, and writes to its own\n\
`output/`. Shared, stable reference material lives in `_config/` and `shared/`.\n";

const LAYER1_TEMPLATE: &str = "# Workspace routing (Layer 1)\n\n\
## Stages\n\
- `01_research` — gather and structure source material.\n\
- `02_script` — turn the research into a draft, following the shared voice.\n\
- `03_production` — turn the draft into the final deliverable.\n\n\
## Shared resources\n\
- `_config/` — voice, conventions, and other factory configuration.\n\
- `shared/` — reference material used across stages.\n";

const RESEARCH_CONTRACT: &str = "## Inputs\n\
- Layer 3 (reference): references/\n\n\
## Process\n\
Gather and structure the source material for this run. Produce a clear,\n\
condensed research document: key points, angles, and supporting facts.\n\n\
## Outputs\n\
- research.md -> output/\n";

const SCRIPT_CONTRACT: &str = "## Inputs\n\
- Layer 4 (working): ../01_research/output/\n\
- Layer 3 (reference): ../../_config/voice.md\n\n\
## Process\n\
Write a draft based on the research output. Follow the tone in voice.md.\n\n\
## Outputs\n\
- script_draft.md -> output/\n";

const PRODUCTION_CONTRACT: &str = "## Inputs\n\
- Layer 4 (working): ../02_script/output/\n\
- Layer 3 (reference): references/\n\n\
## Process\n\
Turn the reviewed draft into the final deliverable, applying production\n\
conventions from references/.\n\n\
## Outputs\n\
- final.md -> output/\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_papers_contract_shape() {
        // The exact stage contract from the ICM paper (§3.3).
        let md = "## Inputs\n\
                  - Layer 4 (working): ../01_research/output/\n\
                  - Layer 3 (reference): ../../_config/voice.md\n\
                  - Layer 3 (reference): references/structure.md\n\n\
                  ## Process\n\
                  Write a script based on the research output.\n\
                  Follow the structure in structure.md.\n\n\
                  ## Outputs\n\
                  - script_draft.md -> output/\n";
        let c = parse_contract(md);
        assert_eq!(c.inputs.len(), 3);
        assert_eq!(c.inputs[0].layer, Layer::Working);
        assert_eq!(c.inputs[0].path, "../01_research/output/");
        assert_eq!(c.inputs[1].layer, Layer::Reference);
        assert_eq!(c.inputs[1].path, "../../_config/voice.md");
        assert!(c.process.contains("Write a script"));
        assert!(c.process.contains("structure.md"));
        assert_eq!(c.outputs.len(), 1);
        assert_eq!(c.outputs[0].name, "script_draft.md");
        assert_eq!(c.outputs[0].dest, "output/");
    }

    #[test]
    fn contract_is_tolerant_of_missing_sections_and_case() {
        let c = parse_contract("## PROCESS\nJust do the thing.\n");
        assert!(c.inputs.is_empty());
        assert!(c.outputs.is_empty());
        assert_eq!(c.process, "Just do the thing.");

        // An output with no `->` defaults its destination to output/.
        let c2 = parse_contract("## Outputs\n- notes.md\n");
        assert_eq!(c2.outputs[0].name, "notes.md");
        assert_eq!(c2.outputs[0].dest, "output/");

        // An unlabeled input defaults to the conservative reference layer.
        let c3 = parse_contract("## Inputs\n- data/facts.md\n");
        assert_eq!(c3.inputs[0].layer, Layer::Reference);
        assert_eq!(c3.inputs[0].path, "data/facts.md");
    }

    #[test]
    fn stage_index_reads_the_numeric_prefix() {
        assert_eq!(stage_index("01_research"), Some(1));
        assert_eq!(stage_index("2-script"), Some(2));
        assert_eq!(stage_index("10_final"), Some(10));
        assert_eq!(stage_index("references"), None);
        assert_eq!(stage_index("_config"), None);
    }

    #[test]
    fn the_scaffold_contracts_round_trip() {
        // Every shipped contract template must parse into a usable contract, so
        // a scaffolded workspace is immediately plannable.
        for md in [RESEARCH_CONTRACT, SCRIPT_CONTRACT, PRODUCTION_CONTRACT] {
            let c = parse_contract(md);
            assert!(!c.process.is_empty(), "process must parse");
            assert_eq!(c.outputs.len(), 1, "each stage declares one output");
        }
        assert_eq!(parse_contract(SCRIPT_CONTRACT).inputs.len(), 2);
    }
}
