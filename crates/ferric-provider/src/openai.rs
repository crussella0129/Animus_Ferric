use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, warn};

use crate::stream_scan::ConstrainedJsonScanner;
use crate::traits::Provider;
use crate::types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, StreamDelta,
    ToolDescriptor,
};
use ferric_core::{MediaPart, Message, Role, ToolCall};

/// Keep one request future alive across cancellation polls. Dropping it on
/// interruption also drops its request/response body; no detached request task
/// can continue waiting after the caller starts cleaning up its owned engine.
async fn with_cancellation<T>(
    cancel: Option<&AtomicBool>,
    operation: impl Future<Output = Result<T, ProviderError>>,
) -> Result<T, ProviderError> {
    let Some(cancel) = cancel else {
        return operation.await;
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(ProviderError::Backend("Interrupted".into()));
    }
    tokio::pin!(operation);
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            biased;
            _ = interval.tick() => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ProviderError::Backend("Interrupted".into()));
                }
            }
            result = &mut operation => return result,
        }
    }
}

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

    /// Preserve a prepared endpoint's authority: generation must not redirect
    /// its prompt or credential to a different server, and a verified loopback
    /// listener must never be reached through an ambient HTTP proxy.
    /// Legacy expert constructors retain their existing transport behavior.
    pub fn for_prepared_endpoint(config: OpenAiConfig) -> Result<Self, ProviderError> {
        let client = prepared_client_builder(&config.base_url, Client::builder())?
            .build()
            .map_err(|_| ProviderError::Backend("Cannot prepare endpoint transport".into()))?;
        Ok(Self { config, client })
    }

    fn map_message(msg: &Message) -> serde_json::Value {
        let role_str = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };

        let mut out = json!({ "role": role_str });

        if !msg.media.is_empty() {
            // Multimodal (ADR-023): an OpenAI content-parts array — the text (if
            // any) as a `text` part, then one part per media item. llama-server /
            // Ollama accept this shape over `/chat/completions`.
            let mut parts: Vec<serde_json::Value> = Vec::new();
            if let Some(text) = &msg.text {
                parts.push(json!({ "type": "text", "text": text }));
            }
            parts.extend(msg.media.iter().map(media_part_json));
            out["content"] = json!(parts);
        } else if let Some(text) = &msg.text {
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

fn prepared_client_builder(
    base: &str,
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, ProviderError> {
    let url = reqwest::Url::parse(base)
        .map_err(|_| ProviderError::InvalidRequest("Invalid prepared endpoint".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::InvalidRequest(
            "Prepared endpoint requires HTTP(S) without embedded credentials or query parameters"
                .into(),
        ));
    }
    let builder = builder.redirect(reqwest::redirect::Policy::none());
    Ok(
        if matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
            builder.no_proxy()
        } else {
            builder
        },
    )
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
            supports_media: true,
        }
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        request.validate()?;
        with_cancellation(cancel_flag.as_deref(), async {
            let body = self.build_body(&request);

            let url = format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            );

            debug!(
                model = %self.config.model,
                messages = request.messages.len(),
                constrained = request.constraint.is_some(),
                "POST /chat/completions"
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
                    warn!(error = %e, "network error contacting the inference server (retryable)");
                    return Err(ProviderError::RetryableBackend(format!(
                        "Network error: {}",
                        e
                    )));
                }
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                warn!(%status, "inference server returned an error status");
                return Err(ProviderError::Backend(format!("HTTP {}: {}", status, text)));
            }

            let json_res: serde_json::Value = response.json().await.map_err(|e| {
                ProviderError::Backend(format!("Failed to parse response JSON: {}", e))
            })?;

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

            // ADR-024 fallback: ollama's OpenAI-compatible endpoint returns a tool
            // call as plain text in `content` with `tool_calls` null. If nothing
            // parsed natively but the content is itself a tool-call object, recover
            // it so the native path sees the call instead of `no_action`.
            if tool_calls.is_empty()
                && let Some(text) = &content
                && let Some(tc) = toolcall_from_content(text)
            {
                tool_calls.push(tc);
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
                    media: Vec::new(),
                },
                input_tokens,
                output_tokens,
                truncated,
            })
        })
        .await
    }

    /// Real streaming (ADR-047): sets `stream: true`, reads the response body
    /// incrementally via `Response::chunk()` (no `stream` cargo feature or
    /// extra dependency needed — simpler than `bytes_stream()`), buffers into
    /// complete SSE lines, and feeds each `data:` line's JSON payload to a
    /// `StreamAccumulator`. Under `ConstrainedJson`, the accumulated content
    /// is re-scanned by a `ConstrainedJsonScanner` each line (extracting the
    /// tool-name/summary signals from the opaque action JSON); otherwise
    /// (`NativeTools`/`TextXml`) `delta.content` is already prose and is
    /// forwarded directly.
    async fn complete_streaming(
        &self,
        request: CompletionRequest,
        on_delta: &(dyn Fn(StreamDelta) + Sync),
        cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        request.validate()?;
        with_cancellation(cancel_flag.as_deref(), async {
            let constrained = request.constraint.is_some();
            let mut body = self.build_body(&request);
            body["stream"] = json!(true);

            let url = format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            );
            let mut response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| ProviderError::RetryableBackend(format!("Network error: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(ProviderError::Backend(format!("HTTP {status}: {text}")));
            }

            let mut acc = StreamAccumulator::new(constrained, on_delta);
            let mut decoder = SseDecoder::default();
            while let Some(bytes) = response
                .chunk()
                .await
                .map_err(|e| ProviderError::RetryableBackend(format!("stream error: {e}")))?
            {
                for line in decoder.push(&bytes)? {
                    match line {
                        SseLine::Data(data) => acc.feed_line(&data),
                        SseLine::Done => return Ok(acc.finish()),
                        SseLine::Ignored => {}
                    }
                }
            }
            decoder.finish()?;
            Ok(acc.finish())
        })
        .await
    }
}

/// Frame SSE in bytes before decoding UTF-8. A network chunk can end anywhere
/// within a code point; incomplete final bytes are errors, not replacement
/// characters silently inserted into content or native tool arguments.
#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
    scanned: usize,
    done: bool,
}

impl SseDecoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseLine>, ProviderError> {
        if self.done {
            return Ok(Vec::new());
        }
        self.pending.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut consumed = 0;
        let mut search_from = self.scanned;
        while let Some(offset) = self.pending[search_from..].iter().position(|b| *b == b'\n') {
            let end = search_from + offset;
            let line = std::str::from_utf8(&self.pending[consumed..end]).map_err(sse_utf8_error)?;
            let line = classify_sse_line(line.trim_end_matches('\r'));
            let done = line == SseLine::Done;
            lines.push(line);
            consumed = end + 1;
            search_from = consumed;
            if done {
                self.pending.clear();
                self.scanned = 0;
                self.done = true;
                return Ok(lines);
            }
        }
        self.pending.drain(..consumed);
        // Do not rescan a growing incomplete line on every tiny TCP chunk.
        self.scanned = self.pending.len();
        Ok(lines)
    }

    fn finish(self) -> Result<(), ProviderError> {
        // Preserve the existing behavior of ignoring an unterminated SSE line,
        // but still reject a malformed or truncated UTF-8 tail at EOF.
        std::str::from_utf8(&self.pending).map_err(sse_utf8_error)?;
        Ok(())
    }
}

fn sse_utf8_error(error: std::str::Utf8Error) -> ProviderError {
    ProviderError::Backend(format!("Malformed UTF-8 in SSE response: {error}"))
}

/// Classify one already-line-split piece of SSE framing. Pure — no I/O — so
/// the line-splitting/classification logic is directly unit-testable
/// separately from the async byte-stream reading loop.
#[derive(Debug, Clone, PartialEq)]
enum SseLine {
    /// A `data: <json>` line carrying one chunk's payload.
    Data(String),
    /// The `data: [DONE]` terminator.
    Done,
    /// Blank lines (frame separators) and other SSE fields (`event:`, `id:`,
    /// `: comment`) — not an error, just not a data payload.
    Ignored,
}

fn classify_sse_line(line: &str) -> SseLine {
    let Some(data) = line.strip_prefix("data:") else {
        return SseLine::Ignored;
    };
    let data = data.trim_start();
    if data == "[DONE]" {
        SseLine::Done
    } else {
        SseLine::Data(data.to_string())
    }
}

/// Accumulates SSE `data:` line payloads into a final `Completion`, firing
/// `StreamDelta`s to `on_delta` as content becomes available. Pure with
/// respect to I/O — `feed_line` takes an already-decoded line string, so this
/// is directly unit-testable with plain `&str` inputs, no networking.
struct StreamAccumulator<'a> {
    content: String,
    tool_calls: Vec<PartialToolCall>,
    finish_reason: String,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    constrained: bool,
    scanner: ConstrainedJsonScanner,
    on_delta: &'a (dyn Fn(StreamDelta) + Sync),
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    args: String,
}

impl<'a> StreamAccumulator<'a> {
    fn new(constrained: bool, on_delta: &'a (dyn Fn(StreamDelta) + Sync)) -> Self {
        Self {
            content: String::new(),
            tool_calls: Vec::new(),
            finish_reason: String::new(),
            input_tokens: None,
            output_tokens: None,
            constrained,
            scanner: ConstrainedJsonScanner::new(),
            on_delta,
        }
    }

    /// Feed one SSE `data:` line's JSON payload (the text after `data:`, not
    /// including the `[DONE]` sentinel, which the caller handles separately).
    fn feed_line(&mut self, line: &str) {
        let Ok(chunk) = serde_json::from_str::<serde_json::Value>(line) else {
            return; // malformed/ignorable line
        };
        let choice = &chunk["choices"][0];
        let delta = &choice["delta"];

        if let Some(text) = delta["content"].as_str() {
            self.content.push_str(text);
            if self.constrained {
                for d in self.scanner.scan(&self.content) {
                    (self.on_delta)(d);
                }
            } else {
                (self.on_delta)(StreamDelta::Text(text.to_string()));
            }
        }

        if let Some(tcs) = delta["tool_calls"].as_array() {
            for tc in tcs {
                let index = tc["index"].as_u64().unwrap_or(0) as usize;
                while self.tool_calls.len() <= index {
                    self.tool_calls.push(PartialToolCall::default());
                }
                let entry = &mut self.tool_calls[index];
                if let Some(id) = tc["id"].as_str() {
                    entry.id.push_str(id);
                }
                if let Some(name) = tc["function"]["name"].as_str() {
                    entry.name.push_str(name);
                }
                if let Some(args) = tc["function"]["arguments"].as_str() {
                    entry.args.push_str(args);
                }
            }
        }

        if let Some(fr) = choice["finish_reason"].as_str() {
            self.finish_reason = fr.to_string();
        }
        if let Some(usage) = chunk.get("usage") {
            if let Some(v) = usage["prompt_tokens"].as_u64() {
                self.input_tokens = Some(v as u32);
            }
            if let Some(v) = usage["completion_tokens"].as_u64() {
                self.output_tokens = Some(v as u32);
            }
        }
    }

    /// Finalize accumulation into the same `Completion` shape `complete()`
    /// produces for an equivalent non-streaming response, including the
    /// ADR-024 ollama-plain-text-tool-call recovery fallback.
    fn finish(self) -> Completion {
        let mut tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| {
                let args = serde_json::from_str(&tc.args)
                    .unwrap_or_else(|_| serde_json::Value::String(tc.args.clone()));
                ToolCall {
                    id: tc.id,
                    name: tc.name,
                    args,
                }
            })
            .collect();

        let text = if self.content.is_empty() {
            None
        } else {
            Some(self.content)
        };

        if tool_calls.is_empty()
            && let Some(t) = &text
            && let Some(tc) = toolcall_from_content(t)
        {
            tool_calls.push(tc);
        }

        Completion {
            message: Message {
                role: Role::Assistant,
                text,
                tool_calls,
                tool_call_id: None,
                media: Vec::new(),
            },
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            truncated: self.finish_reason == "length",
        }
    }
}

/// Map one `MediaPart` to an OpenAI content-part (ADR-023). Audio →
/// `input_audio` (base64 + format); image/video → `image_url` with a `data:`
/// URL (llama.cpp vision accepts data URLs; video support is server-dependent,
/// best-effort).
fn media_part_json(part: &MediaPart) -> serde_json::Value {
    if let Some(subtype) = part.mime.strip_prefix("audio/") {
        let format = match subtype {
            "mpeg" => "mp3",
            other => other,
        };
        json!({
            "type": "input_audio",
            "input_audio": { "data": part.data, "format": format }
        })
    } else {
        json!({
            "type": "image_url",
            "image_url": { "url": format!("data:{};base64,{}", part.mime, part.data) }
        })
    }
}

/// ADR-024: ollama's OpenAI-compatible endpoint emits a tool call as plain text
/// in `content` (`{"name": ..., "arguments": {...}}`) with `tool_calls` null.
/// Recover a `ToolCall` from such content. Accepts the ollama shape
/// (`{name, arguments}`) and the harness shape (`{tool, args}`); `arguments`
/// may be a JSON object or a JSON-encoded string. Both a name and an
/// arguments object must be present, so ordinary assistant prose (or a stray
/// JSON object without them) is never misread as a call.
fn toolcall_from_content(content: &str) -> Option<ToolCall> {
    let trimmed = content.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let name = v
        .get("name")
        .or_else(|| v.get("tool"))?
        .as_str()?
        .to_string();
    if name.is_empty() {
        return None;
    }
    let raw_args = v.get("arguments").or_else(|| v.get("args"))?;
    let args = match raw_args {
        // ollama sometimes JSON-encodes the args as a string.
        serde_json::Value::String(s) => serde_json::from_str(s).ok()?,
        other => other.clone(),
    };
    Some(ToolCall {
        id: String::new(),
        name,
        args,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::types::SamplingParams;

    /// `OpenAiConfig` holds the API key in plaintext and reaches an
    /// `Authorization: Bearer` header. It is NOT `Debug` today, and that
    /// absence is the only thing stopping a `{:?}` from putting the credential
    /// in a log — an absence a later `#[derive(Debug)]` erases silently
    /// (ADR-097). Detection uses inherent-impl priority: an inherent
    /// associated const shadows the trait's, so `IS_DEBUG` is `true` only when
    /// the type really implements `Debug`.
    #[test]
    fn openai_config_is_not_debug_printable() {
        struct IsDebug<T>(std::marker::PhantomData<T>);

        trait Fallback {
            const IS_DEBUG: bool = false;
        }
        impl<T> Fallback for IsDebug<T> {}

        impl<T: std::fmt::Debug> IsDebug<T> {
            const IS_DEBUG: bool = true;
        }

        // Compile-time, not runtime: a `#[derive(Debug)]` here should fail the
        // BUILD, not merely a test someone might not run. The positive control
        // comes first — without it, a detector that always answered "not Debug"
        // would satisfy the real check while verifying nothing.
        const _: () = assert!(
            <IsDebug<String>>::IS_DEBUG,
            "the detector must recognise a type that IS Debug"
        );
        const _: () = assert!(
            !<IsDebug<OpenAiConfig>>::IS_DEBUG,
            "OpenAiConfig carries the API key; if it must become Debug, give it \
             a redacting impl rather than a derive"
        );
    }

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
    fn toolcall_from_content_ollama_shape() {
        // ADR-024: ollama returns the call as text in `content`.
        let tc = toolcall_from_content(r#"{"name": "read_file", "arguments": {"path": "a.txt"}}"#)
            .expect("ollama-shaped content is a tool call");
        assert_eq!(tc.name, "read_file");
        assert_eq!(tc.args["path"], "a.txt");
    }

    #[test]
    fn toolcall_from_content_harness_shape() {
        let tc = toolcall_from_content(r#"{"tool": "read_file", "args": {"path": "a.txt"}}"#)
            .expect("harness-shaped content is a tool call");
        assert_eq!(tc.name, "read_file");
        assert_eq!(tc.args["path"], "a.txt");
    }

    #[test]
    fn toolcall_from_content_args_as_json_string() {
        let tc =
            toolcall_from_content(r#"{"name": "read_file", "arguments": "{\"path\": \"a.txt\"}"}"#)
                .expect("string-encoded args parse");
        assert_eq!(tc.args["path"], "a.txt");
    }

    #[test]
    fn toolcall_from_content_prose_is_none() {
        assert!(toolcall_from_content("Sure, I'll read the file for you.").is_none());
        // Has a name but no args → not a call (guards against misreading prose).
        assert!(toolcall_from_content(r#"{"name": "Bob"}"#).is_none());
        assert!(toolcall_from_content(r#"{"summary": "done"}"#).is_none());
    }

    #[test]
    fn map_message_text_only_is_string() {
        // WHEN no media THEN content stays a plain string (unchanged shape).
        let mapped = OpenAiProvider::map_message(&Message::user("hello"));
        assert_eq!(mapped["content"], json!("hello"));
    }

    #[test]
    fn map_message_media_is_parts_array() {
        // WHEN media present THEN content is the OpenAI parts array.
        let msg = Message::user_with_media(
            "describe",
            vec![
                MediaPart {
                    mime: "image/png".to_string(),
                    data: "AAAA".to_string(),
                },
                MediaPart {
                    mime: "audio/wav".to_string(),
                    data: "BBBB".to_string(),
                },
            ],
        );
        let mapped = OpenAiProvider::map_message(&msg);
        let parts = mapped["content"].as_array().expect("content is an array");
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "describe");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert_eq!(parts[2]["type"], "input_audio");
        assert_eq!(parts[2]["input_audio"]["data"], "BBBB");
        assert_eq!(parts[2]["input_audio"]["format"], "wav");
    }

    #[test]
    fn capabilities_supports_media() {
        assert!(provider().capabilities().supports_media);
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

    /// Test-critique C-002: `complete_streaming` validates the request
    /// exactly like `complete()` (ADR-010: constraint + tools is invalid) —
    /// pinned so a future refactor can't accidentally drop or reorder the
    /// `validate()?` call ahead of the network send.
    #[test]
    fn complete_streaming_rejects_invalid_request() {
        let mut req = base_request();
        req.tools = vec![tool()];
        req.constraint = Some(Constraint::JsonSchema(json!({"type": "object"})));
        let sink = |_: StreamDelta| panic!("must not fire any delta on a validation failure");
        let result = futures_executor::block_on(provider().complete_streaming(req, &sink, None));
        assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
    }

    #[test]
    fn capabilities_advertise_constraint_and_native() {
        let caps = provider().capabilities();
        assert!(caps.supports_constraint);
        assert!(caps.supports_native_tool_calls);
    }

    #[test]
    fn classify_sse_line_variants() {
        assert_eq!(
            classify_sse_line(r#"data: {"a":1}"#),
            SseLine::Data(r#"{"a":1}"#.to_string())
        );
        assert_eq!(classify_sse_line("data: [DONE]"), SseLine::Done);
        assert_eq!(classify_sse_line(""), SseLine::Ignored);
        assert_eq!(classify_sse_line(": a comment"), SseLine::Ignored);
        assert_eq!(classify_sse_line("event: message"), SseLine::Ignored);
    }

    fn collect_deltas(constrained: bool, lines: &[&str]) -> (Completion, Vec<StreamDelta>) {
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);
        let mut acc = StreamAccumulator::new(constrained, &sink);
        for line in lines {
            acc.feed_line(line);
        }
        (acc.finish(), deltas.into_inner().unwrap())
    }

    /// T-3703: `delta.content`-only SSE lines (`NativeTools`/`TextXml` shape)
    /// accumulate into `Completion.message.text`; `on_delta` fires per chunk.
    #[test]
    fn accumulate_lines_content_only() {
        let (completion, deltas) = collect_deltas(
            false,
            &[
                r#"{"choices":[{"index":0,"delta":{"content":"Hello"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":", world"},"finish_reason":"stop"}]}"#,
            ],
        );
        assert_eq!(completion.message.text.as_deref(), Some("Hello, world"));
        assert!(!completion.truncated);
        assert_eq!(
            deltas,
            vec![
                StreamDelta::Text("Hello".to_string()),
                StreamDelta::Text(", world".to_string()),
            ]
        );
    }

    /// T-3703: `delta.tool_calls` fragments accumulate into the same final
    /// tool-call shape `complete()`'s non-streaming parser would produce.
    #[test]
    fn accumulate_lines_tool_call_only() {
        let (completion, _deltas) = collect_deltas(
            false,
            &[
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":""}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\""}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"a.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        assert_eq!(completion.message.tool_calls.len(), 1);
        let tc = &completion.message.tool_calls[0];
        assert_eq!(tc.id, "call_1");
        assert_eq!(tc.name, "read_file");
        assert_eq!(tc.args["path"], "a.txt");
    }

    /// A `data: [DONE]`-equivalent scenario is handled by the caller
    /// (`complete_streaming`'s outer loop stops on `SseLine::Done` before
    /// calling `feed_line`) — confirmed here at the classification level.
    #[test]
    fn accumulate_lines_done_terminates() {
        assert_eq!(classify_sse_line("data: [DONE]"), SseLine::Done);
        assert_ne!(classify_sse_line("data: [DONE]"), SseLine::Ignored);
    }

    #[test]
    fn accumulate_lines_ignores_blank_and_comment_lines() {
        let (completion, deltas) = collect_deltas(
            false,
            &[
                "",
                ": keep-alive comment",
                r#"{"choices":[{"index":0,"delta":{"content":"ok"}}]}"#,
            ],
        );
        // The blank/comment "lines" here are fed straight to `feed_line`
        // (which itself no-ops on non-JSON); the real ignoring happens at
        // `classify_sse_line` before `feed_line` is ever called in
        // `complete_streaming`'s loop — this proves `feed_line` degrades
        // gracefully too (belt-and-suspenders).
        assert_eq!(completion.message.text.as_deref(), Some("ok"));
        assert_eq!(deltas, vec![StreamDelta::Text("ok".to_string())]);
    }

    /// Under `ConstrainedJson`, `on_delta` receives the scanner's
    /// `ToolNamed`/`Text` deltas (not raw JSON fragments) as content
    /// accumulates — proves T-3703 wires T-3702's scanner in correctly.
    #[test]
    fn accumulate_lines_constrained_json_uses_scanner() {
        let (completion, deltas) = collect_deltas(
            true,
            &[
                r#"{"choices":[{"index":0,"delta":{"content":"{\"tool\":\"task_complete\","}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":"\"args\":{\"summary\":\"done\"}}"},"finish_reason":"stop"}]}"#,
            ],
        );
        assert_eq!(
            completion.message.text.as_deref(),
            Some(r#"{"tool":"task_complete","args":{"summary":"done"}}"#)
        );
        assert!(
            deltas.contains(&StreamDelta::ToolNamed("task_complete".to_string())),
            "deltas: {deltas:?}"
        );
        assert!(
            deltas.contains(&StreamDelta::Text("done".to_string())),
            "deltas: {deltas:?}"
        );
        // No raw JSON fragments (e.g. the literal opening brace) were ever
        // forwarded as prose.
        assert!(
            !deltas
                .iter()
                .any(|d| matches!(d, StreamDelta::Text(t) if t.contains('{')))
        );
    }
}

/// T-3703 E2E: a real `.chunk()`-driven read against an actual local TCP
/// socket — proves the real wire protocol, not just the pure accumulator
/// logic above.
#[cfg(test)]
mod streaming_e2e {
    use std::sync::Mutex;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::types::SamplingParams;

    #[tokio::test]
    async fn complete_streaming_over_real_tcp_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            // Drain enough of the request to know it was sent; this fake
            // server doesn't need to parse it.
            let _ = socket.read(&mut buf).await;

            let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n\
                        \n\
                        data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\
                        \n\
                        data: [DONE]\n\
                        \n";
            // `Connection: close`, NOT `Content-Length` -- an SSE body is
            // unbounded-length; a wrong/missing length would make reqwest
            // hang waiting for more bytes instead of completing (test-critique
            // C-009).
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        };

        let provider = OpenAiProvider::new(OpenAiConfig {
            base_url: format!("http://{addr}/v1"),
            api_key: "test".to_string(),
            model: "test-model".to_string(),
        });
        let request = CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        };

        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);

        let (served, completion) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(3), server),
            tokio::time::timeout(
                Duration::from_secs(3),
                provider.complete_streaming(request, &sink, None),
            ),
        );
        served.expect("server fixture completed within its deadline");
        let completion = completion
            .expect("provider fixture completed within its deadline")
            .expect("streaming completion should succeed");

        assert_eq!(completion.message.text.as_deref(), Some("Hello world"));
        assert!(!completion.truncated);
        assert_eq!(
            deltas.into_inner().unwrap(),
            vec![
                StreamDelta::Text("Hello".to_string()),
                StreamDelta::Text(" world".to_string()),
            ]
        );
    }

    /// Test-critique C-003: a non-2xx HTTP status is surfaced as
    /// `ProviderError::Backend`, not a hang or a panic, over the real wire
    /// (mirrors `complete()`'s equivalent status check, but exercised
    /// through `complete_streaming`'s own branch).
    #[tokio::test]
    async fn complete_streaming_surfaces_http_error_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = "model not found";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        };

        let provider = OpenAiProvider::new(OpenAiConfig {
            base_url: format!("http://{addr}/v1"),
            api_key: "test".to_string(),
            model: "test-model".to_string(),
        });
        let request = CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        };
        let sink = |_: StreamDelta| panic!("no delta should fire on an HTTP error response");

        let (served, result) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(3), server),
            tokio::time::timeout(
                Duration::from_secs(3),
                provider.complete_streaming(request, &sink, None),
            ),
        );
        served.expect("server fixture completed within its deadline");
        let result = result.expect("provider fixture completed within its deadline");

        match result {
            Err(ProviderError::Backend(msg)) => {
                assert!(msg.contains("404"), "error should name the status: {msg}");
                assert!(
                    msg.contains("model not found"),
                    "error should carry the body: {msg}"
                );
            }
            other => panic!("expected ProviderError::Backend, got {other:?}"),
        }
    }

    /// Test-critique C-005: a single logical SSE `data:` line split across
    /// TWO separate socket writes (genuine mid-line TCP fragmentation, not
    /// incidental OS buffering) must still reassemble correctly — this is
    /// the one piece of new I/O-adjacent logic (`buf`/`drain`/`find('\n')`
    /// in `complete_streaming`'s read loop) that the pure `feed_line` unit
    /// tests can't exercise, since those pre-split lines by construction.
    #[tokio::test]
    async fn complete_streaming_reassembles_a_line_split_mid_write() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;

            let headers =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            socket.write_all(headers.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();

            // Split squarely in the middle of the `data:` line's JSON, not
            // at a line boundary.
            let first_half = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hel";
            socket.write_all(first_half.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;

            let second_half = "lo\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            socket.write_all(second_half.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        };

        let provider = OpenAiProvider::new(OpenAiConfig {
            base_url: format!("http://{addr}/v1"),
            api_key: "test".to_string(),
            model: "test-model".to_string(),
        });
        let request = CompletionRequest {
            messages: vec![Message::user("hi")],
            sampling: SamplingParams::default(),
            tools: Vec::new(),
            constraint: None,
        };
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);

        let (served, completion) = tokio::join!(
            tokio::time::timeout(Duration::from_secs(3), server),
            tokio::time::timeout(
                Duration::from_secs(3),
                provider.complete_streaming(request, &sink, None),
            ),
        );
        served.expect("server fixture completed within its deadline");
        let completion = completion
            .expect("provider fixture completed within its deadline")
            .expect("a mid-line split must not break parsing");

        assert_eq!(completion.message.text.as_deref(), Some("Hello"));
        assert_eq!(
            deltas.into_inner().unwrap(),
            vec![StreamDelta::Text("Hello".to_string())]
        );
    }
}

#[cfg(test)]
#[path = "openai_io_tests.rs"]
mod io_tests;
