//! Context-budget compaction (sprint 40, ADR-050): fold older turns into one
//! model-summarized message when a session's `input_tokens` approaches
//! `RunPolicy.prompt_budget_tokens`. Always-on, no CLI flag — mirrors the
//! repetition/no-progress/failure guards' precedent (ADR-037/038): a real,
//! numeric threshold that no existing (short) session can accidentally cross.
//!
//! # Turn-number tracking is ABSOLUTE, not relative
//! `turn_starts` records `(absolute turn number, start index in messages)` for
//! every turn completed since the last fold, in original numbering — never a
//! zero-based/offset scheme. This is a deliberate correction made during the
//! plan-critic pass (an earlier draft used a `turn_offset` accumulator; the
//! critic found its role and update formula were never derived, only
//! asserted). Storing absolute numbers directly removes the ambiguity
//! entirely: `through_turn` for the trace is read straight off the tracked
//! pair, no arithmetic needed, and a resumed session's compactor (which may
//! start counting from a nonzero `turns`) needs zero special-casing.
//!
//! # Same-provider reuse
//! Ferric runs one local GGUF model per session — there is no second, cheaper
//! model to delegate summarization to (a documented divergence from every
//! cloud-multi-model framework surveyed in research). The summarizer call
//! reuses the SAME `provider` via the existing `complete_with_backoff`.

use ferric_core::{FerricError, Message, Role, RunPolicy};
use ferric_provider::{CompletionRequest, Provider, SamplingParams};
use ferric_trace::{Event, JsonlSink};

use crate::backoff::complete_with_backoff;
use crate::run::Sleeper;

/// Fires when the last known `completion.input_tokens` reaches this fraction
/// of `policy.prompt_budget_tokens` (matches a `deepagents`-style precedent
/// cited in research — the user specified the mechanism, not an exact
/// number). Fixed constant for v1, not a per-tier `RunPolicy` field.
const COMPACT_TRIGGER_FRACTION: f64 = 0.85;

/// The most recent N turns are always preserved verbatim, never folded
/// (mirrors the Microsoft Agent Framework's `MinimumPreserved`/
/// `keep_last_groups` floor). Fixed constant for v1.
const KEEP_LAST_TURNS: usize = 2;

const COMPACT_SYSTEM_PROMPT: &str = "You are summarizing an in-progress coding agent's own \
history so older turns can be dropped from context. Write a concise, factual account of what \
has been done and learned so far (files created/edited, commands run, decisions made, results \
observed) so the agent can continue the task without re-reading the full history.";

/// Tracks per-turn message boundaries since the last fold and performs the
/// fold itself. Constructed once per `run()` call; `pub(crate)` only (no
/// public API surface — internal to the loop).
pub(crate) struct HistoryCompactor {
    /// `messages.len()` right after the session's initial seeding (fresh or
    /// resumed) — the fixed floor a fold never crosses. For a resumed session
    /// this covers the ENTIRE replayed history: only NEW turns generated
    /// after resuming are foldable (a deliberate v1 scope limit; see
    /// ADR-050).
    head_len: usize,
    /// `(absolute turn number, start index in `messages`)` for every turn
    /// completed since the last fold, in order — including the just-started
    /// CURRENT turn's own entry, always the last one pushed and always
    /// excluded from folding (see `maybe_compact`'s `completed` slice).
    turn_starts: Vec<(u32, usize)>,
}

impl HistoryCompactor {
    pub(crate) fn new(head_len: usize) -> Self {
        Self {
            head_len,
            turn_starts: Vec::new(),
        }
    }

    /// Call once per loop iteration, right after writing `Event::TurnStart`
    /// for `turn` — this ordering is load-bearing (see `run.rs`'s wiring):
    /// it's what lets `replay()`'s existing "commit on next TurnStart" rule
    /// safely finalize the previous turn before any fold this call triggers
    /// reads it back out.
    pub(crate) fn record_turn_start(&mut self, turn: u32, messages_len: usize) {
        self.turn_starts.push((turn, messages_len));
    }

    /// No-op unless `last_input_tokens` has crossed the trigger fraction AND
    /// enough turns have completed since the last fold. On a successful
    /// fold: traces one `Event::HistoryCompacted`, splices `messages`, and
    /// keeps only the surviving (preserved-tail) entries in `turn_starts`. A
    /// summarizer failure (provider error or empty output) is non-fatal: logs
    /// an `Event::Note` and leaves everything unchanged.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn maybe_compact(
        &mut self,
        provider: &dyn Provider,
        sleeper: &dyn Sleeper,
        sink: &mut JsonlSink,
        policy: &RunPolicy,
        messages: &mut Vec<Message>,
        last_input_tokens: Option<u32>,
    ) -> Result<(), FerricError> {
        let Some(tokens) = last_input_tokens else {
            return Ok(());
        };
        if (tokens as f64) < COMPACT_TRIGGER_FRACTION * policy.prompt_budget_tokens as f64 {
            return Ok(());
        }
        if self.turn_starts.is_empty() {
            return Ok(());
        }
        // Exclude the just-started CURRENT turn's own entry (always the last
        // pushed) — this is the structural mechanism, not just a call-order
        // convention, that guarantees the in-flight turn can never be folded.
        let completed = &self.turn_starts[..self.turn_starts.len() - 1];
        if completed.len() <= KEEP_LAST_TURNS {
            return Ok(());
        }
        let fold_count = completed.len() - KEEP_LAST_TURNS;
        let (through_turn, _) = completed[fold_count - 1];
        let fold_from_idx = self.head_len;
        // By construction of the slice split, this is exactly "the start
        // index of the first entry beyond the folded range" — no off-by-one
        // derivation needed.
        let fold_to_idx = completed[fold_count].1;

        let transcript = render_transcript(&messages[fold_from_idx..fold_to_idx]);
        let summary = match summarize_history(provider, sleeper, &transcript).await {
            Ok(s) => s,
            Err(e) => {
                sink.write_event(Event::Note {
                    text: format!("compaction skipped: {e}"),
                })?;
                return Ok(());
            }
        };

        sink.write_event(Event::HistoryCompacted {
            through_turn,
            dropped_turns: fold_count as u32,
            summary: summary.clone(),
        })?;

        messages.splice(
            fold_from_idx..fold_to_idx,
            std::iter::once(Message::user(format!("[compacted history] {summary}"))),
        );
        let shift = (fold_to_idx - fold_from_idx) - 1;
        self.turn_starts = self.turn_starts[fold_count..]
            .iter()
            .map(|&(t, i)| (t, i - shift))
            .collect();
        Ok(())
    }
}

/// One line per message, naming its role, any tool-call names, and its text.
/// Pure and protocol-agnostic — the summarizer only needs a readable
/// transcript, not a live-conversation-accurate replay of each protocol's
/// exact message framing.
fn render_transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            let calls = if m.tool_calls.is_empty() {
                String::new()
            } else {
                let names: Vec<&str> = m.tool_calls.iter().map(|c| c.name.as_str()).collect();
                format!(" (calls: {})", names.join(", "))
            };
            let text = m.text.as_deref().unwrap_or("");
            format!("[{role}]{calls} {text}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Single-shot, no-tools, unconstrained free-text completion that condenses
/// `transcript` into a "progress so far" summary. Reuses the SAME provider
/// driving the main loop (no second, cheaper model exists in this
/// architecture) via the existing retry policy.
async fn summarize_history(
    provider: &dyn Provider,
    sleeper: &dyn Sleeper,
    transcript: &str,
) -> Result<String, FerricError> {
    let request = CompletionRequest {
        messages: vec![
            Message::system(COMPACT_SYSTEM_PROMPT),
            Message::user(transcript.to_string()),
        ],
        sampling: SamplingParams::default(),
        tools: Vec::new(),
        constraint: None,
    };
    let completion = complete_with_backoff(provider, request, sleeper)
        .await
        .map_err(|e| FerricError::Other(e.to_string()))?;
    completion
        .message
        .text
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| FerricError::Other("compaction summarizer returned empty text".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_core::ToolCall;
    use ferric_provider::{Completion, MockProvider};
    use serde_json::json;

    fn nano_policy() -> RunPolicy {
        ferric_core::policy_for(&ferric_core::ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "test".to_string(),
            measured_level: None,
        })
    }

    struct NoopSleeper;
    impl Sleeper for NoopSleeper {
        fn sleep(&self, _duration: std::time::Duration) {}
    }

    fn open_sink() -> (tempfile::TempDir, JsonlSink) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trace.jsonl");
        let sink = JsonlSink::open(&path, "s-1").unwrap();
        (dir, sink)
    }

    fn summary_completion(text: &str) -> Completion {
        Completion {
            message: Message::assistant(text),
            input_tokens: Some(20),
            output_tokens: Some(10),
            truncated: false,
        }
    }

    #[test]
    fn render_transcript_names_roles_and_tool_calls() {
        let messages = vec![
            Message::system("sys"),
            Message::user("do it"),
            Message {
                role: Role::Assistant,
                text: None,
                tool_calls: vec![ToolCall {
                    id: "tc-0".to_string(),
                    name: "write_file".to_string(),
                    args: json!({}),
                }],
                tool_call_id: None,
                media: Vec::new(),
            },
            Message::tool_result("tc-0", "wrote 2 bytes"),
        ];
        let out = render_transcript(&messages);
        assert!(out.contains("[system] sys"));
        assert!(out.contains("[user] do it"));
        assert!(out.contains("[assistant] (calls: write_file)"));
        assert!(out.contains("[tool] wrote 2 bytes"));
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures_executor::block_on(f)
    }

    fn seed_completed_turns(compactor: &mut HistoryCompactor, messages: &mut Vec<Message>, n: u32) {
        // Simulate `n` completed turns, each appending one assistant message,
        // mirroring how `record_turn_start` is called before that turn's
        // messages are appended in run()'s real loop.
        for turn in 0..n {
            compactor.record_turn_start(turn, messages.len());
            messages.push(Message::assistant(format!("turn {turn} result")));
        }
        // The just-started NEXT turn's own boundary entry (never populated —
        // maybe_compact must exclude it from folding).
        compactor.record_turn_start(n, messages.len());
    }

    #[test]
    fn maybe_compact_below_trigger_is_noop() {
        let mut messages = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages.len();
        let mut compactor = HistoryCompactor::new(head_len);
        seed_completed_turns(&mut compactor, &mut messages, 5);
        let before = messages.clone();
        let provider = MockProvider::new(vec![]);
        let (_dir, mut sink) = open_sink();
        block(compactor.maybe_compact(
            &provider,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(100), // far below 85% of 2800
        ))
        .unwrap();
        assert_eq!(messages, before);
        assert!(!provider_was_called(&provider));
    }

    fn provider_was_called(provider: &MockProvider) -> bool {
        !provider.requests().is_empty()
    }

    /// Test-critic C-003: pins the exact 85%-of-2800 boundary (2380.0) rather
    /// than only ever testing values comfortably clear of it — `tokens < 2380`
    /// must stay a no-op, `tokens == 2380` must trigger.
    #[test]
    fn maybe_compact_trigger_boundary_is_exclusive_below() {
        let mut messages_below = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages_below.len();
        let mut compactor_below = HistoryCompactor::new(head_len);
        seed_completed_turns(&mut compactor_below, &mut messages_below, 5);
        let before = messages_below.clone();
        let provider_below = MockProvider::new(vec![]);
        let (_dir1, mut sink1) = open_sink();
        block(compactor_below.maybe_compact(
            &provider_below,
            &NoopSleeper,
            &mut sink1,
            &nano_policy(),
            &mut messages_below,
            Some(2379), // just below 0.85 * 2800 = 2380.0
        ))
        .unwrap();
        assert_eq!(messages_below, before, "2379 must not trigger a fold");

        let mut messages_at = vec![Message::system("sys"), Message::user("task")];
        let mut compactor_at = HistoryCompactor::new(head_len);
        seed_completed_turns(&mut compactor_at, &mut messages_at, 5);
        let provider_at = MockProvider::new(vec![summary_completion("did turns 0-2")]);
        let (_dir2, mut sink2) = open_sink();
        block(compactor_at.maybe_compact(
            &provider_at,
            &NoopSleeper,
            &mut sink2,
            &nano_policy(),
            &mut messages_at,
            Some(2380), // exactly 0.85 * 2800
        ))
        .unwrap();
        assert!(
            provider_was_called(&provider_at),
            "2380 (the exact threshold) must trigger a fold"
        );
    }

    #[test]
    fn maybe_compact_not_enough_history_is_noop() {
        let mut messages = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages.len();
        let mut compactor = HistoryCompactor::new(head_len);
        // Only 2 completed turns (KEEP_LAST_TURNS == 2) — nothing foldable.
        seed_completed_turns(&mut compactor, &mut messages, 2);
        let before = messages.clone();
        let provider = MockProvider::new(vec![]);
        let (_dir, mut sink) = open_sink();
        block(compactor.maybe_compact(
            &provider,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(2500), // above 85% of 2800
        ))
        .unwrap();
        assert_eq!(messages, before);
        assert!(!provider_was_called(&provider));
    }

    #[test]
    fn maybe_compact_folds_older_turns_keeps_recent_tail() {
        let mut messages = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages.len();
        let mut compactor = HistoryCompactor::new(head_len);
        // 5 completed turns; KEEP_LAST_TURNS=2 → fold turns 0,1,2 (3 turns),
        // keep turns 3,4 verbatim.
        seed_completed_turns(&mut compactor, &mut messages, 5);
        let preserved_tail = messages[messages.len() - 2..].to_vec();

        let provider = MockProvider::new(vec![summary_completion("did turns 0-2")]);
        let (_dir, mut sink) = open_sink();
        block(compactor.maybe_compact(
            &provider,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(2500),
        ))
        .unwrap();

        // head + 1 summary message + 2 preserved turns.
        assert_eq!(messages.len(), head_len + 1 + preserved_tail.len());
        assert_eq!(
            messages[head_len].text.as_deref(),
            Some("[compacted history] did turns 0-2")
        );
        assert_eq!(&messages[head_len + 1..], &preserved_tail[..]);

        let trace_path = sink_path(&_dir);
        let records: Vec<_> = ferric_trace::TraceReader::open(&trace_path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let compacted = records
            .iter()
            .find_map(|r| match &r.event {
                ferric_trace::ParsedEvent::Known(Event::HistoryCompacted {
                    through_turn,
                    dropped_turns,
                    summary,
                }) => Some((*through_turn, *dropped_turns, summary.clone())),
                _ => None,
            })
            .expect("a HistoryCompacted event");
        assert_eq!(compacted, (2, 3, "did turns 0-2".to_string()));
    }

    fn sink_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join("trace.jsonl")
    }

    #[test]
    fn maybe_compact_summarizer_failure_is_nonfatal() {
        let mut messages = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages.len();
        let mut compactor = HistoryCompactor::new(head_len);
        seed_completed_turns(&mut compactor, &mut messages, 5);
        let before = messages.clone();

        // Empty text is treated as a failure (no valid summary).
        let provider = MockProvider::new(vec![summary_completion("")]);
        let (_dir, mut sink) = open_sink();
        block(compactor.maybe_compact(
            &provider,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(2500),
        ))
        .unwrap();

        assert_eq!(messages, before);
        let trace_path = sink_path(&_dir);
        let records: Vec<_> = ferric_trace::TraceReader::open(&trace_path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(records.iter().any(
            |r| matches!(&r.event, ferric_trace::ParsedEvent::Known(Event::Note { text }) if text.contains("compaction skipped"))
        ));
        assert!(!records.iter().any(|r| matches!(
            &r.event,
            ferric_trace::ParsedEvent::Known(Event::HistoryCompacted { .. })
        )));
    }

    #[test]
    fn maybe_compact_repeat_fold_never_accumulates() {
        let mut messages = vec![Message::system("sys"), Message::user("task")];
        let head_len = messages.len();
        let mut compactor = HistoryCompactor::new(head_len);
        seed_completed_turns(&mut compactor, &mut messages, 5);

        let provider = MockProvider::new(vec![summary_completion("first summary")]);
        let (_dir, mut sink) = open_sink();
        block(compactor.maybe_compact(
            &provider,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(2500),
        ))
        .unwrap();

        // 3 more completed turns since the fold (5,6,7) — enough to fold again.
        for turn in 5..8 {
            compactor.record_turn_start(turn, messages.len());
            messages.push(Message::assistant(format!("turn {turn} result")));
        }
        compactor.record_turn_start(8, messages.len());

        let provider2 = MockProvider::new(vec![summary_completion("second summary")]);
        block(compactor.maybe_compact(
            &provider2,
            &NoopSleeper,
            &mut sink,
            &nano_policy(),
            &mut messages,
            Some(2500),
        ))
        .unwrap();

        let summary_count = messages
            .iter()
            .filter(|m| {
                m.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("[compacted history]"))
            })
            .count();
        assert_eq!(summary_count, 1, "never more than one summary message");
        assert_eq!(
            messages[head_len].text.as_deref(),
            Some("[compacted history] second summary")
        );
    }
}
