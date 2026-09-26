//! T-12605 unit tests for the pure request transform.

use super::*;

fn read_file_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read a text file.",
            "parameters": {
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }
        }
    })
}

fn request(messages: Value) -> Value {
    json!({
        "model": "local",
        "messages": messages,
        "tools": [read_file_tool()],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "stream": true,
        "max_tokens": 512,
        "temperature": 0.7,
        "chat_template_kwargs": {"enable_thinking": false}
    })
}

fn constrained(request: &Value) -> Constrained {
    match transform(request).unwrap() {
        Transformed::Constrained(constrained) => *constrained,
        Transformed::PassThrough => panic!("expected constrained mode"),
    }
}

fn base_messages() -> Value {
    json!([
        {"role": "system", "content": "You are Hermes."},
        {"role": "user", "content": "What is in notes.txt?"}
    ])
}

fn tool_round_messages() -> Value {
    json!([
        {"role": "system", "content": "You are Hermes."},
        {"role": "user", "content": "What is in notes.txt?"},
        {
            "role": "assistant",
            "content": null,
            "reasoning_content": "I should read the file.",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "read_file", "arguments": "{\"path\":\"notes.txt\"}"}
            }]
        },
        {"role": "tool", "tool_call_id": "call_1", "content": "buy milk"}
    ])
}

#[test]
fn constrained_transform_rewrites_request() {
    let out = constrained(&request(base_messages()));
    let body = out.upstream_body.as_object().unwrap();
    assert!(!body.contains_key("tools"));
    assert!(!body.contains_key("tool_choice"));
    assert!(!body.contains_key("parallel_tool_calls"));
    assert_eq!(body["stream"], json!(true));
    let format = &body["response_format"];
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["json_schema"]["name"], "ferric_action");
    assert_eq!(format["json_schema"]["strict"], json!(true));
    let branches: Vec<_> = format["json_schema"]["schema"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["properties"]["tool"]["const"].as_str().unwrap())
        .collect();
    assert_eq!(branches, ["read_file", "task_complete"]);
    assert_eq!(out.admitted, ["read_file", "task_complete"]);
    assert_eq!(out.tools_offered, 1);
    assert!(out.client_stream);

    let system = out.upstream_messages()[0]["content"].as_str().unwrap();
    assert!(system.starts_with("You are Hermes.\n\n"));
    assert!(system.contains(CONSTRAINED_TEACHING));
    assert!(system.ends_with(
        "\n\nAvailable tools:\n- read_file: Read a text file.\n- task_complete: Reply to the user: answer, ask a clarifying question, or report that the work is done. Args: {\"summary\": string} - your complete reply to the user.\n"
    ));
}

#[test]
fn constrained_transform_inserts_system_when_absent() {
    let out = constrained(&request(json!([{"role": "user", "content": "hi"}])));
    let messages = out.upstream_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert!(
        messages[0]["content"]
            .as_str()
            .unwrap()
            .starts_with(CONSTRAINED_TEACHING)
    );
    assert_eq!(messages[1], json!({"role": "user", "content": "hi"}));
}

#[test]
fn constrained_transform_forwards_unknown_fields() {
    let out = constrained(&request(base_messages()));
    assert_eq!(out.upstream_body["model"], "local");
    assert_eq!(out.upstream_body["max_tokens"], 512);
    assert_eq!(out.upstream_body["temperature"], json!(0.7));
    assert_eq!(
        out.upstream_body["chat_template_kwargs"],
        json!({"enable_thinking": false})
    );
}

#[test]
fn passthrough_when_no_tools() {
    let mut body = request(base_messages());
    body["tools"] = json!([]);
    assert_eq!(transform(&body).unwrap(), Transformed::PassThrough);
    body.as_object_mut().unwrap().remove("tools");
    assert_eq!(transform(&body).unwrap(), Transformed::PassThrough);
}

#[test]
fn passthrough_when_tool_choice_none() {
    let mut body = request(base_messages());
    body["tool_choice"] = json!("none");
    assert_eq!(transform(&body).unwrap(), Transformed::PassThrough);
}

#[test]
fn passthrough_when_response_format() {
    let mut body = request(base_messages());
    body["response_format"] = json!({"type": "json_object"});
    assert_eq!(transform(&body).unwrap(), Transformed::PassThrough);
}

#[test]
fn tool_choice_required_withholds_reply_and_named_function_admits_one() {
    let mut body = request(base_messages());
    body["tool_choice"] = json!("required");
    assert_eq!(constrained(&body).admitted, ["read_file"]);
    body["tool_choice"] = json!({"type": "function", "function": {"name": "read_file"}});
    assert_eq!(constrained(&body).admitted, ["read_file"]);
    body["tool_choice"] = json!({"type": "function", "function": {"name": "missing"}});
    assert_eq!(
        transform(&body).unwrap_err(),
        TransformError::ToolChoiceNotOffered("missing".to_string())
    );
}

#[test]
fn history_tool_call_projects_to_canonical_action() {
    let out = constrained(&request(tool_round_messages()));
    assert_eq!(
        out.upstream_messages()[2],
        json!({
            "role": "assistant",
            "content": "{\"thought\":\"I should read the file.\",\"tool\":\"read_file\",\"args\":{\"path\":\"notes.txt\"}}"
        })
    );
}

#[test]
fn history_tool_message_projects_to_tool_result_text() {
    let out = constrained(&request(tool_round_messages()));
    assert_eq!(
        out.upstream_messages()[3],
        json!({"role": "user", "content": "[tool_result for read_file] buy milk"})
    );
}

#[test]
fn history_final_answer_projects_to_task_complete() {
    let mut messages = tool_round_messages();
    messages
        .as_array_mut()
        .unwrap()
        .push(json!({"role": "assistant", "content": "It says: buy milk."}));
    let out = constrained(&request(messages));
    assert_eq!(
        out.upstream_messages()[4],
        json!({
            "role": "assistant",
            "content": "{\"thought\":\"\",\"tool\":\"task_complete\",\"args\":{\"summary\":\"It says: buy milk.\"}}"
        })
    );
}

#[test]
fn orphan_tool_call_id_is_rejected() {
    let mut messages = tool_round_messages();
    messages[3]["tool_call_id"] = json!("call_unknown");
    assert_eq!(
        transform(&request(messages)).unwrap_err(),
        TransformError::OrphanToolResult {
            index: 3,
            id: "call_unknown".to_string()
        }
    );
}

#[test]
fn non_object_arguments_are_rejected() {
    let mut messages = tool_round_messages();
    messages[2]["tool_calls"][0]["function"]["arguments"] = json!("[1, 2]");
    assert_eq!(
        transform(&request(messages)).unwrap_err(),
        TransformError::ArgumentsNotObject {
            index: 2,
            name: "read_file".to_string()
        }
    );
}

#[test]
fn transform_is_deterministic() {
    let body = request(tool_round_messages());
    let first = constrained(&body);
    let second = constrained(&body);
    assert_eq!(
        serde_json::to_vec(&first.upstream_body).unwrap(),
        serde_json::to_vec(&second.upstream_body).unwrap()
    );
    assert_eq!(first.schema_hash, second.schema_hash);
    assert_eq!(first.tool_catalog_hash, second.tool_catalog_hash);
}

#[test]
fn appended_turns_preserve_rendered_prefix() {
    let earlier = constrained(&request(tool_round_messages()));
    let mut messages = tool_round_messages();
    let list = messages.as_array_mut().unwrap();
    list.push(json!({"role": "assistant", "content": "It says: buy milk."}));
    list.push(json!({"role": "user", "content": "Thanks. Anything else?"}));
    let later = constrained(&request(messages));
    let earlier_hashes = message_hashes(earlier.upstream_messages());
    let later_hashes = message_hashes(later.upstream_messages());
    assert!(later_hashes.len() > earlier_hashes.len());
    assert_eq!(
        &later_hashes[..earlier_hashes.len()],
        earlier_hashes.as_slice()
    );
    assert_ne!(
        prefix_hash(earlier.upstream_messages()),
        prefix_hash(later.upstream_messages())
    );
}
