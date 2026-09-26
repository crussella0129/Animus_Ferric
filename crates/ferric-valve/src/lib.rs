//! `ferric-valve` — Ferric's constrained decoding behind Hermes Agent's
//! OpenAI-compatible custom endpoint (INT-0012).
//!
//! Hermes keeps its loop, tool validation, authorization, execution and
//! history. The valve only changes how the next action is decoded: it turns a
//! native-tools request into one harness-authored JSON-Schema action
//! constraint for a llama.cpp backend, and turns the constrained action back
//! into an ordinary OpenAI response. It is stateless per request.

pub mod transform;

pub use transform::{
    CONSTRAINED_TEACHING, Constrained, TransformError, Transformed, final_answer_descriptor,
    message_hashes, prefix_hash, sha256_hex, transform,
};
