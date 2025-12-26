use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Reference;
use git2::Repository;

/// Retrieves the default remote HEAD reference from the repository.
///
/// Iterates over all configured remotes and returns the first valid
/// `refs/remotes/{remote}/HEAD` reference, which typically points to the
/// default branch (e.g., main, or master) on that remote.
///
/// # Arguments
/// - `repo` The repository to query for remotes.
///
/// # Returns
/// The default remote HEAD reference.
///
/// # Errors
/// - If no remote has a valid `HEAD` reference.
pub fn get_default(repo: &Repository) -> color_eyre::Result<Reference<'_>> {
    for remote_name in repo.remotes()?.iter().flatten() {
        if let Ok(default_remote_ref) = repo.find_reference(&format!("refs/remotes/{remote_name}/HEAD")) {
            return Ok(default_remote_ref);
        }
    }
    bail!("error missing default remote")
}

pub fn get_all_urls(repo: &Repository) -> color_eyre::Result<Vec<String>> {
    let mut urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        let remote = repo.find_remote(remote_name)?;
        urls.push(
            remote
                .url()
                .ok_or_else(|| eyre!("error getting remote URL | remote_name={remote_name:?}"))?
                .to_owned(),
        )
    }
    Ok(urls)
}
