//! Argument parsing shared by every builtin tool. An [`ArgError`] converts
//! into both the legacy `String` error channel and an `InvalidArguments`
//! [`PrepareError`], so `run` and `prepare` report the same text.

use serde_json::Value;

use crate::control::{PrepareError, PrepareErrorKind};

pub(super) struct ArgError(String);

impl From<ArgError> for String {
    fn from(error: ArgError) -> Self {
        error.0
    }
}

impl From<ArgError> for PrepareError {
    fn from(error: ArgError) -> Self {
        PrepareError::new(PrepareErrorKind::InvalidArguments, error.0)
    }
}

/// A required string argument.
pub(super) fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, ArgError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ArgError(format!("missing required string argument: {key}")))
}

/// A required array argument.
pub(super) fn array<'a>(args: &'a Value, key: &str) -> Result<&'a Vec<Value>, ArgError> {
    args.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ArgError(format!("missing required array argument: {key}")))
}

/// An optional argument that, when present, must be a positive integer.
pub(super) fn optional_positive(args: &Value, key: &str) -> Result<Option<u64>, ArgError> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    value
        .as_u64()
        .filter(|number| *number > 0)
        .map(Some)
        .ok_or_else(|| ArgError(format!("{key} must be a positive integer")))
}

/// The legacy reading of an optional count: any unsigned integer, else `default`.
pub(super) fn legacy_count(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(default)
}

/// An argument-level refusal raised after parsing (e.g. an empty pattern).
pub(super) fn invalid(message: impl Into<String>) -> ArgError {
    ArgError(message.into())
}

/// The `from`/`to` endpoints of a two-path tool, for the registry's guard.
pub(super) fn endpoint_paths(args: &Value) -> Vec<String> {
    ["from", "to"]
        .iter()
        .filter_map(|key| args.get(*key).and_then(Value::as_str).map(String::from))
        .collect()
}
