use std::path::Path;
use std::process::Command;

use cmd::CmdExt as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Repository;

/// Finds the root directory of the Git repository containing the given file path, or the current directory if none
/// provided.
///
/// # Errors
///
/// Returns an error if:
/// - Executing `sh` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - Running `git rev-parse --show-toplevel` fails.
/// - The given path is not inside a Git repository.
pub fn get_repo_root(file_path: Option<&Path>) -> color_eyre::Result<String> {
    let cmd = if let Some(file_path) = file_path {
        let file_parent_dir = file_path
            .parent()
            .ok_or_else(|| eyre!("cannot get parent dir from path {file_path:#?}"))?
            .to_str()
            .ok_or_else(|| eyre!("cannot get str from Path {file_path:#?}"))?;
        format!("-C {file_parent_dir}")
    } else {
        String::new()
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
///
/// # Errors
///
/// Returns an error if:
/// - Executing `git` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
pub fn get_current_branch() -> color_eyre::Result<String> {
    Ok(std::str::from_utf8(
        &Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .exec()?
            .stdout,
    )?
    .trim()
    .to_owned())
}

/// Switches to the specified Git branch.
///
/// # Errors
///
/// Returns an error if:
/// - Executing `git` fails or returns a non-zero exit status.
pub fn switch_branch(branch_name: &str) -> color_eyre::Result<()> {
    if branch_name == "-" {
        Command::new("git").args(["switch", branch_name]).exec()?;
        return Ok(());
    }

    let repo = Repository::discover(".")?;

    // Find the branch reference
    let (object, reference) = repo.revparse_ext(branch_name)?;

    // Checkout the branch head
    repo.checkout_tree(&object, None)?;

    // Set HEAD to point to the new branch reference
    match reference {
        Some(reference) => repo.set_head(reference.name().ok_or_else(|| eyre!("reference name is not UTF-8"))?)?,
        None => repo.set_head_detached(object.id())?,
    }

    Ok(())
}
