use async_trait::async_trait;
use pyo3::prelude::*;
use std::path::PathBuf;

use ferric_core::{Message, Role, ToolCall};
use crate::traits::Provider;
use crate::types::{
    Capabilities, Completion, CompletionRequest, ProviderError, SamplingParams, ToolDescriptor,
};

pub struct PythonConfig {
    pub model_dir: PathBuf,
}

pub struct PythonProvider {
    config: PythonConfig,
}

impl PythonProvider {
    pub fn new(config: PythonConfig) -> Self {
        pyo3::prepare_freethreaded_python();
        Self { config }
    }
}

#[async_trait]
impl Provider for PythonProvider {
    fn id(&self) -> &str {
        "python"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supports_native_tool_calls: false,
            exposes_logits: false,
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Completion, ProviderError> {
        request.validate()?;

        let model_dir_str = self.config.model_dir.display().to_string();
        
        // Serialize the CompletionRequest so Python can easily parse it
        // We will manually serialize tools and messages if `CompletionRequest` doesn't derive Serialize
        // Wait, does CompletionRequest derive Serialize? Let's assume it might not.
        // It's safer to build a generic JSON payload.
        let messages_json: Vec<_> = request.messages.iter().map(|msg| {
            let role_str = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            let mut map = serde_json::json!({ "role": role_str });
            if let Some(text) = &msg.text {
                map["content"] = serde_json::json!(text);
            }
            if !msg.tool_calls.is_empty() {
                let calls: Vec<_> = msg.tool_calls.iter().map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.args).unwrap_or_default()
                        }
                    })
                }).collect();
                map["tool_calls"] = serde_json::json!(calls);
            }
            if let Some(tool_call_id) = &msg.tool_call_id {
                map["tool_call_id"] = serde_json::json!(tool_call_id);
            }
            map
        }).collect();

        let tools_json: Vec<_> = request.tools.iter().map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema
                }
            })
        }).collect();

        let payload = serde_json::json!({
            "messages": messages_json,
            "tools": tools_json,
            "temperature": request.sampling.temperature,
            "max_tokens": request.sampling.max_tokens,
            "top_p": request.sampling.top_p,
        });

        let payload_str = payload.to_string();

        let py_result: Result<String, String> = tokio::task::spawn_blocking(move || {
            Python::with_gil(|py| -> Result<String, PyErr> {
                let sys_path = py.import_bound("sys")?.getattr("path")?;
                let current_dir = std::env::current_dir().unwrap_or_default();
                let script_dir = current_dir.join("crates").join("ferric-provider").join("python");
                sys_path.call_method1("append", (script_dir.display().to_string(),))?;

                let inference_mod = py.import_bound("inference")?;
                let result: String = inference_mod
                    .getattr("generate_completion")?
                    .call1((model_dir_str, payload_str))?
                    .extract()?;

                Ok(result)
            }).map_err(|e| Python::with_gil(|py| e.to_string()))
        })
        .await
        .map_err(|e| ProviderError::Backend(format!("Task panic: {}", e)))?;

        let response_str = py_result.map_err(|e| ProviderError::Backend(format!("Python Error: {}", e)))?;
        
        let json_res: serde_json::Value = serde_json::from_str(&response_str)
            .map_err(|e| ProviderError::Backend(format!("Failed to parse Python response JSON: {}", e)))?;

        let text = json_res["text"].as_str().map(|s| s.to_string());
        let truncated = json_res["truncated"].as_bool().unwrap_or(false);
        let input_tokens = json_res["input_tokens"].as_u64().map(|v| v as u32);
        let output_tokens = json_res["output_tokens"].as_u64().map(|v| v as u32);

        let mut tool_calls = Vec::new();
        if let Some(tcs) = json_res["tool_calls"].as_array() {
            for tc in tcs {
                let id = tc["id"].as_str().unwrap_or_default().to_string();
                let name = tc["name"].as_str().unwrap_or_default().to_string();
                let args_str = tc["arguments"].as_str().unwrap_or_default();
                let args = serde_json::from_str(args_str)
                    .unwrap_or_else(|_| serde_json::Value::String(args_str.to_string()));

                tool_calls.push(ToolCall { id, name, args });
            }
        }

        Ok(Completion {
            message: Message {
                role: Role::Assistant,
                text,
                tool_calls,
                tool_call_id: None,
            },
            input_tokens,
            output_tokens,
            truncated,
        })
    }
}
