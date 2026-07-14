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
use ferric_trace::Event;

use crate::backoff::complete_with_backoff;
use crate::run::Sleeper;

const COMPACT_SYSTEM_PROMPT: &str = "You are summarizing an in-progress coding agent's own \
history so older turns can be dropped from context. Write a concise, factual account of what \
has been done and learned so far (files created/edited, commands run, decisions made, results \
observed) so the agent can continue the task without re-reading the full history.";

use crate::projector::TraceProjector;

/// No-op unless `last_input_tokens` has crossed the trigger fraction AND
/// enough turns have completed since the last fold. On a successful
/// fold: returns an `Event::HistoryCompacted`. A summarizer failure
/// (provider error or empty output) is non-fatal: returns an `Event::Note`.
pub(crate) async fn maybe_compact(
    projector: &TraceProjector,
    provider: &dyn Provider,
    sleeper: &dyn Sleeper,
    policy: &RunPolicy,
    last_input_tokens: Option<u32>,
) -> Result<Option<Event>, FerricError> {
    let Some(tokens) = last_input_tokens else {
        return Ok(None);
    };
    if (tokens as f32) < policy.compact_trigger_fraction * policy.prompt_budget_tokens as f32 {
        return Ok(None);
    }
    if projector.committed_turn_starts.is_empty() {
        return Ok(None);
    }
    // TraceProjector's `committed_turn_starts` only contains turns that
    // have been fully committed (it does not contain the in-flight turn N).
    // So all entries here are completed and eligible for folding.
    let completed = &projector.committed_turn_starts[..];
    let keep_last = policy.compact_keep_last_turns as usize;
    if completed.len() <= keep_last {
        return Ok(None);
    }
    let fold_count = completed.len() - keep_last;
    let (through_turn, _) = completed[fold_count - 1];
    let fold_from_idx = projector.head_len;
    // By construction of the slice split, this is exactly "the start
    // index of the first entry beyond the folded range" — no off-by-one
    // derivation needed.
    let fold_to_idx = completed[fold_count].1;

    let transcript = render_transcript(&projector.messages[fold_from_idx..fold_to_idx]);
    let summary = match summarize_history(provider, sleeper, &transcript).await {
        Ok(s) => s,
        Err(e) => {
            return Ok(Some(Event::Note {
                text: format!("compaction skipped: {e}"),
            }));
        }
    };

    Ok(Some(Event::HistoryCompacted {
        through_turn,
        dropped_turns: fold_count as u32,
        summary,
    }))
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
        let mut p = ferric_core::policy_for(&ferric_core::ModelProfile {
            params_b: 1.0,
            quant: "Q4_K_M".to_string(),
            ctx: 4096,
            family: "test".to_string(),
            measured_level: None,
        });
        p.compact_trigger_fraction = 0.85;
        p.compact_keep_last_turns = 2;
        p
    }

    struct NoopSleeper;
    impl Sleeper for NoopSleeper {
        fn sleep(&self, _duration: std::time::Duration) {}
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

    fn seed_completed_turns(projector: &mut TraceProjector, n: u32) {
        // Simulate `n` completed turns.
        for turn in 0..n {
            projector.committed_turn_starts.push((turn, projector.messages.len()));
            projector.messages.push(Message::assistant(format!("turn {turn} result")));
        }
    }

    fn setup_projector() -> TraceProjector {
        let mut projector = TraceProjector::new();
        projector.messages = vec![Message::system("sys"), Message::user("task")];
        projector.head_len = projector.messages.len();
        projector
    }

    #[test]
    fn maybe_compact_below_trigger_is_noop() {
        let mut projector = setup_projector();
        seed_completed_turns(&mut projector, 5);
        let before = projector.messages.clone();
        let provider = MockProvider::new(vec![]);
        
        let result = block(maybe_compact(
            &projector,
            &provider,
            &NoopSleeper,
            &nano_policy(),
            Some(100), // far below 85% of 2800
        ))
        .unwrap();
        
        assert!(result.is_none());
        assert_eq!(projector.messages, before);
        assert!(!provider_was_called(&provider));
    }

    fn provider_was_called(provider: &MockProvider) -> bool {
        !provider.requests().is_empty()
    }

    #[test]
    fn maybe_compact_trigger_boundary_is_exclusive_below() {
        let mut projector_below = setup_projector();
        seed_completed_turns(&mut projector_below, 5);
        let provider_below = MockProvider::new(vec![]);
        
        let res_below = block(maybe_compact(
            &projector_below,
            &provider_below,
            &NoopSleeper,
            &nano_policy(),
            Some(2379), // just below 0.85 * 2800 = 2380.0
        ))
        .unwrap();
        assert!(res_below.is_none(), "2379 must not trigger a fold");

        let mut projector_at = setup_projector();
        seed_completed_turns(&mut projector_at, 5);
        let provider_at = MockProvider::new(vec![summary_completion("did turns 0-2")]);
        
        let res_at = block(maybe_compact(
            &projector_at,
            &provider_at,
            &NoopSleeper,
            &nano_policy(),
            Some(2380), // exactly 0.85 * 2800
        ))
        .unwrap();
        
        assert!(res_at.is_some(), "2380 must trigger a fold");
        assert!(provider_was_called(&provider_at));
    }

    #[test]
    fn maybe_compact_not_enough_history_is_noop() {
        let mut projector = setup_projector();
        // Only 2 completed turns (KEEP_LAST_TURNS == 2) — nothing foldable.
        seed_completed_turns(&mut projector, 2);
        let provider = MockProvider::new(vec![]);
        
        let result = block(maybe_compact(
            &projector,
            &provider,
            &NoopSleeper,
            &nano_policy(),
            Some(2500), // above 85% of 2800
        ))
        .unwrap();
        assert!(result.is_none());
        assert!(!provider_was_called(&provider));
    }

    #[test]
    fn maybe_compact_folds_older_turns_keeps_recent_tail() {
        let mut projector = setup_projector();
        seed_completed_turns(&mut projector, 5);
        let preserved_tail = projector.messages[projector.messages.len() - 2..].to_vec();

        let provider = MockProvider::new(vec![summary_completion("did turns 0-2")]);
        let result = block(maybe_compact(
            &projector,
            &provider,
            &NoopSleeper,
            &nano_policy(),
            Some(2500),
        ))
        .unwrap();

        let event = result.expect("should return an event");
        if let Event::HistoryCompacted { through_turn, dropped_turns, summary } = &event {
            assert_eq!(*through_turn, 2);
            assert_eq!(*dropped_turns, 3);
            assert_eq!(summary, "did turns 0-2");
        } else {
            panic!("Expected HistoryCompacted event");
        }

        // Apply event to projector to test projector logic!
        projector.step(&event);

        // head + 1 summary message + 2 preserved turns.
        assert_eq!(projector.messages.len(), projector.head_len + 1 + preserved_tail.len());
        assert_eq!(
            projector.messages[projector.head_len].text.as_deref(),
            Some("[compacted history] did turns 0-2")
        );
        assert_eq!(&projector.messages[projector.head_len + 1..], &preserved_tail[..]);
    }

    #[test]
    fn maybe_compact_summarizer_failure_is_nonfatal() {
        let mut projector = setup_projector();
        seed_completed_turns(&mut projector, 5);

        // Empty text is treated as a failure (no valid summary).
        let provider = MockProvider::new(vec![summary_completion("")]);
        let result = block(maybe_compact(
            &projector,
            &provider,
            &NoopSleeper,
            &nano_policy(),
            Some(2500),
        ))
        .unwrap();

        let event = result.expect("should return a note event");
        assert!(matches!(event, Event::Note { .. }));
    }

    #[test]
    fn maybe_compact_repeat_fold_never_accumulates() {
        let mut projector = setup_projector();
        seed_completed_turns(&mut projector, 5);

        let provider = MockProvider::new(vec![summary_completion("first summary")]);
        let event1 = block(maybe_compact(
            &projector,
            &provider,
            &NoopSleeper,
            &nano_policy(),
            Some(2500),
        )).unwrap().unwrap();
        projector.step(&event1);

        // 3 more completed turns since the fold (5,6,7) — enough to fold again.
        for turn in 5..8 {
            projector.committed_turn_starts.push((turn, projector.messages.len()));
            projector.messages.push(Message::assistant(format!("turn {turn} result")));
        }
        projector.committed_turn_starts.push((8, projector.messages.len()));

        let provider2 = MockProvider::new(vec![summary_completion("second summary")]);
        let event2 = block(maybe_compact(
            &projector,
            &provider2,
            &NoopSleeper,
            &nano_policy(),
            Some(2500),
        )).unwrap().unwrap();
        projector.step(&event2);

        let summary_count = projector.messages
            .iter()
            .filter(|m| {
                m.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("[compacted history]"))
            })
            .count();
        assert_eq!(summary_count, 1, "never more than one summary message");
        assert_eq!(
            projector.messages[projector.head_len].text.as_deref(),
            Some("[compacted history] second summary")
        );
    }
}
