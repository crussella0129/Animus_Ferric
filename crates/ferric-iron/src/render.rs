//! The constrained protocol's prompt-side conventions (T-12601, INT-0011).
//!
//! Byte-for-byte the formats `ferric-loop` has always sent, extracted so that a
//! second harness (the Hermes valve) speaks exactly the protocol Ferric's
//! recorded results used.

use crate::types::ToolDescriptor;

/// The tool listing appended to the default system prompt: a blank line, the
/// `Available tools:` heading, then one `- name: description` line per tool,
/// in order. Callers append the control descriptors after the registry tools.
pub fn render_tool_listing(tools: &[ToolDescriptor]) -> String {
    let mut listing = String::from("\n\nAvailable tools:\n");
    for tool in tools {
        listing.push_str(&format!("- {}: {}\n", tool.name, tool.description));
    }
    listing
}

/// How a tool result is replayed to the model under the constrained (and XML)
/// protocols: a user message naming the tool, then its (already truncated)
/// output.
pub fn tool_result_text(name: &str, output: &str) -> String {
    format!("[tool_result for {name}] {output}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn descriptor(name: &str, description: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: json!({"type": "object"}),
        }
    }

    /// The literal bytes `ferric-loop` produced before the move.
    #[test]
    fn tool_listing_bytes_match_legacy_format() {
        let listing = render_tool_listing(&[
            descriptor("read_file", "Read a file."),
            descriptor("task_complete", "Finish."),
        ]);
        assert_eq!(
            listing,
            "\n\nAvailable tools:\n- read_file: Read a file.\n- task_complete: Finish.\n"
        );
    }

    #[test]
    fn tool_result_text_matches_legacy_format() {
        assert_eq!(
            tool_result_text("read_file", "x"),
            "[tool_result for read_file] x"
        );
    }
}
