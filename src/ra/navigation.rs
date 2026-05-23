use lsp_types::{GotoDefinitionResponse, LocationLink};

pub(crate) fn definition_locations(
    response: Option<lsp_types::GotoDefinitionResponse>,
) -> Vec<(lsp_types::Uri, lsp_types::Range)> {
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

pub(crate) fn references_truncated(total: usize, max_results: usize) -> bool {
    total > max_results
}
