use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_DEFINITION_CONTEXT_LINES: u32 = 8;
pub const DEFAULT_REFERENCE_CONTEXT_LINES: u32 = 4;
pub const DEFAULT_MAX_RESULTS: u32 = 50;
pub const DEFAULT_DIAGNOSTICS_WAIT_MS: u64 = 1_500;
pub const DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS: u64 = 3_000;
pub const DEFAULT_MAX_FILES: u32 = 100;
pub const DEFAULT_MAX_DIAGNOSTICS: u32 = 300;
pub const DEFAULT_MAX_SNIPPET_BYTES: usize = 8_192;

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
pub struct RenamePreviewParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
    pub new_name: String,
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

pub(crate) fn validate_rename_name(new_name: &str) -> Result<(), &'static str> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err("new_name must not be empty or whitespace-only");
    }
    if trimmed.starts_with('-') {
        return Err("new_name must not start with '-'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DEFAULT_MAX_RESULTS, HoverParams, RenamePreviewParams, validate_rename_name};

    #[test]
    fn hover_params_schema_is_generated() {
        let schema = schemars::schema_for!(HoverParams);
        let schema_json = serde_json::to_value(schema).unwrap();
        assert_eq!(schema_json["title"], "HoverParams");
    }

    #[test]
    fn default_max_results_remains_fifty() {
        assert_eq!(json!(DEFAULT_MAX_RESULTS), json!(50));
    }

    #[test]
    fn rename_preview_params_schema_is_generated() {
        let schema = schemars::schema_for!(RenamePreviewParams);
        let schema_json = serde_json::to_value(schema).unwrap();
        assert_eq!(schema_json["title"], "RenamePreviewParams");
    }

    #[test]
    fn rename_name_validation_rejects_empty_whitespace_and_option_like_values() {
        assert_eq!(
            validate_rename_name("").unwrap_err(),
            "new_name must not be empty or whitespace-only"
        );
        assert_eq!(
            validate_rename_name("   ").unwrap_err(),
            "new_name must not be empty or whitespace-only"
        );
        assert_eq!(
            validate_rename_name(" --bad").unwrap_err(),
            "new_name must not start with '-'"
        );
        assert!(validate_rename_name("renamed_answer").is_ok());
    }
}
