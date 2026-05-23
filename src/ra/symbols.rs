pub(crate) fn document_symbols_result(
    symbols: Option<lsp_types::DocumentSymbolResponse>,
) -> serde_json::Value {
    serde_json::json!({ "symbols": symbols })
}
