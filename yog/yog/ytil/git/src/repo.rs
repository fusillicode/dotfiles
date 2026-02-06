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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_when_path_is_inside_repo_returns_repo() {
        let (_temp_dir, repo) = crate::tests::init_test_repo(None);
        let workdir = repo.workdir().unwrap();
        assert2::let_assert!(Ok(_repo) = discover(workdir));
    }

    #[test]
    fn discover_when_path_is_not_a_repo_returns_error() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        assert2::let_assert!(Err(err) = discover(temp_dir.path()));
        assert!(err.to_string().contains("error discovering repo"));
    }

    #[test]
    fn get_root_returns_path_without_dot_git_suffix() {
        let (_temp_dir, repo) = crate::tests::init_test_repo(None);
        let root = get_root(&repo);
        pretty_assertions::assert_eq!(root.ends_with(".git"), false);
    }

    #[test]
    fn get_relative_path_to_repo_when_path_inside_repo_returns_rooted_relative() {
        let (_temp_dir, repo) = crate::tests::init_test_repo(None);
        let workdir = repo.workdir().unwrap();
        let file_path = workdir.join("src").join("main.rs");
        assert2::let_assert!(Ok(rel) = get_relative_path_to_repo(&file_path, &repo));
        pretty_assertions::assert_eq!(rel, PathBuf::from("/src/main.rs"));
    }

    #[test]
    fn get_relative_path_to_repo_when_path_outside_repo_returns_error() {
        let (_temp_dir, repo) = crate::tests::init_test_repo(None);
        let outside_path = Path::new("/completely/different/path");
        assert2::let_assert!(Err(_err) = get_relative_path_to_repo(outside_path, &repo));
    }
}
