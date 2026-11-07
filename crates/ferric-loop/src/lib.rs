//! The production agent loop for Animus Ferric.
//!
//! Policy-budgeted turns over any `dyn Provider`, with the lineage's
//! hard-won fixes built in: the `task_complete` structured terminator, the
//! hash-ALL-calls repetition guard, exponential backoff on transient
//! provider errors — and every stage written to the JSONL trace, which is
//! the source of truth (ADR-002).
//!
//! Executor-agnostic: no tokio here. Callers drive `run` on whatever
//! executor suits their backend (futures-executor for mocks, a tokio
//! runtime for the OpenAI HTTP backend).

mod backoff;
mod compact;
mod controlled_dispatch;
mod controller;
mod failure;
mod grammar;
mod hooks_exec;
mod oscillation;
mod outcome;
mod progress;
mod projector;
mod repetition;
mod replay;
mod run;
mod trace_structure;

pub use controller::ControllerState;
pub use grammar::parse_action;
pub use outcome::{LoopOutcome, NeedsInput, StopReason};
pub use replay::{ReplayError, ReplayedState, replay, validate_resume_target};
pub use run::{
    DEFAULT_SYSTEM_PROMPT, EditApprover, EditPreview, PromptLineage, RunArgs, Sleeper,
    ThreadSleeper, run,
};
pub use trace_structure::TraceStructure;
