use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct SetWorkspaceParams {
    pub workspace_path: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub struct PositionParams {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
}

