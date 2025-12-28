use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::Context as _;
use color_eyre::eyre::ContextCompat as _;
use color_eyre::eyre::eyre;
use git2::Repository;

/// Discover the Git repository containing `path` by walking
/// parent directories upward until a repo root is found.
///
/// # Errors
/// - If the path is not inside a Git repository.
pub fn discover(path: &Path) -> color_eyre::Result<Repository> {
    Repository::discover(path).wrap_err_with(|| eyre!("error discovering repo | path={}", path.display()))
}

/// Absolute working tree root path for repository
///
/// Derived from [`Repository::commondir`] with any trailing `.git` removed (non‑bare repos).
/// Bare repositories return their directory path unchanged.
pub fn get_root(repo: &Repository) -> PathBuf {
    repo.commondir()
        .components()
        .filter(|c| c.as_os_str() != ".git")
        .collect()
}

/// Computes the relative path from the repository root to the given absolute path.
///
/// # Errors
/// - If the repository does not have a working directory (bare repository).
/// - If the provided path is not within the repository's working directory.
pub fn get_relative_path_to_repo(path: &Path, repo: &Repository) -> color_eyre::Result<PathBuf> {
    let repo_workdir = repo.workdir().wrap_err_with(|| {
        format!(
            "error getting repository working directory | repo={:?}",
            repo.path().display()
        )
    })?;
    Ok(Path::new("/").join(path.strip_prefix(repo_workdir)?))
}
