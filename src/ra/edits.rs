use lsp_types::{
    CodeActionOrCommand, DocumentChangeOperation, DocumentChanges, ResourceOp, WorkspaceEdit,
};
use serde::Serialize;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceEditSummary {
    pub document_count: usize,
    pub change_count: usize,
    pub resource_operation_count: usize,
}

pub(crate) fn summarize_workspace_edit(edit: &WorkspaceEdit) -> WorkspaceEditSummary {
    let mut document_count = 0_usize;
    let mut change_count = 0_usize;
    let mut resource_operation_count = 0_usize;

    if let Some(changes) = &edit.changes {
        document_count += changes.len();
        change_count += changes.values().map(Vec::len).sum::<usize>();
    }

    if let Some(document_changes) = &edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                document_count += edits.len();
                change_count += edits.iter().map(|edit| edit.edits.len()).sum::<usize>();
            }
            DocumentChanges::Operations(operations) => {
                for operation in operations {
                    match operation {
                        DocumentChangeOperation::Edit(edit) => {
                            document_count += 1;
                            change_count += edit.edits.len();
                        }
                        DocumentChangeOperation::Op(
                            ResourceOp::Create(_) | ResourceOp::Rename(_) | ResourceOp::Delete(_),
                        ) => {
                            resource_operation_count += 1;
                        }
                    }
                }
            }
        }
    }

    WorkspaceEditSummary {
        document_count,
        change_count,
        resource_operation_count,
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{
        DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
        Position, Range, RenameFile, ResourceOp, TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
    };

    use super::summarize_workspace_edit;

    fn edit() -> TextEdit {
        TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 6)),
            new_text: "renamed".to_string(),
        }
    }

    #[test]
    fn workspace_edit_summary_counts_plain_changes() {
        let uri: Uri = "file:///workspace/src/lib.rs".parse().unwrap();
        let edit = WorkspaceEdit {
            changes: Some([(uri, vec![edit(), edit()])].into_iter().collect()),
            document_changes: None,
            change_annotations: None,
        };

        let summary = summarize_workspace_edit(&edit);

        assert_eq!(summary.document_count, 1);
        assert_eq!(summary.change_count, 2);
        assert_eq!(summary.resource_operation_count, 0);
    }

    #[test]
    fn workspace_edit_summary_counts_document_change_edits() {
        let first_uri: Uri = "file:///workspace/src/lib.rs".parse().unwrap();
        let second_uri: Uri = "file:///workspace/src/main.rs".parse().unwrap();
        let edit = WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(vec![
                TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: first_uri,
                        version: None,
                    },
                    edits: vec![OneOf::Left(edit()), OneOf::Left(edit())],
                },
                TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: second_uri,
                        version: None,
                    },
                    edits: vec![OneOf::Left(edit())],
                },
            ])),
            change_annotations: None,
        };

        let summary = summarize_workspace_edit(&edit);

        assert_eq!(summary.document_count, 2);
        assert_eq!(summary.change_count, 3);
        assert_eq!(summary.resource_operation_count, 0);
    }

    #[test]
    fn workspace_edit_summary_counts_document_changes_and_resource_ops() {
        let uri: Uri = "file:///workspace/src/lib.rs".parse().unwrap();
        let rename_uri: Uri = "file:///workspace/src/old.rs".parse().unwrap();
        let new_uri: Uri = "file:///workspace/src/new.rs".parse().unwrap();
        let edit = WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits: vec![OneOf::Left(edit())],
                }),
                DocumentChangeOperation::Op(ResourceOp::Rename(RenameFile {
                    old_uri: rename_uri,
                    new_uri,
                    options: None,
                    annotation_id: None,
                })),
            ])),
            change_annotations: None,
        };

        let summary = summarize_workspace_edit(&edit);

        assert_eq!(summary.document_count, 1);
        assert_eq!(summary.change_count, 1);
        assert_eq!(summary.resource_operation_count, 1);
    }
}
