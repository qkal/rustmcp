use std::path::Path;

use lsp_types::{Diagnostic, Hover, Uri};

use crate::{error::Result, workspace::Workspace};

#[derive(Debug)]
pub struct RustAnalyzerClient {
    #[allow(dead_code)]
    workspace: Workspace,
}

impl RustAnalyzerClient {
    pub async fn spawn(workspace: Workspace) -> Result<Self> {
        let _ = which::which("rust-analyzer")
            .map_err(|_| crate::error::RaMcpError::RustAnalyzerMissing)?;
        Ok(Self { workspace })
    }

    pub async fn hover(&mut self, _file: &Path, _line: u32, _character: u32) -> Result<Option<Hover>> {
        Err(crate::error::RaMcpError::AnalyzerNotRunning)
    }

    pub async fn diagnostics_for(&self, _uri: &Uri) -> Vec<Diagnostic> {
        Vec::new()
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

