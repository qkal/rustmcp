use lsp_types::CodeActionOrCommand;
use serde_json::{Value, json};

pub(crate) fn summarize_code_actions(actions: Vec<CodeActionOrCommand>) -> Vec<Value> {
    actions
        .into_iter()
        .map(|action| match action {
            CodeActionOrCommand::Command(command) => json!({
                "title": command.title,
                "kind": "command",
                "command": command.command,
                "arguments": command.arguments,
            }),
            CodeActionOrCommand::CodeAction(action) => json!({
                "title": action.title,
                "kind": action.kind,
                "diagnostics": action.diagnostics,
                "edit": action.edit,
                "command": action.command,
            }),
        })
        .collect()
}
