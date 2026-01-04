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

/// Retrieves HTTPS URLs for all configured remotes in the repository.
///
/// # Errors
/// - If listing remotes fails.
/// - If finding a remote by name fails.
/// - If a remote has no URL configured.
/// - If URL has an unsupported protocol.
pub fn get_https_urls(repo: &Repository) -> color_eyre::Result<Vec<String>> {
    let mut https_urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        let remote = repo.find_remote(remote_name)?;
        let url = remote
            .url()
            .ok_or_else(|| eyre!("error invalid URL for remote | remote={remote_name:?}"))
            .and_then(map_to_https_url)?;
        https_urls.push(url);
    }
    Ok(https_urls)
}

/// Supported Git hosting providers.
pub enum GitProvider {
    /// GitHub.com or GitHub Enterprise.
    GitHub,
    /// GitLab.com or self-hosted GitLab.
    GitLab,
}

impl GitProvider {
    /// Detects the Git provider by inspecting HTTP response headers from the given URL.
    ///
    /// # Errors
    /// - If the HTTP request fails (network issues, invalid URL, etc.).
    ///
    /// # Rationale
    /// Header-based detection avoids parsing HTML content or relying on URL patterns,
    /// providing a more reliable and lightweight approach to provider identification.
    pub fn get(url: &str) -> color_eyre::Result<Option<Self>> {
        let resp = reqwest::blocking::get(url)?;

        let out = Ok(None);
        for (name, _) in resp.headers() {
            let name = name.as_str();
            if name.contains("gitlab") {
                return Ok(Some(Self::GitLab));
            }
            if name.contains("github") {
                return Ok(Some(Self::GitHub));
            }
        }

        out
    }
}

fn map_to_https_url(url: &str) -> color_eyre::Result<String> {
    if url.starts_with("https://") {
        return Ok(url.to_owned());
    }
    if let Some(rest) = url
        .strip_prefix("ssh://")
        .and_then(|no_ssh| no_ssh.strip_prefix("git@"))
        .or_else(|| url.strip_prefix("git@"))
    {
        return Ok(format!("https://{}", rest.replace(':', "/").trim_end_matches(".git")));
    }
    bail!("error unsupported protocol for URL | url={url}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("https://github.com/user/repo", "https://github.com/user/repo")]
    #[case("https://gitlab.com/user/repo", "https://gitlab.com/user/repo")]
    #[case("git@github.com:user/repo.git", "https://github.com/user/repo")]
    #[case("ssh://git@github.com/user/repo.git", "https://github.com/user/repo")]
    #[case("git@gitlab.com:user/repo.git", "https://gitlab.com/user/repo")]
    #[case("https://bitbucket.org/user/repo", "https://bitbucket.org/user/repo")]
    fn map_to_https_url_when_valid_input_maps_successfully(#[case] input: &str, #[case] expected: &str) {
        let result = map_to_https_url(input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("ftp://example.com/repo")]
    #[case("http://github.com/user/repo")]
    #[case("invalid")]
    #[case("")]
    fn map_to_https_url_when_unsupported_protocol_returns_error(#[case] input: &str) {
        let result = map_to_https_url(input);
        assert2::let_assert!(Err(err) = result);
        assert!(err.to_string().contains("error unsupported protocol for URL"));
    }
}
