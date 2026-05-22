use std::{fs, path::Path};

use rust_analyzer_mcp::workspace::{LocationKind, Workspace};

#[test]
fn relative_path_inside_workspace_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let src = write_file(temp.path(), "src/lib.rs", "pub fn answer() -> i32 { 42 }\n");
    let workspace = Workspace::new(temp.path()).unwrap();

    let resolved = workspace.resolve_existing_file("src/lib.rs").unwrap();

    assert_eq!(resolved, src.canonicalize().unwrap());
}

#[test]
fn absolute_path_inside_workspace_succeeds() {
    let temp = tempfile::tempdir().unwrap();
    let src = write_file(temp.path(), "src/main.rs", "fn main() {}\n");
    let workspace = Workspace::new(temp.path()).unwrap();

    let resolved = workspace.resolve_existing_file(src.as_path()).unwrap();

    assert_eq!(resolved, src.canonicalize().unwrap());
}

#[test]
fn parent_escape_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let workspace = Workspace::new(temp.path()).unwrap();

    let err = workspace
        .resolve_existing_file(format!(
            "../{}",
            outside.path().file_name().unwrap().to_string_lossy()
        ))
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("outside workspace") || err.contains("does not exist"),
        "{err}"
    );
}

#[test]
fn missing_file_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(temp.path()).unwrap();

    let err = workspace
        .resolve_existing_file("src/missing.rs")
        .unwrap_err()
        .to_string();

    assert!(err.contains("does not exist"));
}

#[test]
fn file_uri_round_trips_to_workspace_classification() {
    let temp = tempfile::tempdir().unwrap();
    let src = write_file(temp.path(), "src/lib.rs", "");
    let workspace = Workspace::new(temp.path()).unwrap();
    let uri = workspace.uri_for_file(&src).unwrap();

    let classified = workspace.classify_url(&uri).unwrap();

    assert_eq!(classified.kind, LocationKind::Workspace);
    assert_eq!(classified.path.unwrap(), src.canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected_on_unix() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let link = temp.path().join("link.rs");
    symlink(outside.path(), &link).unwrap();
    let workspace = Workspace::new(temp.path()).unwrap();

    let err = workspace
        .resolve_existing_file("link.rs")
        .unwrap_err()
        .to_string();

    assert!(err.contains("outside workspace"));
}

fn write_file(root: &Path, path: &str, contents: &str) -> std::path::PathBuf {
    let full_path = root.join(path);
    fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    fs::write(&full_path, contents).unwrap();
    full_path
}
