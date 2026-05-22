use std::{process::Stdio, time::Duration};

use rust_analyzer_mcp::lsp::client::RustAnalyzerClient;
use rust_analyzer_mcp::lsp::framing::{FrameDecoder, encode_message};
use rust_analyzer_mcp::workspace::Workspace;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn diagnostics_cache_updates_by_uri() {
    let cache = rust_analyzer_mcp::lsp::protocol::DiagnosticsCache::default();
    let uri = "file:///tmp/example.rs".parse().unwrap();
    let diagnostic = lsp_types::Diagnostic {
        range: lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(0, 3),
        ),
        severity: Some(lsp_types::DiagnosticSeverity::ERROR),
        message: "boom".to_string(),
        ..Default::default()
    };

    cache.update(uri.clone(), vec![diagnostic.clone()]).await;

    assert_eq!(cache.get(&uri).await, vec![diagnostic]);
}

#[tokio::test]
async fn rust_analyzer_smoke_hover_when_available() {
    if which::which("rust-analyzer").is_err() {
        eprintln!("skipping: rust-analyzer not found on PATH");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"ra_mcp_smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn answer() -> i32 { 42 }\n",
    )
    .unwrap();

    let workspace = Workspace::new(temp.path()).unwrap();
    let mut client = RustAnalyzerClient::spawn(workspace.clone()).await.unwrap();
    let file = workspace.resolve_existing_file("src/lib.rs").unwrap();
    let hover = client.hover(&file, 0, 7).await.unwrap();
    client.shutdown().await.unwrap();

    assert!(hover.is_some());
}

#[tokio::test]
async fn mcp_tools_list_smoke_has_mvp_tools_and_protocol_stdout() {
    let exe = env!("CARGO_BIN_EXE_rust-analyzer-mcp");
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"ra_mcp_mcp_smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let mut child = tokio::process::Command::new(exe)
        .arg("--workspace")
        .arg(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut decoder = FrameDecoder::new();

    write_mcp(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0.0.0"}
            }
        }),
    )
    .await;
    let init = read_mcp(&mut stdout, &mut decoder).await;
    assert_eq!(init["id"], 1);

    write_mcp(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await;
    write_mcp(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await;
    let tools = read_mcp(&mut stdout, &mut decoder).await;
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();

    for expected in [
        "ra_set_workspace",
        "ra_hover",
        "ra_definition",
        "ra_references",
        "ra_document_symbols",
        "ra_completion",
        "ra_format",
        "ra_code_actions",
        "ra_diagnostics",
        "ra_workspace_diagnostics",
    ] {
        assert!(names.contains(&expected), "missing {expected}; got {names:?}");
    }

    child.kill().await.unwrap();
}

async fn write_mcp(stdin: &mut tokio::process::ChildStdin, value: Value) {
    let frame = encode_message(&value).unwrap();
    stdin.write_all(&frame).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn read_mcp(
    stdout: &mut tokio::process::ChildStdout,
    decoder: &mut FrameDecoder,
) -> Value {
    let deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    let mut buf = [0_u8; 4096];
    loop {
        if let Some(value) = decoder.next_message().unwrap() {
            return value;
        }
        tokio::select! {
            read = stdout.read(&mut buf) => {
                let read = read.unwrap();
                assert_ne!(read, 0, "server stdout closed before response");
                decoder.push(&buf[..read]);
            }
            _ = &mut deadline => panic!("timed out waiting for MCP response"),
        }
    }
}

