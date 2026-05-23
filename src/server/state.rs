use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use tokio::sync::{Mutex, MutexGuard};

use crate::{lsp::client::RustAnalyzerClient, workspace::Workspace};

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
    workspace: Workspace,
}

impl ServerState {
    pub(crate) fn new(workspace: Workspace) -> Self {
        Self { workspace }
    }

    pub(crate) fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub(crate) fn workspace_snapshot(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            workspace: self.workspace.clone(),
            root: self.workspace_root(),
            notes: self.workspace_notes(),
        }
    }

    pub(crate) fn set_workspace(&mut self, workspace: Workspace) {
        self.workspace = workspace;
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

pub(crate) struct WorkspaceSnapshot {
    pub(crate) workspace: Workspace,
    pub(crate) root: String,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Default)]
pub(crate) struct ClientHandle {
    inner: Arc<Mutex<Option<RustAnalyzerClient>>>,
}

impl ClientHandle {
    pub(crate) async fn ensure_client(
        &self,
        workspace: Workspace,
    ) -> crate::error::Result<ClientGuard<'_>> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            *guard = Some(RustAnalyzerClient::spawn(workspace).await?);
        }
        Ok(ClientGuard { guard })
    }

    pub(crate) async fn restart(&self, workspace: Workspace) -> crate::error::Result<()> {
        let mut guard = self.inner.lock().await;
        if let Some(mut client) = guard.take() {
            let _ = client.shutdown().await;
        }
        *guard = Some(RustAnalyzerClient::spawn(workspace).await?);
        Ok(())
    }
}

pub(crate) struct ClientGuard<'a> {
    guard: MutexGuard<'a, Option<RustAnalyzerClient>>,
}

impl Deref for ClientGuard<'_> {
    type Target = RustAnalyzerClient;

    fn deref(&self) -> &Self::Target {
        self.guard
            .as_ref()
            .expect("client handle is initialized before use")
    }
}

impl DerefMut for ClientGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard
            .as_mut()
            .expect("client handle is initialized before use")
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
