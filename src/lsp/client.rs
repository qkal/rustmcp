use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use lsp_types::{
    CodeActionContext, CodeActionParams, CodeActionResponse, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, FormattingOptions, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, Location, PartialResultParams, Position,
    PublishDiagnosticsParams, Range, ReferenceContext, ReferenceParams, RenameParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceEdit,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{Mutex, mpsc, oneshot},
};
use tracing::{debug, warn};

use crate::{
    error::{RaMcpError, Result},
    lsp::{
        framing::{FrameDecoder, encode_message},
        protocol::{DiagnosticsCache, JsonRpcError, lsp_uri_from_url},
    },
    workspace::Workspace,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type PendingSender = oneshot::Sender<Result<Value>>;
type PendingMap = Arc<Mutex<HashMap<u64, PendingSender>>>;

#[derive(Debug)]
pub struct RustAnalyzerClient {
    workspace: Workspace,
    tx: mpsc::Sender<Value>,
    pending: PendingMap,
    next_id: AtomicU64,
    diagnostics: DiagnosticsCache,
    opened: HashMap<PathBuf, OpenDocument>,
    child: Child,
}

#[derive(Debug, Clone)]
struct OpenDocument {
    uri: Uri,
    version: i32,
    hash: u64,
}

impl RustAnalyzerClient {
    pub async fn spawn(workspace: Workspace) -> Result<Self> {
        let analyzer =
            which::which("rust-analyzer").map_err(|_| RaMcpError::RustAnalyzerMissing)?;
        let mut command = Command::new(analyzer);
        command
            .current_dir(workspace.root())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or(RaMcpError::AnalyzerNotRunning)?;
        let stdout = child.stdout.take().ok_or(RaMcpError::AnalyzerNotRunning)?;
        let stderr = child.stderr.take().ok_or(RaMcpError::AnalyzerNotRunning)?;

        let (tx, rx) = mpsc::channel::<Value>(128);
        let pending = PendingMap::default();
        let diagnostics = DiagnosticsCache::default();

        spawn_writer(stdin, rx);
        spawn_reader(
            workspace.clone(),
            stdout,
            tx.clone(),
            pending.clone(),
            diagnostics.clone(),
        );
        spawn_stderr_logger(stderr);

        let client = Self {
            workspace,
            tx,
            pending,
            next_id: AtomicU64::new(1),
            diagnostics,
            opened: HashMap::new(),
            child,
        };

        client.initialize().await?;
        client.notify("initialized", json!({})).await?;
        Ok(client)
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub async fn hover(&mut self, file: &Path, line: u32, character: u32) -> Result<Option<Hover>> {
        let uri = self.open_document(file).await?;
        let params = HoverParams {
            text_document_position_params: position_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.request_optional("textDocument/hover", params).await
    }

    pub async fn definition(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = self.open_document(file).await?;
        let params = GotoDefinitionParams {
            text_document_position_params: position_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.request_optional("textDocument/definition", params)
            .await
    }

    pub async fn references(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>> {
        let uri = self.open_document(file).await?;
        let params = ReferenceParams {
            text_document_position: position_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };
        self.request_optional("textDocument/references", params)
            .await
            .map(Option::unwrap_or_default)
    }

    pub async fn document_symbols(
        &mut self,
        file: &Path,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = self.open_document(file).await?;
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier::new(uri),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.request_optional("textDocument/documentSymbol", params)
            .await
    }

    pub async fn completion(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<CompletionResponse>> {
        let uri = self.open_document(file).await?;
        let params = CompletionParams {
            text_document_position: position_params(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        };
        self.request_optional("textDocument/completion", params)
            .await
    }

    pub async fn formatting(&mut self, file: &Path) -> Result<Vec<lsp_types::TextEdit>> {
        let uri = self.open_document(file).await?;
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier::new(uri),
            options: FormattingOptions {
                tab_size: 4,
                insert_spaces: true,
                properties: HashMap::new(),
                trim_trailing_whitespace: Some(true),
                insert_final_newline: Some(true),
                trim_final_newlines: Some(true),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.request_optional("textDocument/formatting", params)
            .await
            .map(Option::unwrap_or_default)
    }

    pub async fn code_actions(&mut self, file: &Path, range: Range) -> Result<CodeActionResponse> {
        let uri = self.open_document(file).await?;
        let diagnostics = self.diagnostics.get(&uri).await;
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier::new(uri),
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.request_optional("textDocument/codeAction", params)
            .await
            .map(Option::unwrap_or_default)
    }

    pub async fn rename(
        &mut self,
        file: &Path,
        line: u32,
        character: u32,
        new_name: String,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = self.open_document(file).await?;
        let params = RenameParams {
            text_document_position: position_params(uri, line, character),
            new_name,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        self.request_optional("textDocument/rename", params).await
    }

    pub async fn open_document(&mut self, file: &Path) -> Result<Uri> {
        let canonical = self.workspace.resolve_existing_file(file)?;
        let text = tokio::fs::read_to_string(&canonical).await?;
        let uri = lsp_uri_from_url(&self.workspace.uri_for_file(&canonical)?)?;
        let hash = hash_text(&text);

        match self.opened.get_mut(&canonical) {
            Some(open) if open.hash == hash => Ok(open.uri.clone()),
            Some(open) => {
                open.version += 1;
                open.hash = hash;
                let params = DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier::new(
                        open.uri.clone(),
                        open.version,
                    ),
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text,
                    }],
                };
                let uri = open.uri.clone();
                self.notify("textDocument/didChange", serde_json::to_value(params)?)
                    .await?;
                Ok(uri)
            }
            None => {
                let params = DidOpenTextDocumentParams {
                    text_document: TextDocumentItem::new(uri.clone(), "rust".to_string(), 1, text),
                };
                self.notify("textDocument/didOpen", serde_json::to_value(params)?)
                    .await?;
                self.opened.insert(
                    canonical,
                    OpenDocument {
                        uri: uri.clone(),
                        version: 1,
                        hash,
                    },
                );
                Ok(uri)
            }
        }
    }

    pub async fn diagnostics_for(&self, uri: &Uri) -> Vec<lsp_types::Diagnostic> {
        self.diagnostics.get(uri).await
    }

    pub async fn all_diagnostics(&self) -> Vec<(String, Vec<lsp_types::Diagnostic>)> {
        self.diagnostics.all().await
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            self.request_value("shutdown", json!(null)),
        )
        .await;
        let _ = self.notify("exit", json!(null)).await;
        match tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                let _ = self.child.kill().await;
                Ok(())
            }
        }
    }

    async fn initialize(&self) -> Result<()> {
        let root_uri = self.workspace_root_uri()?;
        let root_name = self
            .workspace
            .root()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri.as_str(),
            "workspaceFolders": [{
                "uri": root_uri.as_str(),
                "name": root_name
            }],
            "capabilities": {
                "workspace": {
                    "workspaceFolders": true,
                    "configuration": true,
                    "workspaceEdit": {
                        "documentChanges": true
                    }
                },
                "textDocument": {
                    "hover": {
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "definition": {
                        "linkSupport": true
                    },
                    "references": {},
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "completion": {
                        "completionItem": {
                            "documentationFormat": ["markdown", "plaintext"],
                            "snippetSupport": false
                        }
                    },
                    "formatting": {},
                    "codeAction": {
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source"]
                            }
                        }
                    },
                    "rename": {},
                    "publishDiagnostics": {
                        "relatedInformation": true,
                        "versionSupport": true
                    },
                    "inlayHint": {}
                },
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            },
            "clientInfo": {
                "name": "rust-analyzer-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        let _ = self.request_value("initialize", params).await?;
        Ok(())
    }

    async fn request_optional<P, T>(&self, method: &str, params: P) -> Result<Option<T>>
    where
        P: serde::Serialize,
        T: serde::de::DeserializeOwned,
    {
        let value = self
            .request_value(method, serde_json::to_value(params)?)
            .await?;
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(value)?))
        }
    }

    async fn request_value(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        if self.tx.send(message).await.is_err() {
            let _ = self.pending.lock().await.remove(&id);
            return Err(RaMcpError::AnalyzerNotRunning);
        }

        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RaMcpError::AnalyzerNotRunning),
            Err(_) => {
                let _ = self.pending.lock().await.remove(&id);
                Err(RaMcpError::Lsp(format!("request {method} timed out")))
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.tx
            .send(json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }))
            .await
            .map_err(|_| RaMcpError::AnalyzerNotRunning)
    }

    fn workspace_root_uri(&self) -> Result<url::Url> {
        url::Url::from_directory_path(self.workspace.root()).map_err(|_| RaMcpError::UrlConversion)
    }
}

fn spawn_writer(
    mut stdin: tokio::process::ChildStdin,
    mut rx: mpsc::Receiver<Value>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(value) = rx.recv().await {
            let frame = match encode_message(&value) {
                Ok(frame) => frame,
                Err(error) => {
                    warn!(%error, "failed to encode LSP message");
                    continue;
                }
            };
            if let Err(error) = stdin.write_all(&frame).await {
                warn!(%error, "failed to write to rust-analyzer stdin");
                break;
            }
            if let Err(error) = stdin.flush().await {
                warn!(%error, "failed to flush rust-analyzer stdin");
                break;
            }
        }
    })
}

fn spawn_reader(
    workspace: Workspace,
    mut stdout: tokio::process::ChildStdout,
    tx: mpsc::Sender<Value>,
    pending: PendingMap,
    diagnostics: DiagnosticsCache,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut decoder = FrameDecoder::new();
        let mut buf = [0_u8; 8192];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => {
                    fail_all_pending(&pending, "rust-analyzer stdout closed").await;
                    break;
                }
                Ok(read) => {
                    decoder.push(&buf[..read]);
                    loop {
                        match decoder.next_message() {
                            Ok(Some(value)) => {
                                handle_incoming(&workspace, &tx, &pending, &diagnostics, value)
                                    .await;
                            }
                            Ok(None) => break,
                            Err(error) => {
                                warn!(%error, "failed to decode rust-analyzer message");
                                fail_all_pending(&pending, error.to_string()).await;
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    warn!(%error, "failed to read rust-analyzer stdout");
                    fail_all_pending(&pending, error.to_string()).await;
                    break;
                }
            }
        }
    })
}

fn spawn_stderr_logger(stderr: tokio::process::ChildStderr) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => debug!(target: "rust_analyzer", "{line}"),
                Ok(None) => break,
                Err(error) => {
                    debug!(%error, "failed to read rust-analyzer stderr");
                    break;
                }
            }
        }
    })
}

async fn handle_incoming(
    workspace: &Workspace,
    tx: &mpsc::Sender<Value>,
    pending: &PendingMap,
    diagnostics: &DiagnosticsCache,
    value: Value,
) {
    if value.get("id").is_some() && (value.get("result").is_some() || value.get("error").is_some())
    {
        handle_response(pending, value).await;
    } else if value.get("id").is_some() && value.get("method").is_some() {
        handle_server_request(workspace, tx, value).await;
    } else if value.get("method").is_some() {
        handle_notification(diagnostics, value).await;
    } else {
        debug!(message = %value, "ignoring unknown rust-analyzer message");
    }
}

async fn handle_response(pending: &PendingMap, value: Value) {
    let Some(id) = value.get("id").and_then(Value::as_u64) else {
        debug!(message = %value, "ignoring response with non-numeric id");
        return;
    };
    let Some(sender) = pending.lock().await.remove(&id) else {
        debug!(id, "response for unknown request id");
        return;
    };

    if let Some(error_value) = value.get("error") {
        let error = serde_json::from_value::<JsonRpcError>(error_value.clone())
            .map(|error| RaMcpError::Lsp(format!("{} ({})", error.message, error.code)))
            .unwrap_or_else(|_| RaMcpError::Lsp(error_value.to_string()));
        let _ = sender.send(Err(error));
    } else {
        let _ = sender.send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
    }
}

async fn handle_server_request(workspace: &Workspace, tx: &mpsc::Sender<Value>, value: Value) {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "window/workDoneProgress/create" => Value::Null,
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        "workspace/workspaceFolders" => workspace_folders_result(workspace),
        "workspace/configuration" => configuration_result(&value),
        _ => {
            debug!(method, "replying null to unhandled rust-analyzer request");
            Value::Null
        }
    };
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let _ = tx.send(response).await;
}

async fn handle_notification(diagnostics: &DiagnosticsCache, value: Value) {
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return;
    };
    match method {
        "textDocument/publishDiagnostics" => {
            if let Some(params) = value.get("params") {
                match serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) {
                    Ok(params) => diagnostics.update(params.uri, params.diagnostics).await,
                    Err(error) => debug!(%error, "failed to parse publishDiagnostics"),
                }
            }
        }
        "window/logMessage" | "$/progress" => {
            debug!(method, message = %value, "rust-analyzer notification")
        }
        _ => debug!(method, "unhandled rust-analyzer notification"),
    }
}

async fn fail_all_pending(pending: &PendingMap, message: impl Into<String>) {
    let message = message.into();
    let pending = std::mem::take(&mut *pending.lock().await);
    for (_, sender) in pending {
        let _ = sender.send(Err(RaMcpError::Lsp(message.clone())));
    }
}

fn position_params(uri: Uri, line: u32, character: u32) -> TextDocumentPositionParams {
    TextDocumentPositionParams::new(
        TextDocumentIdentifier::new(uri),
        Position::new(line, character),
    )
}

fn workspace_folders_result(workspace: &Workspace) -> Value {
    let Ok(uri) = url::Url::from_directory_path(workspace.root()) else {
        return Value::Null;
    };
    let name = workspace
        .root()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    json!([{"uri": uri.as_str(), "name": name}])
}

fn configuration_result(value: &Value) -> Value {
    let count = value
        .pointer("/params/items")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    Value::Array((0..count).map(|_| json!({})).collect())
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::configuration_result;
    use serde_json::json;

    #[test]
    fn configuration_response_matches_requested_item_count() {
        let result = configuration_result(&json!({
            "params": {
                "items": [{"section": "rust-analyzer"}, {"section": "cargo"}]
            }
        }));

        assert_eq!(result.as_array().unwrap().len(), 2);
    }
}
