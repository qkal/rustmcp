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
    error::{RaMcpError, hint_for_error},
    server::response::{failure, success},
};

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
{
    if !cargo_tools_enabled {
        return failure(
            tool,
            workspace_root,
            &params,
            "cargo tools are disabled",
            "Restart without --disable-cargo-tools to enable cargo tool execution.",
        );
    }

    let invocation = match CargoInvocation::new(kind, &params) {
        Ok(invocation) => invocation,
        Err(error) => {
            let error = RaMcpError::CargoValidation(error.to_string());
            return failure(
                tool,
                workspace_root,
                &params,
                error.to_string(),
                hint_for_error(&error),
            );
        }
    };

    let _permit = cargo_run_lock
        .acquire()
        .await
        .expect("cargo semaphore is never closed");
    let mut output = match run_cargo(&workspace_path, invocation).await {
        Ok(output) => output,
        Err(error) => {
            return failure(
                tool,
                workspace_root,
                &params,
                error.to_string(),
                hint_for_error(&error),
            );
        }
    };

    let mut notes = cargo_notes();
    notes.extend(output.notes.iter().cloned());
    prepare_cargo_output_for_response(kind, &mut output, &mut notes);
    output.notes.clear();
    let truncated = output.stdout_truncated || output.stderr_truncated;
    success(
        tool,
        workspace_root,
        &params,
        json!(output),
        notes,
        truncated,
    )
}

fn prepare_cargo_output_for_response(
    kind: CargoCommandKind,
    output: &mut CargoRunOutput,
    notes: &mut Vec<String>,
) {
    if kind == CargoCommandKind::Metadata
        && output.metadata_json.is_some()
        && !output.stdout.is_empty()
    {
        output.stdout.clear();
        notes.push(
            "Raw cargo metadata stdout omitted from MCP response; use metadata_json.".to_string(),
        );
    }
}

fn cargo_notes() -> Vec<String> {
    vec![
        "cargo tools execute fixed cargo commands in the active workspace; cargo may run workspace code, build scripts, proc macros, and tests with arbitrary project-defined side effects, write artifacts under target/, and update Cargo.lock unless locked or frozen is used.".to_string(),
    ]
}
