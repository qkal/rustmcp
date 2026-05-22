use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::Serialize;
use url::Url;

use crate::error::{RaMcpError, Result};

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    warnings: WorkspaceWarnings,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkspaceWarnings {
    pub missing_cargo_toml: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    Workspace,
    ExternalDependencySource,
    NonFileUri,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedLocation {
    pub uri: String,
    pub kind: LocationKind,
    pub path: Option<PathBuf>,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let input = root.as_ref();
        if !input.exists() {
            return Err(RaMcpError::WorkspaceMissing(input.to_path_buf()));
        }
        if !input.is_dir() {
            return Err(RaMcpError::WorkspaceNotDirectory(input.to_path_buf()));
        }

        let root = input.canonicalize()?;
        let warnings = WorkspaceWarnings {
            missing_cargo_toml: !root.join("Cargo.toml").is_file(),
        };

        Ok(Self { root, warnings })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn warnings(&self) -> &WorkspaceWarnings {
        &self.warnings
    }

    pub fn resolve_existing_file(&self, file_path: impl AsRef<Path>) -> Result<PathBuf> {
        let file_path = file_path.as_ref();
        let candidate = if file_path.is_absolute() {
            file_path.to_path_buf()
        } else {
            self.root.join(file_path)
        };

        if !candidate.exists() {
            return Err(RaMcpError::FileMissing(candidate));
        }

        let canonical = candidate.canonicalize()?;
        if !canonical.starts_with(&self.root) {
            return Err(RaMcpError::OutsideWorkspace);
        }
        if !canonical.is_file() {
            return Err(RaMcpError::NotAFile(canonical));
        }

        Ok(canonical)
    }

    pub fn uri_for_file(&self, file: impl AsRef<Path>) -> Result<Url> {
        let file = file.as_ref();
        let canonical = if file.is_absolute() {
            file.canonicalize()?
        } else {
            self.resolve_existing_file(file)?
        };

        if !canonical.starts_with(&self.root) {
            return Err(RaMcpError::OutsideWorkspace);
        }

        Url::from_file_path(canonical).map_err(|_| RaMcpError::UrlConversion)
    }

    pub fn classify_url(&self, uri: &Url) -> Result<ClassifiedLocation> {
        if uri.scheme() != "file" {
            return Ok(ClassifiedLocation {
                uri: uri.to_string(),
                kind: LocationKind::NonFileUri,
                path: None,
            });
        }

        let path = uri
            .to_file_path()
            .map_err(|_| RaMcpError::InvalidFileUri(uri.to_string()))?;
        let canonical = if path.exists() {
            Some(path.canonicalize()?)
        } else {
            None
        };
        let kind = match &canonical {
            Some(path) if path.starts_with(&self.root) => LocationKind::Workspace,
            _ => LocationKind::ExternalDependencySource,
        };

        Ok(ClassifiedLocation {
            uri: uri.to_string(),
            kind,
            path: canonical,
        })
    }

    pub fn classify_lsp_uri(&self, uri: &lsp_types::Uri) -> Result<ClassifiedLocation> {
        let parsed = Url::parse(uri.as_str())
            .map_err(|_| RaMcpError::InvalidFileUri(uri.as_str().to_string()))?;
        self.classify_url(&parsed)
    }
}

pub fn is_rust_file(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("rs"))
}

