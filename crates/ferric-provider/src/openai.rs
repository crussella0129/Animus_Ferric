use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{debug, warn};

use crate::traits::Provider;
use crate::types::{
    Capabilities, Completion, CompletionRequest, Constraint, ProviderError, StreamDelta,
};
use ferric_core::{MediaPart, Message, Role, ToolCall};
use ferric_iron::ConstrainedJsonScanner;
use ferric_iron::wire::{SseDecoder, SseLine, openai_tool, openai_tool_call, response_format};

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
                    let arguments = serde_json::to_string(&tc.args).unwrap_or_default();
                    openai_tool_call(&tc.id, &tc.name, &arguments)
                })
                .collect();
            out["tool_calls"] = json!(calls);
        }

        if let Some(tool_call_id) = &msg.tool_call_id {
            out["tool_call_id"] = json!(tool_call_id);
        }

        out
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
    /// system prompt (the loop's `ConstrainedJson` path does, via the CLI's prompt composer).
    ///
    /// A constraint kind this backend cannot transmit is an error, never a
    /// silently unconstrained request (INT-0011 AC-4, T-12603).
    fn build_body(&self, request: &CompletionRequest) -> Result<serde_json::Value, ProviderError> {
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
                body["response_format"] = response_format("ferric_action", schema);
            }
            // llama.cpp accepts a GBNF/Lark grammar via the `grammar` field.
            Some(Constraint::Lark(grammar)) => {
                body["grammar"] = json!(grammar);
            }
            // No standard OpenAI-compatible field carries a bare regex. Sending
            // the request without it would claim a constraint the backend never
            // saw, so refuse before any network I/O (INT-0011 AC-4).
            Some(Constraint::Regex(_)) => {
                return Err(ProviderError::InvalidRequest(
                    "a regex constraint cannot be transmitted to an OpenAI-compatible \
                     backend; refusing to send an unconstrained request"
                        .to_string(),
                ));
            }
            None => {
                if !request.tools.is_empty() {
                    let tools: Vec<_> = request.tools.iter().map(openai_tool).collect();
                    body["tools"] = json!(tools);
                    body["tool_choice"] = json!("auto");
                }
            }
        }

        Ok(body)
    }

    /// POST `body` to `/chat/completions`. A transport failure is retryable;
    /// a non-2xx status is a backend error carrying the response body.
    async fn post(&self, body: &serde_json::Value) -> Result<reqwest::Response, ProviderError> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| {
                warn!(error = %e, "network error contacting the inference server (retryable)");
                ProviderError::RetryableBackend(format!("Network error: {e}"))
            })?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            warn!(%status, "inference server returned an error status");
            return Err(ProviderError::Backend(format!("HTTP {status}: {text}")));
        }
        Ok(response)
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
            supports_media: true,
        }
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<Completion, ProviderError> {
        request.validate()?;
        let body = self.build_body(&request)?;
        with_cancellation(cancel_flag.as_deref(), async {
            debug!(
                model = %self.config.model,
                messages = request.messages.len(),
                constrained = request.constraint.is_some(),
                "POST /chat/completions"
            );
            let json_res: serde_json::Value =
                self.post(&body).await?.json().await.map_err(|e| {
                    ProviderError::Backend(format!("Failed to parse response JSON: {}", e))
                })?;

            let choice = json_res["choices"]
                .as_array()
                .and_then(|c| c.first())
                .ok_or_else(|| ProviderError::Backend("Response carried no choices".to_string()))?;

            let message = &choice["message"];
            let field = |value: &serde_json::Value| value.as_str().unwrap_or_default().to_string();
            let calls = message["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tc| {
                    let function = &tc["function"];
                    (
                        field(&tc["id"]),
                        field(&function["name"]),
                        field(&function["arguments"]),
                    )
                })
                .collect();
            let usage = &json_res["usage"];
            Ok(assemble(
                message["content"].as_str().map(|s| s.to_string()),
                calls,
                choice["finish_reason"].as_str().unwrap_or(""),
                usage["prompt_tokens"].as_u64().map(|v| v as u32),
                usage["completion_tokens"].as_u64().map(|v| v as u32),
            ))
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
        let mut body = self.build_body(&request)?;
        body["stream"] = json!(true);
        with_cancellation(cancel_flag.as_deref(), async {
            let mut response = self.post(&body).await?;
            let mut acc = StreamAccumulator::new(request.constraint.is_some(), on_delta);
            let mut decoder = SseDecoder::default();
            while let Some(bytes) = response
                .chunk()
                .await
                .map_err(|e| ProviderError::RetryableBackend(format!("stream error: {e}")))?
            {
                for line in decoder.push(&bytes).map_err(sse_utf8_error)? {
                    match line {
                        SseLine::Data(chunk) => acc.feed(&chunk)?,
                        SseLine::Done => return Ok(acc.finish()),
                        SseLine::Other => {}
                    }
                }
            }
            decoder.finish().map_err(sse_utf8_error)?;
            Ok(acc.finish())
        })
        .await
    }
}

fn sse_utf8_error(error: std::str::Utf8Error) -> ProviderError {
    ProviderError::Backend(format!("Malformed UTF-8 in SSE response: {error}"))
}

/// Accumulates SSE `data:` chunks into a final `Completion`, firing
/// `StreamDelta`s to `on_delta` as content becomes available. Pure with
/// respect to I/O, so it is directly unit-testable without networking.
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

    /// Feed one SSE `data:` chunk. An in-band `{"error": …}` event ends the
    /// stream as a backend error, as the valve also treats it.
    fn feed(&mut self, chunk: &serde_json::Value) -> Result<(), ProviderError> {
        if let Some(error) = chunk.get("error") {
            return Err(ProviderError::Backend(format!(
                "stream error event: {error}"
            )));
        }
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
        Ok(())
    }

    /// Finalize into the `Completion` `complete()` builds for an equivalent
    /// non-streaming response.
    fn finish(self) -> Completion {
        let calls = self
            .tool_calls
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| (tc.id, tc.name, tc.args))
            .collect();
        let text = (!self.content.is_empty()).then_some(self.content);
        assemble(
            text,
            calls,
            &self.finish_reason,
            self.input_tokens,
            self.output_tokens,
        )
    }
}

/// The assistant `Completion` both paths return: each call's JSON-encoded
/// `(id, name, arguments)` parsed (arguments that are not JSON stay a string),
/// the ADR-024 plain-text tool-call recovery when no call came natively, and
/// a `length` finish as truncation.
fn assemble(
    text: Option<String>,
    calls: Vec<(String, String, String)>,
    finish_reason: &str,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
) -> Completion {
    let mut tool_calls: Vec<ToolCall> = calls
        .into_iter()
        .map(|(id, name, arguments)| {
            let args = serde_json::from_str(&arguments)
                .unwrap_or_else(|_| serde_json::Value::String(arguments));
            ToolCall { id, name, args }
        })
        .collect();
    // ADR-024 fallback: ollama's OpenAI-compatible endpoint returns a tool
    // call as plain text in `content` with `tool_calls` null. If nothing
    // parsed natively but the content is itself a tool-call object, recover
    // it so the native path sees the call instead of `no_action`.
    if tool_calls.is_empty()
        && let Some(tc) = text.as_deref().and_then(toolcall_from_content)
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
        input_tokens,
        output_tokens,
        truncated: finish_reason == "length",
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
    use crate::types::ToolDescriptor;

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

    /// A provider whose endpoint is a closed loopback port: any request that
    /// reaches the network fails with a connection error, never
    /// `InvalidRequest`.
    fn provider() -> OpenAiProvider {
        OpenAiProvider::new(OpenAiConfig {
            base_url: "http://127.0.0.1:9/v1".to_string(),
            api_key: "test".to_string(),
            model: "test-model".to_string(),
        })
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

    /// Test-critique C-002: `complete_streaming` validates the request
    /// exactly like `complete()` (ADR-010: constraint + tools is invalid) —
    /// pinned so a future refactor can't accidentally drop or reorder the
    /// `validate()?` call ahead of the network send.
    #[test]
    fn complete_streaming_rejects_invalid_request() {
        let mut req = CompletionRequest::fixture();
        req.tools = vec![tool()];
        req.constraint = Some(Constraint::JsonSchema(json!({"type": "object"})));
        let sink = |_: StreamDelta| panic!("must not fire any delta on a validation failure");
        let result = futures_executor::block_on(provider().complete_streaming(req, &sink, None));
        assert!(matches!(result, Err(ProviderError::InvalidRequest(_))));
    }

    /// T-12603 (INT-0011 AC-4): a regex constraint is refused before any
    /// network I/O. The base URL points at a closed loopback port, so reaching
    /// the network would surface a connection error instead of `InvalidRequest`.
    #[test]
    fn regex_constraint_is_rejected_without_network() {
        let mut req = CompletionRequest::fixture();
        req.constraint = Some(Constraint::Regex("^a$".to_string()));
        let result = futures_executor::block_on(provider().complete(req, None));
        assert!(
            matches!(&result, Err(ProviderError::InvalidRequest(message)) if message.contains("regex")),
            "{result:?}"
        );
    }

    #[test]
    fn regex_constraint_is_rejected_when_streaming() {
        let mut req = CompletionRequest::fixture();
        req.constraint = Some(Constraint::Regex("^a$".to_string()));
        let sink = |_: StreamDelta| panic!("must not stream for an untransmittable constraint");
        let result = futures_executor::block_on(provider().complete_streaming(req, &sink, None));
        assert!(
            matches!(&result, Err(ProviderError::InvalidRequest(message)) if message.contains("regex")),
            "{result:?}"
        );
    }

    /// T-12603: the supported request shapes serialize exactly as before the
    /// fallible signature — full-body equality, not field spot checks.
    #[test]
    fn build_body_unchanged_for_supported_constraints() {
        let schema = json!({"type": "object", "required": ["tool"]});
        let mut constrained = CompletionRequest::fixture();
        constrained.constraint = Some(Constraint::JsonSchema(schema.clone()));
        let mut lark = CompletionRequest::fixture();
        lark.constraint = Some(Constraint::Lark("root ::= \"a\"".to_string()));
        let mut tools = CompletionRequest::fixture();
        tools.tools = vec![tool()];
        let plain = CompletionRequest::fixture();

        let base = |req: &CompletionRequest| {
            json!({
                "model": provider().config.model,
                "messages": req.messages.iter().map(OpenAiProvider::map_message).collect::<Vec<_>>(),
                "max_tokens": req.sampling.max_tokens,
                "temperature": req.sampling.temperature,
                "top_p": req.sampling.top_p,
            })
        };
        let mut expected = base(&constrained);
        expected["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {"name": "ferric_action", "schema": schema, "strict": true}
        });
        assert_eq!(provider().build_body(&constrained).unwrap(), expected);

        let mut expected = base(&lark);
        expected["grammar"] = json!("root ::= \"a\"");
        assert_eq!(provider().build_body(&lark).unwrap(), expected);

        let mut expected = base(&tools);
        expected["tools"] = json!([{
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "read a file",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
            }
        }]);
        expected["tool_choice"] = json!("auto");
        assert_eq!(provider().build_body(&tools).unwrap(), expected);

        assert_eq!(provider().build_body(&plain).unwrap(), base(&plain));
    }

    #[test]
    fn capabilities_advertise_constraint_and_native() {
        assert_eq!(
            provider().capabilities(),
            Capabilities {
                supports_native_tool_calls: true,
                supports_constraint: true,
                supports_media: true,
            }
        );
    }

    fn collect_deltas(constrained: bool, lines: &[&str]) -> (Completion, Vec<StreamDelta>) {
        let deltas: Mutex<Vec<StreamDelta>> = Mutex::new(Vec::new());
        let sink = |d: StreamDelta| deltas.lock().unwrap().push(d);
        let mut acc = StreamAccumulator::new(constrained, &sink);
        for line in lines {
            acc.feed(&serde_json::from_str(line).unwrap()).unwrap();
        }
        (acc.finish(), deltas.into_inner().unwrap())
    }

    /// D-4 (sprint 129): an in-band SSE error event is a backend error, not
    /// a silently empty completion.
    #[test]
    fn stream_error_event_is_a_backend_error() {
        let sink = |_: StreamDelta| {};
        let mut acc = StreamAccumulator::new(false, &sink);
        acc.feed(&json!({"choices": [{"delta": {"content": "par"}}]}))
            .unwrap();
        let error = acc
            .feed(&json!({"error": {"message": "context exceeded"}}))
            .unwrap_err();
        assert!(!error.is_retryable());
        assert!(error.to_string().contains("context exceeded"), "{error}");
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

#[cfg(test)]
#[path = "openai_io_tests.rs"]
mod io_tests;
