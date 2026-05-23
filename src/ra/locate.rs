use lsp_types::Range;
use serde::Serialize;

use crate::{
    lsp::snippets::{SourceSnippet, read_snippet},
    ra::params::DEFAULT_MAX_SNIPPET_BYTES,
    workspace::{ClassifiedLocation, LocationKind, Workspace},
};

#[derive(Debug, Serialize)]
pub(crate) struct LocatedRange {
    uri: String,
    path: Option<String>,
    kind: LocationKind,
    range: Range,
    snippet: Option<SourceSnippet>,
    notes: Vec<String>,
}

pub(crate) fn locate(
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
            Some(path) => match read_snippet(path, range, context_lines, DEFAULT_MAX_SNIPPET_BYTES)
            {
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
