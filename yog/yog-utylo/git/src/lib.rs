use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use cmd::CmdExt as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Repository;

/// Returns the absolute path to the root directory of the Git repository containing `path`.
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered starting from `path`.
/// - `path` is not inside a Git repository.
pub fn get_repo_root(path: &Path) -> color_eyre::Result<PathBuf> {
    Ok(Repository::discover(path)?.commondir().to_path_buf())
}

/// Returns the name of the current branch (e.g. `main`).
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - `HEAD` is detached.
/// - The branch name is not valid UTF-8.
pub fn get_current_branch() -> color_eyre::Result<String> {
    let repo_path = ".";
    let repo = Repository::discover(repo_path)?;

    if repo.head_detached()? {
        bail!("detached head for git {repo_path}")
    }

    repo.head()?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| eyre!("shorthand is not valid UTF-8"))
}

/// Creates a new local branch pointing to the current `HEAD` commit.
///
/// Does not switch to the new branch.
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - `HEAD` cannot be resolved to a commit.
/// - The branch already exists.
pub fn create_branch(branch_name: &str) -> color_eyre::Result<()> {
    let repo_path = ".";
    let repo = Repository::discover(repo_path)?;

    let commit = repo.head()?.peel_to_commit()?;
    repo.branch(branch_name, &commit, false)?;

    Ok(())
}

/// Switches `HEAD` to `branch_name` (or detaches if it resolves to a commit).
///
/// Special case: if `branch_name` is `-` it shell‑invokes `git switch -` to reuse Git's previous branch logic.
///
/// # Errors
///
/// Returns an error if:
/// - The repository cannot be discovered.
/// - Name resolution fails.
/// - Checkout fails.
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

pub fn get_remotes(repo_path: &Path) -> color_eyre::Result<Vec<String>> {
    let repo = Repository::discover(repo_path)?;
    Ok(repo.remotes()?.iter().flatten().map(str::to_owned).collect::<Vec<_>>())
}
