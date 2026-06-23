use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use crate::traits::Provider;
use crate::types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, ToolDescriptor,
};
use ferric_core::{Message, Role, ToolCall};

pub struct OpenAiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "ollama".to_string(),
            model: "gemma4:e4b".to_string(),
        }
    }
}

pub struct OpenAiProvider {
    config: OpenAiConfig,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    fn map_message(msg: &Message) -> serde_json::Value {
        let role_str = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        let mut out = json!({ "role": role_str });

        if let Some(text) = &msg.text {
            out["content"] = json!(text);
        } else if msg.role == Role::Assistant && !msg.tool_calls.is_empty() {
            // OpenAI requires an explicit empty string or null for content if only making tool calls
            out["content"] = json!("");
        }

        if !msg.tool_calls.is_empty() {
            let calls: Vec<_> = msg
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.args).unwrap_or_default()
                        }
                    })
                })
                .collect();
            out["tool_calls"] = json!(calls);
        }

        if let Some(tool_call_id) = &msg.tool_call_id {
            out["tool_call_id"] = json!(tool_call_id);
        }

        out
    }

    fn map_tool(tool: &ToolDescriptor) -> serde_json::Value {
        json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema
            }
        })
    }

    /// Build the `/chat/completions` request body. Pure (no network) so the
    /// constraint/tool wiring is unit-testable. ADR-010 holds by construction:
    /// a `Constraint` and `tools` are mutually exclusive, so they live in
    /// disjoint match arms — a constrained request never carries `tools`.
    ///
    /// `JsonSchema` becomes server-enforced `response_format` (llama.cpp /
    /// OpenAI structured outputs); this is where "the harness owns decoding"
    /// is actually true for the HTTP valve. The schema is NOT injected into the
    /// prompt by the server, so callers must still describe the tools in the
    /// system prompt (the loop's `ConstrainedJson` path does, via ferric-prompt).
    fn build_body(&self, request: &CompletionRequest) -> serde_json::Value {
        let messages: Vec<_> = request.messages.iter().map(Self::map_message).collect();

        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": request.sampling.max_tokens,
            "temperature": request.sampling.temperature,
            "top_p": request.sampling.top_p,
        });

        match &request.constraint {
            Some(Constraint::JsonSchema(schema)) => {
                body["response_format"] = json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": "ferric_action",
                        "schema": schema,
                        "strict": true,
                    }
                });
            }
            // llama.cpp accepts a GBNF/Lark grammar via the `grammar` field.
            Some(Constraint::Lark(grammar)) => {
                body["grammar"] = json!(grammar);
            }
            // No standard OpenAI-compatible field carries a bare regex; the
            // loop only ever emits `JsonSchema` today, so this is unreachable
            // in practice and deliberately left unconstrained rather than faked.
            Some(Constraint::Regex(_)) => {}
            None => {
                if !request.tools.is_empty() {
                    let tools: Vec<_> = request.tools.iter().map(Self::map_tool).collect();
                    body["tools"] = json!(tools);
                    body["tool_choice"] = json!("auto");
                }
            }
        }

        body
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai-http"
    }

    fn capabilities(&self) -> Capabilities {
        // Honest: the HTTP valve enforces a JSON-Schema constraint server-side
        // via `response_format`, AND speaks native tool calling. The request's
        // `constraint`/`tools` (ADR-010 mutually exclusive) selects which.
        Capabilities {
            supports_native_tool_calls: true,
            supports_constraint: true,
            exposes_logits: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError> {
        request.validate()?;

        let body = self.build_body(&request);

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        let response = match response {
            Ok(res) => res,
            Err(e) => {
                return Err(ProviderError::RetryableBackend(format!(
                    "Network error: {}",
                    e
                )));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Backend(format!("HTTP {}: {}", status, text)));
        }

        let json_res: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Backend(format!("Failed to parse response JSON: {}", e)))?;

        let choice = json_res["choices"]
            .as_array()
            .and_then(|c| c.first())
            .ok_or_else(|| ProviderError::Backend("Response carried no choices".to_string()))?;

        let message = &choice["message"];
        let content = message["content"].as_str().map(|s| s.to_string());

        let mut tool_calls = Vec::new();
        if let Some(tcs) = message["tool_calls"].as_array() {
            for tc in tcs {
                let id = tc["id"].as_str().unwrap_or_default().to_string();
                let name = tc["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or_default();

                let args = serde_json::from_str(args_str)
                    .unwrap_or_else(|_| serde_json::Value::String(args_str.to_string()));

                tool_calls.push(ToolCall { id, name, args });
            }
        }

        let finish_reason = choice["finish_reason"].as_str().unwrap_or("");
        let truncated = finish_reason == "length";

        let usage = &json_res["usage"];
        let input_tokens = usage["prompt_tokens"].as_u64().map(|v| v as u32);
        let output_tokens = usage["completion_tokens"].as_u64().map(|v| v as u32);

        Ok(Completion {
            message: Message {
                role: Role::Assistant,
                text: content,
                tool_calls,
                tool_call_id: None,
            },
            input_tokens,
            output_tokens,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SamplingParams;

    fn provider() -> OpenAiProvider {
        OpenAiProvider::new(OpenAiConfig::default())
    }

    fn base_request() -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        }
    }

    fn tool() -> ToolDescriptor {
        ToolDescriptor {
            name: "read_file".to_string(),
            description: "read a file".to_string(),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }
    }

    #[test]
    fn build_body_constraint_emits_response_format() {
        // WHEN a JSON-Schema constraint is present THEN response_format carries
        // it (strict) and tools are absent (ADR-010).
        let schema = json!({"type": "object", "required": ["tool", "args"]});
        let mut req = base_request();
        req.constraint = Some(Constraint::JsonSchema(schema.clone()));
        let body = provider().build_body(&req);

        assert_eq!(body["response_format"]["type"], json!("json_schema"));
        assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
        assert_eq!(
            body["response_format"]["json_schema"]["strict"],
            json!(true)
        );
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn build_body_tools_no_response_format() {
        // WHEN tools are present and no constraint THEN tools/tool_choice are
        // set and response_format is absent.
        let mut req = base_request();
        req.tools = vec![tool()];
        let body = provider().build_body(&req);

        assert!(body["tools"].is_array());
        assert_eq!(body["tool_choice"], json!("auto"));
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn capabilities_advertise_constraint_and_native() {
        let caps = provider().capabilities();
        assert!(caps.supports_constraint);
        assert!(caps.supports_native_tool_calls);
    }
}
