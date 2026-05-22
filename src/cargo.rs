use std::{
    path::Path,
    process::{Command as StdCommand, Stdio},
    time::{Duration, Instant},
};

use serde::Serialize;
use thiserror::Error;
use tokio::{io::AsyncReadExt, process::Command as TokioCommand, task::JoinHandle};

use crate::error::{RaMcpError, Result as CrateResult};
use crate::tools::{
    CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams,
    DEFAULT_CARGO_METADATA_STDOUT_BYTES, DEFAULT_CARGO_STDERR_BYTES, DEFAULT_CARGO_STDOUT_BYTES,
    DEFAULT_CARGO_TIMEOUT_MS, MAX_CARGO_OUTPUT_BYTES, MAX_CARGO_TIMEOUT_MS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoCommandKind {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TruncatedText {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CargoStatus {
    pub code: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CargoRunOutput {
    pub command: String,
    pub args: Vec<String>,
    pub status: CargoStatus,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub notes: Vec<String>,
    pub metadata_json: Option<serde_json::Value>,
}

pub fn truncate_text(bytes: &[u8], max_bytes: usize) -> TruncatedText {
    if bytes.len() <= max_bytes {
        return TruncatedText {
            text: String::from_utf8_lossy(bytes).into_owned(),
            truncated: false,
        };
    }

    let mut end = max_bytes.min(bytes.len());
    while end > 0 && end < bytes.len() && is_utf8_continuation(bytes[end]) {
        end -= 1;
    }

    TruncatedText {
        text: String::from_utf8_lossy(&bytes[..end]).into_owned(),
        truncated: true,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

pub async fn run_cargo(
    workspace_root: &Path,
    invocation: CargoInvocation,
) -> CrateResult<CargoRunOutput> {
    if which::which(&invocation.command).is_err() {
        return Err(RaMcpError::CargoMissing);
    }

    let started = Instant::now();
    let mut command = StdCommand::new(&invocation.command);
    command
        .args(&invocation.args)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_tree_root(&mut command);

    let mut child = TokioCommand::from(command)
        .spawn()
        .map_err(|error| RaMcpError::CargoExecution(error.to_string()))?;
    let child_pid = child.id();

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RaMcpError::CargoExecution("failed to capture cargo stdout".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RaMcpError::CargoExecution("failed to capture cargo stderr".to_string()))?;
    let stdout_task = tokio::spawn(read_limited(stdout, invocation.max_stdout_bytes));
    let stderr_task = tokio::spawn(read_limited(stderr, invocation.max_stderr_bytes));

    let mut notes = Vec::new();
    let timeout = Duration::from_millis(invocation.timeout_ms);
    let (status, mut timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => (
            CargoStatus {
                code: status.code(),
                success: status.success(),
            },
            false,
        ),
        Ok(Err(error)) => return Err(RaMcpError::CargoExecution(error.to_string())),
        Err(_) => {
            notes.push(format!(
                "cargo timed out after {} ms",
                invocation.timeout_ms
            ));
            cleanup_process_tree(child_pid, &mut child, &mut notes).await;
            (
                CargoStatus {
                    code: None,
                    success: false,
                },
                true,
            )
        }
    };

    let (stdout, stderr) = if timed_out {
        stdout_task.abort();
        stderr_task.abort();
        notes.push("cargo output collection stopped after timeout".to_string());
        truncated_output_pair()
    } else {
        let remaining_timeout = timeout
            .saturating_sub(started.elapsed())
            .max(Duration::from_millis(100));
        match collect_task_outputs(stdout_task, stderr_task, remaining_timeout).await? {
            OutputCollection::Complete(stdout, stderr) => (stdout, stderr),
            OutputCollection::TimedOut => {
                timed_out = true;
                notes.push("cargo output collection stopped after timeout".to_string());
                truncated_output_pair()
            }
        }
    };
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let metadata_json = metadata_json(&invocation, &status, &stdout, &mut notes);

    Ok(CargoRunOutput {
        command: invocation.command,
        args: invocation.args,
        status,
        duration_ms,
        stdout: stdout.text,
        stderr: stderr.text,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
        timed_out,
        notes,
        metadata_json,
    })
}

enum OutputCollection {
    Complete(TruncatedText, TruncatedText),
    TimedOut,
}

async fn collect_task_outputs(
    mut stdout_task: JoinHandle<std::io::Result<TruncatedText>>,
    mut stderr_task: JoinHandle<std::io::Result<TruncatedText>>,
    timeout: Duration,
) -> CrateResult<OutputCollection> {
    match tokio::time::timeout(timeout, async {
        let stdout = task_output(&mut stdout_task, "stdout").await?;
        let stderr = task_output(&mut stderr_task, "stderr").await?;
        Ok::<_, RaMcpError>((stdout, stderr))
    })
    .await
    {
        Ok(output) => output.map(|(stdout, stderr)| OutputCollection::Complete(stdout, stderr)),
        Err(_) => {
            stdout_task.abort();
            stderr_task.abort();
            Ok(OutputCollection::TimedOut)
        }
    }
}

fn truncated_output_pair() -> (TruncatedText, TruncatedText) {
    (
        TruncatedText {
            text: String::new(),
            truncated: true,
        },
        TruncatedText {
            text: String::new(),
            truncated: true,
        },
    )
}

#[cfg(unix)]
fn configure_process_tree_root(command: &mut StdCommand) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_tree_root(_command: &mut StdCommand) {}

#[cfg(windows)]
async fn cleanup_process_tree(
    child_pid: Option<u32>,
    child: &mut tokio::process::Child,
    notes: &mut Vec<String>,
) {
    let Some(pid) = child_pid else {
        kill_child_fallback(child, notes).await;
        return;
    };

    match TokioCommand::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            notes.push(format!(
                "cargo process tree cleanup requested with taskkill for PID {pid}"
            ));
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            notes.push(format!(
                "cargo process tree cleanup failed for PID {pid}: taskkill exited with {}; {}",
                output.status,
                stderr.trim()
            ));
            kill_child_fallback(child, notes).await;
        }
        Err(error) => {
            notes.push(format!(
                "cargo process tree cleanup failed for PID {pid}: {error}"
            ));
            kill_child_fallback(child, notes).await;
        }
    }

    reap_child_after_cleanup(child, notes).await;
}

#[cfg(unix)]
async fn cleanup_process_tree(
    child_pid: Option<u32>,
    child: &mut tokio::process::Child,
    notes: &mut Vec<String>,
) {
    let Some(pid) = child_pid else {
        kill_child_fallback(child, notes).await;
        return;
    };

    let Ok(pid) = libc::pid_t::try_from(pid) else {
        notes.push(format!(
            "cargo process tree cleanup skipped because PID {pid} does not fit pid_t"
        ));
        kill_child_fallback(child, notes).await;
        reap_child_after_cleanup(child, notes).await;
        return;
    };
    if pid == 0 {
        notes.push("cargo process tree cleanup skipped because cargo PID was zero".to_string());
        kill_child_fallback(child, notes).await;
        reap_child_after_cleanup(child, notes).await;
        return;
    }

    let process_group = -pid;
    let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
    if result == 0 {
        notes.push(format!(
            "cargo process tree cleanup sent SIGKILL to process group {pid}"
        ));
    } else {
        notes.push(format!(
            "cargo process tree cleanup failed for process group {pid}: {}",
            std::io::Error::last_os_error()
        ));
        kill_child_fallback(child, notes).await;
    }

    reap_child_after_cleanup(child, notes).await;
}

#[cfg(not(any(unix, windows)))]
async fn cleanup_process_tree(
    _child_pid: Option<u32>,
    child: &mut tokio::process::Child,
    notes: &mut Vec<String>,
) {
    notes.push("cargo process tree cleanup is not supported on this platform".to_string());
    kill_child_fallback(child, notes).await;
    reap_child_after_cleanup(child, notes).await;
}

async fn kill_child_fallback(child: &mut tokio::process::Child, notes: &mut Vec<String>) {
    match child.kill().await {
        Ok(()) => notes.push("cargo top-level process killed after cleanup fallback".to_string()),
        Err(error) => notes.push(format!("failed to kill timed out cargo process: {error}")),
    }
}

async fn reap_child_after_cleanup(child: &mut tokio::process::Child, notes: &mut Vec<String>) {
    match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => notes.push(format!("failed to reap timed out cargo process: {error}")),
        Err(_) => notes.push("timed out while reaping cargo process after cleanup".to_string()),
    }
}

async fn task_output(
    task: &mut JoinHandle<std::io::Result<TruncatedText>>,
    stream_name: &str,
) -> CrateResult<TruncatedText> {
    task.await
        .map_err(|error| {
            RaMcpError::CargoExecution(format!(
                "failed to join cargo {stream_name} reader: {error}"
            ))
        })?
        .map_err(|error| {
            RaMcpError::CargoExecution(format!("failed to read cargo {stream_name}: {error}"))
        })
}

async fn read_limited<R>(mut reader: R, max_bytes: usize) -> std::io::Result<TruncatedText>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut retained = Vec::with_capacity(max_bytes);
    let mut buffer = [0; 8192];
    let mut total_bytes = 0usize;

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }

        total_bytes = total_bytes.saturating_add(read);
        if retained.len() < max_bytes {
            let remaining = max_bytes - retained.len();
            retained.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }

    let mut text = truncate_text(&retained, max_bytes);
    text.truncated = total_bytes > max_bytes;
    Ok(text)
}

fn metadata_json(
    invocation: &CargoInvocation,
    status: &CargoStatus,
    stdout: &TruncatedText,
    notes: &mut Vec<String>,
) -> Option<serde_json::Value> {
    if !invocation.parse_metadata_json || !status.success || stdout.truncated {
        return None;
    }

    match serde_json::from_str(&stdout.text) {
        Ok(value) => Some(value),
        Err(error) => {
            notes.push(format!("failed to parse cargo metadata JSON: {error}"));
            None
        }
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
