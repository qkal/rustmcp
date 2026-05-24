use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CARGO_TIMEOUT_MS: u64 = 120_000;
pub const MAX_CARGO_TIMEOUT_MS: u64 = 600_000;
pub const DEFAULT_CARGO_STDOUT_BYTES: usize = 60_000;
pub const DEFAULT_CARGO_STDERR_BYTES: usize = 60_000;
pub const DEFAULT_CARGO_METADATA_STDOUT_BYTES: usize = 120_000;
pub const MAX_CARGO_OUTPUT_BYTES: usize = 240_000;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema, Serialize)]
pub struct CargoBuildParams {
    pub workspace: Option<bool>,
    pub package: Option<String>,
    pub features: Option<Vec<String>>,
    pub all_features: Option<bool>,
    pub no_default_features: Option<bool>,
    pub target: Option<String>,
    pub all_targets: Option<bool>,
    pub release: Option<bool>,
    pub locked: Option<bool>,
    pub offline: Option<bool>,
    pub frozen: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema, Serialize)]
pub struct CargoTestParams {
    pub workspace: Option<bool>,
    pub package: Option<String>,
    pub features: Option<Vec<String>>,
    pub all_features: Option<bool>,
    pub no_default_features: Option<bool>,
    pub target: Option<String>,
    pub all_targets: Option<bool>,
    pub locked: Option<bool>,
    pub offline: Option<bool>,
    pub frozen: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
    pub test_filter: Option<String>,
    pub nocapture: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema, Serialize)]
pub struct CargoFmtCheckParams {
    pub package: Option<String>,
    pub all: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema, Serialize)]
pub struct CargoMetadataParams {
    pub features: Option<Vec<String>>,
    pub all_features: Option<bool>,
    pub no_default_features: Option<bool>,
    pub filter_platform: Option<String>,
    pub no_deps: Option<bool>,
    pub locked: Option<bool>,
    pub offline: Option<bool>,
    pub frozen: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::{CargoBuildParams, DEFAULT_CARGO_TIMEOUT_MS};

    #[test]
    fn cargo_build_params_default_to_empty_options() {
        let params = CargoBuildParams::default();
        assert_eq!(params.workspace, None);
        assert_eq!(params.package, None);
        assert_eq!(params.release, None);
        assert_eq!(params.timeout_ms, None);
    }

    #[test]
    fn default_cargo_timeout_is_two_minutes() {
        assert_eq!(DEFAULT_CARGO_TIMEOUT_MS, 120_000);
    }
}
