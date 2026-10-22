//! `ferric-iron` — Ferric's constrained-decoding core (INT-0011).
//!
//! Everything a harness needs to speak Ferric's constrained action protocol to
//! a constraint-honoring backend, and nothing else: the protocol vocabulary,
//! action-grammar authoring, action parsing, capability-driven protocol
//! selection, the control branches, the incremental constrained-JSON scanner,
//! and the prompt-side conventions (tool listing, tool-result replay).
//!
//! This crate is **not a decoder**. It authors constraints; the backend (e.g.
//! llama.cpp) enforces the token mask. It has no async runtime, no HTTP and no
//! filesystem access, so another harness — the Hermes valve — can depend on it
//! without pulling in Ferric's loop, tools or CLI.

pub mod grammar;
pub mod openai_tools;
pub mod protocol;
pub mod render;
pub mod stream_scan;
pub mod terminator;
pub mod types;

pub use grammar::{ActionParseError, action_schema, parse_json_action};
pub use openai_tools::{OpenAiToolsError, RESERVED_CONTROL_NAMES, descriptors_from_openai_tools};
pub use protocol::select_protocol;
pub use render::{render_tool_listing, tool_result_text};
pub use stream_scan::ConstrainedJsonScanner;
pub use types::{Capabilities, Constraint, StreamDelta, ToolDescriptor};
