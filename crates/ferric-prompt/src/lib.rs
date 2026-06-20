//! Per-tier, per-protocol system-prompt composition (ADR-016).
//!
//! A versioned library of prompt *atoms* (oovra elements, in `prompts/`) is
//! composed per `(Tier, ActionProtocol)` via a deterministic recipe matrix.
//! The result is clean prose (oovra `render_text`) plus the composition
//! genealogy — which atoms, at which versions — which the caller records as a
//! `PromptComposed` trace event. The loop owns no prompt dependency; it
//! receives the text + lineage as plain data and falls back to its
//! `DEFAULT_SYSTEM_PROMPT` when no library is supplied.

use std::path::Path;

use ferric_core::{ActionProtocol, Tier};
use oovra::{Library, render};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("loading prompt library: {0}")]
    Load(String),
    #[error("recipe element '{0}' not found in library")]
    MissingElement(String),
    #[error("element '{id}' version mismatch: recipe pinned {pinned}, library has {actual}")]
    VersionMismatch {
        id: String,
        pinned: String,
        actual: String,
    },
    #[error("rendering prompt: {0}")]
    Render(String),
}

/// A composed system prompt and its genealogy.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedPrompt {
    pub text: String,
    pub output_id: String,
    pub output_version: String,
    /// (element_id, version) of each atom that built this prompt, in order.
    pub composed_of: Vec<(String, String)>,
}

/// Load the prompt-element library rooted at `dir`.
pub fn load_library(dir: &Path) -> Result<Library, PromptError> {
    Library::load(dir).map_err(|e| PromptError::Load(e.to_string()))
}

/// The element recipe for a (tier, protocol) pair: atom ids in composition
/// order, each with an optional version pin. The protocol-teaching atom is
/// chosen to match `protocol` (mutually exclusive). Tier currently selects
/// the same atoms across the band; per-tier wording variants slot in here
/// without changing callers.
pub fn recipe_for(_tier: Tier, protocol: ActionProtocol) -> Vec<(String, Option<String>)> {
    let protocol_atom = match protocol {
        ActionProtocol::NativeTools => "protocol-native-tools",
        ActionProtocol::UnifiedGrammar => "protocol-unified-grammar",
    };
    vec![
        ("role-declaration".to_string(), Some("1.0.0".to_string())),
        ("workspace-rules".to_string(), Some("1.0.0".to_string())),
        (protocol_atom.to_string(), Some("1.0.0".to_string())),
        ("terminator-teaching".to_string(), Some("1.0.0".to_string())),
    ]
}

/// Compose the system prompt for `(tier, protocol)` from `library`.
pub fn compose_system_prompt(
    library: &Library,
    tier: Tier,
    protocol: ActionProtocol,
) -> Result<ComposedPrompt, PromptError> {
    let recipe = recipe_for(tier, protocol);

    let mut elements = Vec::with_capacity(recipe.len());
    let mut composed_of = Vec::with_capacity(recipe.len());
    for (id, pin) in &recipe {
        let element = library
            .get(id)
            .ok_or_else(|| PromptError::MissingElement(id.clone()))?;
        if let Some(pinned) = pin
            && &element.header.version != pinned
        {
            return Err(PromptError::VersionMismatch {
                id: id.clone(),
                pinned: pinned.clone(),
                actual: element.header.version.clone(),
            });
        }
        composed_of.push((id.clone(), element.header.version.clone()));
        elements.push(element);
    }

    let text = render::render_text(&elements).map_err(|e| PromptError::Render(e.to_string()))?;
    Ok(ComposedPrompt {
        text,
        output_id: format!(
            "system-prompt-{}-{}",
            tier_slug(tier),
            protocol_slug(protocol)
        ),
        output_version: "1.0.0".to_string(),
        composed_of,
    })
}

fn tier_slug(tier: Tier) -> &'static str {
    match tier {
        Tier::Nano => "nano",
        Tier::Small => "small",
        Tier::Medium => "medium",
        Tier::Large => "large",
        Tier::Xl => "xl",
        Tier::Ultra => "ultra",
    }
}

fn protocol_slug(protocol: ActionProtocol) -> &'static str {
    match protocol {
        ActionProtocol::NativeTools => "native",
        ActionProtocol::UnifiedGrammar => "grammar",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The in-repo prompts/ library (two levels up from this crate).
    fn prompts_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("prompts")
    }

    fn all_tiers() -> Vec<Tier> {
        vec![
            Tier::Nano,
            Tier::Small,
            Tier::Medium,
            Tier::Large,
            Tier::Xl,
            Tier::Ultra,
        ]
    }

    #[test]
    fn compose_all_pairs() {
        let lib = load_library(&prompts_dir()).expect("load prompts library");
        for tier in all_tiers() {
            for protocol in [ActionProtocol::NativeTools, ActionProtocol::UnifiedGrammar] {
                let composed = compose_system_prompt(&lib, tier, protocol).expect("compose");
                assert!(!composed.text.is_empty());
                // Lineage matches the recipe (id + resolved version).
                let recipe_ids: Vec<String> = recipe_for(tier, protocol)
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                let lineage_ids: Vec<String> = composed
                    .composed_of
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect();
                assert_eq!(lineage_ids, recipe_ids);
                assert!(composed.composed_of.iter().all(|(_, v)| v == "1.0.0"));
            }
        }
    }

    #[test]
    fn protocol_teaching_is_exclusive() {
        let lib = load_library(&prompts_dir()).unwrap();
        let grammar = compose_system_prompt(&lib, Tier::Nano, ActionProtocol::UnifiedGrammar)
            .unwrap()
            .text;
        let native = compose_system_prompt(&lib, Tier::Nano, ActionProtocol::NativeTools)
            .unwrap()
            .text;
        // Grammar prompt teaches the XML tool_call format; native does not.
        assert!(grammar.contains("<tool_call>"));
        assert!(!native.contains("<tool_call>"));
        // Native prompt teaches function-calling; grammar does not.
        assert!(native.contains("function-calling"));
        assert!(!grammar.contains("function-calling"));
    }

    #[test]
    fn missing_library_is_typed_err() {
        let err = load_library(Path::new("/no/such/prompts/dir")).unwrap_err();
        assert!(matches!(err, PromptError::Load(_)));
    }

    #[test]
    fn version_mismatch_is_typed_err() {
        let lib = load_library(&prompts_dir()).unwrap();
        // A recipe pinning a wrong version surfaces VersionMismatch.
        let recipe = vec![("role-declaration".to_string(), Some("9.9.9".to_string()))];
        let mut elements = Vec::new();
        for (id, pin) in &recipe {
            let element = lib.get(id).unwrap();
            if let Some(pinned) = pin
                && &element.header.version != pinned
            {
                let err = PromptError::VersionMismatch {
                    id: id.clone(),
                    pinned: pinned.clone(),
                    actual: element.header.version.clone(),
                };
                assert!(matches!(err, PromptError::VersionMismatch { .. }));
                return;
            }
            elements.push(element);
        }
        panic!("expected a version mismatch");
    }
}
