use std::process::Command;
use std::process::ExitStatus;

use tempfile::TempDir;
use test_that::prelude::*;

#[test]
fn test_rmr_with_single_file_with_suffix_deletes_file() {
    let tmp_dir = TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("sample.txt");
    std::fs::write(&file_path, b"data").unwrap();
    assert_that!(file_path.is_file(), eq(true));

    let res = run_rmr(&[&format!("{}:12", file_path.display())]);

    assert_that!(res, ok(matches_pattern!(ExitStatus { code(): eq(Some(0)) })));
    assert_that!(file_path.exists(), eq(false));
}

#[test]
fn test_rmr_with_directory_with_suffix_deletes_directory_recursively() {
    let tmp_dir = TempDir::new().unwrap();
    let inner_dir = tmp_dir.path().join("inner");
    std::fs::create_dir(&inner_dir).unwrap();
    std::fs::write(inner_dir.join("file.txt"), b"x").unwrap();
    assert_that!(inner_dir.is_dir(), eq(true));

    let res = run_rmr(&[&format!("{}:99", inner_dir.display())]);

    assert_that!(res, ok(matches_pattern!(ExitStatus { code(): eq(Some(0)) })));
    assert_that!(inner_dir.exists(), eq(false));
}

#[test]
fn test_rmr_with_nonexistent_path_returns_error_exit_code() {
    let tmp_dir = TempDir::new().unwrap();
    let bogus = tmp_dir.path().join("nonexistent_target");
    assert_that!(bogus.exists(), eq(false));

    let res = run_rmr(&[&format!("{}:1:2", bogus.display())]);

    assert_that!(res, ok(matches_pattern!(ExitStatus { code(): eq(Some(1)) })));
}

fn run_rmr(args: &[&str]) -> std::io::Result<ExitStatus> {
    Command::new("cargo")
        .args(["run", "--quiet", "--bin", "rmr", "--"])
        .args(args)
        .status()
}
