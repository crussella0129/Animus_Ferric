use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ferric_core::FerricError;

use crate::event::{Event, TRACE_SCHEMA_VERSION, TraceEvent};

/// Append-only JSONL writer. Every event is flushed to disk before
/// `write_event` returns, so a crashed session still leaves a complete trace
/// up to its final event.
pub struct JsonlSink {
    file: File,
    session: String,
    next_seq: u64,
}

impl JsonlSink {
    /// Open (creating or appending to) the trace file at `path`.
    pub fn open(path: impl AsRef<Path>, session: impl Into<String>) -> Result<Self, FerricError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        Ok(Self {
            file,
            session: session.into(),
            next_seq: 0,
        })
    }

    /// Create a brand-new trace and fail if the path already exists.
    ///
    /// Long-lived servers allocate session IDs concurrently. Appending a new
    /// sequence-zero session to an existing file would corrupt both replay and
    /// verification, so production allocators use this constructor and retry
    /// with another opaque ID on `AlreadyExists`.
    pub fn create_new(
        path: impl AsRef<Path>,
        session: impl Into<String>,
    ) -> Result<Self, FerricError> {
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(path.as_ref())?;
        Ok(Self {
            file,
            session: session.into(),
            next_seq: 0,
        })
    }

    /// Returns the session ID for this sink.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Stamp, serialize, write, and flush one event. Returns the sequence
    /// number assigned to it.
    pub fn write_event(&mut self, event: Event) -> Result<u64, FerricError> {
        let seq = self.next_seq;
        let record = TraceEvent {
            v: TRACE_SCHEMA_VERSION,
            ts_ms: now_ms(),
            session: self.session.clone(),
            seq,
            event,
        };
        let line = serde_json::to_string(&record)?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.next_seq += 1;
        Ok(seq)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
