use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use rust_analyzer_mcp::{
    cargo::{CargoCommandKind, CargoInvocation, run_cargo, truncate_text},
    tools::{CargoBuildParams, CargoFmtCheckParams, CargoMetadataParams, CargoTestParams},
};

fn build_params() -> CargoBuildParams {
    CargoBuildParams {
        workspace: Some(true),
        package: None,
        features: Some(vec!["serde".to_string(), "cli".to_string()]),
        all_features: Some(false),
        no_default_features: Some(true),
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        all_targets: Some(true),
        locked: Some(true),
        offline: Some(false),
        frozen: Some(false),
        timeout_ms: Some(30_000),
        max_stdout_bytes: Some(10_000),
        max_stderr_bytes: Some(11_000),
    }
}

#[test]
fn cargo_check_args_are_built_from_structured_options() {
    let invocation = CargoInvocation::new(CargoCommandKind::Check, &build_params()).unwrap();

    assert_eq!(invocation.command, "cargo");
    assert_eq!(
        invocation.args,
        vec![
            "check",
            "--workspace",
            "--features",
            "serde,cli",
            "--no-default-features",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--all-targets",
            "--locked",
        ]
    );
    assert_eq!(invocation.timeout_ms, 30_000);
    assert_eq!(invocation.max_stdout_bytes, 10_000);
    assert_eq!(invocation.max_stderr_bytes, 11_000);
}

#[test]
fn cargo_clippy_args_are_built_from_build_params() {
    let params = CargoBuildParams {
        all_targets: Some(true),
        all_features: Some(true),
        ..CargoBuildParams::default()
    };

    let invocation = CargoInvocation::new(CargoCommandKind::Clippy, &params).unwrap();

    assert_eq!(
        invocation.args,
        vec!["clippy", "--all-features", "--all-targets"]
    );
}

#[test]
fn cargo_test_places_filter_before_test_binary_args() {
    let params = CargoTestParams {
        workspace: None,
        package: Some("rust-analyzer-mcp".to_string()),
        features: None,
        all_features: None,
        no_default_features: None,
        target: None,
        all_targets: None,
        locked: None,
        offline: None,
        frozen: None,
        timeout_ms: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
        test_filter: Some("workspace".to_string()),
        nocapture: Some(true),
    };

    let invocation = CargoInvocation::new(CargoCommandKind::Test, &params).unwrap();

    assert_eq!(
        invocation.args,
        vec![
            "test",
            "-p",
            "rust-analyzer-mcp",
            "workspace",
            "--",
            "--nocapture"
        ]
    );
}

#[test]
fn cargo_fmt_check_uses_fmt_specific_options() {
    let params = CargoFmtCheckParams {
        package: Some("rust-analyzer-mcp".to_string()),
        all: Some(false),
        timeout_ms: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
    };

    let invocation = CargoInvocation::new(CargoCommandKind::FmtCheck, &params).unwrap();

    assert_eq!(
        invocation.args,
        vec!["fmt", "--check", "-p", "rust-analyzer-mcp"]
    );
}

#[test]
fn cargo_metadata_args_include_format_version_and_metadata_flags() {
    let params = CargoMetadataParams {
        features: Some(vec!["server".to_string()]),
        all_features: None,
        no_default_features: Some(true),
        filter_platform: Some("x86_64-pc-windows-msvc".to_string()),
        no_deps: Some(true),
        locked: Some(true),
        offline: None,
        frozen: None,
        timeout_ms: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
    };

    let invocation = CargoInvocation::new(CargoCommandKind::Metadata, &params).unwrap();

    assert_eq!(
        invocation.args,
        vec![
            "metadata",
            "--format-version",
            "1",
            "--features",
            "server",
            "--no-default-features",
            "--filter-platform",
            "x86_64-pc-windows-msvc",
            "--no-deps",
            "--locked",
        ]
    );
}

#[test]
fn rejects_conflicting_workspace_and_package() {
    let params = CargoBuildParams {
        workspace: Some(true),
        package: Some("rust-analyzer-mcp".to_string()),
        ..CargoBuildParams::default()
    };

    let error = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap_err();

    assert!(error.to_string().contains("workspace"));
    assert!(error.to_string().contains("package"));
}

#[test]
fn rejects_option_like_user_values() {
    let params = CargoBuildParams {
        package: Some("--all".to_string()),
        ..CargoBuildParams::default()
    };

    let error = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap_err();

    assert!(error.to_string().contains("package"));
    assert!(error.to_string().contains("must not start with '-'"));
}

#[test]
fn rejects_feature_conflicts() {
    let params = CargoBuildParams {
        features: Some(vec!["serde".to_string()]),
        all_features: Some(true),
        ..CargoBuildParams::default()
    };

    let error = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap_err();

    assert!(error.to_string().contains("all_features"));
    assert!(error.to_string().contains("features"));
}

#[test]
fn rejects_comma_separated_feature_values() {
    let params = CargoBuildParams {
        features: Some(vec!["serde,-Dwarnings".to_string()]),
        ..CargoBuildParams::default()
    };

    let error = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap_err();

    assert!(error.to_string().contains("features"));
    assert!(error.to_string().contains("','"));
}

#[test]
fn build_like_commands_default_to_sixty_kib_output_caps() {
    let invocation =
        CargoInvocation::new(CargoCommandKind::Check, &CargoBuildParams::default()).unwrap();

    assert_eq!(invocation.max_stdout_bytes, 60_000);
    assert_eq!(invocation.max_stderr_bytes, 60_000);
}

#[test]
fn cargo_metadata_defaults_to_larger_stdout_cap() {
    let invocation =
        CargoInvocation::new(CargoCommandKind::Metadata, &CargoMetadataParams::default()).unwrap();

    assert_eq!(invocation.max_stdout_bytes, 120_000);
    assert_eq!(invocation.max_stderr_bytes, 60_000);
}

#[test]
fn clamps_limits_to_hard_maximums() {
    let params = CargoBuildParams {
        timeout_ms: Some(999_999),
        max_stdout_bytes: Some(999_999),
        max_stderr_bytes: Some(999_999),
        ..CargoBuildParams::default()
    };

    let invocation = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap();

    assert_eq!(invocation.timeout_ms, 600_000);
    assert_eq!(invocation.max_stdout_bytes, 240_000);
    assert_eq!(invocation.max_stderr_bytes, 240_000);
}

#[test]
fn truncate_text_reports_when_bytes_are_limited() {
    let truncated = truncate_text(b"abcdef", 3);

    assert_eq!(truncated.text, "abc");
    assert!(truncated.truncated);
}

#[test]
fn truncate_text_preserves_utf8_boundaries() {
    let truncated = truncate_text("aéz".as_bytes(), 2);

    assert_eq!(truncated.text, "a");
    assert!(truncated.truncated);
}

#[test]
fn truncate_text_preserves_invalid_bytes_before_cut() {
    let truncated = truncate_text(&[b'a', 0xff, b'b', 0xe2, 0x82, 0xac, b'z'], 5);

    assert_eq!(truncated.text, "a\u{fffd}b");
    assert!(truncated.truncated);
}

#[tokio::test]
async fn cargo_check_runs_in_workspace_root() {
    let temp = tempfile::tempdir().unwrap();
    write_crate(temp.path());
    let invocation =
        CargoInvocation::new(CargoCommandKind::Check, &CargoBuildParams::default()).unwrap();

    let output = run_cargo(temp.path(), invocation).await.unwrap();

    assert_eq!(output.command, "cargo");
    assert!(output.status.success, "stderr was: {}", output.stderr);
    assert!(!output.timed_out);
}

#[test]
fn cargo_process_stdin_is_null() {
    let exe = std::env::current_exe().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("stdin.txt");
    let mut child = Command::new(exe)
        .args([
            "--exact",
            "run_cargo_stdin_is_null_probe",
            "--ignored",
            "--nocapture",
        ])
        .env("CARGO_MCP_STDIN_MARKER", &marker)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"mcp protocol bytes\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(marker).unwrap(), "");
}

#[tokio::test]
#[ignore]
async fn run_cargo_stdin_is_null_probe() {
    let Some(marker) = std::env::var_os("CARGO_MCP_STDIN_MARKER") else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    write_binary_crate(
        temp.path(),
        r#"
use std::{
    env,
    fs,
    io::{self, Read},
    path::PathBuf,
};

fn main() {
    let marker = PathBuf::from(env::args().nth(1).unwrap());
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin).unwrap();
    fs::write(marker, stdin).unwrap();
}
"#,
    );

    let output = run_cargo(
        temp.path(),
        CargoInvocation {
            command: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "--quiet".to_string(),
                "--".to_string(),
                marker.to_string_lossy().into_owned(),
            ],
            timeout_ms: 30_000,
            max_stdout_bytes: 1_024,
            max_stderr_bytes: 1_024,
            parse_metadata_json: false,
        },
    )
    .await
    .unwrap();

    assert!(output.status.success, "stderr was: {}", output.stderr);
}

#[tokio::test]
async fn cargo_metadata_parses_json_result() {
    let temp = tempfile::tempdir().unwrap();
    write_crate(temp.path());
    let invocation =
        CargoInvocation::new(CargoCommandKind::Metadata, &CargoMetadataParams::default()).unwrap();

    let output = run_cargo(temp.path(), invocation).await.unwrap();

    assert!(output.status.success, "stderr was: {}", output.stderr);
    assert_eq!(
        output.metadata_json.as_ref().unwrap()["packages"][0]["name"],
        "cargo_mcp_temp"
    );
}

#[tokio::test]
async fn cargo_timeout_returns_even_when_build_script_child_keeps_pipes_open() {
    let temp = tempfile::tempdir().unwrap();
    let child_pid_path = temp.path().join("child.pid");
    write_binary_crate(
        temp.path(),
        r#"
use std::{
    env,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn main() {
    let marker = PathBuf::from(env::args().nth(1).unwrap());
    if env::var_os("CARGO_MCP_PIPE_CHILD").is_some() {
        println!("child started");
        thread::sleep(Duration::from_secs(6));
        return;
    }

    let exe = env::current_exe().unwrap();
    let child = Command::new(exe)
        .arg(&marker)
        .env("CARGO_MCP_PIPE_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    fs::write(&marker, child.id().to_string()).unwrap();

    println!("spawned pipe-holding child");
    eprintln!("parent keeps stderr open too");
    thread::sleep(Duration::from_secs(6));
}
"#,
    );

    let build_output = run_cargo(
        temp.path(),
        CargoInvocation {
            command: "cargo".to_string(),
            args: vec!["build".to_string(), "--quiet".to_string()],
            timeout_ms: 60_000,
            max_stdout_bytes: 1_024,
            max_stderr_bytes: 1_024,
            parse_metadata_json: false,
        },
    )
    .await
    .unwrap();
    assert!(
        build_output.status.success,
        "stderr was: {}",
        build_output.stderr
    );

    let invocation = CargoInvocation {
        command: "cargo".to_string(),
        args: vec![
            "run".to_string(),
            "--quiet".to_string(),
            "--".to_string(),
            child_pid_path.to_string_lossy().into_owned(),
        ],
        timeout_ms: 500,
        max_stdout_bytes: 1_024,
        max_stderr_bytes: 1_024,
        parse_metadata_json: false,
    };

    let started = Instant::now();
    let output = tokio::time::timeout(Duration::from_secs(3), run_cargo(temp.path(), invocation))
        .await
        .expect("run_cargo should not wait indefinitely for inherited output pipes")
        .unwrap();

    assert!(output.timed_out);
    assert!(!output.status.success);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "timeout result took {:?}",
        started.elapsed()
    );
    assert!(
        output
            .notes
            .iter()
            .any(|note| note.contains("output collection stopped")),
        "notes were: {:?}",
        output.notes
    );
    assert!(
        output
            .notes
            .iter()
            .any(|note| note.contains("process tree cleanup")),
        "notes were: {:?}",
        output.notes
    );

    wait_for_pid_file(&child_pid_path, Duration::from_secs(2)).await;
    let child_pid = read_pid_file(&child_pid_path);
    wait_for_process_exit(child_pid, Duration::from_secs(3)).await;
}

#[tokio::test]
async fn cargo_output_collection_times_out_after_cargo_exits() {
    let temp = tempfile::tempdir().unwrap();
    let child_pid_path = temp.path().join("child.pid");
    write_binary_crate(
        temp.path(),
        r#"
use std::{
    env,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn main() {
    let marker = PathBuf::from(env::args().nth(1).unwrap());
    if env::var_os("CARGO_MCP_PIPE_CHILD").is_some() {
        println!("child keeps stdout open");
        thread::sleep(Duration::from_secs(2));
        return;
    }

    let exe = env::current_exe().unwrap();
    let child = Command::new(exe)
        .arg(&marker)
        .env("CARGO_MCP_PIPE_CHILD", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    fs::write(&marker, child.id().to_string()).unwrap();
    println!("parent exits while child keeps pipes open");
}
"#,
    );

    let build_output = run_cargo(
        temp.path(),
        CargoInvocation {
            command: "cargo".to_string(),
            args: vec!["build".to_string(), "--quiet".to_string()],
            timeout_ms: 60_000,
            max_stdout_bytes: 1_024,
            max_stderr_bytes: 1_024,
            parse_metadata_json: false,
        },
    )
    .await
    .unwrap();
    assert!(
        build_output.status.success,
        "stderr was: {}",
        build_output.stderr
    );

    let invocation = CargoInvocation {
        command: "cargo".to_string(),
        args: vec![
            "run".to_string(),
            "--quiet".to_string(),
            "--".to_string(),
            child_pid_path.to_string_lossy().into_owned(),
        ],
        timeout_ms: 500,
        max_stdout_bytes: 1_024,
        max_stderr_bytes: 1_024,
        parse_metadata_json: false,
    };

    let started = Instant::now();
    let output = tokio::time::timeout(Duration::from_secs(1), run_cargo(temp.path(), invocation))
        .await
        .expect("run_cargo should bound output collection after cargo exits")
        .unwrap();

    assert!(output.status.success);
    assert!(output.timed_out);
    assert!(output.stdout_truncated);
    assert!(output.stderr_truncated);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "output collection took {:?}",
        started.elapsed()
    );
    assert!(
        output
            .notes
            .iter()
            .any(|note| note.contains("output collection stopped")),
        "notes were: {:?}",
        output.notes
    );
    assert!(
        output
            .notes
            .iter()
            .any(|note| note.contains("process tree cleanup")),
        "notes were: {:?}",
        output.notes
    );

    wait_for_pid_file(&child_pid_path, Duration::from_secs(2)).await;
    let child_pid = read_pid_file(&child_pid_path);
    wait_for_process_exit(child_pid, Duration::from_secs(3)).await;
}

#[tokio::test]
async fn cargo_run_applies_stdout_and_stderr_caps() {
    let temp = tempfile::tempdir().unwrap();
    write_crate_with_build_script(
        temp.path(),
        r#"
fn main() {
    for index in 0..40 {
        println!("cargo:warning=stderr-line-{index}-abcdefghijklmnopqrstuvwxyz");
    }
}
"#,
    );

    let mut metadata_invocation =
        CargoInvocation::new(CargoCommandKind::Metadata, &CargoMetadataParams::default()).unwrap();
    metadata_invocation.max_stdout_bytes = 32;

    let metadata_output = run_cargo(temp.path(), metadata_invocation).await.unwrap();

    assert!(metadata_output.status.success);
    assert!(metadata_output.stdout_truncated);
    assert!(metadata_output.stdout.len() <= 32);
    assert!(metadata_output.metadata_json.is_none());

    let params = CargoBuildParams {
        max_stderr_bytes: Some(128),
        ..CargoBuildParams::default()
    };
    let check_invocation = CargoInvocation::new(CargoCommandKind::Check, &params).unwrap();

    let check_output = run_cargo(temp.path(), check_invocation).await.unwrap();

    assert!(
        check_output.status.success,
        "stderr was: {}",
        check_output.stderr
    );
    assert!(check_output.stderr_truncated);
    assert!(check_output.stderr.len() <= 128);
}

fn write_crate(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cargo_mcp_temp\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
}

fn write_crate_with_build_script(root: &Path, build_script: &str) {
    write_crate(root);
    fs::write(root.join("build.rs"), build_script).unwrap();
}

fn write_binary_crate(root: &Path, main_rs: &str) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cargo_mcp_temp\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), main_rs).unwrap();
}

fn read_pid_file(path: &Path) -> u32 {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read child pid file {}: {error}", path.display()))
        .trim()
        .parse()
        .unwrap()
}

async fn wait_for_pid_file(path: &Path, timeout: Duration) {
    let started = Instant::now();
    loop {
        if fs::read_to_string(path).is_ok_and(|content| !content.trim().is_empty()) {
            return;
        }

        assert!(
            started.elapsed() < timeout,
            "child pid file {} did not appear within {:?}",
            path.display(),
            timeout
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_process_exit(pid: u32, timeout: Duration) {
    let started = Instant::now();
    loop {
        if !process_is_alive(pid) {
            return;
        }

        assert!(
            started.elapsed() < timeout,
            "process {pid} did not exit within {:?}",
            timeout
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.split(',').nth(1) == Some(&format!("\"{pid}\"")))
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}
