//! Tool trait, registry chokepoint, and builtin file tools for Animus Ferric.
//!
//! Every tool execution flows through a single registry chokepoint that
//! performs the guard check, timing, and the full-vs-truncated output split.
