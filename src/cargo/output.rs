use serde::Serialize;

use crate::cargo::args::CargoInvocation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TruncatedText {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CargoStatus {
    pub code: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CargoRunOutput {
    pub command: String,
    pub args: Vec<String>,
    pub status: CargoStatus,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub notes: Vec<String>,
    pub metadata_json: Option<serde_json::Value>,
}

pub fn truncate_text(bytes: &[u8], max_bytes: usize) -> TruncatedText {
    if bytes.len() <= max_bytes {
        return TruncatedText {
            text: String::from_utf8_lossy(bytes).into_owned(),
            truncated: false,
        };
    }

    let mut end = max_bytes.min(bytes.len());
    while end > 0 && end < bytes.len() && is_utf8_continuation(bytes[end]) {
        end -= 1;
    }

    TruncatedText {
        text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
        truncated: true,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

pub(crate) fn metadata_json(
    invocation: &CargoInvocation,
    status: &CargoStatus,
    stdout: &TruncatedText,
    notes: &mut Vec<String>,
) -> Option<serde_json::Value> {
    if !invocation.parse_metadata_json || !status.success || stdout.truncated {
        return None;
    }

    match serde_json::from_str(&stdout.text) {
        Ok(value) => Some(value),
        Err(error) => {
            notes.push(format!("failed to parse cargo metadata JSON: {error}"));
            None
        }
    }
}
