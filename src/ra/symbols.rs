pub(crate) fn document_symbols_result(
    symbols: Option<lsp_types::DocumentSymbolResponse>,
) -> serde_json::Value {
    serde_json::json!({ "symbols": symbols })
}

pub(crate) fn workspace_symbols_result(
    symbols: Option<lsp_types::WorkspaceSymbolResponse>,
    max_results: usize,
) -> (serde_json::Value, bool) {
    match symbols {
        Some(lsp_types::WorkspaceSymbolResponse::Flat(symbols)) => {
            let total = symbols.len();
            let truncated = total > max_results;
            let selected = symbols.into_iter().take(max_results).collect::<Vec<_>>();
            (
                serde_json::json!({
                    "format": "symbol_information",
                    "symbols": selected,
                    "total_returned_by_rust_analyzer": total,
                    "max_results": max_results,
                }),
                truncated,
            )
        }
        Some(lsp_types::WorkspaceSymbolResponse::Nested(symbols)) => {
            let total = symbols.len();
            let truncated = total > max_results;
            let selected = symbols.into_iter().take(max_results).collect::<Vec<_>>();
            (
                serde_json::json!({
                    "format": "workspace_symbol",
                    "symbols": selected,
                    "total_returned_by_rust_analyzer": total,
                    "max_results": max_results,
                }),
                truncated,
            )
        }
        None => (
            serde_json::json!({
                "format": "none",
                "symbols": [],
                "total_returned_by_rust_analyzer": 0,
                "max_results": max_results,
            }),
            false,
        ),
    }
}
