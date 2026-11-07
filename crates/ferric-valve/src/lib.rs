//! `ferric-valve` — Ferric's constrained decoding behind Hermes Agent's
//! OpenAI-compatible custom endpoint (INT-0012).
//!
//! Hermes keeps its loop, tool validation, authorization, execution and
//! history. The valve only changes how the next action is decoded: it turns a
//! native-tools request into one harness-authored JSON-Schema action
//! constraint for a llama.cpp backend, and turns the constrained action back
//! into an ordinary OpenAI response. It is stateless per request.

pub mod pilot;
pub mod probe;
pub mod receipt;
pub mod server;
pub mod sse;
pub mod transform;
pub mod translate;
pub mod upstream;

pub use server::{ValveConfig, ValveMode, gated_router, router, serve};
pub use upstream::{Exchange, ExchangeOutcome, ValveError, run_constrained};
