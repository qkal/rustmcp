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

The `rmcp` macro routing model is a real constraint. The `#[tool_router]`, `#[tool]`, and `#[tool_handler]` annotated methods should remain attached to `RaMcpServer` as thin wrappers unless the macro supports another layout cleanly. The family modules should provide helper functions and result builders; they do not need to own the macro annotations.

When converting `src/server.rs` into `src/server/mod.rs`, do it as an explicit file-to-directory move. The same applies to `src/cargo.rs` becoming `src/cargo/mod.rs`. Avoid a transient state where both `src/server.rs` and `src/server/mod.rs`, or both `src/cargo.rs` and `src/cargo/mod.rs`, define the same module.

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

The state/client access pattern should be explicit. Today each `ra_*` handler resolves paths, ensures the rust-analyzer client, and awaits LSP calls while holding the server state mutex. The refactor should centralize that pattern in a small helper so future tools do not each invent their own locking behavior. If the implementation changes lock duration or concurrency semantics, that change should be intentional and tested.

New `ra_*` tools should add typed methods to `lsp::client` instead of embedding raw JSON requests in server routes. The generic LSP request helpers should remain behind the LSP client boundary.

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

Keep a narrow cargo façade in `cargo::mod` for the pieces used by the rest of the crate and tests, such as `CargoCommandKind`, `CargoInvocation`, `CargoRunOutput`, and `run_cargo`. Internal helpers should be `pub(crate)` unless tests or callers need them directly.

The cargo run semaphore and the `--disable-cargo-tools` setting are server-level execution policy. The cargo modules can implement the command, process, and output behavior, but server or cargo tool orchestration should remain responsible for enforcing disabled-tool behavior and one-at-a-time cargo execution.

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

Additional regression checks should cover:

- `tools/list` still advertises the expected MVP tool names after wrapper extraction.
- `--disable-cargo-tools` still lists cargo tools but returns structured disabled failures.
- `cargo_metadata` still avoids duplicating parsed metadata JSON in raw `stdout`.
- Cargo timeout tests still prove inherited output pipes and descendant cleanup do not hang on Windows.
- At least one `ra_*` smoke test still exercises rust-analyzer through the MCP server, not only through helper units.

## Non-Goals

- Do not add new MCP tools as part of this cleanup.
- Do not introduce a generic plugin or registry framework.
- Do not convert the rmcp macro routing model into a custom dispatcher.
- Do not add write/apply file-editing tools.
- Do not turn cargo tools into free-form command execution.
