use std::process::Command;
use std::process::ExitStatus;

use tempfile::TempDir;

#[test]
fn rmr_with_single_file_with_suffix_deletes_file() {
    let tmp_dir = TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("sample.txt");
    std::fs::write(&file_path, b"data").unwrap();
    assert!(file_path.is_file());

    let res = run_rmr(&[&format!("{}:12", file_path.display())]);

    assert2::let_assert!(Ok(status) = res);
    pretty_assertions::assert_eq!(status.code(), Some(0));
    assert!(!file_path.exists());
}

#[test]
fn rmr_with_directory_with_suffix_deletes_directory_recursively() {
    let tmp_dir = TempDir::new().unwrap();
    let inner_dir = tmp_dir.path().join("inner");
    std::fs::create_dir(&inner_dir).unwrap();
    std::fs::write(inner_dir.join("file.txt"), b"x").unwrap();
    assert!(inner_dir.is_dir());

    let res = run_rmr(&[&format!("{}:99", inner_dir.display())]);

    assert2::let_assert!(Ok(status) = res);
    pretty_assertions::assert_eq!(status.code(), Some(0));
    assert!(!inner_dir.exists());
}

#[test]
fn rmr_with_nonexistent_path_returns_error_exit_code() {
    let tmp_dir = TempDir::new().unwrap();
    let bogus = tmp_dir.path().join("nonexistent_target");
    assert!(!bogus.exists());

    let res = run_rmr(&[&format!("{}:1:2", bogus.display())]);

    assert2::let_assert!(Ok(status) = res);
    assert2::let_assert!(Some(actual_exit_code) = status.code());
    pretty_assertions::assert_eq!(actual_exit_code, 1);
}

fn run_rmr(args: &[&str]) -> std::io::Result<ExitStatus> {
    Command::new("cargo")
        .args(["run", "--quiet", "--bin", "rmr", "--"])
        .args(args)
        .status()
}
