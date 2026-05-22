use std::{collections::HashMap, sync::Arc};

use lsp_types::{Diagnostic, Uri};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum IncomingMessage {
    Response {
        jsonrpc: String,
        id: RequestId,
        result: Option<Value>,
        error: Option<JsonRpcError>,
    },
    Request {
        jsonrpc: String,
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
    Notification {
        jsonrpc: String,
        method: String,
        params: Option<Value>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    String(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Default, Clone)]
pub struct DiagnosticsCache {
    inner: Arc<RwLock<HashMap<String, Vec<Diagnostic>>>>,
}

impl DiagnosticsCache {
    pub async fn update(&self, uri: Uri, diagnostics: Vec<Diagnostic>) {
        self.inner
            .write()
            .await
            .insert(uri.as_str().to_string(), diagnostics);
    }

    pub async fn get(&self, uri: &Uri) -> Vec<Diagnostic> {
        self.inner
            .read()
            .await
            .get(uri.as_str())
            .cloned()
            .unwrap_or_default()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    pub async fn all(&self) -> Vec<(String, Vec<Diagnostic>)> {
        self.inner
            .read()
            .await
            .iter()
            .map(|(uri, diagnostics)| (uri.clone(), diagnostics.clone()))
            .collect()
    }
}

pub fn lsp_uri_from_url(url: &url::Url) -> crate::error::Result<Uri> {
    url.as_str()
        .parse::<Uri>()
        .map_err(|_| crate::error::RaMcpError::InvalidFileUri(url.to_string()))
}

pub fn url_from_lsp_uri(uri: &Uri) -> crate::error::Result<url::Url> {
    url::Url::parse(uri.as_str())
        .map_err(|_| crate::error::RaMcpError::InvalidFileUri(uri.as_str().to_string()))
}
