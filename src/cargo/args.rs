use thiserror::Error;

use crate::cargo::params::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
    DEFAULT_CARGO_METADATA_STDOUT_BYTES, DEFAULT_CARGO_STDERR_BYTES, DEFAULT_CARGO_STDOUT_BYTES,
    DEFAULT_CARGO_TIMEOUT_MS, MAX_CARGO_OUTPUT_BYTES, MAX_CARGO_TIMEOUT_MS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoCommandKind {
    Build,
    Check,
    Clippy,
    Test,
    FmtCheck,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoInvocation {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub parse_metadata_json: bool,
}

impl CargoInvocation {
    pub fn new<T: CargoArgs>(
        kind: CargoCommandKind,
        params: &T,
    ) -> Result<Self, CargoValidationError> {
        let mut args = params.args(kind)?;
        Ok(Self {
            command: "cargo".to_string(),
            args: {
                args.shrink_to_fit();
                args
            },
            timeout_ms: clamp_u64(
                params.timeout_ms(),
                DEFAULT_CARGO_TIMEOUT_MS,
                MAX_CARGO_TIMEOUT_MS,
            ),
            max_stdout_bytes: clamp_usize(
                params.max_stdout_bytes(),
                params.default_stdout_bytes(),
                MAX_CARGO_OUTPUT_BYTES,
            ),
            max_stderr_bytes: clamp_usize(
                params.max_stderr_bytes(),
                DEFAULT_CARGO_STDERR_BYTES,
                MAX_CARGO_OUTPUT_BYTES,
            ),
            parse_metadata_json: kind == CargoCommandKind::Metadata,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CargoValidationError {
    #[error("cargo option conflict: workspace and package cannot both be set")]
    WorkspacePackageConflict,
    #[error("cargo option conflict: all_features cannot be combined with features")]
    FeatureConflict,
    #[error("cargo option conflict: all_features cannot be combined with no_default_features")]
    AllFeaturesNoDefaultFeaturesConflict,
    #[error("cargo option conflict: all and package cannot both be set")]
    AllPackageConflict,
    #[error("cargo option {field} value must not be empty")]
    EmptyValue { field: &'static str },
    #[error("cargo option {field} value must not start with '-'")]
    OptionLikeValue { field: &'static str },
    #[error("cargo option features values must not contain ','")]
    CommaSeparatedFeature,
    #[error("cargo command kind {kind:?} is not supported by these params")]
    UnsupportedKind { kind: CargoCommandKind },
}

pub trait CargoArgs {
    fn args(&self, kind: CargoCommandKind) -> Result<Vec<String>, CargoValidationError>;
    fn timeout_ms(&self) -> Option<u64>;
    fn max_stdout_bytes(&self) -> Option<usize>;
    fn max_stderr_bytes(&self) -> Option<usize>;
    fn default_stdout_bytes(&self) -> usize {
        DEFAULT_CARGO_STDOUT_BYTES
    }
}

impl CargoArgs for CargoBuildParams {
    fn args(&self, kind: CargoCommandKind) -> Result<Vec<String>, CargoValidationError> {
        let command = match kind {
            CargoCommandKind::Build => "build",
            CargoCommandKind::Check => "check",
            CargoCommandKind::Clippy => "clippy",
            kind => return Err(CargoValidationError::UnsupportedKind { kind }),
        };

        validate_package_scope(self.workspace, self.package.as_deref())?;
        validate_feature_flags(
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;

        let mut args = vec![command.to_string()];
        push_package_scope(&mut args, self.workspace, self.package.as_deref())?;
        push_feature_flags(
            &mut args,
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;
        push_optional_value(&mut args, "--target", "target", self.target.as_deref())?;
        push_bool(&mut args, "--all-targets", self.all_targets);
        push_bool(&mut args, "--release", self.release);
        push_bool(&mut args, "--locked", self.locked);
        push_bool(&mut args, "--offline", self.offline);
        push_bool(&mut args, "--frozen", self.frozen);
        Ok(args)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    fn max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    fn max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }
}

impl CargoArgs for CargoTestParams {
    fn args(&self, kind: CargoCommandKind) -> Result<Vec<String>, CargoValidationError> {
        if kind != CargoCommandKind::Test {
            return Err(CargoValidationError::UnsupportedKind { kind });
        }

        validate_package_scope(self.workspace, self.package.as_deref())?;
        validate_feature_flags(
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;

        let mut args = vec!["test".to_string()];
        push_package_scope(&mut args, self.workspace, self.package.as_deref())?;
        push_feature_flags(
            &mut args,
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;
        push_optional_value(&mut args, "--target", "target", self.target.as_deref())?;
        push_bool(&mut args, "--all-targets", self.all_targets);
        push_bool(&mut args, "--locked", self.locked);
        push_bool(&mut args, "--offline", self.offline);
        push_bool(&mut args, "--frozen", self.frozen);
        push_positional(&mut args, "test_filter", self.test_filter.as_deref())?;
        if self.nocapture.unwrap_or(false) {
            args.push("--".to_string());
            args.push("--nocapture".to_string());
        }
        Ok(args)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    fn max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    fn max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }
}

impl CargoArgs for CargoFmtCheckParams {
    fn args(&self, kind: CargoCommandKind) -> Result<Vec<String>, CargoValidationError> {
        if kind != CargoCommandKind::FmtCheck {
            return Err(CargoValidationError::UnsupportedKind { kind });
        }
        if self.all.unwrap_or(false) && self.package.is_some() {
            return Err(CargoValidationError::AllPackageConflict);
        }

        let mut args = vec!["fmt".to_string(), "--check".to_string()];
        push_bool(&mut args, "--all", self.all);
        push_optional_value(&mut args, "-p", "package", self.package.as_deref())?;
        Ok(args)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    fn max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    fn max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }
}

impl CargoArgs for CargoMetadataParams {
    fn args(&self, kind: CargoCommandKind) -> Result<Vec<String>, CargoValidationError> {
        if kind != CargoCommandKind::Metadata {
            return Err(CargoValidationError::UnsupportedKind { kind });
        }
        validate_feature_flags(
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;

        let mut args = vec![
            "metadata".to_string(),
            "--format-version".to_string(),
            "1".to_string(),
        ];
        push_feature_flags(
            &mut args,
            self.features.as_deref(),
            self.all_features,
            self.no_default_features,
        )?;
        push_optional_value(
            &mut args,
            "--filter-platform",
            "filter_platform",
            self.filter_platform.as_deref(),
        )?;
        push_bool(&mut args, "--no-deps", self.no_deps);
        push_bool(&mut args, "--locked", self.locked);
        push_bool(&mut args, "--offline", self.offline);
        push_bool(&mut args, "--frozen", self.frozen);
        Ok(args)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    fn max_stdout_bytes(&self) -> Option<usize> {
        self.max_stdout_bytes
    }

    fn max_stderr_bytes(&self) -> Option<usize> {
        self.max_stderr_bytes
    }

    fn default_stdout_bytes(&self) -> usize {
        DEFAULT_CARGO_METADATA_STDOUT_BYTES
    }
}

fn validate_package_scope(
    workspace: Option<bool>,
    package: Option<&str>,
) -> Result<(), CargoValidationError> {
    if workspace.unwrap_or(false) && package.is_some() {
        return Err(CargoValidationError::WorkspacePackageConflict);
    }
    Ok(())
}

fn validate_feature_flags(
    features: Option<&[String]>,
    all_features: Option<bool>,
    no_default_features: Option<bool>,
) -> Result<(), CargoValidationError> {
    if all_features.unwrap_or(false) && features.is_some_and(|features| !features.is_empty()) {
        return Err(CargoValidationError::FeatureConflict);
    }
    if all_features.unwrap_or(false) && no_default_features.unwrap_or(false) {
        return Err(CargoValidationError::AllFeaturesNoDefaultFeaturesConflict);
    }
    if let Some(features) = features {
        for feature in features {
            validate_feature_value(feature)?;
        }
    }
    Ok(())
}

fn validate_feature_value(value: &str) -> Result<(), CargoValidationError> {
    validate_user_value("features", value)?;
    if value.contains(',') {
        return Err(CargoValidationError::CommaSeparatedFeature);
    }
    Ok(())
}

fn push_package_scope(
    args: &mut Vec<String>,
    workspace: Option<bool>,
    package: Option<&str>,
) -> Result<(), CargoValidationError> {
    push_bool(args, "--workspace", workspace);
    push_optional_value(args, "-p", "package", package)
}

fn push_feature_flags(
    args: &mut Vec<String>,
    features: Option<&[String]>,
    all_features: Option<bool>,
    no_default_features: Option<bool>,
) -> Result<(), CargoValidationError> {
    if let Some(features) = features.filter(|features| !features.is_empty()) {
        args.push("--features".to_string());
        args.push(features.join(","));
    }
    push_bool(args, "--all-features", all_features);
    push_bool(args, "--no-default-features", no_default_features);
    Ok(())
}

fn push_bool(args: &mut Vec<String>, flag: &str, value: Option<bool>) {
    if value.unwrap_or(false) {
        args.push(flag.to_string());
    }
}

fn push_optional_value(
    args: &mut Vec<String>,
    flag: &str,
    field: &'static str,
    value: Option<&str>,
) -> Result<(), CargoValidationError> {
    if let Some(value) = value {
        validate_user_value(field, value)?;
        args.push(flag.to_string());
        args.push(value.to_string());
    }
    Ok(())
}

fn push_positional(
    args: &mut Vec<String>,
    field: &'static str,
    value: Option<&str>,
) -> Result<(), CargoValidationError> {
    if let Some(value) = value {
        validate_user_value(field, value)?;
        args.push(value.to_string());
    }
    Ok(())
}

fn validate_user_value(field: &'static str, value: &str) -> Result<(), CargoValidationError> {
    if value.is_empty() {
        return Err(CargoValidationError::EmptyValue { field });
    }
    if value.starts_with('-') {
        return Err(CargoValidationError::OptionLikeValue { field });
    }
    Ok(())
}

fn clamp_u64(value: Option<u64>, default: u64, max: u64) -> u64 {
    value.unwrap_or(default).min(max)
}

fn clamp_usize(value: Option<usize>, default: usize, max: usize) -> usize {
    value.unwrap_or(default).min(max)
}
