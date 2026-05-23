# Architecture Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `rust-analyzer-mcp` into clearer server, `ra`, and `cargo` modules without adding new MCP tools.

**Architecture:** Keep `RaMcpServer` as the `rmcp` macro attachment point, but make its tool methods thin wrappers around family-specific helpers. Split cargo into params, args, process, and output modules with a narrow public facade. Split rust-analyzer tool behavior into params, location normalization, navigation, symbols, completion, edit previews, and diagnostics helpers.

**Tech Stack:** Rust 2024, `rmcp`, `tokio`, `lsp-types`, `serde`, `schemars`, `thiserror`, `tracing`, `tempfile`, Cargo tests, rust-analyzer.

---

## Scope Check

This plan implements only architecture cleanup from `docs/superpowers/specs/2026-05-23-architecture-cleanup-design.md`.

It does not add new `ra_*` tools, does not change cargo tools into free-form execution, and does not replace the `rmcp` macro routing model.

## File Structure

Create these files:

- `src/server/response.rs`: shared MCP text-envelope construction and total-output truncation.
- `src/server/state.rs`: `ServerConfig`, `ServerState`, workspace/client lifecycle helpers, and common error hints.
- `src/ra/mod.rs`: rust-analyzer tool-family module entry point.
- `src/ra/params.rs`: `ra_*` params and defaults.
- `src/ra/locate.rs`: LSP URI classification and bounded snippets.
- `src/ra/navigation.rs`: definition and references helpers, with a home for implementations and call hierarchy later.
- `src/ra/symbols.rs`: document symbols helper, with a home for workspace symbols later.
- `src/ra/completion.rs`: completion result shaping.
- `src/ra/edits.rs`: format and code-action preview helpers, with a home for rename preview later.
- `src/ra/diagnostics.rs`: file and workspace diagnostics helpers.
- `src/cargo/params.rs`: `cargo_*` params and cargo defaults.
- `src/cargo/args.rs`: `CargoInvocation`, `CargoCommandKind`, `CargoValidationError`, and argument validation.
- `src/cargo/process.rs`: cargo spawn, stdin isolation, timeout, cleanup, bounded readers.
- `src/cargo/output.rs`: `CargoRunOutput`, `CargoStatus`, `TruncatedText`, truncation, metadata parsing.
- `src/cargo/tools.rs`: cargo tool orchestration used by `RaMcpServer` wrappers.

Modify these files:

- `src/lib.rs`: expose `ra`, keep `tools` as a compatibility re-export module, and keep `cargo` facade stable.
- `src/server.rs`, then `src/server/mod.rs`: shrink server routing and state orchestration.
- `src/cargo.rs`, then `src/cargo/mod.rs`: shrink cargo facade and re-export stable helper types.
- `src/tools.rs`: replace mixed implementation with compatibility re-exports.
- `tests/cargo_tests.rs`: keep import compatibility at first, then move tests to new module imports only when each module split is stable.
- `tests/integration_basic.rs`: keep MCP-level regression checks for tool listing, disabled cargo tools, metadata output shaping, and at least one `ra_*` smoke path.

## Task 1: Extract Response Envelope

**Files:**
- Create: `src/server/response.rs`
- Modify: `src/server.rs`
- Modify: `src/tools.rs`

- [ ] **Step 1: Write response module tests**

Create `src/server/response.rs` with only this test module first:

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{failure, success};

    #[test]
    fn success_envelope_contains_stable_shape() {
        let text = success(
            "ra_hover",
            "C:/workspace",
            &json!({"file_path":"src/lib.rs"}),
            json!({"hover":"text"}),
            vec!["note".to_string()],
            false,
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["tool"], "ra_hover");
        assert_eq!(value["workspace_root"], "C:/workspace");
        assert_eq!(value["input"]["file_path"], "src/lib.rs");
        assert_eq!(value["result"]["hover"], "text");
        assert_eq!(value["notes"][0], "note");
        assert_eq!(value["truncated"], false);
        assert!(value.get("error").is_none());
        assert!(value.get("hint").is_none());
    }

    #[test]
    fn failure_envelope_contains_error_and_hint() {
        let text = failure(
            "cargo_check",
            "C:/workspace",
            &json!({}),
            "cargo validation failed",
            "Check parameters.",
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(value["ok"], false);
        assert_eq!(value["tool"], "cargo_check");
        assert_eq!(value["error"], "cargo validation failed");
        assert_eq!(value["hint"], "Check parameters.");
    }
}
```

- [ ] **Step 2: Run the focused test to verify it fails**

Run:

```powershell
cargo test --locked server::response
```

Expected: compile failure because `success` and `failure` are not defined in `src/server/response.rs` yet.

- [ ] **Step 3: Move envelope code from `src/tools.rs`**

In `src/server.rs`, add this near the top of the module:

```rust
pub mod response;
```

Move these exact items from `src/tools.rs` into `src/server/response.rs`:

```rust
use serde::Serialize;
use serde_json::{Value, json};

pub const DEFAULT_MAX_TOTAL_OUTPUT_BYTES: usize = 120_000;

#[derive(Debug, Serialize)]
pub struct ToolEnvelope {
    pub ok: bool,
    pub tool: &'static str,
    pub workspace_root: String,
    pub input: Value,
    pub result: Value,
    pub notes: Vec<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}
```

Also move the current bodies of these functions unchanged:

```rust
pub fn success<T: Serialize>(
    tool: &'static str,
    workspace_root: impl Into<String>,
    input: &T,
    result: Value,
    notes: Vec<String>,
    truncated: bool,
) -> String;

pub fn failure<T: Serialize>(
    tool: &'static str,
    workspace_root: impl Into<String>,
    input: &T,
    error: impl Into<String>,
    hint: impl Into<String>,
) -> String;

pub fn envelope_text(mut envelope: ToolEnvelope) -> String;
```

- [ ] **Step 4: Update imports and compatibility re-exports**

In `src/server.rs`, replace `failure` and `success` imports from `crate::tools` with:

```rust
use self::response::{failure, success};
```

In `src/tools.rs`, temporarily re-export the response helpers so existing external imports remain usable during the refactor:

```rust
pub use crate::server::response::{
    DEFAULT_MAX_TOTAL_OUTPUT_BYTES, ToolEnvelope, envelope_text, failure, success,
};
```

Leave the parameter structs and defaults in `src/tools.rs` for this task.

- [ ] **Step 5: Run response and full quality checks**

Run:

```powershell
cargo fmt --check
cargo test --locked server::response
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass.

- [ ] **Step 6: Commit Task 1**

Run:

```powershell
git add src/server.rs src/server/response.rs src/tools.rs
git commit -m "refactor: extract MCP response envelope"
```

## Task 2: Extract Tool Parameter Modules

**Files:**
- Create: `src/ra/mod.rs`
- Create: `src/ra/params.rs`
- Create: `src/cargo/params.rs`
- Modify: `src/lib.rs`
- Modify: `src/cargo.rs`
- Modify: `src/server.rs`
- Modify: `src/tools.rs`

- [ ] **Step 1: Write schema tests in the new params modules**

Create `src/ra/mod.rs`:

```rust
pub mod params;
```

Create `src/ra/params.rs` with this test module first:

```rust
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DEFAULT_MAX_RESULTS, HoverParams};

    #[test]
    fn hover_params_schema_is_generated() {
        let schema = schemars::schema_for!(HoverParams);
        let schema_json = serde_json::to_value(schema).unwrap();
        assert_eq!(schema_json["title"], "HoverParams");
    }

    #[test]
    fn default_max_results_remains_fifty() {
        assert_eq!(json!(DEFAULT_MAX_RESULTS), json!(50));
    }
}
```

Create `src/cargo/params.rs` with this test module first:

```rust
#[cfg(test)]
mod tests {
    use super::{CargoBuildParams, DEFAULT_CARGO_TIMEOUT_MS};

    #[test]
    fn cargo_build_params_default_to_empty_options() {
        let params = CargoBuildParams::default();
        assert_eq!(params.workspace, None);
        assert_eq!(params.package, None);
        assert_eq!(params.timeout_ms, None);
    }

    #[test]
    fn default_cargo_timeout_is_two_minutes() {
        assert_eq!(DEFAULT_CARGO_TIMEOUT_MS, 120_000);
    }
}
```

- [ ] **Step 2: Run focused tests to verify they fail**

Run:

```powershell
cargo test --locked ra::params
cargo test --locked cargo::params
```

Expected: compile failure because the new params modules do not yet define the referenced types and constants.

- [ ] **Step 3: Move `ra_*` params and defaults**

Move these items from `src/tools.rs` into `src/ra/params.rs` with their current derives and field definitions:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_DEFINITION_CONTEXT_LINES: u32 = 8;
pub const DEFAULT_REFERENCE_CONTEXT_LINES: u32 = 4;
pub const DEFAULT_MAX_RESULTS: u32 = 50;
pub const DEFAULT_DIAGNOSTICS_WAIT_MS: u64 = 1_500;
pub const DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS: u64 = 3_000;
pub const DEFAULT_MAX_FILES: u32 = 100;
pub const DEFAULT_MAX_DIAGNOSTICS: u32 = 300;
pub const DEFAULT_MAX_SNIPPET_BYTES: usize = 8_192;
```

Move these structs unchanged:

```rust
SetWorkspaceParams
HoverParams
DefinitionParams
ReferencesParams
DocumentSymbolsParams
CompletionParams
FormatParams
CodeActionsParams
DiagnosticsParams
WorkspaceDiagnosticsParams
```

- [ ] **Step 4: Move `cargo_*` params and defaults**

At the top of `src/cargo.rs`, add:

```rust
pub mod params;
```

Move these constants from `src/tools.rs` into `src/cargo/params.rs`:

```rust
pub const DEFAULT_CARGO_TIMEOUT_MS: u64 = 120_000;
pub const MAX_CARGO_TIMEOUT_MS: u64 = 600_000;
pub const DEFAULT_CARGO_STDOUT_BYTES: usize = 60_000;
pub const DEFAULT_CARGO_STDERR_BYTES: usize = 60_000;
pub const DEFAULT_CARGO_METADATA_STDOUT_BYTES: usize = 120_000;
pub const MAX_CARGO_OUTPUT_BYTES: usize = 240_000;
```

Move these structs unchanged:

```rust
CargoBuildParams
CargoTestParams
CargoFmtCheckParams
CargoMetadataParams
```

- [ ] **Step 5: Update imports and `tools` compatibility module**

In `src/lib.rs`, add:

```rust
pub mod ra;
```

In `src/server.rs`, update parameter imports to:

```rust
use crate::cargo::params::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
};
use crate::ra::params::{
    CodeActionsParams, CompletionParams, DEFAULT_DEFINITION_CONTEXT_LINES,
    DEFAULT_DIAGNOSTICS_WAIT_MS, DEFAULT_MAX_DIAGNOSTICS, DEFAULT_MAX_FILES,
    DEFAULT_MAX_RESULTS, DEFAULT_MAX_SNIPPET_BYTES, DEFAULT_REFERENCE_CONTEXT_LINES,
    DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS, DefinitionParams, DiagnosticsParams,
    DocumentSymbolsParams, FormatParams, HoverParams, ReferencesParams, SetWorkspaceParams,
    WorkspaceDiagnosticsParams,
};
```

In `src/cargo.rs`, update params imports to:

```rust
use crate::cargo::params::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
    DEFAULT_CARGO_METADATA_STDOUT_BYTES, DEFAULT_CARGO_STDERR_BYTES, DEFAULT_CARGO_STDOUT_BYTES,
    DEFAULT_CARGO_TIMEOUT_MS, MAX_CARGO_OUTPUT_BYTES, MAX_CARGO_TIMEOUT_MS,
};
```

Replace `src/tools.rs` with compatibility re-exports:

```rust
pub use crate::cargo::params::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
    DEFAULT_CARGO_METADATA_STDOUT_BYTES, DEFAULT_CARGO_STDERR_BYTES, DEFAULT_CARGO_STDOUT_BYTES,
    DEFAULT_CARGO_TIMEOUT_MS, MAX_CARGO_OUTPUT_BYTES, MAX_CARGO_TIMEOUT_MS,
};
pub use crate::ra::params::{
    CodeActionsParams, CompletionParams, DEFAULT_DEFINITION_CONTEXT_LINES,
    DEFAULT_DIAGNOSTICS_WAIT_MS, DEFAULT_MAX_DIAGNOSTICS, DEFAULT_MAX_FILES,
    DEFAULT_MAX_RESULTS, DEFAULT_MAX_SNIPPET_BYTES, DEFAULT_REFERENCE_CONTEXT_LINES,
    DEFAULT_WORKSPACE_DIAGNOSTICS_WAIT_MS, DefinitionParams, DiagnosticsParams,
    DocumentSymbolsParams, FormatParams, HoverParams, ReferencesParams, SetWorkspaceParams,
    WorkspaceDiagnosticsParams,
};
pub use crate::server::response::{
    DEFAULT_MAX_TOTAL_OUTPUT_BYTES, ToolEnvelope, envelope_text, failure, success,
};
```

- [ ] **Step 6: Run focused and full checks**

Run:

```powershell
cargo fmt --check
cargo test --locked ra::params
cargo test --locked cargo::params
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass, and `tests/cargo_tests.rs` still compiles through `rust_analyzer_mcp::tools::*` compatibility imports.

- [ ] **Step 7: Commit Task 2**

Run:

```powershell
git add src/lib.rs src/ra/mod.rs src/ra/params.rs src/cargo.rs src/cargo/params.rs src/server.rs src/tools.rs
git commit -m "refactor: split MCP tool params"
```

## Task 3: Split Cargo Argument Construction

**Files:**
- Create: `src/cargo/args.rs`
- Modify: `src/cargo.rs`
- Modify: `tests/cargo_tests.rs`

- [ ] **Step 1: Move argument tests into a named block**

In `tests/cargo_tests.rs`, keep the existing argument-construction tests and update imports to the cargo facade:

```rust
use rust_analyzer_mcp::{
    cargo::{
        CargoCommandKind, CargoInvocation, run_cargo, truncate_text,
        params::{CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams},
    },
};
```

The existing tests that must remain green in this task are:

```text
cargo_check_args_are_built_from_structured_options
cargo_clippy_args_are_built_from_build_params
cargo_test_places_filter_before_test_binary_args
cargo_fmt_check_uses_fmt_specific_options
cargo_metadata_args_include_format_version_and_metadata_flags
rejects_conflicting_workspace_and_package
rejects_option_like_user_values
rejects_feature_conflicts
rejects_comma_separated_feature_values
build_like_commands_default_to_sixty_kib_output_caps
cargo_metadata_defaults_to_larger_stdout_cap
clamps_limits_to_hard_maximums
```

- [ ] **Step 2: Run argument tests before moving code**

Run:

```powershell
cargo test --locked --test cargo_tests cargo_check_args_are_built_from_structured_options
cargo test --locked --test cargo_tests rejects_option_like_user_values
```

Expected: both pass before the move.

- [ ] **Step 3: Create `src/cargo/args.rs`**

Move these items from `src/cargo.rs` to `src/cargo/args.rs`:

```rust
CargoCommandKind
CargoInvocation
impl CargoInvocation
CargoValidationError
CargoArgs
impl CargoArgs for CargoBuildParams
impl CargoArgs for CargoTestParams
impl CargoArgs for CargoFmtCheckParams
impl CargoArgs for CargoMetadataParams
validate_package_scope
validate_feature_flags
validate_feature_value
push_package_scope
push_feature_flags
push_bool
push_optional_value
push_positional
validate_user_value
clamp_u64
clamp_usize
```

At the top of `src/cargo/args.rs`, use:

```rust
use serde::Serialize;
use thiserror::Error;

use crate::cargo::params::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
    DEFAULT_CARGO_METADATA_STDOUT_BYTES, DEFAULT_CARGO_STDERR_BYTES, DEFAULT_CARGO_STDOUT_BYTES,
    DEFAULT_CARGO_TIMEOUT_MS, MAX_CARGO_OUTPUT_BYTES, MAX_CARGO_TIMEOUT_MS,
};
```

- [ ] **Step 4: Re-export through `src/cargo.rs`**

At the top of `src/cargo.rs`, add:

```rust
pub mod args;

pub use args::{CargoArgs, CargoCommandKind, CargoInvocation, CargoValidationError};
```

Remove the moved items from `src/cargo.rs`.

- [ ] **Step 5: Run cargo argument and full checks**

Run:

```powershell
cargo fmt --check
cargo test --locked --test cargo_tests cargo_check_args_are_built_from_structured_options
cargo test --locked --test cargo_tests rejects_option_like_user_values
cargo test --locked --test cargo_tests clamps_limits_to_hard_maximums
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass.

- [ ] **Step 6: Commit Task 3**

Run:

```powershell
git add src/cargo.rs src/cargo/args.rs tests/cargo_tests.rs
git commit -m "refactor: split cargo argument validation"
```

## Task 4: Split Cargo Process And Output Modules

**Files:**
- Create: `src/cargo/output.rs`
- Create: `src/cargo/process.rs`
- Modify: `src/cargo.rs`
- Modify: `tests/cargo_tests.rs`

- [ ] **Step 1: Add focused output tests before moving code**

Keep the existing `truncate_text_*` tests in `tests/cargo_tests.rs`. Add this test near them:

```rust
#[test]
fn truncate_text_allows_exact_utf8_boundary() {
    let truncated = truncate_text("éz".as_bytes(), "é".len());

    assert_eq!(truncated.text, "é");
    assert!(truncated.truncated);
}
```

- [ ] **Step 2: Run the output tests**

Run:

```powershell
cargo test --locked --test cargo_tests truncate_text
```

Expected: pass before the move.

- [ ] **Step 3: Create `src/cargo/output.rs`**

Move these items from `src/cargo.rs` into `src/cargo/output.rs`:

```rust
TruncatedText
CargoStatus
CargoRunOutput
truncate_text
is_utf8_continuation
metadata_json
```

Use these imports at the top of `src/cargo/output.rs`:

```rust
use serde::Serialize;

use crate::cargo::args::CargoInvocation;
```

Make `metadata_json` visible to the process module:

```rust
pub(crate) fn metadata_json(
    invocation: &CargoInvocation,
    status: &CargoStatus,
    stdout: &TruncatedText,
    notes: &mut Vec<String>,
) -> Option<serde_json::Value>
```

- [ ] **Step 4: Create `src/cargo/process.rs`**

Move these items from `src/cargo.rs` into `src/cargo/process.rs`:

```rust
run_cargo
OutputCollection
collect_task_outputs
truncated_output_pair
configure_process_tree_root
cleanup_process_tree
kill_child_fallback
reap_child_after_cleanup
task_output
read_limited
```

Use these imports at the top of `src/cargo/process.rs`:

```rust
use std::{
    path::Path,
    process::{Command as StdCommand, Stdio},
    time::{Duration, Instant},
};

use tokio::{io::AsyncReadExt, process::Command as TokioCommand, task::JoinHandle};

use crate::cargo::{
    args::CargoInvocation,
    output::{CargoRunOutput, CargoStatus, TruncatedText, metadata_json},
};
use crate::error::{RaMcpError, Result as CrateResult};
```

- [ ] **Step 5: Shrink `src/cargo.rs` to a facade**

Replace the remaining top-level module declarations and re-exports in `src/cargo.rs` with:

```rust
pub mod args;
pub mod output;
pub mod params;
pub mod process;

pub use args::{CargoArgs, CargoCommandKind, CargoInvocation, CargoValidationError};
pub use output::{CargoRunOutput, CargoStatus, TruncatedText, truncate_text};
pub use process::run_cargo;
```

Do not move to `src/cargo/mod.rs` yet; that happens after the server split.

- [ ] **Step 6: Run process-sensitive tests**

Run:

```powershell
cargo fmt --check
cargo test --locked --test cargo_tests cargo_check_runs_in_workspace_root
cargo test --locked --test cargo_tests cargo_metadata_parses_json_result
cargo test --locked --test cargo_tests cargo_timeout_returns_even_when_build_script_child_keeps_pipes_open
cargo test --locked --test cargo_tests cargo_output_collection_times_out_after_cargo_exits
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass. The two timeout tests should finish within their existing timeout guards.

- [ ] **Step 7: Commit Task 4**

Run:

```powershell
git add src/cargo.rs src/cargo/output.rs src/cargo/process.rs tests/cargo_tests.rs
git commit -m "refactor: split cargo process and output"
```

## Task 5: Add Cargo Tool Orchestration Module

**Files:**
- Create: `src/cargo/tools.rs`
- Modify: `src/cargo.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Add regression command for disabled cargo tools**

Run this before moving code:

```powershell
cargo test --locked --test integration_basic disabled_cargo_tools_return_structured_failure
```

Expected: pass.

- [ ] **Step 2: Create `src/cargo/tools.rs`**

Move cargo orchestration helpers from `src/server.rs` into `src/cargo/tools.rs`:

```rust
use std::{path::PathBuf, sync::Arc};

use serde::Serialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::{
    cargo::{
        args::{CargoArgs, CargoCommandKind, CargoInvocation},
        output::CargoRunOutput,
        process::run_cargo,
    },
    error::RaMcpError,
    server::response::{failure, success},
};
```

Implement a helper with this signature:

```rust
pub(crate) async fn run_cargo_tool<T>(
    tool: &'static str,
    workspace_path: PathBuf,
    workspace_root: String,
    cargo_tools_enabled: bool,
    cargo_run_lock: Arc<Semaphore>,
    params: T,
    kind: CargoCommandKind,
) -> String
where
    T: CargoArgs + Serialize,
```

Move the current `RaMcpServer::run_cargo_tool` body into this function. Replace `self.config.cargo_tools_enabled` with `cargo_tools_enabled`, `self.cargo_run_lock` with `cargo_run_lock`, and the workspace helper call with the passed `workspace_path` and `workspace_root`.

Move `prepare_cargo_output_for_response` into `src/cargo/tools.rs` as:

```rust
fn prepare_cargo_output_for_response(
    kind: CargoCommandKind,
    output: &mut CargoRunOutput,
    notes: &mut Vec<String>,
)
```

- [ ] **Step 3: Wire server wrappers to cargo tools**

In `src/cargo.rs`, add:

```rust
pub mod tools;
```

In `src/server.rs`, replace the private `run_cargo_tool` and `prepare_cargo_output_for_response` methods with a smaller helper:

```rust
async fn cargo_workspace_root(&self) -> (PathBuf, String) {
    let state = self.state.lock().await;
    (
        state.workspace.root().to_path_buf(),
        Self::workspace_root(&state),
    )
}
```

Each cargo `#[tool]` method should call:

```rust
let (workspace_path, workspace_root) = self.cargo_workspace_root().await;
crate::cargo::tools::run_cargo_tool(
    "cargo_check",
    workspace_path,
    workspace_root,
    self.config.cargo_tools_enabled,
    self.cargo_run_lock.clone(),
    params,
    CargoCommandKind::Check,
)
.await
```

Use the same pattern for `cargo_test`, `cargo_clippy`, `cargo_fmt_check`, and `cargo_metadata` with the existing tool names and command kinds.

- [ ] **Step 4: Run cargo MCP regression checks**

Run:

```powershell
cargo fmt --check
cargo test --locked --test integration_basic mcp_tools_list_smoke_has_mvp_tools_and_protocol_stdout
cargo test --locked --test integration_basic disabled_cargo_tools_return_structured_failure
cargo test --locked --test integration_basic cargo_metadata_response_omits_duplicate_raw_stdout_when_parsed
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass.

- [ ] **Step 5: Commit Task 5**

Run:

```powershell
git add src/cargo.rs src/cargo/tools.rs src/server.rs
git commit -m "refactor: move cargo tool orchestration"
```

## Task 6: Extract Rust-Analyzer Location And Result Helpers

**Files:**
- Create: `src/ra/locate.rs`
- Create: `src/ra/navigation.rs`
- Create: `src/ra/symbols.rs`
- Create: `src/ra/completion.rs`
- Create: `src/ra/edits.rs`
- Create: `src/ra/diagnostics.rs`
- Modify: `src/ra/mod.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Add module declarations**

Update `src/ra/mod.rs`:

```rust
pub mod completion;
pub mod diagnostics;
pub mod edits;
pub mod locate;
pub mod navigation;
pub mod params;
pub mod symbols;
```

- [ ] **Step 2: Move location normalization**

Create `src/ra/locate.rs` and move these items from `src/server.rs`:

```rust
LocatedRange
locate
```

Use these imports:

```rust
use lsp_types::Range;
use serde::Serialize;

use crate::{
    lsp::snippets::{SourceSnippet, read_snippet},
    ra::params::DEFAULT_MAX_SNIPPET_BYTES,
    workspace::{ClassifiedLocation, LocationKind, Workspace},
};
```

Make `LocatedRange` and `locate` visible inside the crate:

```rust
pub(crate) struct LocatedRange { ... }

pub(crate) fn locate(
    workspace: &Workspace,
    uri: lsp_types::Uri,
    range: Range,
    context_lines: u32,
    include_snippet: bool,
) -> LocatedRange
```

- [ ] **Step 3: Move navigation helpers**

Create `src/ra/navigation.rs` and move `definition_locations` from `src/server.rs`.

Use this public helper shape for definitions:

```rust
pub(crate) fn definition_locations(
    response: Option<lsp_types::GotoDefinitionResponse>,
) -> Vec<(lsp_types::Uri, lsp_types::Range)>
```

Add a references helper:

```rust
pub(crate) fn references_truncated(total: usize, max_results: usize) -> bool {
    total > max_results
}
```

Update `src/server.rs` definition and references handlers to import:

```rust
use crate::ra::{
    locate::locate,
    navigation::{definition_locations, references_truncated},
};
```

Replace:

```rust
let truncated = references.len() > max_results;
```

with:

```rust
let truncated = references_truncated(references.len(), max_results);
```

- [ ] **Step 4: Move symbol, completion, edit, and diagnostics helpers**

Create `src/ra/completion.rs` and move `completion_items` from `src/server.rs`:

```rust
pub(crate) fn completion_items(
    response: Option<lsp_types::CompletionResponse>,
) -> (Vec<serde_json::Value>, usize)
```

Create `src/ra/edits.rs` and move:

```rust
summarize_code_actions
```

Create `src/ra/diagnostics.rs` and move:

```rust
diagnostic_summary
```

Create `src/ra/symbols.rs` with a small document-symbol pass-through helper:

```rust
pub(crate) fn document_symbols_result(
    symbols: Option<lsp_types::DocumentSymbolResponse>,
) -> serde_json::Value {
    serde_json::json!({ "symbols": symbols })
}
```

Update `src/server.rs` imports and call sites to use these helpers.

- [ ] **Step 5: Run `ra_*` smoke and full checks**

Run:

```powershell
cargo fmt --check
cargo test --locked --test integration_basic rust_analyzer_smoke_hover_when_available
cargo test --locked --test integration_basic mcp_tools_list_smoke_has_mvp_tools_and_protocol_stdout
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass, and the MCP tool listing still includes all current `ra_*` tool names.

- [ ] **Step 6: Commit Task 6**

Run:

```powershell
git add src/ra/mod.rs src/ra/locate.rs src/ra/navigation.rs src/ra/symbols.rs src/ra/completion.rs src/ra/edits.rs src/ra/diagnostics.rs src/server.rs
git commit -m "refactor: extract rust-analyzer result helpers"
```

## Task 7: Move Server State Into A Dedicated Module

**Files:**
- Create: `src/server/state.rs`
- Modify: `src/server.rs`

- [ ] **Step 1: Add a state-focused unit test**

Create `src/server/state.rs` with this test module first:

```rust
#[cfg(test)]
mod tests {
    use super::ServerConfig;

    #[test]
    fn cargo_tools_are_enabled_by_default() {
        let config = ServerConfig::default();
        assert!(config.cargo_tools_enabled);
    }
}
```

- [ ] **Step 2: Run the state test to verify it fails**

Run:

```powershell
cargo test --locked server::state::tests::cargo_tools_are_enabled_by_default
```

Expected: compile failure because `ServerConfig` is not defined in `src/server/state.rs` yet.

- [ ] **Step 3: Move state types and helpers**

In `src/server.rs`, add:

```rust
pub(crate) mod state;
```

Move these items from `src/server.rs` into `src/server/state.rs`:

```rust
ServerConfig
impl Default for ServerConfig
ServerState
```

Make `ServerState` fields crate-visible so server wrappers can keep using them while the rest of the split proceeds:

```rust
pub(crate) struct ServerState {
    pub(crate) workspace: Workspace,
    pub(crate) client: Option<RustAnalyzerClient>,
}
```

Move these helper methods into `impl ServerState`:

```rust
pub(crate) async fn ensure_client(&mut self) -> crate::error::Result<&mut RustAnalyzerClient>
pub(crate) fn workspace_root(&self) -> String
pub(crate) fn workspace_notes(&self) -> Vec<String>
```

Move this helper into `src/server/state.rs`:

```rust
pub(crate) fn hint_for_error(error: &RaMcpError) -> &'static str
```

Use these imports:

```rust
use crate::{
    error::RaMcpError,
    lsp::client::RustAnalyzerClient,
    workspace::Workspace,
};
```

- [ ] **Step 4: Update server call sites**

In `src/server.rs`, import:

```rust
use self::state::{ServerConfig, ServerState, hint_for_error};
```

Replace:

```rust
Self::ensure_client(&mut state)
Self::workspace_root(&state)
Self::workspace_notes(&state.workspace)
Self::hint_for_error(&error)
```

with:

```rust
state.ensure_client()
state.workspace_root()
state.workspace_notes()
hint_for_error(&error)
```

Keep the current lock behavior unchanged in this task.

- [ ] **Step 5: Run checks**

Run:

```powershell
cargo fmt --check
cargo test --locked server::state
cargo test --locked --test integration_basic rust_analyzer_smoke_hover_when_available
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass.

- [ ] **Step 6: Commit Task 7**

Run:

```powershell
git add src/server.rs src/server/state.rs
git commit -m "refactor: move server state helpers"
```

## Task 8: Move File Modules To Directory `mod.rs` Facades

**Files:**
- Move: `src/server.rs` to `src/server/mod.rs`
- Move: `src/cargo.rs` to `src/cargo/mod.rs`

- [ ] **Step 1: Verify current module directories exist**

Run:

```powershell
Test-Path src/server/response.rs
Test-Path src/server/state.rs
Test-Path src/cargo/args.rs
Test-Path src/cargo/process.rs
```

Expected: all four commands print `True`.

- [ ] **Step 2: Move `server.rs` explicitly**

Run:

```powershell
git mv src/server.rs src/server/mod.rs
```

Expected: `src/server.rs` no longer exists, and `src/server/mod.rs` exists.

- [ ] **Step 3: Move `cargo.rs` explicitly**

Run:

```powershell
git mv src/cargo.rs src/cargo/mod.rs
```

Expected: `src/cargo.rs` no longer exists, and `src/cargo/mod.rs` exists.

- [ ] **Step 4: Build to catch module conflicts**

Run:

```powershell
cargo check --locked
```

Expected: pass. If Rust reports both file and directory module definitions for `server` or `cargo`, stop and remove only the stale moved-from file after verifying `git status`.

- [ ] **Step 5: Run full checks**

Run:

```powershell
cargo fmt --check
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
```

Expected: all commands pass.

- [ ] **Step 6: Commit Task 8**

Run:

```powershell
git add src/server/mod.rs src/cargo/mod.rs
git commit -m "refactor: convert server and cargo to module directories"
```

## Task 9: Final Cleanup And Regression Review

**Files:**
- Modify: `src/server/mod.rs`
- Modify: `src/tools.rs`
- Modify: `tests/cargo_tests.rs`
- Modify: `tests/integration_basic.rs`
- Review: `README.md`

- [ ] **Step 1: Check remaining large-file pressure**

Run:

```powershell
Get-ChildItem src,tests -Recurse -Filter *.rs | Select-Object FullName,@{Name='Lines';Expression={(Get-Content $_.FullName | Measure-Object -Line).Lines}}
```

Expected: `src/server/mod.rs` and `src/cargo/mod.rs` are substantially smaller than the original `src/server.rs` and `src/cargo.rs`. Record the line counts in the implementation notes.

- [ ] **Step 2: Verify no raw cargo spawning leaked outside cargo process module**

Run:

```powershell
rg -n "Command::new|StdCommand::new|TokioCommand::new|run_cargo\\(" src
```

Expected:

- `Command::new("rust-analyzer")` remains only in `src/lsp/client.rs`.
- Cargo process spawning remains only in `src/cargo/process.rs`.
- Cargo tool wrappers call `crate::cargo::tools::run_cargo_tool`.

- [ ] **Step 3: Verify `ra_*` handlers use typed LSP client methods**

Run:

```powershell
rg -n "request_value\\(|request_optional\\(|json!\\(\\{.*method|textDocument/" src/server src/ra
```

Expected:

- `request_value` and `request_optional` remain internal to `src/lsp/client.rs`.
- `textDocument/*` method strings remain in `src/lsp/client.rs`, not in `src/server/mod.rs`.

- [ ] **Step 4: Verify compatibility imports**

Run:

```powershell
cargo test --locked --test cargo_tests cargo_check_args_are_built_from_structured_options
```

Expected: pass. This confirms `rust_analyzer_mcp::cargo::*` and `rust_analyzer_mcp::tools::*` compatibility still works for current tests.

- [ ] **Step 5: Run the complete release-quality gate**

Run:

```powershell
cargo fmt --check
cargo test --locked --all
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --release --locked
.\target\release\rust-analyzer-mcp.exe --version
```

Expected:

- Formatting passes.
- All tests pass.
- Clippy exits with no warnings.
- Release build succeeds.
- Version command prints `rust-analyzer-mcp 0.1.0` to stderr and does not write MCP protocol text.

- [ ] **Step 6: Inspect Git diff for accidental product changes**

Run:

```powershell
git diff --stat HEAD
git diff -- README.md Cargo.toml Cargo.lock
git status --short --branch --ignored
```

Expected:

- `README.md`, `Cargo.toml`, and `Cargo.lock` have no changes unless a previous task intentionally required them.
- `target/` remains ignored.
- No unrelated ignored planning files are staged.

- [ ] **Step 7: Commit final cleanup**

Run:

```powershell
git add src tests README.md Cargo.toml Cargo.lock
git commit -m "refactor: finish architecture cleanup"
```

If `README.md`, `Cargo.toml`, or `Cargo.lock` did not change, Git will ignore those paths during staging.

## Self-Review

Spec coverage:

- Tool-family module split is covered by Tasks 1, 2, 5, 6, 7, and 8.
- Cargo validation, process, output, and orchestration boundaries are covered by Tasks 3, 4, and 5.
- `rmcp` macro wrapper constraint is covered by Tasks 5, 6, and 7; wrappers remain on `RaMcpServer`.
- File-to-directory move risk is covered by Task 8.
- State/client locking discipline is covered by Task 7, which centralizes helpers without changing lock behavior.
- Regression-sensitive tests are covered across Tasks 4, 5, 6, and 9.

Placeholder scan:

- No task uses open-ended vague wording.
- Every code-changing task names exact files, symbols, commands, and expected results.

Type consistency:

- Cargo params live under `crate::cargo::params`.
- Cargo facade re-exports `CargoCommandKind`, `CargoInvocation`, `CargoRunOutput`, `CargoStatus`, `TruncatedText`, `run_cargo`, and `truncate_text`.
- Rust-analyzer params live under `crate::ra::params`.
- `src/tools.rs` remains a compatibility re-export module.
