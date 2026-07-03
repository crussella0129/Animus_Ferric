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
}
