# rust-analyzer-mcp

`rust-analyzer-mcp` is a local stdio MCP server that gives coding agents Rust IDE intelligence through rust-analyzer.

It exposes readonly analysis and edit-preview `ra_*` MCP tools for `ra_hover`, `ra_definition`, `ra_references`, `ra_document_symbols`, `ra_completion`, `ra_format`, `ra_code_actions`, `ra_rename_preview`, `ra_diagnostics`, and `ra_workspace_diagnostics`. `ra_format`, `ra_code_actions`, and `ra_rename_preview` return previews only; they do not mutate files. Workspace control is separate: `ra_set_workspace` mutates server state by switching the active workspace and restarting rust-analyzer.

It also exposes fixed `cargo_*` tools for common Rust verification, builds, and workspace inspection: `cargo_build`, `cargo_check`, `cargo_test`, `cargo_clippy`, `cargo_fmt_check`, and `cargo_metadata`. Cargo tools are enabled by default and can be disabled with `--disable-cargo-tools`.

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

Disable cargo tools when you want rust-analyzer-only behavior:

```sh
./target/release/rust-analyzer-mcp --workspace /path/to/project --disable-cargo-tools
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

### `ra_rename_preview`

Return workspace edits for a symbol rename without applying them.

Params:

```json
{
  "file_path": "src/lib.rs",
  "line": 0,
  "character": 7,
  "new_name": "renamed_symbol"
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

### `cargo_build`

Run fixed `cargo build` in the active workspace.

Cargo tool parameters are structured, validated, and enforced by the server; requests that violate these rules are rejected instead of forwarded to cargo. `workspace` cannot be combined with `package`; `all_features` cannot be combined with `features` or `no_default_features`; string values such as `package`, `features`, and `target` must not be empty or start with `-`; feature values also must not contain `,`.

Params:

```json
{
  "workspace": false,
  "package": "optional package name",
  "features": ["optional", "features"],
  "all_features": false,
  "no_default_features": false,
  "target": "optional target triple",
  "all_targets": false,
  "release": false,
  "locked": false,
  "offline": false,
  "frozen": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 60000,
  "max_stderr_bytes": 60000
}
```

### `cargo_check`

Run fixed `cargo check` in the active workspace.

Cargo tool parameters are structured, validated, and enforced by the server; requests that violate these rules are rejected instead of forwarded to cargo. `workspace` cannot be combined with `package`; `all_features` cannot be combined with `features` or `no_default_features`; string values such as `package`, `features`, and `target` must not be empty or start with `-`; feature values also must not contain `,`.

Params:

```json
{
  "workspace": false,
  "package": "optional package name",
  "features": ["optional", "features"],
  "all_features": false,
  "no_default_features": false,
  "target": "optional target triple",
  "all_targets": false,
  "release": false,
  "locked": false,
  "offline": false,
  "frozen": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 60000,
  "max_stderr_bytes": 60000
}
```

### `cargo_test`

Run fixed `cargo test` in the active workspace.

Params:

```json
{
  "workspace": false,
  "package": "optional package name",
  "features": ["optional", "features"],
  "all_features": false,
  "no_default_features": false,
  "target": "optional target triple",
  "all_targets": false,
  "locked": false,
  "offline": false,
  "frozen": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 60000,
  "max_stderr_bytes": 60000,
  "test_filter": "optional test name or substring",
  "nocapture": false
}
```

### `cargo_clippy`

Run fixed `cargo clippy` in the active workspace. This tool does not append `-- -D warnings`.

Params:

```json
{
  "workspace": false,
  "package": "optional package name",
  "features": ["optional", "features"],
  "all_features": false,
  "no_default_features": false,
  "target": "optional target triple",
  "all_targets": false,
  "release": false,
  "locked": false,
  "offline": false,
  "frozen": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 60000,
  "max_stderr_bytes": 60000
}
```

### `cargo_fmt_check`

Run fixed `cargo fmt --check` in the active workspace.

Params:

```json
{
  "package": "optional package name",
  "all": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 60000,
  "max_stderr_bytes": 60000
}
```

### `cargo_metadata`

Run fixed `cargo metadata --format-version 1` in the active workspace.

Params:

```json
{
  "features": ["optional", "features"],
  "all_features": false,
  "no_default_features": false,
  "filter_platform": "optional target triple",
  "no_deps": false,
  "locked": false,
  "offline": false,
  "frozen": false,
  "timeout_ms": 120000,
  "max_stdout_bytes": 120000,
  "max_stderr_bytes": 60000
}
```

When metadata JSON parses successfully, the response includes `metadata_json` and omits the duplicated raw `stdout` payload to keep the MCP response bounded.

## Safety Model

- User-supplied paths are resolved inside the configured workspace root.
- Absolute paths are accepted only when they canonicalize inside the workspace.
- Symlink escapes and `..` escapes are rejected.
- External crate locations returned by rust-analyzer are marked as external dependency source.
- External snippets are readonly, bounded, and only read when the URI came from rust-analyzer.
- Most `ra_*` tools are readonly analysis or edit-preview tools.
- `ra_format`, `ra_code_actions`, and `ra_rename_preview` return edit previews only. They never write those edits to disk.
- `ra_set_workspace` mutates server state by switching the active workspace and restarting rust-analyzer. It does not write workspace files.
- `cargo_*` tools execute fixed cargo commands in the active workspace. They do not expose arbitrary shell commands or free-form cargo subcommands.
- Cargo commands may execute workspace code, build scripts, proc macros, and tests. Those executions can have arbitrary project-defined side effects, write artifacts under `target/`, and update `Cargo.lock` unless `locked` or `frozen` is used.
- Cargo tools are enabled by default and can be disabled with `--disable-cargo-tools`.
- No write/apply file-editing tools are exposed in the MVP.

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

### No rename edits returned

Make sure the cursor is on the symbol name and that rust-analyzer has finished indexing the workspace.

### cargo not found

Install Rust and make sure `cargo` is on `PATH`:

```sh
rustup --version
cargo --version
```

### cargo tool timed out

Increase `timeout_ms` for large workspaces or run a narrower package/test selection. Timeout cleanup is best effort; the server kills the spawned cargo process and stops output collection after timeout, but it does not claim full process-tree cleanup.

### cargo tools disabled

Restart the server without `--disable-cargo-tools` if you want to use `cargo_build`, `cargo_check`, `cargo_test`, `cargo_clippy`, `cargo_fmt_check`, or `cargo_metadata`.

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
- call hierarchy

## Development

```sh
cargo fmt
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

