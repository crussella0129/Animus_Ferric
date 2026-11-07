use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    ControlCapability, FileObservation, LineRange, PrepareCtx, PrepareError, PrepareErrorKind,
    RequestedLineRange, ToolPreparation, evidence_envelope, logical_line_count,
    normalized_relative_path, sha256_bytes,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::args;
use super::controlled_read::read_controlled_file;

/// Read a UTF-8 text file inside the workspace, optionally paginated by lines.
pub struct ReadFile;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description:
                "Read a UTF-8 text file. Use start_line and end_line to paginate large files \
                and protect your context budget. Args: {\"path\": string, \"start_line\"?: number, \
                \"end_line\"?: number} (Lines are 1-indexed, inclusive)."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the workspace root" },
                    "start_line": { "type": "integer", "description": "Optional starting line number (1-indexed, inclusive)" },
                    "end_line": { "type": "integer", "description": "Optional ending line number (1-indexed, inclusive)" }
                },
                "required": ["path"]
            }),
            permission: PermissionLevel::Read,
            ring: 0,
        }
    }

    fn control_capability(&self) -> ControlCapability {
        ControlCapability::ReadOnly
    }

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = args::string(args, "path")?;
        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        let resolved = ctx.resolve(path)?;

        let content =
            std::fs::read_to_string(&resolved).map_err(|e| format!("read {path}: {e}"))?;

        if start_line.is_none() && end_line.is_none() {
            return Ok(content);
        }

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(usize::MAX);

        let lines: Vec<&str> = content
            .lines()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        Ok(lines.join("\n"))
    }

    fn prepare(
        &self,
        ctx: &PrepareCtx<'_>,
        args: &serde_json::Value,
    ) -> Result<ToolPreparation, PrepareError> {
        let path = args::string(args, "path")?;
        let requested = RequestedLineRange {
            start: args::optional_positive(args, "start_line")?,
            end: args::optional_positive(args, "end_line")?,
        };
        if matches!((requested.start, requested.end), (Some(start), Some(end)) if start > end) {
            return Err(args::invalid("start_line must not be greater than end_line").into());
        }

        let controlled = read_controlled_file(ctx.workspace, path)
            .map_err(|error| PrepareError::new(PrepareErrorKind::Io, error))?;
        let bytes = controlled.bytes;
        std::str::from_utf8(&bytes).map_err(|error| {
            PrepareError::new(
                PrepareErrorKind::Io,
                format!("read {path}: file is not valid UTF-8: {error}"),
            )
        })?;

        let spans = raw_line_spans(&bytes);
        let total_lines = logical_line_count(&bytes);
        debug_assert_eq!(total_lines, spans.len() as u64);
        let returned = selected_line_range(total_lines, requested);
        let selected = returned
            .map(|range| {
                let first = spans[(range.start - 1) as usize].0;
                let last = spans[(range.end - 1) as usize].1;
                &bytes[first..last]
            })
            .unwrap_or_default();
        let selected = std::str::from_utf8(selected).expect("slice of validated UTF-8");
        let path = normalized_relative_path(
            ctx.workspace,
            &ctx.workspace.root().join(controlled.relative),
        )?;

        let mut observation = FileObservation {
            path,
            sha256: sha256_bytes(&bytes),
            bytes: bytes.len() as u64,
            total_lines,
            requested,
            returned,
            model_truncated: false,
            complete: total_lines == 0
                || matches!(returned, Some(LineRange { start: 1, end }) if end == total_lines),
            coverage: returned.into_iter().collect(),
        };
        let full = evidence_envelope(
            "read_file",
            ctx.truncation_limit,
            &mut observation,
            selected,
            render_file_header,
            |observation| {
                observation.model_truncated = true;
                observation.complete = false;
                observation.coverage.clear();
            },
        )?;

        Ok(ToolPreparation::file_observation(full, observation))
    }
}

fn raw_line_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            spans.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < bytes.len() {
        spans.push((start, bytes.len()));
    }
    spans
}

fn selected_line_range(total_lines: u64, requested: RequestedLineRange) -> Option<LineRange> {
    if total_lines == 0 {
        return None;
    }
    let start = requested.start.unwrap_or(1);
    if start > total_lines {
        return None;
    }
    let end = requested.end.unwrap_or(total_lines).min(total_lines);
    (start <= end).then_some(LineRange { start, end })
}

fn render_file_header(observation: &FileObservation) -> String {
    let requested_start = observation
        .requested
        .start
        .map_or_else(|| "*".to_string(), |value| value.to_string());
    let requested_end = observation
        .requested
        .end
        .map_or_else(|| "*".to_string(), |value| value.to_string());
    let returned = observation.returned.map_or_else(
        || "none".to_string(),
        |range| format!("{}..{}", range.start, range.end),
    );
    format!(
        "[ferric:file_observation:v1]\npath={}\nsha256={}\nbytes={}\ntotal_lines={}\nrequested_start={}\nrequested_end={}\nreturned={}\nmodel_truncated={}\ncomplete={}\n[/ferric:file_observation:v1]\n",
        serde_json::to_string(&observation.path).expect("serializing a string cannot fail"),
        observation.sha256,
        observation.bytes,
        observation.total_lines,
        requested_start,
        requested_end,
        returned,
        u8::from(observation.model_truncated),
        u8::from(observation.complete),
    )
}
