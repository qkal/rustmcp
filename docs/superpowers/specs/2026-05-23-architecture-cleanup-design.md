# rust-analyzer-mcp Architecture Cleanup Design

## Goal

Clean up the internal architecture before the project grows, with three priorities:

- Make future `ra_*` navigation and introspection tools easier to add.
- Make cargo tool execution safer and easier to reason about.
- Split large files into clearer modules while preserving the product shape.

This is a refactor-first cleanup. Public behavior should remain sensible and stable, but exact internal names and some response details may change when doing so improves clarity.

## Current Pressure Points

The project is already a working v0.1 MCP server, but several files are carrying too much responsibility:

- `src/server.rs` mixes MCP routing, server state, rust-analyzer tool behavior, location normalization, diagnostics shaping, cargo orchestration, and response construction.
- `src/cargo.rs` mixes command construction, validation, process spawning, timeout cleanup, output collection, truncation, and metadata parsing.
- `src/tools.rs` mixes tool parameter structs, defaults, and response envelope helpers.

These files are still understandable, but future tools such as workspace symbols, implementations, inlay hints, macro expansion, rename preview, and call hierarchy would make the current shape harder to extend safely.

## Proposed Module Shape

Reorganize the crate around tool families and shared infrastructure:

```text
src/
  main.rs
  lib.rs
  server/
    mod.rs              # RaMcpServer, shared state, rmcp routing glue
    response.rs         # success/failure envelope, truncation rules
    state.rs            # ServerState, config, workspace/client lifecycle
  ra/
    mod.rs
    params.rs           # ra_* parameter structs and defaults
    tools.rs            # rmcp-facing ra_* handlers or handler helpers
    navigation.rs       # definition, references, implementations, call hierarchy later
    symbols.rs          # document symbols, workspace symbols later
    edits.rs            # format, code actions, rename preview later
    diagnostics.rs      # file/workspace diagnostics
    locate.rs           # URI classification + snippets for LSP locations
  cargo/
    mod.rs
    params.rs           # cargo_* parameter structs and defaults
    args.rs             # command construction and validation
    process.rs          # spawn, stdin nulling, timeout, cleanup, readers
    output.rs           # truncation, metadata parsing, response shaping
    tools.rs            # cargo_check/test/clippy/fmt/metadata orchestration
  lsp/
    ...
  workspace.rs
  error.rs
```

`server` should expose MCP tools and hold shared state. It should not own the details of LSP result normalization or cargo process cleanup.

`ra` should own rust-analyzer-backed tool behavior. Adding a navigation or introspection tool should usually require one focused handler and reuse of existing location, snippet, truncation, and result-normalization helpers.

`cargo` should make the safety-critical execution path explicit: structured params become a validated invocation, the process runner executes that invocation with bounded IO and timeout handling, and output shaping prepares the MCP response.

## `ra_*` Tool Flow

The rust-analyzer tool path should be:

```text
MCP params
  -> server route
  -> ra tool helper
  -> resolve workspace file / ensure rust-analyzer client
  -> lsp client request
  -> ra result normalizer
  -> response envelope
```

Navigation and introspection tools should share common result normalization:

- Classify LSP URIs as workspace files, external dependency source, or non-file URIs.
- Attach bounded snippets when requested and safe.
- Apply max result limits consistently.
- Report truncation consistently.
- Keep rust-analyzer-specific raw data separate from friendly summary fields when useful.

This structure should support the README phase-two tools, especially workspace symbols, implementations, call hierarchy, and other navigation/introspection features.

## `cargo_*` Tool Flow

The cargo tool path should be:

```text
MCP params
  -> server route
  -> cargo tool helper
  -> CargoInvocation from validated args
  -> process runner with timeout/output caps
  -> output shaper
  -> response envelope
```

The architecture should make it difficult to bypass validation. Tool handlers should not manually push raw args or spawn cargo. The public internal path should be structured params to `CargoInvocation` to `run_cargo`.

Cargo responsibilities should be split as follows:

- `cargo::params`: MCP-visible parameter types and cargo defaults.
- `cargo::args`: command construction and validation rules.
- `cargo::process`: process spawning, stdin isolation, timeout handling, process-tree cleanup, and bounded output readers.
- `cargo::output`: truncation helpers, metadata JSON parsing, notes, and response-facing output shaping.
- `cargo::tools`: orchestration for `cargo_check`, `cargo_test`, `cargo_clippy`, `cargo_fmt_check`, and `cargo_metadata`.

## Compatibility And Scope

The cleanup should keep the core product contract:

- Keep the crate as a stdio MCP server for rust-analyzer.
- Keep existing tool names unless there is a concrete reason to rename.
- Keep existing CLI flags: `--workspace`, `--disable-cargo-tools`, `--help`, and `--version`.
- Keep stdout reserved for MCP protocol messages only.
- Keep path containment and bounded-output safety.
- Keep cargo tools fixed and structured, not free-form shell or arbitrary cargo passthrough.
- Keep all important current behavior covered by tests, adjusting assertions only when a response-shape change is intentional.

The cleanup may:

- Move parameter structs out of `tools.rs`.
- Move response envelope helpers out of `tools.rs`.
- Rename internal types, functions, and modules freely.
- Split response shaping between shared envelope code and family-specific result structs.
- Add focused tests around new boundaries, especially cargo validation, cargo process behavior, cargo output shaping, and `ra` location normalization.

## Testing And Migration

The implementation should be sliced so the project stays green after each major move:

1. Move shared response envelope and parameter structs first, with no behavior change.
2. Split cargo into `params`, `args`, `process`, and `output`, keeping cargo tests mapped to the new modules.
3. Extract `ra` helpers for location/snippet normalization, diagnostics, symbols, edits, and navigation.
4. Shrink `server.rs` into MCP routing and state orchestration.
5. Run the same gates after each slice:
   - `cargo fmt --check`
   - `cargo test --locked --all`
   - `cargo clippy --locked --all-targets --all-features -- -D warnings`
6. Add or adjust tests when moved boundaries expose clearer unit seams.

The highest-risk areas are cargo timeout/process cleanup on Windows and MCP response compatibility. The implementation plan should treat those as regression-sensitive and keep existing integration tests intact until replacements prove equivalent.

## Non-Goals

- Do not add new MCP tools as part of this cleanup.
- Do not introduce a generic plugin or registry framework.
- Do not convert the rmcp macro routing model into a custom dispatcher.
- Do not add write/apply file-editing tools.
- Do not turn cargo tools into free-form command execution.
