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
        .stdin(Stdio::null())
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
                cleanup_process_tree(child_pid, &mut child, &mut notes).await;
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

    let mut text = crate::cargo::output::truncate_text(&retained, max_bytes);
    text.truncated = total_bytes > max_bytes;
    Ok(text)
}
