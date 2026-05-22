# rust-analyzer-mcp

`rust-analyzer-mcp` is a local stdio MCP server that gives coding agents Rust IDE intelligence through rust-analyzer.

It exposes readonly MCP tools for hover, definitions, references, document symbols, completions, formatting edits, code actions, diagnostics, workspace diagnostics, and workspace switching. Formatting and code action tools return previews only; they do not mutate files.

## Prerequisites

- Rust toolchain
- rust-analyzer installed and available on `PATH`

```sh
rustup component add rust-analyzer
```

- A Rust project or workspace with `Cargo.toml`

## Build

```sh
cargo build --release
```

## Run

```sh
./target/release/rust-analyzer-mcp --workspace /path/to/project
```

If `--workspace` is omitted, the server uses the current working directory.

The server uses stdio for MCP protocol messages. It never writes logs, banners, or human text to stdout. Logs and CLI help/errors go to stderr.

## Claude Code

Create `.mcp.json` in your project:

```json
{
  "mcpServers": {
    "rust-analyzer": {
      "command": "/absolute/path/to/rust-analyzer-mcp",
      "args": ["--workspace", "/absolute/path/to/project"]
    }
  }
}
```

## Claude Desktop

Add this to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rust-analyzer": {
      "command": "/absolute/path/to/rust-analyzer-mcp",
      "args": ["--workspace", "/absolute/path/to/project"]
    }
  }
}
```

## Codex CLI

Add this to `~/.codex/config.toml`:

```toml
[mcp_servers.rust-analyzer]
command = "/absolute/path/to/rust-analyzer-mcp"
args = ["--workspace", "/absolute/path/to/project"]
```

## Generic MCP Clients

Configure the client to launch the binary over stdio:

```json
{
  "command": "/absolute/path/to/rust-analyzer-mcp",
  "args": ["--workspace", "/absolute/path/to/project"],
  "transport": "stdio"
}
```

## Tools

All tools return pretty JSON as MCP text content:

```json
{
  "ok": true,
  "tool": "ra_definition",
  "workspace_root": "/path/to/project",
  "input": {},
  "result": {},
  "notes": [],
  "truncated": false
}
```

Recoverable errors return `ok: false` with an `error` and `hint`.

### `ra_set_workspace`

Change the active workspace root and restart rust-analyzer.

Params:

```json
{ "workspace_path": "/path/to/project" }
```

### `ra_hover`

Get hover/type/documentation information at a position.

Params:

```json
{ "file_path": "src/lib.rs", "line": 0, "character": 7 }
```

### `ra_definition`

Find definitions at a position.

Params:

```json
{
  "file_path": "src/lib.rs",
  "line": 0,
  "character": 7,
  "context_lines": 8,
  "include_snippets": true
}
```

### `ra_references`

Find references at a position.

Params:

```json
{
  "file_path": "src/lib.rs",
  "line": 0,
  "character": 7,
  "include_declaration": true,
  "max_results": 50,
  "context_lines": 4,
  "include_snippets": true
}
```

### `ra_document_symbols`

List symbols in a file.

Params:

```json
{ "file_path": "src/lib.rs" }
```

### `ra_completion`

Get completion suggestions at a position.

Params:

```json
{
  "file_path": "src/lib.rs",
  "line": 0,
  "character": 7,
  "max_results": 50
}
```

### `ra_format`

Return formatting text edits for a file without applying them.

Params:

```json
{ "file_path": "src/lib.rs" }
```

### `ra_code_actions`

Return available code actions for a selected range without applying edits.

Params:

```json
{
  "file_path": "src/lib.rs",
  "line": 0,
  "character": 0,
  "end_line": 0,
  "end_character": 10
}
```

### `ra_diagnostics`

Open a file, wait briefly, and return cached diagnostics for that file.

Params:

```json
{ "file_path": "src/lib.rs", "wait_ms": 1500 }
```

### `ra_workspace_diagnostics`

Return known cached diagnostics across the active workspace.

Params:

```json
{ "wait_ms": 3000, "max_files": 100, "max_diagnostics": 300 }
```

## Safety Model

- User-supplied paths are resolved inside the configured workspace root.
- Absolute paths are accepted only when they canonicalize inside the workspace.
- Symlink escapes and `..` escapes are rejected.
- External crate locations returned by rust-analyzer are marked as external dependency source.
- External snippets are readonly, bounded, and only read when the URI came from rust-analyzer.
- No shell execution tools are exposed.
- No write/apply tools are exposed in the MVP.

## Troubleshooting

### rust-analyzer not found

Install it and make sure it is on `PATH`:

```sh
rustup component add rust-analyzer
rust-analyzer --version
```

### Invalid workspace

Use a directory that exists. The server warns when the workspace root does not contain `Cargo.toml`.

### No diagnostics yet

rust-analyzer may still be indexing. Retry `ra_diagnostics` or increase `wait_ms`.

### stdout logging breaks stdio MCP

Do not add `println!`, banners, or stdout logging to this server. stdout is reserved for MCP protocol messages only.

### External crate definitions

Definitions and references can point into Cargo registry or rustup source paths. These are returned as external dependency source locations, with bounded snippets when safe.

## Phase-Two Ideas

These are intentionally not advertised in `tools/list` until implemented:

- workspace symbols
- implementations
- inlay hints
- macro expansion
- rename preview
- call hierarchy

## Development

```sh
cargo fmt
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

