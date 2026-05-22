use std::{process::Stdio, time::Duration};

use rust_analyzer_mcp::lsp::client::RustAnalyzerClient;
use rust_analyzer_mcp::workspace::Workspace;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn diagnostics_cache_updates_by_uri() {
    let cache = rust_analyzer_mcp::lsp::protocol::DiagnosticsCache::default();
    let uri: lsp_types::Uri = "file:///tmp/example.rs".parse().unwrap();
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
    let mut hover = None;
    for _ in 0..20 {
        match client.hover(&file, 0, 7).await {
            Ok(value) => {
                hover = value;
                if hover.is_some() {
                    break;
                }
            }
            Err(error) if error.to_string().contains("content modified") => {}
            Err(error) => panic!("hover failed: {error}"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let mut symbols = None;
    for _ in 0..20 {
        match client.document_symbols(&file).await {
            Ok(value) => {
                symbols = value;
                if symbols.is_some() {
                    break;
                }
            }
            Err(error) if error.to_string().contains("content modified") => {}
            Err(error) => panic!("document symbols failed: {error}"),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    client.shutdown().await.unwrap();

    assert!(
        hover.is_some() || symbols.is_some(),
        "expected hover or document symbols from rust-analyzer"
    );
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

    initialize_mcp(&mut stdin, &mut stdout).await;
    write_mcp_line(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await;
    let tools = read_mcp_line(&mut stdout).await;
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
        "cargo_check",
        "cargo_test",
        "cargo_clippy",
        "cargo_fmt_check",
        "cargo_metadata",
    ] {
        assert!(
            names.contains(&expected),
            "missing {expected}; got {names:?}"
        );
    }

    child.kill().await.unwrap();
}

#[tokio::test]
async fn disabled_cargo_tools_return_structured_failure() {
    let exe = env!("CARGO_BIN_EXE_rust-analyzer-mcp");
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"disabled_cargo_smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let mut child = tokio::process::Command::new(exe)
        .arg("--workspace")
        .arg(temp.path())
        .arg("--disable-cargo-tools")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    initialize_mcp(&mut stdin, &mut stdout).await;

    write_mcp_line(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "cargo_check",
                "arguments": {}
            }
        }),
    )
    .await;
    let response = read_mcp_line(&mut stdout).await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();

    assert_eq!(payload["ok"], false);
    assert!(payload["error"].as_str().unwrap().contains("disabled"));
    assert!(
        payload["hint"]
            .as_str()
            .unwrap()
            .contains("--disable-cargo-tools")
    );

    child.kill().await.unwrap();
}

#[tokio::test]
async fn cargo_metadata_response_omits_duplicate_raw_stdout_when_parsed() {
    let exe = env!("CARGO_BIN_EXE_rust-analyzer-mcp");
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"metadata_payload_smoke\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "pub fn answer() -> i32 { 42 }\n",
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
    initialize_mcp(&mut stdin, &mut stdout).await;

    write_mcp_line(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "cargo_metadata",
                "arguments": { "no_deps": true }
            }
        }),
    )
    .await;
    let response = read_mcp_line(&mut stdout).await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: Value = serde_json::from_str(text).unwrap();

    assert_eq!(payload["ok"], true);
    assert_eq!(payload["result"]["stdout"], "");
    assert!(payload["result"]["notes"].as_array().unwrap().is_empty());
    assert_eq!(
        payload["result"]["metadata_json"]["packages"][0]["name"],
        "metadata_payload_smoke"
    );
    assert!(
        payload["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("stdout omitted")),
        "notes were: {:?}",
        payload["notes"]
    );

    child.kill().await.unwrap();
}

#[test]
fn help_mentions_disable_cargo_tools() {
    let exe = env!("CARGO_BIN_EXE_rust-analyzer-mcp");
    let output = std::process::Command::new(exe)
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--disable-cargo-tools"));
}

async fn initialize_mcp(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut tokio::process::ChildStdout,
) {
    write_mcp_line(
        stdin,
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
    let init = read_mcp_line(stdout).await;
    assert_eq!(init["id"], 1);

    write_mcp_line(
        stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await;
}

async fn write_mcp_line(stdin: &mut tokio::process::ChildStdin, value: Value) {
    let mut line = serde_json::to_vec(&value).unwrap();
    line.push(b'\n');
    stdin.write_all(&line).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn read_mcp_line(stdout: &mut tokio::process::ChildStdout) -> Value {
    let deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    let mut line = Vec::new();
    loop {
        tokio::select! {
            read = stdout.read_buf(&mut line) => {
                let read = read.unwrap();
                assert_ne!(read, 0, "server stdout closed before response");
                if let Some(pos) = line.iter().position(|byte| *byte == b'\n') {
                    let frame = line.drain(..=pos).collect::<Vec<_>>();
                    return serde_json::from_slice(&frame).unwrap();
                }
            }
            _ = &mut deadline => panic!("timed out waiting for MCP response"),
        }
    }
}
