//! The pinned catalog.
//!
//! Every model is fixed to a Hugging Face revision, with the file's SHA-256
//! and size taken from the Hub's LFS metadata at that revision. A changed
//! file upstream cannot change what Ferric installs; it fails verification
//! instead.
//!
//! The engine is pinned to a llama.cpp release tag. Its assets are verified
//! against the SHA-256 digest GitHub records for that release (see
//! [`crate::engine`]).

/// The llama.cpp release Ferric installs (published 2026-10-21).
pub const LLAMA_CPP_TAG: &str = "b11780";

/// One downloadable model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelEntry {
    /// Short name used on the command line (`ferric setup --model 7b`).
    pub id: &'static str,
    pub label: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    pub params_b: f32,
}

impl ModelEntry {
    /// The pinned download URL under a Hugging Face–compatible base.
    pub fn url(&self, base: &str) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            base.trim_end_matches('/'),
            self.repo,
            self.revision,
            self.file
        )
    }

    pub fn size_gb(&self) -> f64 {
        self.bytes as f64 / 1_000_000_000.0
    }
}

/// Qwen2.5-Coder Instruct at Q4_K_M: the family Ferric's small-model fleet
/// is measured on, all Apache-2.0, in single-file GGUFs.
pub const MODELS: &[ModelEntry] = &[
    ModelEntry {
        id: "0.5b",
        label: "Qwen2.5-Coder 0.5B Instruct",
        repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
        revision: "ebb2015119c907b064c512bf053e945850b5875f",
        file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
        bytes: 491_400_064,
        sha256: "1d9614638d18024d0fbb36575a15f1302a3adf044df10345688ec4f6e1c4ff32",
        license: "Apache-2.0",
        params_b: 0.5,
    },
    ModelEntry {
        id: "1.5b",
        label: "Qwen2.5-Coder 1.5B Instruct",
        repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
        revision: "f86cb2c1fa58255f8052cc32aeede1b7482d4361",
        file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
        bytes: 1_117_320_768,
        sha256: "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046",
        license: "Apache-2.0",
        params_b: 1.5,
    },
    ModelEntry {
        id: "7b",
        label: "Qwen2.5-Coder 7B Instruct",
        repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
        revision: "13fb94bfda8c8cf22497dc57b78f391a9acb426a",
        file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
        bytes: 4_683_073_536,
        sha256: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
        license: "Apache-2.0",
        params_b: 7.0,
    },
    ModelEntry {
        id: "14b",
        label: "Qwen2.5-Coder 14B Instruct",
        repo: "Qwen/Qwen2.5-Coder-14B-Instruct-GGUF",
        revision: "d0a692ef765eefbf2fabb130b3cb2e8917e3d225",
        file: "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        bytes: 8_988_110_272,
        sha256: "c1e659736d89ac1065fb495330fb824d94001974a4bfa78e7270e43476a8d940",
        license: "Apache-2.0",
        params_b: 14.0,
    },
];

/// Look a model up by its short id (`1.5b`, `7b`, ...), case-insensitively.
pub fn find_model(id: &str) -> Option<&'static ModelEntry> {
    let id = id.trim().to_ascii_lowercase();
    MODELS.iter().find(|model| model.id == id)
}

/// The default Hugging Face base. `FERRIC_MODEL_BASE_URL` overrides it (a
/// mirror); only `https` or a loopback `http` mirror is accepted.
pub const HUGGING_FACE: &str = "https://huggingface.co";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_pinned_and_well_formed() {
        for model in MODELS {
            assert_eq!(model.revision.len(), 40, "{}", model.id);
            assert_eq!(model.sha256.len(), 64, "{}", model.id);
            assert!(model.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(model.file.ends_with(".gguf"));
            assert!(model.bytes > 100_000_000);
            assert_eq!(model.license, "Apache-2.0");
        }
    }

    #[test]
    fn the_url_names_the_pinned_revision() {
        let model = find_model("7B").unwrap();
        assert_eq!(
            model.url("https://huggingface.co/"),
            "https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/13fb94bfda8c8cf22497dc57b78f391a9acb426a/qwen2.5-coder-7b-instruct-q4_k_m.gguf"
        );
        assert!(find_model("70b").is_none());
    }
}
