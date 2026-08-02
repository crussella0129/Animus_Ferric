use serde_json::json;

use ferric_guard::PermissionLevel;

use crate::control::{
    FileObservation, LineRange, PrepareCtx, PrepareError, PrepareErrorKind, RequestedLineRange,
    ToolPreparation, logical_line_count, normalized_relative_path, sha256_bytes,
};
use crate::spec::{Tool, ToolCtx, ToolSpec};

use super::path_arg;

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

    fn run(&self, ctx: &ToolCtx<'_>, args: &serde_json::Value) -> Result<String, String> {
        let path = path_arg(args)?;
        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        let resolved = ctx
            .workspace
            .resolve(path)
            .map_err(|e| format!("boundary: {e}"))?;

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
        let path = args
            .get("path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                PrepareError::new(
                    PrepareErrorKind::InvalidArguments,
                    "missing required string argument: path",
                )
            })?;
        let requested = RequestedLineRange {
            start: optional_positive_integer(args, "start_line")?,
            end: optional_positive_integer(args, "end_line")?,
        };
        if matches!((requested.start, requested.end), (Some(start), Some(end)) if start > end) {
            return Err(PrepareError::new(
                PrepareErrorKind::InvalidArguments,
                "start_line must not be greater than end_line",
            ));
        }

        let resolved = ctx.workspace.resolve(path).map_err(|error| {
            PrepareError::new(PrepareErrorKind::Io, format!("boundary: {error}"))
        })?;
        let bytes = std::fs::read(&resolved).map_err(|error| {
            PrepareError::new(PrepareErrorKind::Io, format!("read {path}: {error}"))
        })?;
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
        let path = normalized_relative_path(ctx.workspace, &resolved)?;

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
        let header = render_file_header(&observation);
        require_header_fits(&header, ctx.truncation_limit)?;
        let mut full = format!("{header}{selected}");
        if full.chars().count() > ctx.truncation_limit {
            observation.model_truncated = true;
            observation.complete = false;
            observation.coverage.clear();
            let header = render_file_header(&observation);
            require_header_fits(&header, ctx.truncation_limit)?;
            full = format!("{header}{selected}");
        }

        Ok(ToolPreparation::file_observation(full, observation))
    }
}

fn optional_positive_integer(
    args: &serde_json::Value,
    key: &str,
) -> Result<Option<u64>, PrepareError> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let number = value.as_u64().filter(|number| *number > 0).ok_or_else(|| {
        PrepareError::new(
            PrepareErrorKind::InvalidArguments,
            format!("{key} must be a positive integer"),
        )
    })?;
    Ok(Some(number))
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

fn require_header_fits(header: &str, limit: usize) -> Result<(), PrepareError> {
    if header.chars().count() > limit {
        return Err(PrepareError::new(
            PrepareErrorKind::OutputLimitTooSmall,
            format!("model output limit {limit} is too small for the read_file evidence envelope"),
        ));
    }
    Ok(())
}
