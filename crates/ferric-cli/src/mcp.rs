//! `ferric mcp` — the MCP-stdio server (ADR-046, the ADR-005 security call for
//! ADR-012). JSON-RPC 2.0 over newline-delimited stdin/stdout: stdout carries
//! protocol frames ONLY (never a log line — that would corrupt the stream);
//! all diagnostics go to stderr. This module owns the wire framing; handlers
//! land in later tasks of the same sprint.
//!
//! Built incrementally across sprint 36's T-3603-3606; `Command::Mcp` (T-3606)
//! is what makes this module reachable from `main`, so `dead_code` is allowed
//! until then (each intermediate task is independently tested via `cargo test
//! -p ferric-cli mcp::`, just not yet reachable from the binary's entrypoint).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One incoming line, parsed. A request expects a response (`id` present,
/// even if `null`); a notification has no `id` field at all and expects none.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl RpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

impl RpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Parse one input line. On success, the caller distinguishes a request from
/// a notification via `is_notification()`. On failure, a `-32700 Parse error`
/// response is returned with `id: null` (no id could be recovered from
/// unparseable input, per the JSON-RPC 2.0 spec).
pub fn parse_line(line: &str) -> Result<RpcRequest, Box<RpcResponse>> {
    serde_json::from_str::<RpcRequest>(line).map_err(|e| {
        Box::new(RpcResponse::error(
            Value::Null,
            PARSE_ERROR,
            format!("parse error: {e}"),
        ))
    })
}

/// Serialize a response as a single line (no embedded newline) for stdout.
pub fn render_line(resp: &RpcResponse) -> String {
    serde_json::to_string(resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal error serializing response"}}"#
            .to_string()
    })
}

/// Fixed, hardcoded protocol version (ADR-005 spirit: no runtime negotiation
/// surface). A client requesting a different version gets this one back and
/// decides for itself whether to proceed.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

pub fn handle_initialize(id: Value) -> RpcResponse {
    RpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "ferric", "version": env!("CARGO_PKG_VERSION")},
        }),
    )
}

/// The one MCP tool this server exposes. Deliberately has NO `workspace`,
/// `backend`, or `model` field — those are `ferric mcp` launch-time CLI flags,
/// fixed for the server's lifetime (ADR-046: the containment guarantee is
/// structural, not something a handler must remember to enforce).
pub fn ferric_query_tool_schema() -> Value {
    serde_json::json!({
        "name": "ferric_query",
        "description": "Run a one-shot, workspace-scoped, policy-scaled coding/agentic \
            task against the local model this Ferric MCP server was launched with. The \
            full constrained agent loop runs (planning, tool calls, guard-enforced \
            permission checks) inside Ferric's own workspace boundary; only the final \
            answer is returned.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task to perform.",
                },
                "files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional paths to attach. Text/code folds into the \
                        prompt; images/audio/video attach as media if the server's \
                        declared --modality supports it.",
                },
            },
            "required": ["prompt"],
        },
    })
}

pub fn handle_tools_list(id: Value) -> RpcResponse {
    RpcResponse::success(
        id,
        serde_json::json!({"tools": [ferric_query_tool_schema()]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_request_line() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let req = parse_line(line).unwrap();
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.id, Some(Value::from(1)));
        assert!(!req.is_notification());
    }

    #[test]
    fn parses_notification_without_id() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req = parse_line(line).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn malformed_line_yields_parse_error() {
        let resp = parse_line("not json at all").unwrap_err();
        assert_eq!(resp.error.as_ref().unwrap().code, PARSE_ERROR);
        assert_eq!(resp.id, Value::Null);
    }

    #[test]
    fn render_line_has_no_embedded_newline() {
        let resp = RpcResponse::success(Value::from(1), serde_json::json!({"ok": true}));
        let line = render_line(&resp);
        assert!(!line.contains('\n'));
        assert!(line.contains("\"ok\":true"));
    }

    #[test]
    fn initialize_returns_fixed_version_and_tools_capability() {
        let resp = handle_initialize(Value::from(1));
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "ferric");
    }

    #[test]
    fn tools_list_has_exactly_one_tool_named_ferric_query() {
        let resp = handle_tools_list(Value::from(1));
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "ferric_query");
    }

    /// The structural containment-guarantee regression test (ADR-046): the
    /// exposed tool's schema must never grow a workspace/backend/model field,
    /// since that would let a caller redirect containment per-call instead of
    /// it being fixed at `ferric mcp` launch.
    #[test]
    fn ferric_query_schema_has_no_workspace_backend_or_model_field() {
        let schema = ferric_query_tool_schema();
        let properties = &schema["inputSchema"]["properties"];
        assert!(properties.get("workspace").is_none());
        assert!(properties.get("backend").is_none());
        assert!(properties.get("model").is_none());
        assert!(properties.get("prompt").is_some());
    }
}
