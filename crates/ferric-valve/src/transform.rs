//! The pure request transform (T-12605, INT-0012 AC-1).
//!
//! A Hermes chat-completions request either goes upstream unchanged
//! (pass-through) or is rewritten into Ferric's constrained action protocol:
//! one JSON-Schema `response_format` over every offered tool plus the
//! final-answer control, tool descriptions rendered into the system message,
//! and the history projected into the convention Ferric's own loop replays
//! (each action as its canonical JSON, each result as a
//! `[tool_result for NAME] …` user message).
//!
//! No I/O and no clock: the same request always yields byte-identical upstream
//! bytes, and appending turns only appends projected messages, so a backend's
//! prefix cache survives from one request to the next.

use std::collections::BTreeMap;

use ferric_iron::terminator::TASK_COMPLETE;
use ferric_iron::{ToolDescriptor, action_schema, render_tool_listing, tool_result_text};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// The teaching text appended to the system message in constrained mode. It
/// states the action format the grammar enforces and how to reply to the
/// user; the tool listing follows it.
pub const CONSTRAINED_TEACHING: &str = "\
You act by emitting exactly one JSON object and nothing else: no prose, no markdown fences. \
The object has three keys, in this order:\n\
1. \"thought\": your brief reasoning about what to do next.\n\
2. \"tool\": the name of one tool listed below.\n\
3. \"args\": an object with that tool's arguments.\n\
To reply to the user, whether answering, asking a question, or reporting that the work is done, \
use the tool \"task_complete\" with your complete reply in \"summary\".";

/// Why a request cannot be transformed. Each maps to HTTP 400 at the server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransformError {
    #[error("request body must be a JSON object")]
    NotAnObject,
    #[error("`messages` must be an array of objects")]
    BadMessages,
    #[error("invalid tools: {0}")]
    Tools(String),
    #[error("tool_choice {0} is not supported by the constrained valve")]
    UnsupportedToolChoice(String),
    #[error("tool_choice names {0:?}, which is not an offered tool")]
    ToolChoiceNotOffered(String),
    #[error("message {index}: tool_call_id {id:?} matches no earlier assistant tool call")]
    OrphanToolResult { index: usize, id: String },
    #[error("message {index}: tool call {name:?} arguments are not a JSON object")]
    ArgumentsNotObject { index: usize, name: String },
    #[error("message {index}: tool call has no function name")]
    ToolCallWithoutName { index: usize },
}

/// The upstream request for a constrained turn, plus the content-free
/// identities its receipt records.
#[derive(Debug, Clone, PartialEq)]
pub struct Constrained {
    pub upstream_body: Value,
    /// Whether the client asked for SSE. Upstream always streams.
    pub client_stream: bool,
    /// Names the grammar admits, in branch order (offered tools, then controls).
    pub admitted: Vec<String>,
    pub tools_offered: usize,
    pub tool_catalog_hash: String,
    pub schema_hash: String,
}

impl Constrained {
    /// The rendered upstream messages.
    pub fn upstream_messages(&self) -> &[Value] {
        self.upstream_body
            .get("messages")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

/// SHA-256 over the compact JSON array of `messages`: one identity for the
/// whole rendered conversation.
pub fn prefix_hash(messages: &[Value]) -> String {
    sha256_hex(&serde_json::to_vec(messages).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transformed {
    Constrained(Box<Constrained>),
    /// Forward the original bytes unchanged.
    PassThrough,
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// One content-free hash per message, over its compact JSON, in order. A
/// request "extends" another when the other's list is a prefix of its own.
pub fn message_hashes(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .map(|message| sha256_hex(&serde_json::to_vec(message).unwrap_or_default()))
        .collect()
}

/// The final-answer control offered to Hermes: `task_complete`, whose
/// `summary` is the whole reply. Same name and field as Ferric's terminator so
/// the constrained-JSON scanner streams it, with a description suited to a
/// conversation rather than a one-sentence task summary.
pub fn final_answer_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: TASK_COMPLETE.to_string(),
        description: "Reply to the user: answer, ask a clarifying question, or report that the \
                      work is done. Args: {\"summary\": string} - your complete reply to the user."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "Your complete reply to the user" }
            },
            "required": ["summary"]
        }),
    }
}

/// Decide the mode and, for a constrained turn, build the upstream request.
pub fn transform(request: &Value) -> Result<Transformed, TransformError> {
    let body = request.as_object().ok_or(TransformError::NotAnObject)?;

    let tools = body.get("tools").and_then(Value::as_array);
    let has_tools = tools.is_some_and(|tools| !tools.is_empty());
    let tool_choice = body.get("tool_choice");
    let choice_none = matches!(tool_choice, Some(Value::String(choice)) if choice == "none");
    if !has_tools || choice_none || body.contains_key("response_format") {
        return Ok(Transformed::PassThrough);
    }

    let tools_value = body.get("tools").cloned().unwrap_or(Value::Null);
    let offered = ferric_iron::openai_tools::descriptors_from_openai_tools(&tools_value)
        .map_err(|error| TransformError::Tools(error.to_string()))?;

    // Which branches the grammar admits. `auto` (or absent) offers every tool
    // plus the reply control; `required` withholds the reply control; a named
    // function admits only that tool.
    let admitted_descriptors: Vec<ToolDescriptor> = match tool_choice {
        None => with_reply(offered.clone()),
        Some(Value::String(choice)) if choice == "auto" => with_reply(offered.clone()),
        Some(Value::String(choice)) if choice == "required" => offered.clone(),
        Some(Value::Object(object)) => {
            let name = object
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let chosen: Vec<ToolDescriptor> =
                offered.iter().filter(|d| d.name == name).cloned().collect();
            if chosen.is_empty() {
                return Err(TransformError::ToolChoiceNotOffered(name.to_string()));
            }
            chosen
        }
        Some(other) => return Err(TransformError::UnsupportedToolChoice(other.to_string())),
    };

    let schema = action_schema(&admitted_descriptors);
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or(TransformError::BadMessages)?;
    let projected = project_messages(messages, &admitted_descriptors)?;

    let mut upstream = Map::new();
    for (key, value) in body {
        match key.as_str() {
            "tools" | "tool_choice" | "parallel_tool_calls" | "messages" | "stream" => {}
            _ => {
                upstream.insert(key.clone(), value.clone());
            }
        }
    }
    upstream.insert("messages".to_string(), Value::Array(projected));
    upstream.insert("stream".to_string(), Value::Bool(true));
    upstream.insert(
        "response_format".to_string(),
        json!({
            "type": "json_schema",
            "json_schema": { "name": "ferric_action", "schema": schema, "strict": true }
        }),
    );

    Ok(Transformed::Constrained(Box::new(Constrained {
        client_stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        admitted: admitted_descriptors
            .iter()
            .map(|d| d.name.clone())
            .collect(),
        tools_offered: offered.len(),
        tool_catalog_hash: sha256_hex(&serde_json::to_vec(&tools_value).unwrap_or_default()),
        schema_hash: sha256_hex(&serde_json::to_vec(&schema).unwrap_or_default()),
        upstream_body: Value::Object(upstream),
    })))
}

fn with_reply(mut descriptors: Vec<ToolDescriptor>) -> Vec<ToolDescriptor> {
    descriptors.push(final_answer_descriptor());
    descriptors
}

/// A message's text content: a string as-is, or the concatenated `text` parts
/// of a content-part array.
fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn action_text(thought: &str, tool: &str, args: &Value) -> String {
    serde_json::to_string(&json!({ "thought": thought, "tool": tool, "args": args }))
        .unwrap_or_default()
}

/// Project Hermes's history into the constrained convention.
fn project_messages(
    messages: &[Value],
    admitted: &[ToolDescriptor],
) -> Result<Vec<Value>, TransformError> {
    let instructions = format!("{CONSTRAINED_TEACHING}{}", render_tool_listing(admitted));
    let mut projected = Vec::with_capacity(messages.len() + 1);
    let mut call_names: BTreeMap<String, String> = BTreeMap::new();
    let mut system_extended = false;

    for (index, message) in messages.iter().enumerate() {
        let object = message.as_object().ok_or(TransformError::BadMessages)?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match role {
            "system" if !system_extended => {
                let text = text_of(object.get("content"));
                projected.push(
                    json!({"role": "system", "content": format!("{text}\n\n{instructions}")}),
                );
                system_extended = true;
            }
            "assistant" => {
                let thought = object
                    .get("reasoning_content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let calls = object
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .filter(|calls| !calls.is_empty());
                match calls {
                    Some(calls) => {
                        for (position, call) in calls.iter().enumerate() {
                            let function = call.get("function").unwrap_or(&Value::Null);
                            let name = function
                                .get("name")
                                .and_then(Value::as_str)
                                .filter(|name| !name.is_empty())
                                .ok_or(TransformError::ToolCallWithoutName { index })?;
                            let arguments = function
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or("{}");
                            let args: Value = serde_json::from_str(arguments)
                                .ok()
                                .filter(Value::is_object)
                                .ok_or_else(|| TransformError::ArgumentsNotObject {
                                    index,
                                    name: name.to_string(),
                                })?;
                            if let Some(id) = call.get("id").and_then(Value::as_str) {
                                call_names.insert(id.to_string(), name.to_string());
                            }
                            let call_thought = if position == 0 { thought } else { "" };
                            projected.push(json!({
                                "role": "assistant",
                                "content": action_text(call_thought, name, &args)
                            }));
                        }
                    }
                    None => {
                        let reply = text_of(object.get("content"));
                        projected.push(json!({
                            "role": "assistant",
                            "content": action_text(thought, TASK_COMPLETE, &json!({"summary": reply}))
                        }));
                    }
                }
            }
            "tool" => {
                let id = object
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let name = call_names
                    .get(id)
                    .ok_or_else(|| TransformError::OrphanToolResult {
                        index,
                        id: id.to_string(),
                    })?;
                projected.push(json!({
                    "role": "user",
                    "content": tool_result_text(name, &text_of(object.get("content")))
                }));
            }
            _ => {
                projected.push(json!({"role": role, "content": object.get("content").cloned().unwrap_or(Value::String(String::new()))}));
            }
        }
    }
    if !system_extended {
        projected.insert(0, json!({"role": "system", "content": instructions}));
    }
    Ok(projected)
}

#[cfg(test)]
#[path = "transform_tests.rs"]
mod tests;
