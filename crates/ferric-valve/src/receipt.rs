//! One content-free receipt per chat-completions request (T-12607, INT-0012
//! AC-4).
//!
//! A receipt identifies what was sent by hash, never by content, and records
//! what the upstream reported about cost. Anything the upstream did not report
//! is `null`, and its name is listed under `unavailable`. That separates
//! "zero" from "unknown". Receipts are appended as JSON lines.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use crate::upstream::UpstreamStats;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Receipt {
    pub request_id: String,
    /// `constrained`, `pass-through` or `record-only`.
    pub mode: String,
    pub stream: bool,
    pub started_unix_ms: u64,
    pub wall_ms: u64,
    /// `completed`, `failed` or `cancelled`.
    pub outcome: String,
    pub http_status: Option<u16>,
    pub tools_offered: Option<usize>,
    pub tool_catalog_hash: Option<String>,
    pub schema_hash: Option<String>,
    pub prefix_hash: Option<String>,
    pub message_hashes: Vec<String>,
    pub model: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub prompt_eval_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub predicted_tokens: Option<u64>,
    pub prompt_ms: Option<f64>,
    pub predicted_ms: Option<f64>,
    pub finish_reason: Option<String>,
    pub action_valid: Option<bool>,
    pub tool: Option<String>,
    pub error_class: Option<String>,
    pub unavailable: Vec<String>,
}

impl Receipt {
    /// Copy the upstream's reported metrics in, listing what was missing.
    pub fn absorb_stats(&mut self, stats: &UpstreamStats) {
        self.model = stats.model.clone();
        self.prompt_tokens = stats.prompt_tokens;
        self.prompt_eval_tokens = stats.prompt_eval_tokens;
        self.cached_tokens = stats.cached_tokens;
        self.predicted_tokens = stats.predicted_tokens;
        self.prompt_ms = stats.prompt_ms;
        self.predicted_ms = stats.predicted_ms;
        self.unavailable = stats
            .unavailable()
            .into_iter()
            .map(str::to_string)
            .collect();
    }
}

/// Where receipts go: an append-only JSONL file, or nowhere.
#[derive(Debug, Default)]
pub struct ReceiptSink {
    file: Option<Mutex<File>>,
}

impl ReceiptSink {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Some(Mutex::new(file)),
        })
    }

    pub fn disabled() -> Self {
        Self::default()
    }

    /// Append one line and flush it, so a crash after the request loses
    /// nothing already written.
    pub fn write(&self, receipt: &Receipt) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let mut line = serde_json::to_vec(receipt).map_err(io::Error::other)?;
        line.push(b'\n');
        let mut file = file
            .lock()
            .map_err(|_| io::Error::other("receipt file lock poisoned"))?;
        file.write_all(&line)?;
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_has_required_fields() {
        let receipt = Receipt {
            request_id: "r1".to_string(),
            mode: "constrained".to_string(),
            message_hashes: vec!["aa".to_string(), "bb".to_string()],
            ..Receipt::default()
        };
        let value = serde_json::to_value(&receipt).unwrap();
        for key in [
            "request_id",
            "mode",
            "stream",
            "started_unix_ms",
            "wall_ms",
            "outcome",
            "http_status",
            "tools_offered",
            "tool_catalog_hash",
            "schema_hash",
            "prefix_hash",
            "message_hashes",
            "model",
            "prompt_tokens",
            "prompt_eval_tokens",
            "cached_tokens",
            "predicted_tokens",
            "prompt_ms",
            "predicted_ms",
            "finish_reason",
            "action_valid",
            "tool",
            "error_class",
            "unavailable",
        ] {
            assert!(value.get(key).is_some(), "receipt lacks {key}");
        }
        assert_eq!(value["message_hashes"], serde_json::json!(["aa", "bb"]));
    }

    #[test]
    fn receipt_marks_unavailable_metrics() {
        let mut receipt = Receipt::default();
        receipt.absorb_stats(&UpstreamStats {
            model: Some("m".to_string()),
            predicted_tokens: Some(5),
            ..UpstreamStats::default()
        });
        assert_eq!(receipt.predicted_tokens, Some(5));
        assert_eq!(receipt.cached_tokens, None);
        assert!(receipt.unavailable.contains(&"cached_tokens".to_string()));
        assert!(
            !receipt
                .unavailable
                .contains(&"predicted_tokens".to_string())
        );
        assert!(!receipt.unavailable.contains(&"model".to_string()));
    }

    #[test]
    fn sink_appends_one_line_per_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipts.jsonl");
        let sink = ReceiptSink::open(&path).unwrap();
        sink.write(&Receipt::default()).unwrap();
        sink.write(&Receipt::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
        ReceiptSink::disabled().write(&Receipt::default()).unwrap();
    }
}
