use rust_analyzer_mcp::lsp::framing::{FrameDecoder, encode_message};
use serde_json::json;

#[test]
fn encode_content_length_frame_counts_utf8_bytes() {
    let value = json!({"message": "zażółć"});
    let frame = encode_message(&value).expect("frame encodes");
    let split = frame
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header terminator");
    let header = std::str::from_utf8(&frame[..split]).expect("utf8 header");
    let body = &frame[split + 4..];

    assert!(header.contains(&format!("Content-Length: {}", body.len())));
    assert_eq!(serde_json::from_slice::<serde_json::Value>(body).unwrap(), value);
}

#[test]
fn decoder_returns_one_complete_frame() {
    let value = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
    let frame = encode_message(&value).unwrap();
    let mut decoder = FrameDecoder::new();

    decoder.push(&frame);

    assert_eq!(decoder.next_message().unwrap().unwrap(), value);
    assert!(decoder.next_message().unwrap().is_none());
}

#[test]
fn decoder_returns_multiple_frames_from_one_buffer() {
    let first = json!({"id":1});
    let second = json!({"id":2});
    let mut bytes = encode_message(&first).unwrap();
    bytes.extend(encode_message(&second).unwrap());
    let mut decoder = FrameDecoder::new();

    decoder.push(&bytes);

    assert_eq!(decoder.next_message().unwrap().unwrap(), first);
    assert_eq!(decoder.next_message().unwrap().unwrap(), second);
    assert!(decoder.next_message().unwrap().is_none());
}

#[test]
fn decoder_waits_for_partial_body() {
    let value = json!({"id":1,"result":"slow"});
    let frame = encode_message(&value).unwrap();
    let split = frame.len() - 3;
    let mut decoder = FrameDecoder::new();

    decoder.push(&frame[..split]);
    assert!(decoder.next_message().unwrap().is_none());

    decoder.push(&frame[split..]);
    assert_eq!(decoder.next_message().unwrap().unwrap(), value);
}

#[test]
fn decoder_tolerates_extra_headers() {
    let value = json!({"id":1});
    let body = serde_json::to_vec(&value).unwrap();
    let frame = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut decoder = FrameDecoder::new();

    decoder.push(frame.as_bytes());
    decoder.push(&body);

    assert_eq!(decoder.next_message().unwrap().unwrap(), value);
}

