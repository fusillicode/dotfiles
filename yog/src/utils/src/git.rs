use std::path::Path;
use std::process::Command;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

use crate::cmd::CmdExt as _;

/// Finds the root directory of the Git repository containing the given file path, or the current directory if none
/// provided.
pub fn get_repo_root(file_path: Option<&Path>) -> color_eyre::Result<String> {
    let cmd = if let Some(file_path) = file_path {
        let file_parent_dir = file_path
            .parent()
            .ok_or_else(|| eyre!("cannot get parent dir from path {file_path:#?}"))?
            .to_str()
            .ok_or_else(|| eyre!("cannot get str from Path {file_path:#?}"))?;
        format!("-C {file_parent_dir}")
    } else {
        "".into()
    };

    // Without spawning an additional `sh` shell I get an empty `Command` output 🥲
    let git_repo_root = Command::new("sh")
        .args(["-c", &format!("git {cmd} rev-parse --show-toplevel")])
        .output()?
        .stdout;

    if git_repo_root.is_empty() {
        bail!("{file_path:#?} is not in a git repository");
    }

    Ok(String::from_utf8(git_repo_root)?.trim().to_owned())
}

/// Retrieves the name of the current Git branch.
pub fn get_current_branch() -> color_eyre::Result<String> {
    Ok(std::str::from_utf8(
        &Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .exec()?
            .stdout,
    )?
    .trim()
    .to_string())
}
