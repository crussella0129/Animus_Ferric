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
mod failure;
mod grammar;
mod hooks_exec;
mod oscillation;
mod outcome;
mod progress;
mod projector;
mod protocol;
mod repetition;
mod replay;
mod run;
mod terminator;
mod trace_structure;

pub use backoff::{BASE_DELAY_MS, MAX_RETRIES};
pub use grammar::{ActionParseError, action_schema, parse_action, parse_json_action};
pub use outcome::{LoopOutcome, NeedsInput, StopReason};
pub use protocol::select_protocol;
pub use replay::{ReplayError, ReplayedState, replay, validate_resume_target};
pub use run::{
    DEFAULT_SYSTEM_PROMPT, EditApprover, EditPreview, PromptLineage, RunArgs, Sleeper,
    ThreadSleeper, run,
};
pub use terminator::{
    REQUEST_USER_INPUT, SUBMIT_PLAN, TASK_COMPLETE, UserInputRequestError, control_descriptors,
    is_request_user_input, request_of, request_user_input_descriptor,
};
pub use trace_structure::TraceStructure;
