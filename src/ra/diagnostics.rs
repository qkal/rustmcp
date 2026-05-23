use lsp_types::{Diagnostic, DiagnosticSeverity};
use serde_json::{Value, json};

pub(crate) fn diagnostic_summary(diagnostics: &[Diagnostic]) -> Value {
    let mut errors = 0_usize;
    let mut warnings = 0_usize;
    let mut info = 0_usize;
    let mut hints = 0_usize;
    for diagnostic in diagnostics {
        match diagnostic.severity {
            Some(DiagnosticSeverity::ERROR) => errors += 1,
            Some(DiagnosticSeverity::WARNING) => warnings += 1,
            Some(DiagnosticSeverity::INFORMATION) => info += 1,
            Some(DiagnosticSeverity::HINT) => hints += 1,
            _ => info += 1,
        }
    }
    json!({
        "total": diagnostics.len(),
        "errors": errors,
        "warnings": warnings,
        "information": info,
        "hints": hints,
    })
}
