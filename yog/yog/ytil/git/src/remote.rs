use std::str::FromStr;

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

pub fn get_https_urls(repo: &Repository) -> color_eyre::Result<Vec<RepoHttpsUrl>> {
    let mut urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        let remote = repo.find_remote(remote_name)?;
        let url = remote
            .url()
            .ok_or_else(|| eyre!("error getting remote URL | remote_name={remote_name:?}"))?;
        urls.push(RepoHttpsUrl::from_str(url)?);
    }
    Ok(urls)
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum RepoHttpsUrl {
    GitHub(String),
    GitLab(String),
}

impl RepoHttpsUrl {
    pub fn value(self) -> String {
        match self {
            Self::GitHub(value) | Self::GitLab(value) => value,
        }
    }
}

impl FromStr for RepoHttpsUrl {
    type Err = color_eyre::eyre::Error;

    fn from_str(url: &str) -> Result<Self, Self::Err> {
        let url = if url.starts_with("https://") {
            url.to_owned()
        } else if let Some(rest) = url
            .strip_prefix("ssh://")
            .and_then(|no_ssh| no_ssh.strip_prefix("git@"))
            .or_else(|| url.strip_prefix("git@"))
        {
            format!("https://{}", rest.replace(':', "/"))
        } else {
            bail!("error unsupported protocol for URL | url={url}")
        };

        Ok(if url.contains("github.com") {
            Self::GitHub(url)
        } else {
            Self::GitLab(url)
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("https://github.com/user/repo", RepoHttpsUrl::GitHub("https://github.com/user/repo".to_string()))]
    #[case("https://gitlab.com/user/repo", RepoHttpsUrl::GitLab("https://gitlab.com/user/repo".to_string()))]
    #[case("git@github.com:user/repo.git", RepoHttpsUrl::GitHub("https://github.com/user/repo.git".to_string()))]
    #[case("ssh://git@github.com/user/repo.git", RepoHttpsUrl::GitHub("https://github.com/user/repo.git".to_string()))]
    #[case("git@gitlab.com:user/repo.git", RepoHttpsUrl::GitLab("https://gitlab.com/user/repo.git".to_string()))]
    #[case("https://bitbucket.org/user/repo", RepoHttpsUrl::GitLab("https://bitbucket.org/user/repo".to_string()))]
    fn repo_https_url_from_str_when_valid_input_parses_successfully(
        #[case] input: &str,
        #[case] expected: RepoHttpsUrl,
    ) {
        let result = RepoHttpsUrl::from_str(input);
        assert2::let_assert!(Ok(actual) = result);
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("ftp://example.com/repo")]
    #[case("http://github.com/user/repo")]
    #[case("invalid")]
    #[case("")]
    fn repo_https_url_from_str_when_unsupported_protocol_returns_error(#[case] input: &str) {
        let result = RepoHttpsUrl::from_str(input);
        assert2::let_assert!(Err(err) = result);
        assert!(err.to_string().contains("error unsupported protocol for URL"));
    }
}
