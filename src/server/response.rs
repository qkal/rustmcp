use serde::Serialize;
use serde_json::{Value, json};

pub const DEFAULT_MAX_TOTAL_OUTPUT_BYTES: usize = 120_000;

#[derive(Debug, Serialize)]
pub struct ToolEnvelope {
    pub ok: bool,
    pub tool: &'static str,
    pub workspace_root: String,
    pub input: Value,
    pub result: Value,
    pub notes: Vec<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub fn success<T: Serialize>(
    tool: &'static str,
    workspace_root: impl Into<String>,
    input: &T,
    result: Value,
    notes: Vec<String>,
    truncated: bool,
) -> String {
    envelope_text(ToolEnvelope {
        ok: true,
        tool,
        workspace_root: workspace_root.into(),
        input: serde_json::to_value(input).unwrap_or(Value::Null),
        result,
        notes,
        truncated,
        error: None,
        hint: None,
    })
}

pub fn failure<T: Serialize>(
    tool: &'static str,
    workspace_root: impl Into<String>,
    input: &T,
    error: impl Into<String>,
    hint: impl Into<String>,
) -> String {
    envelope_text(ToolEnvelope {
        ok: false,
        tool,
        workspace_root: workspace_root.into(),
        input: serde_json::to_value(input).unwrap_or(Value::Null),
        result: json!({}),
        notes: Vec::new(),
        truncated: false,
        error: Some(error.into()),
        hint: Some(hint.into()),
    })
}

pub fn envelope_text(mut envelope: ToolEnvelope) -> String {
    let mut text = serialize_envelope(&envelope);
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    envelope.truncated = true;
    envelope.result = json!({
        "message": "tool output exceeded max_total_output_bytes",
        "max_total_output_bytes": DEFAULT_MAX_TOTAL_OUTPUT_BYTES,
    });
    envelope
        .notes
        .push("Result payload was truncated before serialization.".to_string());
    text = serialize_envelope(&envelope);
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    envelope.input = Value::Null;
    envelope.notes =
        vec!["Response payload was minimized after exceeding max_total_output_bytes.".to_string()];
    text = serialize_envelope(&envelope);
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    envelope.workspace_root = truncate_string(&envelope.workspace_root, 1024);
    envelope.error = envelope.error.map(|error| truncate_string(&error, 1024));
    envelope.hint = envelope.hint.map(|hint| truncate_string(&hint, 1024));
    envelope.notes.clear();
    text = serialize_envelope(&envelope);
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    envelope.workspace_root = truncate_string(&envelope.workspace_root, 128);
    envelope.result = json!({
        "message": "response exceeded max_total_output_bytes",
        "max_total_output_bytes": DEFAULT_MAX_TOTAL_OUTPUT_BYTES,
    });
    envelope.error = Some("response exceeded max_total_output_bytes".to_string());
    envelope.hint = None;
    text = serialize_envelope(&envelope);
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    format!(
        r#"{{"ok":false,"tool":"{}","truncated":true,"error":"response exceeded max_total_output_bytes"}}"#,
        envelope.tool
    )
}

fn serialize_envelope(envelope: &ToolEnvelope) -> String {
    serde_json::to_string_pretty(envelope)
        .unwrap_or_else(|error| format!(r#"{{"ok":false,"error":"{error}"}}"#))
}

fn truncate_string(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DEFAULT_MAX_TOTAL_OUTPUT_BYTES, failure, success};

    #[test]
    fn success_envelope_contains_stable_shape() {
        let text = success(
            "ra_hover",
            "C:/workspace",
            &json!({"file_path":"src/lib.rs"}),
            json!({"hover":"text"}),
            vec!["note".to_string()],
            false,
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["tool"], "ra_hover");
        assert_eq!(value["workspace_root"], "C:/workspace");
        assert_eq!(value["input"]["file_path"], "src/lib.rs");
        assert_eq!(value["result"]["hover"], "text");
        assert_eq!(value["notes"][0], "note");
        assert_eq!(value["truncated"], false);
        assert!(value.get("error").is_none());
        assert!(value.get("hint").is_none());
    }

    #[test]
    fn failure_envelope_contains_error_and_hint() {
        let text = failure(
            "cargo_check",
            "C:/workspace",
            &json!({}),
            "cargo validation failed",
            "Check parameters.",
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(value["ok"], false);
        assert_eq!(value["tool"], "cargo_check");
        assert_eq!(value["error"], "cargo validation failed");
        assert_eq!(value["hint"], "Check parameters.");
    }

    #[test]
    fn oversized_envelope_is_capped_after_second_pass() {
        let huge = "x".repeat(DEFAULT_MAX_TOTAL_OUTPUT_BYTES);
        let text = success(
            "ra_hover",
            huge.clone(),
            &json!({ "huge_input": huge }),
            json!({ "huge_result": "x".repeat(DEFAULT_MAX_TOTAL_OUTPUT_BYTES) }),
            vec!["x".repeat(DEFAULT_MAX_TOTAL_OUTPUT_BYTES)],
            false,
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert!(text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES);
        assert_eq!(value["truncated"], true);
    }
}
