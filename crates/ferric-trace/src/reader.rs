use std::fs::File;
use std::io::{self, BufRead, BufReader};
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

/// Controls how [`TraceReader`] handles the final physical record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceReadMode {
    /// Report every malformed record, including an unterminated final record.
    Strict,
    /// Ignore only a syntactically incomplete final record when it is not
    /// newline-terminated.
    ///
    /// This mode is for replay after a process may have been interrupted in
    /// the middle of appending an event. Corruption in a newline-terminated
    /// record remains an error. An incomplete terminal UTF-8 code point is
    /// treated like an unexpected end of JSON; every other UTF-8 error and
    /// every other kind of JSON corruption remains an error.
    ReplayRecovery,
}

/// Iterator over the records of a JSONL trace file.
pub struct TraceReader {
    reader: BufReader<File>,
    mode: TraceReadMode,
    finished: bool,
}

impl TraceReader {
    /// Open a trace in strict mode.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FerricError> {
        Self::open_with_mode(path, TraceReadMode::Strict)
    }

    /// Open a trace with an explicit final-record recovery policy.
    pub fn open_with_mode(
        path: impl AsRef<Path>,
        mode: TraceReadMode,
    ) -> Result<Self, FerricError> {
        let file = File::open(path.as_ref())?;
        Ok(Self {
            reader: BufReader::new(file),
            mode,
            finished: false,
        })
    }
}

impl Iterator for TraceReader {
    type Item = Result<TraceRecord, FerricError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            let mut line = Vec::new();
            let bytes_read = match self.reader.read_until(b'\n', &mut line) {
                Ok(bytes_read) => bytes_read,
                Err(e) => return Some(Err(e.into())),
            };
            if bytes_read == 0 {
                self.finished = true;
                return None;
            }

            let newline_terminated = line.ends_with(b"\n");
            let line = match std::str::from_utf8(&line) {
                Ok(line) => line,
                Err(error)
                    if self.mode == TraceReadMode::ReplayRecovery
                        && !newline_terminated
                        && incomplete_utf8_follows_incomplete_json(&line, error) =>
                {
                    self.finished = true;
                    return None;
                }
                Err(error) => {
                    return Some(Err(io::Error::new(io::ErrorKind::InvalidData, error).into()));
                }
            };
            if line.trim().is_empty() {
                continue;
            }

            match parse_line(line) {
                Ok(record) => return Some(Ok(record)),
                Err(error)
                    if self.mode == TraceReadMode::ReplayRecovery
                        && !newline_terminated
                        && error.is_eof() =>
                {
                    self.finished = true;
                    return None;
                }
                Err(error) => return Some(Err(error.into())),
            }
        }
    }
}

fn incomplete_utf8_follows_incomplete_json(line: &[u8], error: std::str::Utf8Error) -> bool {
    if error.error_len().is_some() {
        return false;
    }

    let valid_prefix = std::str::from_utf8(&line[..error.valid_up_to()])
        .expect("Utf8Error::valid_up_to always identifies valid UTF-8");
    serde_json::from_str::<serde_json::Value>(valid_prefix).is_err_and(|error| error.is_eof())
}

fn parse_line(line: &str) -> Result<TraceRecord, serde_json::Error> {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::{TempDir, tempdir};

    use super::*;

    const VALID_RECORD: &str =
        r#"{"v":1,"ts_ms":1,"session":"s","seq":0,"event":{"type":"future_event"}}"#;

    fn write_trace(contents: &str) -> (TempDir, std::path::PathBuf) {
        write_trace_bytes(contents.as_bytes())
    }

    fn write_trace_bytes(contents: &[u8]) -> (TempDir, std::path::PathBuf) {
        let dir = tempdir().expect("create temporary trace directory");
        let path = dir.path().join("trace.jsonl");
        fs::write(&path, contents).expect("write trace fixture");
        (dir, path)
    }

    #[test]
    fn strict_mode_rejects_incomplete_unterminated_final_record() {
        let (_dir, path) = write_trace(&format!("{VALID_RECORD}\n{{\"v\":1"));

        let mut reader = TraceReader::open(path).expect("open trace");
        assert!(reader.next().expect("first record").is_ok());
        assert!(reader.next().expect("truncated record error").is_err());
    }

    #[test]
    fn replay_recovery_ignores_incomplete_unterminated_final_record() {
        let (_dir, path) = write_trace(&format!("{VALID_RECORD}\n{{\"v\":1"));

        let records = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect("recover trace");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].seq, 0);
    }

    #[test]
    fn replay_recovery_rejects_incomplete_newline_terminated_record() {
        let (_dir, path) = write_trace(&format!("{VALID_RECORD}\n{{\"v\":1\n"));

        let error = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("terminated corruption must fail");

        assert!(matches!(error, FerricError::Serde(_)));
    }

    #[test]
    fn replay_recovery_rejects_complete_invalid_unterminated_record() {
        let (_dir, path) = write_trace(&format!("{VALID_RECORD}\n{{not-json}}"));

        let error = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("non-EOF corruption must fail");

        assert!(matches!(error, FerricError::Serde(_)));
    }

    #[test]
    fn replay_recovery_reads_valid_unterminated_final_record() {
        let (_dir, path) = write_trace(&format!("{VALID_RECORD}\n{VALID_RECORD}"));

        let records = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect("read valid unterminated record");

        assert_eq!(records.len(), 2);
    }

    #[test]
    fn replay_recovery_ignores_incomplete_unterminated_utf8_code_point() {
        let mut contents = format!("{VALID_RECORD}\n{{\"message\":\"").into_bytes();
        contents.extend_from_slice(&[0xe2, 0x82]);
        let (_dir, path) = write_trace_bytes(&contents);

        let records = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect("recover trace");

        assert_eq!(records.len(), 1);
    }

    #[test]
    fn strict_mode_rejects_incomplete_unterminated_utf8_code_point() {
        let mut contents = format!("{VALID_RECORD}\n{{\"message\":\"").into_bytes();
        contents.extend_from_slice(&[0xe2, 0x82]);
        let (_dir, path) = write_trace_bytes(&contents);

        let error = TraceReader::open(path)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("strict mode must reject torn UTF-8");

        assert!(matches!(error, FerricError::Io(_)));
    }

    #[test]
    fn replay_recovery_rejects_incomplete_newline_terminated_utf8_code_point() {
        let mut contents = format!("{VALID_RECORD}\n{{\"message\":\"").into_bytes();
        contents.extend_from_slice(&[0xe2, 0x82, b'\n']);
        let (_dir, path) = write_trace_bytes(&contents);

        let error = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("terminated torn UTF-8 must fail");

        assert!(matches!(error, FerricError::Io(_)));
    }

    #[test]
    fn replay_recovery_rejects_complete_invalid_unterminated_utf8() {
        let mut contents = format!("{VALID_RECORD}\n{{\"message\":\"").into_bytes();
        contents.push(0xff);
        let (_dir, path) = write_trace_bytes(&contents);

        let error = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("complete invalid UTF-8 must fail");

        assert!(matches!(error, FerricError::Io(_)));
    }

    #[test]
    fn replay_recovery_rejects_invalid_json_before_incomplete_utf8() {
        let mut contents = format!("{VALID_RECORD}\n{{not-json}}").into_bytes();
        contents.extend_from_slice(&[0xe2, 0x82]);
        let (_dir, path) = write_trace_bytes(&contents);

        let error = TraceReader::open_with_mode(path, TraceReadMode::ReplayRecovery)
            .expect("open trace")
            .collect::<Result<Vec<_>, _>>()
            .expect_err("torn UTF-8 must not hide an invalid JSON prefix");

        assert!(matches!(error, FerricError::Io(_)));
    }
}
