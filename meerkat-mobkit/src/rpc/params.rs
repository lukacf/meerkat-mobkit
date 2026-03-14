//! Shared parameter extraction helpers for RPC handlers.
//!
//! Every extractor returns a concrete error string on failure so callers can
//! wrap it in a `-32602 Invalid params` JSON-RPC error without duplicating
//! boilerplate.

#![allow(dead_code)] // Helpers are available for incremental adoption across handlers.

use serde_json::Value;

/// Extract a required string field, trimmed.  Returns an error message on
/// missing, non-string, or empty-after-trim values.
pub fn required_str<'a>(params: &'a Value, field: &str) -> Result<&'a str, String> {
    let value = params
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{field} must be a non-empty string"))?;
    Ok(value)
}

/// Extract an optional string field, trimmed.  Returns `Ok(None)` when the
/// key is absent or `null`, and an error when the value is present but not a
/// valid non-empty string.
pub fn optional_str<'a>(params: &'a Value, field: &str) -> Result<Option<&'a str>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("{field} must be a string when provided"))?;
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err(format!("{field} must be a non-empty string when provided"));
            }
            Ok(Some(trimmed))
        }
    }
}

/// Extract a required array-of-strings field.  Rejects missing fields,
/// non-array values, and non-string entries (no silent filtering).
pub fn required_string_array(params: &Value, field: &str) -> Result<Vec<String>, String> {
    let arr = params
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{field} must be an array of strings"))?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let s = entry.as_str().ok_or_else(|| {
            format!(
                "{field}[{i}] must be a string, got {}",
                entry_type_name(entry)
            )
        })?;
        out.push(s.to_string());
    }
    Ok(out)
}

/// Extract an optional u64 field.  Returns `Ok(None)` when absent/null,
/// errors when present but not a valid u64.
pub fn optional_u64(params: &Value, field: &str) -> Result<Option<u64>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| format!("{field} must be a positive integer when provided"))?;
            Ok(Some(n))
        }
    }
}

/// Extract an optional bool field.
pub fn optional_bool(params: &Value, field: &str) -> Result<Option<bool>, String> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let b = v
                .as_bool()
                .ok_or_else(|| format!("{field} must be a boolean when provided"))?;
            Ok(Some(b))
        }
    }
}

fn entry_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
