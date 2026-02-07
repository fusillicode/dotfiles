//! Lightweight GitHub helpers using the `gh` CLI.
#![feature(exit_status_error)]

use std::path::Path;
use std::process::Command;

use rootcause::prelude::ResultExt;
use rootcause::report;
use url::Url;

pub mod issue;
pub mod pr;

/// The GitHub host domain.
const GITHUB_HOST: &str = "github.com";
/// The URL path segment prefix for pull requests.
const GITHUB_PR_ID_PREFIX: &str = "pull";
/// The query parameter key used for pull request IDs in GitHub Actions URLs.
const GITHUB_PR_ID_QUERY_KEY: &str = "pr";

/// Repository fields available for querying via `gh repo view`.
#[derive(strum::AsRefStr, Debug)]
pub enum RepoViewField {
    #[strum(serialize = "nameWithOwner")]
    NameWithOwner,
    #[strum(serialize = "url")]
    Url,
}

impl RepoViewField {
    /// Returns the jq representation of the field for GitHub CLI queries.
    pub fn jq_repr(&self) -> String {
        format!(".{}", self.as_ref())
    }
}

/// Return the specified repository field via `gh repo view`.
///
/// Invokes: `gh repo view --json <field> --jq .<field>`.
///
/// # Errors
/// - Spawning or executing the `gh repo view` command fails.
/// - Command exits with non‑zero status.
/// - Output is not valid UTF‑8.
pub fn get_repo_view_field(field: &RepoViewField) -> rootcause::Result<String> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", field.as_ref(), "--jq", &field.jq_repr()])
        .output()
        .context("error getting repo view field")
        .attach_with(|| format!("field={field:?}"))?;

    ytil_cmd::extract_success_output(&output)
}

/// Ensures the user is authenticated with the GitHub CLI.
///
/// Runs `gh auth status`; if not authenticated it invokes an interactive `gh auth login`.
///
/// # Errors
/// - Checking auth status fails.
/// - The login command fails or exits with a non-zero status.
pub fn log_into_github() -> rootcause::Result<()> {
    if ytil_cmd::silent_cmd("gh")
        .args(["auth", "status"])
        .status()
        .context("error checking gh auth status")?
        .success()
    {
        return Ok(());
    }

    Ok(ytil_cmd::silent_cmd("sh")
        .args(["-c", "gh auth login"])
        .status()
        .context("error running gh auth login command")?
        .exit_ok()
        .context("error running gh auth login")?)
}

/// Retrieves the latest release tag name for the specified GitHub repository.
///
/// # Errors
/// - Executing `gh` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - Invoking `gh api` fails.
pub fn get_latest_release(repo: &str) -> rootcause::Result<String> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}/releases/latest"), "--jq=.tag_name"])
        .output()
        .context("error getting latest release")
        .attach_with(|| format!("repo={repo:?}"))?;

    ytil_cmd::extract_success_output(&output)
}

/// Extracts the branch name from a GitHub pull request [`Url`].
///
/// # Errors
/// - Executing `gh` fails or returns a non-zero exit status.
/// - Invoking `gh pr view` fails.
/// - Output cannot be parsed.
pub fn get_branch_name_from_url(url: &Url) -> rootcause::Result<String> {
    let pr_id = extract_pr_id_form_url(url)?;

    let output = Command::new("gh")
        .args(["pr", "view", &pr_id, "--json", "headRefName", "--jq", ".headRefName"])
        .output()
        .context("error getting branch name")
        .attach_with(|| format!("pr_id={pr_id:?}"))?;

    ytil_cmd::extract_success_output(&output)
}

/// Returns all GitHub remote URLs for the repository rooted at `repo_path`.
///
/// Filters remotes to those that parse as GitHub URLs.
///
/// # Errors
/// - The repository cannot be opened.
/// - A remote cannot be resolved.
/// - A remote URL is invalid UTF-8.
pub fn get_repo_urls(repo_path: &Path) -> rootcause::Result<Vec<Url>> {
    let repo = ytil_git::repo::discover(repo_path)
        .context("error opening repo")
        .attach_with(|| format!("path={}", repo_path.display()))?;
    let mut repo_urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        repo_urls.push(
            repo.find_remote(remote_name)
                .context("error finding remote")
                .attach_with(|| format!("remote={remote_name:?}"))?
                .url()
                .map(parse_github_url_from_git_remote_url)
                .ok_or_else(|| report!("error invalid remote URL UTF-8"))
                .attach_with(|| format!("remote={remote_name:?}"))
                .context("error parsing remote URL")
                .attach_with(|| format!("remote={remote_name:?}"))??,
        );
    }
    Ok(repo_urls)
}

/// Converts a Git remote URL (SSH or HTTPS) to a canonical GitHub HTTPS URL without the `.git` suffix.
///
/// Accepts formats like:
/// - `git@github.com:owner/repo.git`
/// - `https://github.com/owner/repo[.git]`
///
/// # Errors
/// - The URL cannot be parsed or lacks a path component.
fn parse_github_url_from_git_remote_url(git_remote_url: &str) -> rootcause::Result<Url> {
    if let Ok(mut url) = Url::parse(git_remote_url) {
        url.set_path(url.clone().path().trim_end_matches(".git"));
        return Ok(url);
    }

    let path = git_remote_url
        .split_once(':')
        .map(|(_, path)| path.trim_end_matches(".git"))
        .ok_or_else(|| report!("error extracting URL path"))
        .attach_with(|| format!("git_remote_url={git_remote_url:?}"))?;

    let mut url = Url::parse("https://github.com").context("error parsing base GitHub URL")?;
    url.set_path(path);

    Ok(url)
}

/// Extracts the pull request numeric ID from a GitHub URL.
///
/// Supported forms:
/// - Direct PR path: `.../pull/<ID>` (ID may not be last segment).
/// - Actions run URL with `?pr=<ID>` (also supports `/job/<JOB_ID>` variants).
///
/// # Errors
/// - Host is not `github.com`.
/// - The PR id segment or query parameter is missing, empty, duplicated, or malformed.
fn extract_pr_id_form_url(url: &Url) -> rootcause::Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| report!("error extracting host from URL"))
        .attach_with(|| format!("url={url}"))?;
    if host != GITHUB_HOST {
        Err(report!("error host mismatch"))
            .attach_with(|| format!("host={host:?} expected={GITHUB_HOST:?} URL={url}"))?;
    }

    // To handle URLs like:
    // - https://github.com/<OWNER>/<REPO>/actions/runs/<RUN_ID>?pr=<PR_ID>
    // - https://github.com/<OWNER>/<REPO>/actions/runs/<RUN_ID>/job/<JOB_ID>?pr=<PR_ID>
    if let Some(pr_id) = url
        .query_pairs()
        .find(|(key, _)| key == GITHUB_PR_ID_QUERY_KEY)
        .map(|(_, pr_id)| pr_id.to_string())
    {
        return Ok(pr_id);
    }

    let path_segments = url
        .path_segments()
        .ok_or_else(|| report!("error URL cannot be base"))
        .attach_with(|| format!("url={url}"))?
        .enumerate()
        .collect::<Vec<_>>();

    match path_segments
        .iter()
        .filter(|(_, ps)| ps == &GITHUB_PR_ID_PREFIX)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [(idx, _)] => Ok(path_segments
            .get(idx.saturating_add(1))
            .ok_or_else(|| report!("error missing PR ID"))
            .attach_with(|| format!("url={url} path_segments={path_segments:#?}"))
            .and_then(|(_, pr_id)| {
                if pr_id.is_empty() {
                    return Err(
                        report!("error empty PR ID").attach(format!("url={url} path_segments={path_segments:#?}"))
                    );
                }
                Ok((*pr_id).to_string())
            })?),
        [] => Err(report!("error missing PR ID prefix").attach(format!(
            "prefix={GITHUB_PR_ID_PREFIX:?} url={url} path_segments={path_segments:#?}"
        ))),
        _ => Err(report!("error multiple PR ID prefixes").attach(format!(
            "prefix={GITHUB_PR_ID_PREFIX:?} url={url} path_segments={path_segments:#?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_host_cannot_be_extracted() {
        let url = Url::parse("mailto:foo@bar.com").unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(
            err.format_current_context().to_string(),
            "error extracting host from URL"
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_is_not_from_github() {
        let url = Url::parse("https://foo.bar").unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(err.format_current_context().to_string(), "error host mismatch");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_path_segments() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}")).unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(err.format_current_context().to_string(), "error missing PR ID prefix");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull")).unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(err.format_current_context().to_string(), "error missing PR ID");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doenst_have_the_expected_pr_id_prefix() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/foo")).unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(err.format_current_context().to_string(), "error missing PR ID prefix");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_has_multiple_pr_id_prefixes() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/pull/43")).unwrap();
        assert2::let_assert!(Err(err) = extract_pr_id_form_url(&url));
        assert_eq!(
            err.format_current_context().to_string(),
            "error multiple PR ID prefixes"
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_that_ends_with_the_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42")).unwrap();
        assert_eq!(extract_pr_id_form_url(&url).unwrap(), "42");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_that_does_not_end_with_the_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/foo")).unwrap();
        assert_eq!(extract_pr_id_form_url(&url).unwrap(), "42");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_if_pr_id_prefix_is_not_1st_path_segment()
    {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/foo/pull/42/foo")).unwrap();
        assert_eq!(extract_pr_id_form_url(&url).unwrap(), "42");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_if_pr_is_in_query_string() {
        let url = Url::parse(&format!(
            "https://{GITHUB_HOST}/<OWNER>/<REPO>/actions/runs/<RUN_ID>?pr=42"
        ))
        .unwrap();
        assert_eq!(extract_pr_id_form_url(&url).unwrap(), "42");

        let url = Url::parse(&format!(
            "https://{GITHUB_HOST}/<OWNER>/<REPO>/actions/runs/<RUN_ID>/job/<JOB_ID>?pr=42"
        ))
        .unwrap();
        assert_eq!(extract_pr_id_form_url(&url).unwrap(), "42");
    }

    #[rstest]
    #[case::ssh_url_with_git_suffix(
        "git@github.com:fusillicode/dotfiles.git",
        Url::parse("https://github.com/fusillicode/dotfiles").unwrap()
    )]
    #[case::https_url_without_git_suffix(
        "https://github.com/fusillicode/dotfiles",
        Url::parse("https://github.com/fusillicode/dotfiles").unwrap()
    )]
    fn parse_github_url_from_git_remote_url_works_as_expected(#[case] input: &str, #[case] expected: Url) {
        let result = parse_github_url_from_git_remote_url(input).unwrap();
        assert_eq!(result, expected);
    }
}
