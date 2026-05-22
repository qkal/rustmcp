use serde_json::Value;

use crate::error::{RaMcpError, Result};

const HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";

#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub fn next_message(&mut self) -> Result<Option<Value>> {
        let Some(header_end) = find_subslice(&self.buffer, HEADER_TERMINATOR) else {
            return Ok(None);
        };
        let header_bytes = &self.buffer[..header_end];
        let header = std::str::from_utf8(header_bytes)
            .map_err(|error| RaMcpError::Framing(error.to_string()))?;
        let content_length = parse_content_length(header)?;
        let body_start = header_end + HEADER_TERMINATOR.len();
        let frame_end = body_start + content_length;

        if self.buffer.len() < frame_end {
            return Ok(None);
        }

        let body = self.buffer[body_start..frame_end].to_vec();
        self.buffer.drain(..frame_end);
        let value = serde_json::from_slice(&body)?;
        Ok(Some(value))
    }
}

pub fn encode_message(value: &Value) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend(body);
    Ok(frame)
}

fn parse_content_length(header: &str) -> Result<usize> {
    for line in header.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse::<usize>()
                .map_err(|error| RaMcpError::Framing(error.to_string()));
        }
    }
    Err(RaMcpError::Framing(
        "missing Content-Length header".to_string(),
    ))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
