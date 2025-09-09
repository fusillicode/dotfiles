use std::path::Path;
use std::path::PathBuf;
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
pub fn get_repo_root(path: &Path) -> color_eyre::Result<PathBuf> {
    Ok(Repository::discover(path)?.commondir().to_path_buf())
}

/// Retrieves the name of the current Git branch.
///
/// # Errors
///
/// Returns an error if:
/// - Executing `git` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
pub fn get_current_branch() -> color_eyre::Result<String> {
    let repo_path = ".";
    let repo = Repository::open(repo_path)?;

    if repo.head_detached()? {
        bail!("detached head for git {repo_path}")
    }

    repo.head()?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| eyre!("shorthand is not valid UTF-8"))
}

/// Switches to the specified Git branch.
///
/// # Errors
///
/// Returns an error if:
/// - Executing `git` fails or returns a non-zero exit status.
pub fn switch_branch(branch_name: &str) -> color_eyre::Result<()> {
    // TODO: understand if there is a straightforward way with git2
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
