use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RaMcpError {
    #[error("workspace path does not exist: {0}")]
    WorkspaceMissing(PathBuf),
    #[error("workspace path is not a directory: {0}")]
    WorkspaceNotDirectory(PathBuf),
    #[error("file_path is outside workspace root")]
    OutsideWorkspace,
    #[error("file_path does not exist: {0}")]
    FileMissing(PathBuf),
    #[error("file_path is not a file: {0}")]
    NotAFile(PathBuf),
    #[error("invalid file URI: {0}")]
    InvalidFileUri(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("URL conversion failed")]
    UrlConversion,
    #[error("LSP framing error: {0}")]
    Framing(String),
    #[error("rust-analyzer was not found on PATH")]
    RustAnalyzerMissing,
    #[error("rust-analyzer process is not running")]
    AnalyzerNotRunning,
    #[error("LSP request failed: {0}")]
    Lsp(String),
    #[error("serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, RaMcpError>;

