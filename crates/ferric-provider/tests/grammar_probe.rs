//! Root-cause probe for the s2 grammar hang (ADR-020).
//!
//! Loads the real 1B and calls `complete()` ONCE with a `Constraint::JsonSchema`
//! whose complexity is chosen by `FERRIC_PROBE`:
//!   trivial      — {object, one required string prop}        (is the constraint path usable at all?)
//!   bounded      — trivial but maxLength 32 on the string    (does bounding the string matter?)
//!   anyof2       — anyOf of two const-discriminated branches  (does anyOf trigger it?)
//!   unified      — the exact action_schema (read/write/list/move/make_dir + task_complete)
//!   noxg         — unified minus the x-guidance extension     (does x-guidance trigger it?)
//!
//! Prints `PROBE RETURNED in <ms>` on success. Run each variant as its own
//! bounded subprocess so a hang is killed externally, never unbounded again:
//!   FERRIC_PROBE=trivial FERRIC_SMOKE_MODEL_DIR=... FERRIC_SMOKE_MODEL_FILE=... \
//!   cargo test -p ferric-provider --release --features backend-mistralrs \
//!     --test grammar_probe -- --ignored --nocapture
#![cfg(feature = "backend-mistralrs")]

use std::time::Instant;

use ferric_provider::mistralrs::{MistralRsConfig, MistralRsProvider};
use ferric_provider::{CompletionRequest, Constraint, Provider, SamplingParams};
use serde_json::{Value, json};

fn branch(tool: &str) -> Value {
    json!({
        "type": "object",
        "properties": { "tool": {"const": tool}, "args": {"type": "object", "additionalProperties": false} },
        "required": ["tool", "args"],
        "additionalProperties": false
    })
}

fn schema(kind: &str) -> Value {
    match kind {
        "trivial" => json!({
            "type": "object",
            "properties": {"x": {"type": "string"}},
            "required": ["x"]
        }),
        "bounded" => json!({
            "type": "object",
            "properties": {"x": {"type": "string", "maxLength": 32}},
            "required": ["x"]
        }),
        "anyof2" => json!({
            "type": "object",
            "anyOf": [branch("read_file"), branch("task_complete")]
        }),
        "noxg" => json!({
            "type": "object",
            "anyOf": [branch("read_file"), branch("write_file"), branch("task_complete")]
        }),
        // The real thing, including x-guidance.
        "unified" => json!({
            "x-guidance": {"whitespace_flexible": false},
            "type": "object",
            "anyOf": [branch("read_file"), branch("write_file"), branch("list_dir"), branch("task_complete")]
        }),
        other => panic!("unknown FERRIC_PROBE variant: {other}"),
    }
}

#[test]
#[ignore = "requires a local GGUF + FERRIC_PROBE; bounded subprocess only"]
fn grammar_probe() {
    let dir = std::env::var("FERRIC_SMOKE_MODEL_DIR").expect("FERRIC_SMOKE_MODEL_DIR");
    let file = std::env::var("FERRIC_SMOKE_MODEL_FILE").expect("FERRIC_SMOKE_MODEL_FILE");
    let kind = std::env::var("FERRIC_PROBE").unwrap_or_else(|_| "trivial".to_string());
    let s = schema(&kind);
    eprintln!("--- probe variant: {kind}\nschema: {s}");

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let mut config = MistralRsConfig::new(dir, file);
        // The fix under test: feed the real tokenizer.json (ADR-020 root cause).
        config.tokenizer_json = std::env::var_os("FERRIC_TOKENIZER_JSON").map(Into::into);
        config.tok_model_id = std::env::var("FERRIC_TOK_MODEL_ID").ok();
        eprintln!(
            "--- tokenizer_json: {:?}, tok_model_id: {:?}",
            config.tokenizer_json, config.tok_model_id
        );
        let load0 = Instant::now();
        let provider = MistralRsProvider::load(config).await.expect("model load");
        eprintln!("--- model loaded in {:?}", load0.elapsed());

        let request = CompletionRequest {
            messages: vec![ferric_core::Message::user(
                "Reply with a JSON object that satisfies the schema.",
            )],
            sampling: SamplingParams {
                temperature: 0.0,
                max_tokens: 128,
                ..SamplingParams::default()
            },
            tools: Vec::new(),
            constraint: Some(Constraint::JsonSchema(s)),
        };

        let t0 = Instant::now();
        let completion = provider.complete(request).await.expect("complete");
        eprintln!(
            "PROBE RETURNED in {:?}: {:?}",
            t0.elapsed(),
            completion.message.text
        );
    });
}
