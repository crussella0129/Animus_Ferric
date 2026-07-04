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
//! runtime for mistral.rs).

mod backoff;
mod failure;
mod grammar;
mod outcome;
mod progress;
mod protocol;
mod repetition;
mod replay;
mod run;
mod terminator;

pub use backoff::{BASE_DELAY_MS, MAX_RETRIES};
pub use grammar::{ActionParseError, action_schema, parse_action, parse_json_action};
pub use outcome::{LoopOutcome, StopReason};
pub use protocol::select_protocol;
pub use replay::{ReplayError, ReplayedState, replay};
pub use run::{DEFAULT_SYSTEM_PROMPT, PromptLineage, RunArgs, Sleeper, ThreadSleeper, run};
pub use terminator::TASK_COMPLETE;
