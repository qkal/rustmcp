use std::path::PathBuf;

use crate::workspace::Workspace;

#[derive(Clone)]
pub struct RaMcpServer {
    #[allow(dead_code)]
    workspace: Workspace,
}

impl RaMcpServer {
    pub fn new(workspace: PathBuf) -> crate::error::Result<Self> {
        Ok(Self {
            workspace: Workspace::new(workspace)?,
        })
    }
}

#[rmcp::tool_router(server_handler)]
impl RaMcpServer {}
