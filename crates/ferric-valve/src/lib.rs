//! `ferric-valve` — Ferric's constrained decoding behind Hermes Agent's
//! OpenAI-compatible custom endpoint (INT-0012).
//!
//! Hermes keeps its loop, tool validation, authorization, execution and
//! history. The valve only changes how the next action is decoded: it turns a
//! native-tools request into one harness-authored JSON-Schema action
//! constraint for a llama.cpp backend, and turns the constrained action back
//! into an ordinary OpenAI response. It is stateless per request.

pub mod probe;
pub mod receipt;
pub mod server;
pub mod sse;
pub mod transform;
pub mod translate;
pub mod upstream;

pub use server::{ValveConfig, ValveMode, router, serve};
pub use transform::{
    CONSTRAINED_TEACHING, Constrained, TransformError, Transformed, final_answer_descriptor,
    message_hashes, prefix_hash, sha256_hex, transform,
};
pub use translate::{ActionAssembler, ActionError, Resolution};
pub use upstream::{
    Exchange, ExchangeOutcome, UpstreamStats, ValveError, completion_object, run_constrained,
};
