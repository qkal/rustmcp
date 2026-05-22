use std::{fs, path::Path};

use lsp_types::Range;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Clone, Serialize)]
pub struct SourceSnippet {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub truncated: bool,
}

pub fn read_snippet(
    path: &Path,
    range: Range,
    context_lines: u32,
    max_bytes: usize,
) -> Result<Option<SourceSnippet>> {
    if !path.is_file() {
        return Ok(None);
    }

    let text = fs::read_to_string(path)?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Ok(Some(SourceSnippet {
            start_line: 0,
            end_line: 0,
            text: String::new(),
            truncated: false,
        }));
    }

    let start = range.start.line.saturating_sub(context_lines) as usize;
    let end = (range.end.line.saturating_add(context_lines) as usize).min(lines.len() - 1);
    let mut out = String::new();
    let mut truncated = false;

    for (offset, line) in lines[start..=end].iter().enumerate() {
        let line_no = start + offset;
        let next = format!("{:>5} | {}\n", line_no + 1, line);
        if out.len() + next.len() > max_bytes {
            truncated = true;
            break;
        }
        out.push_str(&next);
    }

    Ok(Some(SourceSnippet {
        start_line: start as u32,
        end_line: end as u32,
        text: out,
        truncated,
    }))
}

