use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const DEFAULT_DEFINITION_CONTEXT_LINES: u32 = 8;
pub const DEFAULT_REFERENCE_CONTEXT_LINES: u32 = 4;
pub const DEFAULT_MAX_RESULTS: u32 = 50;
pub const DEFAULT_DIAGNOSTICS_WAIT_MS: u64 = 1_500;
pub const DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS: u64 = 3_000;
pub const DEFAULT_MAX_FILES: u32 = 100;
pub const DEFAULT_MAX_DIAGNOSTICS: u32 = 300;
pub const DEFAULT_MAX_SNIPPET_BYTES: usize = 8_192;
pub const DEFAULT_MAX_TOTAL_OUTPUT_BYTES: usize = 120_000;

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct SetWorkspaceParams {
    pub workspace_path: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct HoverParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct DefinitionParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub context_lines: Option<u32>,
    pub include_snippets: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct ReferencesParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub include_declaration: Option<bool>,
    pub max_results: Option<u32>,
    pub context_lines: Option<u32>,
    pub include_snippets: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct DocumentSymbolsParams {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct CompletionParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub max_results: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct FormatParams {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct CodeActionsParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct DiagnosticsParams {
    pub file_path: String,
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct WorkspaceDiagnosticsParams {
    pub wait_ms: Option<u64>,
    pub max_files: Option<u32>,
    pub max_diagnostics: Option<u32>,
}

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
    let mut text = serde_json::to_string_pretty(&envelope)
        .unwrap_or_else(|error| format!(r#"{{"ok":false,"error":"{error}"}}"#));
    if text.len() <= DEFAULT_MAX_TOTAL_OUTPUT_BYTES {
        return text;
    }

    envelope.truncated = true;
    envelope.result = json!({
        "message": "tool output exceeded max_total_output_bytes",
        "max_total_output_bytes": DEFAULT_MAX_TOTAL_OUTPUT_BYTES,
    });
    envelope.notes.push("Result payload was truncated before serialization.".to_string());
    text = serde_json::to_string_pretty(&envelope)
        .unwrap_or_else(|error| format!(r#"{{"ok":false,"error":"{error}"}}"#));
    text
}

#[cfg(test)]
mod tests {
    use super::HoverParams;

    #[test]
    fn hover_params_schema_is_generated() {
        let schema = schemars::schema_for!(HoverParams);
        let schema_json = serde_json::to_value(schema).unwrap();
        assert_eq!(schema_json["title"], "HoverParams");
    }
}

