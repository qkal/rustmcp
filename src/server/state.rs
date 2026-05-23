use crate::{error::RaMcpError, lsp::client::RustAnalyzerClient, workspace::Workspace};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub cargo_tools_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            cargo_tools_enabled: true,
        }
    }
}

pub(crate) struct ServerState {
    pub(crate) workspace: Workspace,
    pub(crate) client: Option<RustAnalyzerClient>,
}

impl ServerState {
    pub(crate) async fn ensure_client(&mut self) -> crate::error::Result<&mut RustAnalyzerClient> {
        if self.client.is_none() {
            self.client = Some(RustAnalyzerClient::spawn(self.workspace.clone()).await?);
        }
        self.client.as_mut().ok_or(RaMcpError::AnalyzerNotRunning)
    }

    pub(crate) fn workspace_root(&self) -> String {
        self.workspace.root().display().to_string()
    }

    pub(crate) fn workspace_notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.workspace.warnings().missing_cargo_toml {
            notes.push("Workspace root does not contain Cargo.toml.".to_string());
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;

    #[test]
    fn cargo_tools_are_enabled_by_default() {
        let config = ServerConfig::default();
        assert!(config.cargo_tools_enabled);
    }
}
