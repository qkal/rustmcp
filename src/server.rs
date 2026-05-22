use std::{path::PathBuf, sync::Arc, time::Duration};

use lsp_types::{
    CodeActionOrCommand, CompletionResponse, Diagnostic, DiagnosticSeverity,
    GotoDefinitionResponse, Hover, HoverContents, LocationLink, MarkedString, Position, Range,
};
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, tool, tool_handler, tool_router,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    error::RaMcpError,
    lsp::{
        client::RustAnalyzerClient,
        snippets::{SourceSnippet, read_snippet},
    },
    tools::{
        CodeActionsParams, CompletionParams, DEFAULT_DEFINITION_CONTEXT_LINES,
        DEFAULT_DIAGNOSTICS_WAIT_MS, DEFAULT_MAX_DIAGNOSTICS, DEFAULT_MAX_FILES,
        DEFAULT_MAX_RESULTS, DEFAULT_MAX_SNIPPET_BYTES, DEFAULT_REFERENCE_CONTEXT_LINES,
        DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS, DefinitionParams, DiagnosticsParams,
        DocumentSymbolsParams, FormatParams, HoverParams, ReferencesParams, SetWorkspaceParams,
        WorkspaceDiagnosticsParams, failure, success,
    },
    workspace::{ClassifiedLocation, LocationKind, Workspace},
};

#[derive(Clone)]
pub struct RaMcpServer {
    state: Arc<Mutex<ServerState>>,
}

struct ServerState {
    workspace: Workspace,
    client: Option<RustAnalyzerClient>,
}

#[derive(Debug, Serialize)]
struct LocatedRange {
    uri: String,
    path: Option<String>,
    kind: LocationKind,
    range: Range,
    snippet: Option<SourceSnippet>,
    notes: Vec<String>,
}

impl RaMcpServer {
    pub fn new(workspace: PathBuf) -> crate::error::Result<Self> {
        Ok(Self {
            state: Arc::new(Mutex::new(ServerState {
                workspace: Workspace::new(workspace)?,
                client: None,
            })),
        })
    }

    async fn ensure_client(state: &mut ServerState) -> crate::error::Result<&mut RustAnalyzerClient> {
        if state.client.is_none() {
            state.client = Some(RustAnalyzerClient::spawn(state.workspace.clone()).await?);
        }
        state.client.as_mut().ok_or(RaMcpError::AnalyzerNotRunning)
    }

    fn workspace_root(state: &ServerState) -> String {
        state.workspace.root().display().to_string()
    }

    fn workspace_notes(workspace: &Workspace) -> Vec<String> {
        let mut notes = Vec::new();
        if workspace.warnings().missing_cargo_toml {
            notes.push("Workspace root does not contain Cargo.toml.".to_string());
        }
        notes
    }

    fn hint_for_error(error: &RaMcpError) -> &'static str {
        match error {
            RaMcpError::OutsideWorkspace => {
                "Pass a path relative to the configured Rust workspace."
            }
            RaMcpError::RustAnalyzerMissing => {
                "Install rust-analyzer, for example: rustup component add rust-analyzer."
            }
            RaMcpError::FileMissing(_) | RaMcpError::NotAFile(_) => {
                "Pass an existing Rust source file inside the workspace root."
            }
            _ => "Check the workspace path, rust-analyzer installation, and input parameters.",
        }
    }
}

#[tool_router]
impl RaMcpServer {
    #[tool(
        name = "ra_set_workspace",
        description = "Change the active Rust workspace root and restart rust-analyzer."
    )]
    async fn ra_set_workspace(
        &self,
        Parameters(params): Parameters<SetWorkspaceParams>,
    ) -> String {
        let mut state = self.state.lock().await;
        let old_root = Self::workspace_root(&state);
        let new_workspace = match Workspace::new(&params.workspace_path) {
            Ok(workspace) => workspace,
            Err(error) => {
                return failure(
                    "ra_set_workspace",
                    old_root,
                    &params,
                    error.to_string(),
                    Self::hint_for_error(&error),
                );
            }
        };

        if let Some(mut client) = state.client.take() {
            let _ = client.shutdown().await;
        }

        state.workspace = new_workspace;
        let mut notes = Self::workspace_notes(&state.workspace);
        let restart = match RustAnalyzerClient::spawn(state.workspace.clone()).await {
            Ok(client) => {
                state.client = Some(client);
                true
            }
            Err(error) => {
                notes.push(format!("rust-analyzer did not restart: {error}"));
                false
            }
        };

        success(
            "ra_set_workspace",
            Self::workspace_root(&state),
            &params,
            json!({
                "workspace_root": Self::workspace_root(&state),
                "rust_analyzer_restarted": restart,
                "warnings": state.workspace.warnings(),
            }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_hover", description = "Get hover/type/documentation information at a position.")]
    async fn ra_hover(&self, Parameters(params): Parameters<HoverParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_hover", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let workspace = state.workspace.clone();
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_hover", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let hover = match client.hover(&file, params.line, params.character).await {
            Ok(hover) => hover,
            Err(error) => {
                return failure("ra_hover", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let uri = workspace.uri_for_file(&file).map(|uri| uri.to_string()).unwrap_or_default();
        let mut notes = Self::workspace_notes(&workspace);
        if hover.is_none() {
            notes.push("No hover returned. rust-analyzer may still be indexing or the position has no symbol.".to_string());
        }

        success(
            "ra_hover",
            root,
            &params,
            json!({
                "file_uri": uri,
                "hover": hover.as_ref().map(hover_to_markdown),
                "raw_hover": hover,
            }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_definition", description = "Find definitions at a position.")]
    async fn ra_definition(&self, Parameters(params): Parameters<DefinitionParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_definition", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let workspace = state.workspace.clone();
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_definition", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let response = match client.definition(&file, params.line, params.character).await {
            Ok(response) => response,
            Err(error) => {
                return failure("ra_definition", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let context_lines = params.context_lines.unwrap_or(DEFAULT_DEFINITION_CONTEXT_LINES);
        let include_snippets = params.include_snippets.unwrap_or(true);
        let mut notes = Self::workspace_notes(&workspace);
        let locations = definition_locations(response)
            .into_iter()
            .map(|(uri, range)| locate(&workspace, uri, range, context_lines, include_snippets))
            .collect::<Vec<_>>();
        if locations.is_empty() {
            notes.push("No definitions returned.".to_string());
        }

        success(
            "ra_definition",
            root,
            &params,
            json!({ "locations": locations }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_references", description = "Find references at a position.")]
    async fn ra_references(&self, Parameters(params): Parameters<ReferencesParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_references", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let workspace = state.workspace.clone();
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_references", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let references = match client
            .references(
                &file,
                params.line,
                params.character,
                params.include_declaration.unwrap_or(true),
            )
            .await
        {
            Ok(references) => references,
            Err(error) => {
                return failure("ra_references", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };

        let max_results = params.max_results.unwrap_or(DEFAULT_MAX_RESULTS) as usize;
        let truncated = references.len() > max_results;
        let context_lines = params.context_lines.unwrap_or(DEFAULT_REFERENCE_CONTEXT_LINES);
        let include_snippets = params.include_snippets.unwrap_or(true);
        let located = references
            .into_iter()
            .take(max_results)
            .map(|location| locate(&workspace, location.uri, location.range, context_lines, include_snippets))
            .collect::<Vec<_>>();

        success(
            "ra_references",
            root,
            &params,
            json!({
                "references": located,
                "max_results": max_results,
            }),
            Self::workspace_notes(&workspace),
            truncated,
        )
    }

    #[tool(name = "ra_document_symbols", description = "List symbols in a Rust source file.")]
    async fn ra_document_symbols(
        &self,
        Parameters(params): Parameters<DocumentSymbolsParams>,
    ) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_document_symbols", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let notes = Self::workspace_notes(&state.workspace);
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_document_symbols", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let symbols = match client.document_symbols(&file).await {
            Ok(symbols) => symbols,
            Err(error) => {
                return failure("ra_document_symbols", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        success(
            "ra_document_symbols",
            root,
            &params,
            json!({ "symbols": symbols }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_completion", description = "Get completion suggestions at a position.")]
    async fn ra_completion(&self, Parameters(params): Parameters<CompletionParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_completion", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let notes = Self::workspace_notes(&state.workspace);
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_completion", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let completions = match client.completion(&file, params.line, params.character).await {
            Ok(completions) => completions,
            Err(error) => {
                return failure("ra_completion", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let max_results = params.max_results.unwrap_or(DEFAULT_MAX_RESULTS) as usize;
        let (items, total) = completion_items(completions);
        let truncated = total > max_results;
        success(
            "ra_completion",
            root,
            &params,
            json!({
                "items": items.into_iter().take(max_results).collect::<Vec<_>>(),
                "total_returned_by_rust_analyzer": total,
                "max_results": max_results,
            }),
            notes,
            truncated,
        )
    }

    #[tool(name = "ra_format", description = "Return formatting text edits for a file without applying them.")]
    async fn ra_format(&self, Parameters(params): Parameters<FormatParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_format", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let mut notes = Self::workspace_notes(&state.workspace);
        notes.push("This tool returns formatting edits only; it does not mutate files.".to_string());
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_format", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let edits = match client.formatting(&file).await {
            Ok(edits) => edits,
            Err(error) => {
                return failure("ra_format", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        success("ra_format", root, &params, json!({ "text_edits": edits }), notes, false)
    }

    #[tool(name = "ra_code_actions", description = "Return available code actions for a selected range without applying edits.")]
    async fn ra_code_actions(
        &self,
        Parameters(params): Parameters<CodeActionsParams>,
    ) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_code_actions", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let mut notes = Self::workspace_notes(&state.workspace);
        notes.push("This tool returns code actions only; it does not apply edits or commands.".to_string());
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_code_actions", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let actions = match client
            .code_actions(
                &file,
                Range::new(
                    Position::new(params.line, params.character),
                    Position::new(params.end_line, params.end_character),
                ),
            )
            .await
        {
            Ok(actions) => actions,
            Err(error) => {
                return failure("ra_code_actions", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        success(
            "ra_code_actions",
            root,
            &params,
            json!({ "code_actions": summarize_code_actions(actions) }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_diagnostics", description = "Return cached diagnostics for a Rust source file.")]
    async fn ra_diagnostics(&self, Parameters(params): Parameters<DiagnosticsParams>) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let file = match state.workspace.resolve_existing_file(&params.file_path) {
            Ok(file) => file,
            Err(error) => {
                return failure("ra_diagnostics", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let workspace = state.workspace.clone();
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_diagnostics", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        let uri = match client.open_document(&file).await {
            Ok(uri) => uri,
            Err(error) => {
                return failure("ra_diagnostics", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        tokio::time::sleep(Duration::from_millis(
            params.wait_ms.unwrap_or(DEFAULT_DIAGNOSTICS_WAIT_MS),
        ))
        .await;
        let diagnostics = client.diagnostics_for(&uri).await;
        let mut notes = Self::workspace_notes(&workspace);
        if diagnostics.is_empty() {
            notes.push("No diagnostics are currently cached for this file; rust-analyzer may still be indexing or the file has no diagnostics.".to_string());
        }
        success(
            "ra_diagnostics",
            root,
            &params,
            json!({
                "file_uri": uri.as_str(),
                "summary": diagnostic_summary(&diagnostics),
                "diagnostics": diagnostics,
            }),
            notes,
            false,
        )
    }

    #[tool(name = "ra_workspace_diagnostics", description = "Return known cached diagnostics across the active workspace.")]
    async fn ra_workspace_diagnostics(
        &self,
        Parameters(params): Parameters<WorkspaceDiagnosticsParams>,
    ) -> String {
        let mut state = self.state.lock().await;
        let root = Self::workspace_root(&state);
        let max_files = params.max_files.unwrap_or(DEFAULT_MAX_FILES) as usize;
        let max_diagnostics = params.max_diagnostics.unwrap_or(DEFAULT_MAX_DIAGNOSTICS) as usize;
        let notes = Self::workspace_notes(&state.workspace);
        let client = match Self::ensure_client(&mut state).await {
            Ok(client) => client,
            Err(error) => {
                return failure("ra_workspace_diagnostics", root, &params, error.to_string(), Self::hint_for_error(&error));
            }
        };
        tokio::time::sleep(Duration::from_millis(
            params.wait_ms.unwrap_or(DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS),
        ))
        .await;
        let all = client.all_diagnostics().await;
        let mut total_seen = 0_usize;
        let mut grouped = Vec::new();
        for (uri, diagnostics) in all.into_iter().take(max_files) {
            let remaining = max_diagnostics.saturating_sub(total_seen);
            if remaining == 0 {
                break;
            }
            let selected = diagnostics.into_iter().take(remaining).collect::<Vec<_>>();
            total_seen += selected.len();
            grouped.push(json!({
                "uri": uri.as_str(),
                "summary": diagnostic_summary(&selected),
                "diagnostics": selected,
            }));
        }
        let truncated = grouped.len() >= max_files || total_seen >= max_diagnostics;
        success(
            "ra_workspace_diagnostics",
            root,
            &params,
            json!({
                "files": grouped,
                "max_files": max_files,
                "max_diagnostics": max_diagnostics,
            }),
            notes,
            truncated,
        )
    }
}

#[tool_handler(
    name = "rust-analyzer-mcp",
    instructions = "Readonly Rust IDE intelligence through rust-analyzer. Tools return structured JSON text and do not mutate files."
)]
impl ServerHandler for RaMcpServer {}

fn hover_to_markdown(hover: &Hover) -> String {
    match hover.contents.clone() {
        HoverContents::Scalar(marked) => marked_string(&marked),
        HoverContents::Array(items) => items.iter().map(marked_string).collect::<Vec<_>>().join("\n\n"),
        HoverContents::Markup(markup) => markup.value.clone(),
    }
}

fn marked_string(marked: &MarkedString) -> String {
    match marked {
        MarkedString::String(text) => text.clone(),
        MarkedString::LanguageString(language) => {
            format!("```{}\n{}\n```", language.language, language.value)
        }
    }
}

fn definition_locations(response: Option<GotoDefinitionResponse>) -> Vec<(lsp_types::Uri, Range)> {
    match response {
        Some(GotoDefinitionResponse::Scalar(location)) => vec![(location.uri, location.range)],
        Some(GotoDefinitionResponse::Array(locations)) => locations
            .into_iter()
            .map(|location| (location.uri, location.range))
            .collect(),
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link: LocationLink| (link.target_uri, link.target_selection_range))
            .collect(),
        None => Vec::new(),
    }
}

fn locate(
    workspace: &Workspace,
    uri: lsp_types::Uri,
    range: Range,
    context_lines: u32,
    include_snippet: bool,
) -> LocatedRange {
    let classified = workspace
        .classify_lsp_uri(&uri)
        .unwrap_or_else(|_| ClassifiedLocation {
            uri: uri.as_str().to_string(),
            kind: LocationKind::NonFileUri,
            path: None,
        });
    let mut notes = Vec::new();
    let snippet = if include_snippet {
        match &classified.path {
            Some(path) => match read_snippet(path, range, context_lines, DEFAULT_MAX_SNIPPET_BYTES) {
                Ok(snippet) => snippet,
                Err(error) => {
                    notes.push(format!("snippet unavailable: {error}"));
                    None
                }
            },
            None => {
                notes.push("snippet unavailable for non-file or missing URI".to_string());
                None
            }
        }
    } else {
        None
    };
    LocatedRange {
        uri: classified.uri,
        path: classified.path.map(|path| path.display().to_string()),
        kind: classified.kind,
        range,
        snippet,
        notes,
    }
}

fn completion_items(response: Option<CompletionResponse>) -> (Vec<Value>, usize) {
    let items = match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    let total = items.len();
    let values = items
        .into_iter()
        .map(|item| {
            json!({
                "label": item.label,
                "kind": item.kind,
                "detail": item.detail,
                "documentation": item.documentation,
                "insert_text": item.insert_text,
                "text_edit": item.text_edit,
            })
        })
        .collect();
    (values, total)
}

fn summarize_code_actions(actions: Vec<CodeActionOrCommand>) -> Vec<Value> {
    actions
        .into_iter()
        .map(|action| match action {
            CodeActionOrCommand::Command(command) => json!({
                "title": command.title,
                "kind": "command",
                "command": command.command,
                "arguments": command.arguments,
            }),
            CodeActionOrCommand::CodeAction(action) => json!({
                "title": action.title,
                "kind": action.kind,
                "diagnostics": action.diagnostics,
                "edit": action.edit,
                "command": action.command,
            }),
        })
        .collect()
}

fn diagnostic_summary(diagnostics: &[Diagnostic]) -> Value {
    let mut errors = 0_usize;
    let mut warnings = 0_usize;
    let mut info = 0_usize;
    let mut hints = 0_usize;
    for diagnostic in diagnostics {
        match diagnostic.severity {
            Some(DiagnosticSeverity::ERROR) => errors += 1,
            Some(DiagnosticSeverity::WARNING) => warnings += 1,
            Some(DiagnosticSeverity::INFORMATION) => info += 1,
            Some(DiagnosticSeverity::HINT) => hints += 1,
            _ => info += 1,
        }
    }
    json!({
        "total": diagnostics.len(),
        "errors": errors,
        "warnings": warnings,
        "information": info,
        "hints": hints,
    })
}
