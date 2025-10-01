//! Integrate with the GitHub CLI: ensure login, derive PR branch names, fetch latest releases and normalize remote
//! URLs.
//!
//! Provide focused wrappers around `gh` subcommands plus URL parsing helpers for PR IDs and remote
//! canonicalization.

#![feature(exit_status_error)]

use std::path::Path;
use std::process::Command;
use std::process::Output;

use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use url::Url;

/// The GitHub host domain.
const GITHUB_HOST: &str = "github.com";
/// The URL path segment prefix for pull requests.
const GITHUB_PR_ID_PREFIX: &str = "pull";
/// The query parameter key used for pull request IDs in GitHub Actions URLs.
const GITHUB_PR_ID_QUERY_KEY: &str = "pr";

/// Ensures the user is authenticated with the GitHub CLI.
///
/// Runs `gh auth status`; if not authenticated it invokes an interactive `gh auth login`.
///
/// # Errors
/// In case:
/// - Checking auth status fails.
/// - The login command fails or exits with a non-zero status.
pub fn log_into_github() -> color_eyre::Result<()> {
    if ytil_cmd::silent_cmd("gh").args(["auth", "status"]).status()?.success() {
        return Ok(());
    }

    Ok(ytil_cmd::silent_cmd("sh")
        .args(["-c", "gh auth login"])
        .status()?
        .exit_ok()?)
}

/// Retrieves the latest release tag name for the specified GitHub repository.
///
/// # Errors
/// In case:
/// - Executing `gh` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - Invoking `gh api` fails.
pub fn get_latest_release(repo: &str) -> color_eyre::Result<String> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}/releases/latest"), "--jq=.tag_name"])
        .output()?;

    extract_success_output(&output)
}

/// Extracts the branch name from a GitHub pull request [`Url`].
///
/// # Errors
/// In case:
/// - Executing `gh` fails or returns a non-zero exit status.
/// - Invoking `gh pr view` fails.
/// - Output cannot be parsed.
pub fn get_branch_name_from_url(url: &Url) -> color_eyre::Result<String> {
    let pr_id = extract_pr_id_form_url(url)?;

    let output = Command::new("gh")
        .args(["pr", "view", &pr_id, "--json", "headRefName", "--jq", ".headRefName"])
        .output()?;

    extract_success_output(&output)
}

/// Returns all GitHub remote URLs for the repository rooted at `repo_path`.
///
/// Filters remotes to those that parse as GitHub URLs.
///
/// # Errors
/// In case:
/// - The repository cannot be opened.
/// - A remote cannot be resolved.
/// - A remote URL is invalid UTF-8.
pub fn get_repo_urls(repo_path: &Path) -> color_eyre::Result<Vec<Url>> {
    let repo = ytil_git::get_repo(repo_path)?;
    let mut repo_urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        repo_urls.push(
            repo.find_remote(remote_name)?
                .url()
                .map(parse_github_url_from_git_remote_url)
                .ok_or_else(|| eyre!("remote url is invalid UTF-8"))??,
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
/// In case:
/// - The URL cannot be parsed or lacks a path component.
fn parse_github_url_from_git_remote_url(git_remote_url: &str) -> color_eyre::Result<Url> {
    if let Ok(mut url) = Url::parse(git_remote_url) {
        url.set_path(url.clone().path().trim_end_matches(".git"));
        return Ok(url);
    }

    let path = git_remote_url
        .split_once(':')
        .map(|(_, path)| path.trim_end_matches(".git"))
        .ok_or_else(|| eyre!("cannot extract URL path from '{git_remote_url}'"))?;

    let mut url = Url::parse("https://github.com")?;
    url.set_path(path);

    Ok(url)
}

/// Extracts and validates successful command output, converting it to a trimmed string.
///
/// # Errors
/// In case:
/// - UTF-8 conversion fails.
fn extract_success_output(output: &Output) -> color_eyre::Result<String> {
    output.status.exit_ok()?;
    Ok(std::str::from_utf8(&output.stdout)?.trim().into())
}

/// Extracts the pull request numeric ID from a GitHub URL.
///
/// Supported forms:
/// - Direct PR path: `.../pull/<ID>` (ID may not be last segment).
/// - Actions run URL with `?pr=<ID>` (also supports `/job/<JOB_ID>` variants).
///
/// # Errors
/// In case:
/// - Host is not `github.com`.
/// - The PR id segment or query parameter is missing, empty, duplicated, or malformed.
fn extract_pr_id_form_url(url: &Url) -> color_eyre::Result<String> {
    let host = url.host_str().ok_or_else(|| eyre!("cannot extract host from {url}"))?;
    if host != GITHUB_HOST {
        bail!("host {host:#?} in {url} doesn't match {GITHUB_HOST:#?}")
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
        .ok_or_else(|| eyre!("{url} cannot-be-a-base"))?
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
            .ok_or_else(|| eyre!("missing PR id in {url} path segments {path_segments:#?}"))
            .and_then(|(_, pr_id)| {
                if pr_id.is_empty() {
                    return Err(eyre!("empty PR id in {url} path segments {path_segments:#?}"));
                }
                Ok((*pr_id).to_string())
            })?),
        [] => Err(eyre!(
            "missing PR id prefix {GITHUB_PR_ID_PREFIX:#?} in {url} path segments {path_segments:#?}"
        )),
        _ => Err(eyre!(
            "multiple {GITHUB_PR_ID_PREFIX:#?} found in {url} path segments {path_segments:#?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_host_cannot_be_extracted() {
        let url = Url::parse("mailto:foo@bar.com").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!("cannot extract host from mailto:foo@bar.com", error.to_string());
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_is_not_from_github() {
        let url = Url::parse("https://foo.bar").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            r#"host "foo.bar" in https://foo.bar/ doesn't match "github.com""#,
            error.to_string()
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_path_segments() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            "missing PR id prefix \"pull\" in https://github.com/ path segments [\n    (\n        0,\n        \"\",\n    ),\n]",
            error.to_string()
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            "missing PR id in https://github.com/pull path segments [\n    (\n        0,\n        \"pull\",\n    ),\n]",
            error.to_string()
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doenst_have_the_expected_pr_id_prefix() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/foo")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            "missing PR id prefix \"pull\" in https://github.com/foo path segments [\n    (\n        0,\n        \"foo\",\n    ),\n]",
            error.to_string()
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_has_multiple_pr_id_prefixes() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/pull/43")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            "multiple \"pull\" found in https://github.com/pull/42/pull/43 path segments [\n    (\n        0,\n        \"pull\",\n    ),\n    (\n        1,\n        \"42\",\n    ),\n    (\n        2,\n        \"pull\",\n    ),\n    (\n        3,\n        \"43\",\n    ),\n]",
            error.to_string()
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_that_ends_with_the_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42")).unwrap();
        assert_eq!("42", extract_pr_id_form_url(&url).unwrap());
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_that_does_not_end_with_the_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/foo")).unwrap();
        assert_eq!("42", extract_pr_id_form_url(&url).unwrap());
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_if_pr_id_prefix_is_not_1st_path_segment()
    {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/foo/pull/42/foo")).unwrap();
        assert_eq!("42", extract_pr_id_form_url(&url).unwrap());
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_pr_id_from_a_github_pr_url_if_pr_is_in_query_string() {
        let url = Url::parse(&format!(
            "https://{GITHUB_HOST}/<OWNER>/<REPO>/actions/runs/<RUN_ID>?pr=42"
        ))
        .unwrap();
        assert_eq!("42", extract_pr_id_form_url(&url).unwrap());

        let url = Url::parse(&format!(
            "https://{GITHUB_HOST}/<OWNER>/<REPO>/actions/runs/<RUN_ID>/job/<JOB_ID>?pr=42"
        ))
        .unwrap();
        assert_eq!("42", extract_pr_id_form_url(&url).unwrap());
    }

    #[rstest]
    #[case("git@github.com:fusillicode/dotfiles.git", Url::parse("https://github.com/fusillicode/dotfiles").unwrap())]
    #[case("https://github.com/fusillicode/dotfiles", Url::parse("https://github.com/fusillicode/dotfiles").unwrap())]
    fn parse_github_url_from_git_remote_url_works_as_expected(#[case] input: &str, #[case] expected: Url) {
        let result = parse_github_url_from_git_remote_url(input).unwrap();
        assert_eq!(expected, result);
    }
}
