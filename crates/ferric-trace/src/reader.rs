use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::Path;

use serde::Deserialize;

use ferric_core::FerricError;

use crate::event::Event;

/// A decoded trace line. `event` is `Unknown` (raw JSON preserved) when the
/// event type postdates this reader — old binaries must keep reading new
/// traces (ADR-002).
#[derive(Debug, Clone, PartialEq)]
pub struct TraceRecord {
    pub v: u32,
    pub ts_ms: u64,
    pub session: String,
    pub seq: u64,
    pub event: ParsedEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedEvent {
    Known(Event),
    Unknown(serde_json::Value),
}

#[derive(Deserialize)]
struct RawRecord {
    v: u32,
    ts_ms: u64,
    session: String,
    seq: u64,
    event: serde_json::Value,
}

/// Iterator over the records of a JSONL trace file.
pub struct TraceReader {
    lines: Lines<BufReader<File>>,
}

impl TraceReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FerricError> {
        let file = File::open(path.as_ref())?;
        Ok(Self {
            lines: BufReader::new(file).lines(),
        })
    }
}

impl Iterator for TraceReader {
    type Item = Result<TraceRecord, FerricError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.lines.next()? {
                Ok(line) => line,
                Err(e) => return Some(Err(e.into())),
            };
            if line.trim().is_empty() {
                continue;
            }
            return Some(parse_line(&line));
        }
    }
}

fn parse_line(line: &str) -> Result<TraceRecord, FerricError> {
    let raw: RawRecord = serde_json::from_str(line)?;
    let event = match Event::deserialize(&raw.event) {
        Ok(event) => ParsedEvent::Known(event),
        Err(_) => ParsedEvent::Unknown(raw.event),
    };
    Ok(TraceRecord {
        v: raw.v,
        ts_ms: raw.ts_ms,
        session: raw.session,
        seq: raw.seq,
        event,
    })
}
