//! `ConstrainedJsonScanner` — the incremental `ConstrainedJson` action-object
//! scanner (ADR-047). Every turn's completion under `ConstrainedJson` is ONE
//! opaque JSON object (`{"tool":...,"args":...}`), including the final
//! `task_complete` answer — raw token deltas of that are not human-readable.
//! This scanner extracts exactly two useful signals from the accumulated raw
//! text as it streams in: the tool name (an early "which tool" activity
//! signal, available well before `args` finishes) and, only when the tool is
//! `task_complete`, the live-decoded characters of `args.summary` — the one
//! field that IS human-readable prose.
//!
//! Fed with the FULL accumulated text on each `scan()` call (not a diff);
//! internally tracks what it has already emitted so repeated calls only
//! return NEW deltas.

use crate::types::StreamDelta;

const TASK_COMPLETE: &str = "task_complete";

/// Stateful, pure (no I/O) scanner. One instance per streaming turn.
#[derive(Debug, Default)]
pub struct ConstrainedJsonScanner {
    thought_value_start: Option<usize>,
    thought_emitted_len: usize,
    thought_done: bool,
    thought_value_end: Option<usize>,

    tool_name: Option<String>,
    summary_value_start: Option<usize>,
    summary_emitted_len: usize,
    summary_done: bool,
}

impl ConstrainedJsonScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scan(&mut self, accumulated: &str) -> Vec<StreamDelta> {
        let mut deltas = Vec::new();

        if !self.thought_done {
            if self.thought_value_start.is_none() {
                self.thought_value_start = find_thought_value_start(accumulated);
            }
            if let Some(start) = self.thought_value_start {
                if start <= accumulated.len() {
                    let (decoded, complete, consumed_bytes) = decode_json_string_prefix(&accumulated[start..]);
                    if decoded.len() > self.thought_emitted_len {
                        let new_part = decoded[self.thought_emitted_len..].to_string();
                        self.thought_emitted_len = decoded.len();
                        deltas.push(StreamDelta::Thought(new_part));
                    }
                    if complete {
                        self.thought_done = true;
                        self.thought_value_end = Some(start + consumed_bytes);
                    }
                }
            }
        }

        // Must wait for thought to finish so we don't falsely match "tool":"..." inside the thought string.
        let search_start_after_thought = self.thought_value_end.unwrap_or(0);
        if !self.thought_done {
            return deltas;
        }

        if self.tool_name.is_none() {
            match extract_tool_name(accumulated, search_start_after_thought) {
                Some(name) => {
                    deltas.push(StreamDelta::ToolNamed(name.clone()));
                    self.tool_name = Some(name);
                }
                None => return deltas,
            }
        }

        if self.tool_name.as_deref() != Some(TASK_COMPLETE) || self.summary_done {
            return deltas;
        }

        if self.summary_value_start.is_none() {
            self.summary_value_start = find_summary_value_start(accumulated, search_start_after_thought);
        }
        let Some(start) = self.summary_value_start else {
            return deltas;
        };
        if start > accumulated.len() {
            return deltas;
        }

        let (decoded, complete, _) = decode_json_string_prefix(&accumulated[start..]);
        if decoded.len() > self.summary_emitted_len {
            let new_part = decoded[self.summary_emitted_len..].to_string();
            self.summary_emitted_len = decoded.len();
            deltas.push(StreamDelta::Text(new_part));
        }
        if complete {
            self.summary_done = true;
        }

        deltas
    }
}

fn extract_tool_name(accumulated: &str, start_offset: usize) -> Option<String> {
    let start = find_key_value_start(accumulated, start_offset, "\"tool\"")?;
    let rest = &accumulated[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn find_thought_value_start(accumulated: &str) -> Option<usize> {
    find_key_value_start(accumulated, 0, "\"thought\"")
}

fn find_summary_value_start(accumulated: &str, start_offset: usize) -> Option<usize> {
    find_key_value_start(accumulated, start_offset, "\"summary\"")
}

/// Finds a key like `"tool"`, then skips any spaces, tabs, newlines, and the colon,
/// until it finds the opening `"` of the string value. Returns the offset immediately
/// AFTER that opening `"`.
fn find_key_value_start(accumulated: &str, start_offset: usize, key: &str) -> Option<usize> {
    if start_offset >= accumulated.len() { return None; }
    let key_idx = accumulated[start_offset..].find(key)? + start_offset;
    let mut chars = accumulated[key_idx + key.len()..].char_indices();
    
    let mut found_colon = false;
    for (i, c) in chars {
        if c.is_whitespace() { continue; }
        if c == ':' && !found_colon {
            found_colon = true;
            continue;
        }
        if c == '"' && found_colon {
            return Some(key_idx + key.len() + i + 1);
        }
        // If we hit anything else before the quote (e.g. invalid json), bail out
        return None;
    }
    None
}

fn decode_json_string_prefix(raw: &str) -> (String, bool, usize) {
    let mut out = String::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (out, true, i + 1),
            '\\' => match chars.peek().copied() {
                None => return (out, false, 0),
                Some((_, next)) => match next {
                    '"' => {
                        out.push('"');
                        chars.next();
                    }
                    '\\' => {
                        out.push('\\');
                        chars.next();
                    }
                    '/' => {
                        out.push('/');
                        chars.next();
                    }
                    'n' => {
                        out.push('\n');
                        chars.next();
                    }
                    't' => {
                        out.push('\t');
                        chars.next();
                    }
                    'r' => {
                        out.push('\r');
                        chars.next();
                    }
                    'b' => {
                        out.push('\u{8}');
                        chars.next();
                    }
                    'f' => {
                        out.push('\u{c}');
                        chars.next();
                    }
                    'u' => {
                        chars.next(); // consume 'u'
                        let mut hex = String::with_capacity(4);
                        // Distinguish "ran out of input" (genuinely
                        // incomplete -- more text may still resolve it) from
                        // "a non-hex character arrived" (malformed; no amount
                        // of further text will ever complete a valid escape
                        // here — test-critique C-001: the two were
                        // conflated, which could stall the live display
                        // forever on adversarial/buggy backend output).
                        let mut malformed = false;
                        let mut incomplete = false;
                        for _ in 0..4 {
                            match chars.peek().copied() {
                                Some((_, hc)) if hc.is_ascii_hexdigit() => {
                                    hex.push(hc);
                                    chars.next();
                                }
                                Some(_) => {
                                    malformed = true;
                                    break;
                                }
                                None => {
                                    incomplete = true;
                                    break;
                                }
                            }
                        }
                        if incomplete {
                            // Hold back everything from the backslash
                            // onward; wait for more text.
                            return (out, false, 0);
                        }
                        if malformed {
                            // Not valid JSON (shouldn't occur under
                            // strict-schema enforcement) — stop decoding
                            // this field cleanly rather than stalling
                            // forever; treat it as if the string ended here.
                            return (out, true, i);
                        }
                        if let Ok(cp) = u32::from_str_radix(&hex, 16)
                            && let Some(ch) = char::from_u32(cp)
                        {
                            out.push(ch);
                        }
                    }
                    _ => { /* unrecognized escape (shouldn't occur for well-formed JSON); skip the backslash defensively */
                    }
                },
            },
            other => out.push(other),
        }
    }
    (out, false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ToolNamed` fires exactly once, even across many subsequent `scan()`
    /// calls with growing accumulated text.
    #[test]
    fn tool_named_fires_once() {
        let mut scanner = ConstrainedJsonScanner::new();
        let full = r#"{"thought":"","tool":"task_complete","args":{"summary":"done"}}"#;
        let mut all_tool_named = Vec::new();
        for end in [10, 20, 30, full.len()] {
            let deltas = scanner.scan(&full[..end.min(full.len())]);
            all_tool_named.extend(
                deltas
                    .into_iter()
                    .filter(|d| matches!(d, StreamDelta::ToolNamed(_))),
            );
        }
        assert_eq!(
            all_tool_named,
            vec![StreamDelta::ToolNamed("task_complete".to_string())]
        );
    }

    /// `task_complete`'s summary streams incrementally: feeding growing
    /// prefixes, the cumulative concatenation of emitted `Text` deltas
    /// equals the full summary, with no repeated characters.
    #[test]
    fn summary_streams_incrementally() {
        let full = r#"{"thought":"","tool":"task_complete","args":{"summary":"Hello, world!"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let mut collected = String::new();
        for end in 1..=full.len() {
            if !full.is_char_boundary(end) {
                continue;
            }
            for delta in scanner.scan(&full[..end]) {
                if let StreamDelta::Text(t) = delta {
                    collected.push_str(&t);
                }
            }
        }
        assert_eq!(collected, "Hello, world!");
    }

    /// A non-`task_complete` tool (e.g. `write_file`) never emits `Text` for
    /// its args — only the `ToolNamed` activity signal fires.
    #[test]
    fn non_task_complete_tool_never_emits_text() {
        let full = r#"{"thought":"","tool":"write_file","args":{"path":"a.txt","content":"secret data"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let mut texts = Vec::new();
        let mut tool_named = Vec::new();
        for end in [8, 16, 24, 40, full.len()] {
            for delta in scanner.scan(&full[..end.min(full.len())]) {
                match delta {
                    StreamDelta::Text(t) => texts.push(t),
                    StreamDelta::ToolNamed(n) => tool_named.push(n),
                    StreamDelta::Thought(_) => {}
                }
            }
        }
        assert!(
            texts.is_empty(),
            "no Text deltas for a non-task_complete tool"
        );
        assert_eq!(tool_named, vec!["write_file".to_string()]);
    }

    /// Regression pinning the false-positive-safety argument itself (plan
    /// critique C-002): an `args` string value containing text that LOOKS
    /// like a tool/task_complete reference (`"path":"my_task_complete_file.txt"`)
    /// must never be misread as the tool name or trigger summary extraction.
    /// Valid JSON syntax makes a literal `"tool":"` decoy inside a string
    /// value structurally impossible (an unescaped `"` can't appear inside a
    /// string), but the ordinary substring "task_complete" as plain text
    /// content is a real, cheap thing to confirm doesn't confuse the
    /// tool-identity or summary-field detection. `action_schema` also always
    /// emits `tool` as the object's first key (ADR-016), independently
    /// guaranteeing the real key is seen first regardless.
    #[test]
    fn tool_field_always_precedes_args_content() {
        let full = r#"{"thought":"","tool":"write_file","args":{"path":"my_task_complete_file.txt","content":"summary text unrelated"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let mut tool_named = Vec::new();
        let mut texts = Vec::new();
        for delta in scanner.scan(full) {
            match delta {
                StreamDelta::ToolNamed(n) => tool_named.push(n),
                StreamDelta::Text(t) => texts.push(t),
                StreamDelta::Thought(_) => {}
            }
        }
        assert_eq!(tool_named, vec!["write_file".to_string()]);
        assert!(
            texts.is_empty(),
            "no Text delta for a non-task_complete tool, even with decoy-like content"
        );
    }

    /// Escaped characters (`\"`, `\\`, `\n`, `\t`) decode to their correct
    /// literal characters.
    #[test]
    fn escaped_characters_decode_correctly() {
        let full = r#"{"thought":"","tool":"task_complete","args":{"summary":"a \"quote\", a \\backslash, a\nline, a\ttab"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let mut collected = String::new();
        for delta in scanner.scan(full) {
            if let StreamDelta::Text(t) = delta {
                collected.push_str(&t);
            }
        }
        assert_eq!(collected, "a \"quote\", a \\backslash, a\nline, a\ttab");
    }

    /// A trailing lone `\` (ambiguous: could start any escape) is held back
    /// until a later call resolves it — no wrong/garbled output.
    #[test]
    fn ambiguous_trailing_escape_is_held_back() {
        let mut scanner = ConstrainedJsonScanner::new();
        let partial = r#"{"thought":"","tool":"task_complete","args":{"summary":"back\"#;
        let deltas = scanner.scan(partial);
        let text: String = deltas
            .into_iter()
            .filter_map(|d| match d {
                StreamDelta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            text, "back",
            "the trailing backslash must not be emitted yet"
        );

        // Resolve it: the backslash was actually escaping a quote.
        let resolved = r#"{"thought":"","tool":"task_complete","args":{"summary":"back\"slash""#;
        let more: String = scanner
            .scan(resolved)
            .into_iter()
            .filter_map(|d| match d {
                StreamDelta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(more, "\"slash", "resolves correctly with no corruption");
    }

    /// A `\uXXXX` escape split across `scan()` calls at each possible cut
    /// point holds back correctly at every point, only decoding once all 4
    /// hex digits are present.
    #[test]
    fn ambiguous_trailing_unicode_escape_is_held_back() {
        let prefix = r#"{"thought":"","tool":"task_complete","args":{"summary":"x"#;
        // A == 'A'
        let cuts = ["\\", "\\u", "\\u0", "\\u00", "\\u004"];
        for cut in cuts {
            let mut scanner = ConstrainedJsonScanner::new();
            let partial = format!("{prefix}{cut}");
            let text: String = scanner
                .scan(&partial)
                .into_iter()
                .filter_map(|d| match d {
                    StreamDelta::Text(t) => Some(t),
                    _ => None,
                })
                .collect();
            assert_eq!(
                text, "x",
                "cut {cut:?} must not emit any part of the incomplete escape"
            );
        }

        // Fully resolved.
        let mut scanner = ConstrainedJsonScanner::new();
        let full = format!("{prefix}\\u0041\"");
        let text: String = scanner
            .scan(&full)
            .into_iter()
            .filter_map(|d| match d {
                StreamDelta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(text, "xA");
    }

    /// Test-critique C-001: a genuinely MALFORMED `\u` escape (not merely
    /// incomplete — more text will never resolve it, since the character
    /// after `\u12` is not a hex digit) must stop decoding cleanly rather
    /// than stalling forever waiting for a resolution that can never come.
    #[test]
    fn malformed_unicode_escape_stops_gracefully() {
        let full = r#"{"thought":"","tool":"task_complete","args":{"summary":"x\u12XY more text"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let text: String = scanner
            .scan(full)
            .into_iter()
            .filter_map(|d| match d {
                StreamDelta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            text, "x",
            "only the text before the malformed escape decodes"
        );

        // A further scan call with the SAME (or even more) text must not
        // hang, re-emit, or emit garbage -- the scanner treats the malformed
        // point as if the string had ended there.
        let more: Vec<StreamDelta> = scanner.scan(full);
        assert!(
            more.is_empty(),
            "scanner must not stall or re-emit: {more:?}"
        );
    }

    /// The closing unescaped quote stops emission for that field, even if
    /// more accumulated text (the rest of the JSON object) follows.
    #[test]
    fn closing_quote_stops_emission() {
        let full = r#"{"thought":"","tool":"task_complete","args":{"summary":"done"}}"#;
        let mut scanner = ConstrainedJsonScanner::new();
        let text: String = scanner
            .scan(full)
            .into_iter()
            .filter_map(|d| match d {
                StreamDelta::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(text, "done");

        // A further scan call (e.g. the caller re-scanning the same final
        // text) must not re-emit or emit trailing `}}` as if it were prose.
        let more: Vec<StreamDelta> = scanner.scan(full);
        assert!(more.is_empty());
    }
}
