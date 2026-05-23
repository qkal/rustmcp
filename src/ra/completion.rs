use lsp_types::CompletionResponse;
use serde_json::json;

pub(crate) fn completion_items(
    response: Option<lsp_types::CompletionResponse>,
) -> (Vec<serde_json::Value>, usize) {
    let items = match response {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };
    let total = items.len();
    let values = items
        .into_iter()
        .map(|item| {
            json!({
                "label": item.label,
                "kind": item.kind,
                "detail": item.detail,
                "documentation": item.documentation,
                "insert_text": item.insert_text,
                "text_edit": item.text_edit,
            })
        })
        .collect();
    (values, total)
}
