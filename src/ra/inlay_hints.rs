use std::collections::BTreeSet;

use lsp_types::{InlayHintKind, Position, Range};
use serde_json::{Value, json};

use crate::ra::params::{DEFAULT_MAX_INLAY_HINTS, MAX_INLAY_HINTS};

// Wired by the upcoming ra_inlay_hints server and formatting tasks.
#[allow(dead_code)]
type ValidationResult<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InlayHintKindFilter {
    Type,
    Parameter,
    Other,
}

// Wired by the upcoming ra_inlay_hints server task.
#[allow(dead_code)]
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

// Wired by the upcoming ra_inlay_hints server task.
#[allow(dead_code)]
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

// Wired by the upcoming ra_inlay_hints server task.
#[allow(dead_code)]
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

// Wired by the upcoming ra_inlay_hints formatting task.
#[allow(dead_code)]
fn kind_filter_for(kind: Option<InlayHintKind>) -> InlayHintKindFilter {
    match kind {
        Some(kind) if kind == InlayHintKind::TYPE => InlayHintKindFilter::Type,
        Some(kind) if kind == InlayHintKind::PARAMETER => InlayHintKindFilter::Parameter,
        _ => InlayHintKindFilter::Other,
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
}
