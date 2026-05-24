use std::{collections::BTreeSet, fs, path::Path};

use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};
use serde::Serialize;
use serde_json::{Value, json};

use crate::ra::params::{DEFAULT_MAX_INLAY_HINTS, MAX_INLAY_HINTS};

type ValidationResult<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InlayHintKindFilter {
    Type,
    Parameter,
    Other,
}

pub(crate) fn max_hints_value(value: Option<u32>) -> ValidationResult<usize> {
    let value = value.unwrap_or(DEFAULT_MAX_INLAY_HINTS);
    if !(1..=MAX_INLAY_HINTS).contains(&value) {
        return Err(format!(
            "max_hints must be between 1 and {}",
            MAX_INLAY_HINTS
        ));
    }
    Ok(value as usize)
}

pub(crate) fn request_range(
    source_lines: &[String],
    start_line: Option<u32>,
    end_line: Option<u32>,
) -> ValidationResult<(Range, Value)> {
    match (start_line, end_line) {
        (None, None) => {
            let end_line = source_lines.len() as u32;
            let display_end = end_line.saturating_sub(1);
            Ok((
                Range::new(Position::new(0, 0), Position::new(end_line, 0)),
                json!({"start_line": 0, "end_line": display_end}),
            ))
        }
        (Some(start), Some(end)) => {
            if source_lines.is_empty() {
                return Err("selected ranges require a non-empty file".to_string());
            }
            if start > end {
                return Err("start_line must be less than or equal to end_line".to_string());
            }
            let line_count = source_lines.len() as u32;
            if start >= line_count || end >= line_count {
                return Err(
                    "start_line and end_line must be valid zero-based line numbers".to_string(),
                );
            }
            Ok((
                Range::new(Position::new(start, 0), Position::new(end + 1, 0)),
                json!({"start_line": start, "end_line": end}),
            ))
        }
        _ => Err("start_line and end_line must be supplied together".to_string()),
    }
}

pub(crate) fn parse_kind_filters(
    kinds: Option<&[String]>,
) -> ValidationResult<Option<BTreeSet<InlayHintKindFilter>>> {
    let Some(kinds) = kinds else {
        return Ok(None);
    };

    let mut filters = BTreeSet::new();
    for kind in kinds {
        let parsed = match kind.as_str() {
            "type" => InlayHintKindFilter::Type,
            "parameter" => InlayHintKindFilter::Parameter,
            "other" => InlayHintKindFilter::Other,
            unknown => {
                return Err(format!(
                    "unknown inlay hint kind '{unknown}'; expected type, parameter, or other"
                ));
            }
        };
        filters.insert(parsed);
    }
    Ok(Some(filters))
}

fn kind_filter_for(kind: Option<InlayHintKind>) -> InlayHintKindFilter {
    match kind {
        Some(kind) if kind == InlayHintKind::TYPE => InlayHintKindFilter::Type,
        Some(kind) if kind == InlayHintKind::PARAMETER => InlayHintKindFilter::Parameter,
        _ => InlayHintKindFilter::Other,
    }
}

#[derive(Debug)]
pub(crate) struct FormattedInlayHints {
    pub result: Value,
    pub notes: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
struct InlayHintSummary {
    character: u32,
    label: String,
    kind: &'static str,
    padding_left: bool,
    padding_right: bool,
}

#[derive(Debug, Serialize)]
struct InlayHintGroup {
    line: u32,
    text: String,
    hints: Vec<InlayHintSummary>,
}

pub(crate) fn read_source_lines(path: &Path) -> crate::error::Result<Vec<String>> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().map(str::to_string).collect())
}

pub(crate) fn format_inlay_hints(
    file_path: &str,
    range: Value,
    source_lines: &[String],
    hints: Vec<InlayHint>,
    filters: Option<&BTreeSet<InlayHintKindFilter>>,
    max_hints: usize,
    include_raw: bool,
) -> ValidationResult<FormattedInlayHints> {
    let mut filtered = hints
        .into_iter()
        .filter(|hint| {
            filters
                .map(|filters| filters.contains(&kind_filter_for(hint.kind)))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    filtered.sort_by_key(|hint| (hint.position.line, hint.position.character));

    let total = filtered.len();
    let truncated = total > max_hints;
    let selected = filtered.into_iter().take(max_hints).collect::<Vec<_>>();

    let mut groups = Vec::<InlayHintGroup>::new();
    for hint in &selected {
        let summary = InlayHintSummary {
            character: hint.position.character,
            label: label_text(&hint.label),
            kind: kind_name(hint.kind),
            padding_left: hint.padding_left.unwrap_or(false),
            padding_right: hint.padding_right.unwrap_or(false),
        };

        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.line == hint.position.line)
        {
            group.hints.push(summary);
        } else {
            groups.push(InlayHintGroup {
                line: hint.position.line,
                text: source_lines
                    .get(hint.position.line as usize)
                    .cloned()
                    .unwrap_or_default(),
                hints: vec![summary],
            });
        }
    }

    let mut notes = Vec::new();
    if total == 0 {
        notes.push(
            "No inlay hints returned. rust-analyzer may still be indexing, or the selected range has no hints."
                .to_string(),
        );
    }
    if truncated {
        notes.push(format!(
            "Returned {max_hints} of {total} inlay hints; use a narrower range or higher max_hints up to {MAX_INLAY_HINTS}."
        ));
    }

    let mut result = json!({
        "file_path": file_path,
        "range": range,
        "total": total,
        "returned": selected.len(),
        "groups": groups,
    });
    if include_raw {
        let raw_hints = serde_json::to_value(&selected).map_err(|error| error.to_string())?;
        result
            .as_object_mut()
            .expect("inlay hints result is a JSON object")
            .insert("raw_hints".to_string(), raw_hints);
    }

    Ok(FormattedInlayHints {
        result,
        notes,
        truncated,
    })
}

fn label_text(label: &InlayHintLabel) -> String {
    match label {
        InlayHintLabel::String(value) => value.clone(),
        InlayHintLabel::LabelParts(parts) => {
            let mut label = String::new();
            for part in parts {
                label.push_str(&part.value);
            }
            label
        }
    }
}

fn kind_name(kind: Option<InlayHintKind>) -> &'static str {
    match kind_filter_for(kind) {
        InlayHintKindFilter::Type => "type",
        InlayHintKindFilter::Parameter => "parameter",
        InlayHintKindFilter::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_hints_uses_default_and_rejects_out_of_range_values() {
        assert_eq!(max_hints_value(None).unwrap(), 200);
        assert_eq!(max_hints_value(Some(1)).unwrap(), 1);
        assert_eq!(max_hints_value(Some(1_000)).unwrap(), 1_000);
        assert_eq!(
            max_hints_value(Some(0)).unwrap_err(),
            "max_hints must be between 1 and 1000"
        );
        assert_eq!(
            max_hints_value(Some(1_001)).unwrap_err(),
            "max_hints must be between 1 and 1000"
        );
    }

    #[test]
    fn selected_range_requires_both_endpoints() {
        let lines = vec!["fn main() {}".to_string()];

        assert_eq!(
            request_range(&lines, Some(0), None).unwrap_err(),
            "start_line and end_line must be supplied together"
        );
        assert_eq!(
            request_range(&lines, None, Some(0)).unwrap_err(),
            "start_line and end_line must be supplied together"
        );
    }

    #[test]
    fn request_range_maps_whole_file_and_inclusive_selected_range() {
        let lines = vec![
            "fn main() {".to_string(),
            "    let value = 42;".to_string(),
            "}".to_string(),
        ];

        let (whole, whole_json) = request_range(&lines, None, None).unwrap();
        assert_eq!(whole.start, Position::new(0, 0));
        assert_eq!(whole.end, Position::new(3, 0));
        assert_eq!(whole_json, json!({"start_line": 0, "end_line": 2}));

        let (selected, selected_json) = request_range(&lines, Some(1), Some(2)).unwrap();
        assert_eq!(selected.start, Position::new(1, 0));
        assert_eq!(selected.end, Position::new(3, 0));
        assert_eq!(selected_json, json!({"start_line": 1, "end_line": 2}));
    }

    #[test]
    fn request_range_rejects_reversed_and_out_of_bounds_ranges() {
        let lines = vec!["fn main() {}".to_string()];

        assert_eq!(
            request_range(&lines, Some(1), Some(0)).unwrap_err(),
            "start_line must be less than or equal to end_line"
        );
        assert_eq!(
            request_range(&lines, Some(0), Some(1)).unwrap_err(),
            "start_line and end_line must be valid zero-based line numbers"
        );
        assert_eq!(
            request_range(&[], Some(0), Some(0)).unwrap_err(),
            "selected ranges require a non-empty file"
        );
    }

    #[test]
    fn kind_filters_accept_known_values_and_reject_unknown_values() {
        let filters = parse_kind_filters(Some(&[
            "type".to_string(),
            "parameter".to_string(),
            "other".to_string(),
        ]))
        .unwrap()
        .unwrap();

        assert!(filters.contains(&InlayHintKindFilter::Type));
        assert!(filters.contains(&InlayHintKindFilter::Parameter));
        assert!(filters.contains(&InlayHintKindFilter::Other));
        assert!(parse_kind_filters(None).unwrap().is_none());
        assert_eq!(
            parse_kind_filters(Some(&["bogus".to_string()])).unwrap_err(),
            "unknown inlay hint kind 'bogus'; expected type, parameter, or other"
        );
    }

    fn hint(
        line: u32,
        character: u32,
        label: InlayHintLabel,
        kind: Option<InlayHintKind>,
    ) -> InlayHint {
        InlayHint {
            position: Position::new(line, character),
            label,
            kind,
            text_edits: None,
            tooltip: None,
            padding_left: Some(false),
            padding_right: Some(true),
            data: None,
        }
    }

    #[test]
    fn labels_are_rendered_from_strings_and_label_parts() {
        let string_label = label_text(&InlayHintLabel::String(": i32".to_string()));
        let parts_label = label_text(&InlayHintLabel::LabelParts(vec![
            lsp_types::InlayHintLabelPart {
                value: ": ".to_string(),
                ..Default::default()
            },
            lsp_types::InlayHintLabelPart {
                value: "Client".to_string(),
                ..Default::default()
            },
        ]));

        assert_eq!(string_label, ": i32");
        assert_eq!(parts_label, ": Client");
    }

    #[test]
    fn result_groups_hints_by_line_and_sorts_by_position() {
        let source_lines = vec![
            "fn main() {".to_string(),
            "    let value = 42;".to_string(),
            "}".to_string(),
        ];
        let formatted = format_inlay_hints(
            "src/lib.rs",
            json!({"start_line": 0, "end_line": 2}),
            &source_lines,
            vec![
                hint(
                    1,
                    12,
                    InlayHintLabel::String(": i32".to_string()),
                    Some(InlayHintKind::TYPE),
                ),
                hint(
                    1,
                    8,
                    InlayHintLabel::String("value: ".to_string()),
                    Some(InlayHintKind::PARAMETER),
                ),
            ],
            None,
            10,
            false,
        )
        .unwrap();

        assert!(!formatted.truncated);
        assert!(formatted.notes.is_empty());
        assert_eq!(formatted.result["file_path"], "src/lib.rs");
        assert_eq!(formatted.result["total"], 2);
        assert_eq!(formatted.result["returned"], 2);
        assert_eq!(formatted.result["groups"][0]["line"], 1);
        assert_eq!(formatted.result["groups"][0]["text"], "    let value = 42;");
        assert_eq!(formatted.result["groups"][0]["hints"][0]["character"], 8);
        assert_eq!(
            formatted.result["groups"][0]["hints"][0]["kind"],
            "parameter"
        );
        assert_eq!(formatted.result["groups"][0]["hints"][1]["character"], 12);
        assert_eq!(formatted.result["groups"][0]["hints"][1]["kind"], "type");
    }

    #[test]
    fn kind_filters_and_truncation_are_applied_before_grouping() {
        let source_lines = vec!["let value = 42;".to_string()];
        let filters = parse_kind_filters(Some(&["type".to_string()])).unwrap();
        let formatted = format_inlay_hints(
            "src/lib.rs",
            json!({"start_line": 0, "end_line": 0}),
            &source_lines,
            vec![
                hint(
                    0,
                    4,
                    InlayHintLabel::String("value: ".to_string()),
                    Some(InlayHintKind::PARAMETER),
                ),
                hint(
                    0,
                    9,
                    InlayHintLabel::String(": i32".to_string()),
                    Some(InlayHintKind::TYPE),
                ),
                hint(
                    0,
                    12,
                    InlayHintLabel::String(": u32".to_string()),
                    Some(InlayHintKind::TYPE),
                ),
            ],
            filters.as_ref(),
            1,
            false,
        )
        .unwrap();

        assert!(formatted.truncated);
        assert_eq!(formatted.result["total"], 2);
        assert_eq!(formatted.result["returned"], 1);
        assert_eq!(formatted.result["groups"][0]["hints"][0]["label"], ": i32");
        assert!(
            formatted
                .notes
                .iter()
                .any(|note| note.contains("narrower range or higher max_hints"))
        );
    }

    #[test]
    fn raw_hints_are_included_only_when_requested() {
        let source_lines = vec!["let value = 42;".to_string()];
        let hints = vec![hint(
            0,
            9,
            InlayHintLabel::String(": i32".to_string()),
            Some(InlayHintKind::TYPE),
        )];

        let without_raw = format_inlay_hints(
            "src/lib.rs",
            json!({"start_line": 0, "end_line": 0}),
            &source_lines,
            hints.clone(),
            None,
            10,
            false,
        )
        .unwrap();
        let with_raw = format_inlay_hints(
            "src/lib.rs",
            json!({"start_line": 0, "end_line": 0}),
            &source_lines,
            hints,
            None,
            10,
            true,
        )
        .unwrap();

        assert!(without_raw.result.get("raw_hints").is_none());
        assert!(with_raw.result["raw_hints"].is_array());
    }
}
