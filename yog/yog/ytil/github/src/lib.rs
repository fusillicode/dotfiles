//! Provide lightweight GitHub helpers using the `gh` CLI.
//!
//! Provide focused wrappers around `gh` subcommands plus URL parsing helpers for PR IDs and remote
//! canonicalization.
//!
//! # Rationale
//! This module shells out to the GitHub CLI ('gh') instead of using a direct HTTP client (e.g. `octocrab`) because:
//! - Reuses the user's existing authenticated `gh` session (no PAT / device-flow code, fewer secrets to manage).
//! - Keeps this utility crate synchronous and lightweight (avoids adding `tokio` + `reqwest` dependency graph).
//! - Minimizes compile time and binary size in the broader workspace.
//! - Leverages 'gh' stable porcelain for JSON output (`--json` / `--jq`) and future compatibility with GitHub auth
//!   flows / SSO.
//! - Current feature surface (latest release tag, PR head branch lookup) is small; process spawn overhead is negligible
//!   versus HTTP setup cost.
//!
//! Trade-offs accepted:
//! - Less fine-grained control over rate limiting and retries.
//! - Tight coupling to `gh` output flags (low churn historically, but still external).
//! - Requires `gh` binary presence in runtime environments.

#![feature(exit_status_error)]

use std::path::Path;
use std::process::Command;
use std::process::Output;

use color_eyre::eyre::Context;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use url::Url;

pub mod pr;

/// The GitHub host domain.
const GITHUB_HOST: &str = "github.com";
/// The URL path segment prefix for pull requests.
const GITHUB_PR_ID_PREFIX: &str = "pull";
/// The query parameter key used for pull request IDs in GitHub Actions URLs.
const GITHUB_PR_ID_QUERY_KEY: &str = "pr";

/// Represents a newly created GitHub issue.
///
/// Contains the parsed details from the `gh issue create` command output.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct CreatedIssue {
    /// The title of the created issue.
    pub title: String,
    /// The repository URL prefix (e.g., `https://github.com/owner/repo/`).
    pub repo: String,
    /// The issue number (e.g., "123").
    pub issue_nr: String,
}

impl CreatedIssue {
    /// Creates a [`CreatedIssue`] from the `gh issue create` command output.
    ///
    /// Parses the output URL to extract repository and issue number.
    ///
    /// # Arguments
    /// - `title` The issue title.
    /// - `output` The stdout from `gh issue create`.
    ///
    /// # Returns
    /// The parsed [`CreatedIssue`].
    ///
    /// # Errors
    /// - Output does not contain "issues".
    /// - Repository or issue number parts are empty.
    fn new(title: &str, output: &str) -> color_eyre::Result<Self> {
        let get_not_empty_field = |maybe_value: Option<&str>, field: &str| -> color_eyre::Result<String> {
            maybe_value
                .ok_or_else(|| eyre!("error building CreateIssueOutput | missing={field:?} output={output:?}"))
                .and_then(|s| {
                    if s.is_empty() {
                        Err(eyre!(
                            "error building CreateIssueOutput | empty={field:?} output={output:?}"
                        ))
                    } else {
                        Ok(s.trim_matches('/').to_string())
                    }
                })
        };

        let mut split = output.split("issues");

        Ok(Self {
            title: title.to_string(),
            repo: get_not_empty_field(split.next(), "repo")?,
            issue_nr: get_not_empty_field(split.next(), "issue_nr")?,
        })
    }

    /// Formats the issue title for use as a pull request title.
    ///
    /// # Returns
    /// A string in the format `[issue_nr]: title`.
    pub fn pr_title(&self) -> String {
        format!("[{}]: {}", self.issue_nr, self.title)
    }
}

/// Repository fields available for querying via `gh repo view`.
#[derive(strum::AsRefStr, Debug)]
pub enum RepoViewField {
    /// The repository name with owner in `owner/name` format.
    #[strum(serialize = "nameWithOwner")]
    NameWithOwner,
    /// The repository URL.
    #[strum(serialize = "url")]
    Url,
}

impl RepoViewField {
    /// Returns the jq representation of the field for GitHub CLI queries.
    ///
    /// # Returns
    /// A string prefixed with `.` for use in jq expressions.
    pub fn jq_repr(&self) -> String {
        format!(".{}", self.as_ref())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CreatePrError {
    #[error("PR already exist pr_url={pr_url}")]
    AlreadyExist { pr_url: String },
    #[error(transparent)]
    Other(color_eyre::eyre::Error),
}

/// Creates a new GitHub issue with the specified title.
///
/// This function invokes `gh issue create --title <title> --body ""` to create the issue.
///
/// # Arguments
/// - `title` The title of the issue to create.
///
/// # Returns
/// The [`CreatedIssue`] containing the parsed issue details.
///
/// # Errors
/// - If `title` is empty.
/// - Spawning or executing the `gh issue create` command fails.
/// - Command exits with non-zero status.
/// - Output cannot be parsed as a valid issue URL.
pub fn create_issue(title: &str) -> color_eyre::Result<CreatedIssue> {
    if title.is_empty() {
        bail!("cannot create GitHub issue with empty title")
    }

    let output = Command::new("gh")
        .args(["issue", "create", "--title", title, "--body", ""])
        .output()
        .wrap_err_with(|| eyre!("error creating GitHub issue | title={title:?}"))?;

    let created_issue = extract_success_output(&output)
        .and_then(|output| CreatedIssue::new(title, &output))
        .wrap_err_with(|| eyre!("error parsing created issue output | title={title:?}"))?;

    Ok(created_issue)
}

/// Creates a new GitHub pull request using the `gh` CLI.
///
/// Invokes: `gh pr create --title <title> --body "" --head <branch>`.
/// If `branch` is `None`, uses the default branch from the repository.
///
/// # Arguments
/// - `title` The title of the pull request to create.
/// - `branch` The head branch for the PR; if `None`, defaults to the repository's default branch.
///
/// # Returns
/// The URL of the created pull request.
///
/// # Errors
/// - Spawning or executing the `gh pr create` command fails.
/// - Command exits with non-zero status.
/// - Output is not valid UTF-8.
/// - Retrieving the default branch fails when `branch` is `None`.
pub fn create_pr(title: &str, branch: Option<&str>) -> Result<String, CreatePrError> {
    let mut args = vec!["pr", "create", "--title", title, "--body", ""];

    let branch = if let Some(branch) = branch {
        branch
    } else {
        &ytil_git::get_default_branch().map_err(CreatePrError::Other)?
    };

    args.extend(["--head", branch]);

    let output = Command::new("gh")
        .args(args)
        .output()
        .wrap_err_with(|| eyre!("error creating PR from branch | title={title:?} branch={branch:?}"))
        .map_err(CreatePrError::Other)?;

    handle_create_pr_output(&output)
}

/// Return the specified repository field via `gh repo view`.
///
/// Invokes: `gh repo view --json <field> --jq .<field>`.
///
/// # Arguments
/// - `field` The repository field to retrieve.
///
/// # Returns
/// The value of the requested field as a string.
///
/// # Errors
/// - Spawning or executing the `gh repo view` command fails.
/// - Command exits with non‑zero status.
/// - Output is not valid UTF‑8.
pub fn get_repo_view_field(field: &RepoViewField) -> color_eyre::Result<String> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", field.as_ref(), "--jq", &field.jq_repr()])
        .output()
        .wrap_err_with(|| eyre!("error getting repo view field | field={field:?}"))?;

    extract_success_output(&output)
}

/// Ensures the user is authenticated with the GitHub CLI.
///
/// Runs `gh auth status`; if not authenticated it invokes an interactive `gh auth login`.
///
/// # Returns
/// Nothing on success.
///
/// # Errors
/// - Checking auth status fails.
/// - The login command fails or exits with a non-zero status.
pub fn log_into_github() -> color_eyre::Result<()> {
    if ytil_cmd::silent_cmd("gh")
        .args(["auth", "status"])
        .status()
        .wrap_err_with(|| eyre!("error checking gh auth status"))?
        .success()
    {
        return Ok(());
    }

    ytil_cmd::silent_cmd("sh")
        .args(["-c", "gh auth login"])
        .status()
        .wrap_err_with(|| eyre!("error running gh auth login command"))?
        .exit_ok()
        .wrap_err_with(|| eyre!("error running gh auth login"))
}

/// Retrieves the latest release tag name for the specified GitHub repository.
///
/// # Errors
/// - Executing `gh` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - Invoking `gh api` fails.
pub fn get_latest_release(repo: &str) -> color_eyre::Result<String> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}/releases/latest"), "--jq=.tag_name"])
        .output()
        .wrap_err_with(|| eyre!("error getting latest release | repo={repo:?}"))?;

    extract_success_output(&output)
}

/// Extracts the branch name from a GitHub pull request [`Url`].
///
/// # Errors
/// - Executing `gh` fails or returns a non-zero exit status.
/// - Invoking `gh pr view` fails.
/// - Output cannot be parsed.
pub fn get_branch_name_from_url(url: &Url) -> color_eyre::Result<String> {
    let pr_id = extract_pr_id_form_url(url)?;

    let output = Command::new("gh")
        .args(["pr", "view", &pr_id, "--json", "headRefName", "--jq", ".headRefName"])
        .output()
        .wrap_err_with(|| eyre!("error getting branch name | pr_id={pr_id:?}"))?;

    extract_success_output(&output)
}

/// Returns all GitHub remote URLs for the repository rooted at `repo_path`.
///
/// Filters remotes to those that parse as GitHub URLs.
///
/// # Errors
/// - The repository cannot be opened.
/// - A remote cannot be resolved.
/// - A remote URL is invalid UTF-8.
pub fn get_repo_urls(repo_path: &Path) -> color_eyre::Result<Vec<Url>> {
    let repo =
        ytil_git::get_repo(repo_path).wrap_err_with(|| eyre!("error opening repo | path={}", repo_path.display()))?;
    let mut repo_urls = vec![];
    for remote_name in repo.remotes()?.iter().flatten() {
        repo_urls.push(
            repo.find_remote(remote_name)
                .wrap_err_with(|| eyre!("error finding remote | remote={remote_name:?}"))?
                .url()
                .map(parse_github_url_from_git_remote_url)
                .ok_or_else(|| eyre!("error invalid remote URL UTF-8 | remote={remote_name:?}"))
                .wrap_err_with(|| eyre!("error parsing remote URL | remote={remote_name:?}"))??,
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
fn parse_github_url_from_git_remote_url(git_remote_url: &str) -> color_eyre::Result<Url> {
    if let Ok(mut url) = Url::parse(git_remote_url) {
        url.set_path(url.clone().path().trim_end_matches(".git"));
        return Ok(url);
    }

    let path = git_remote_url
        .split_once(':')
        .map(|(_, path)| path.trim_end_matches(".git"))
        .ok_or_else(|| eyre!("error extracting URL path | git_remote_url={git_remote_url:?}"))?;

    let mut url = Url::parse("https://github.com").wrap_err_with(|| eyre!("error parsing base GitHub URL"))?;
    url.set_path(path);

    Ok(url)
}

/// Handles the output of the `gh pr create` command.
///
/// Parses the stdout for success or stderr for errors, including already-existing PRs.
///
/// # Returns
/// - The PR URL on success.
/// - [`CreatePrError::AlreadyExist`] if a PR already exists.
/// - [`CreatePrError::Other`] for other failures.
fn handle_create_pr_output(output: &Output) -> Result<String, CreatePrError> {
    let stderr = std::str::from_utf8(&output.stderr)
        .wrap_err_with(|| eyre!("error decoding stderr | cmd=\"gh pr create\""))
        .map_err(CreatePrError::Other)?
        .trim();

    if stderr.is_empty() {
        return extract_success_output(output).map_err(CreatePrError::Other);
    }

    let mut split = stderr.split("already exists:");
    split.next();
    let Some(pr_url) = split.next().map(str::trim) else {
        return Err(CreatePrError::Other(eyre!(
            "error extracting pr_url from cmd stderr | stderr={stderr:?}"
        )));
    };

    Err(CreatePrError::AlreadyExist {
        pr_url: pr_url.to_string(),
    })
}

/// Extracts and validates successful command output, converting it to a trimmed string.
///
/// # Errors
/// - UTF-8 conversion fails.
fn extract_success_output(output: &Output) -> color_eyre::Result<String> {
    output
        .status
        .exit_ok()
        .wrap_err_with(|| eyre!("command exited with non-zero status"))?;
    Ok(std::str::from_utf8(&output.stdout)
        .wrap_err_with(|| eyre!("error decoding command stdout"))?
        .trim()
        .into())
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
fn extract_pr_id_form_url(url: &Url) -> color_eyre::Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| eyre!("error extracting host from URL | url={url}"))?;
    if host != GITHUB_HOST {
        bail!("error host mismatch | host={host:?} expected={GITHUB_HOST:?} URL={url}")
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
        .ok_or_else(|| eyre!("error URL cannot be base | url={url}"))?
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
            .ok_or_else(|| eyre!("error missing PR ID | url={url} path_segments={path_segments:#?}"))
            .and_then(|(_, pr_id)| {
                if pr_id.is_empty() {
                    return Err(eyre!("error empty PR ID | url={url} path_segments={path_segments:#?}"));
                }
                Ok((*pr_id).to_string())
            })?),
        [] => Err(eyre!(
            "error missing PR ID prefix | prefix={GITHUB_PR_ID_PREFIX:?} url={url} path_segments={path_segments:#?}"
        )),
        _ => Err(eyre!(
            "error multiple PR ID prefixes | prefix={GITHUB_PR_ID_PREFIX:?} url={url} path_segments={path_segments:#?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use rstest::rstest;

    use super::*;

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_host_cannot_be_extracted() {
        let url = Url::parse("mailto:foo@bar.com").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        assert_eq!(
            error.to_string(),
            "error extracting host from URL | url=mailto:foo@bar.com"
        );
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_is_not_from_github() {
        let url = Url::parse("https://foo.bar").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        let msg = error.to_string();
        assert!(msg.starts_with("error host mismatch |"));
        assert!(msg.contains(r#"host="foo.bar""#), "actual: {msg}");
        assert!(msg.contains(r#"expected="github.com""#), "actual: {msg}");
        assert!(msg.contains("URL=https://foo.bar/"), "actual: {msg}");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_path_segments() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        let msg = error.to_string();
        assert!(msg.starts_with("error missing PR ID prefix |"), "actual: {msg}");
        assert!(msg.contains("prefix=\"pull\""), "actual: {msg}");
        assert!(msg.contains("url=https://github.com/"), "actual: {msg}");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doesnt_have_pr_id() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        let msg = error.to_string();
        assert!(msg.starts_with("error missing PR ID |"), "actual: {msg}");
        assert!(msg.contains("url=https://github.com/pull"), "actual: {msg}");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_doenst_have_the_expected_pr_id_prefix() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/foo")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        let msg = error.to_string();
        assert!(msg.starts_with("error missing PR ID prefix |"), "actual: {msg}");
        assert!(msg.contains("prefix=\"pull\""), "actual: {msg}");
        assert!(msg.contains("url=https://github.com/foo"), "actual: {msg}");
    }

    #[test]
    fn extract_pr_id_form_url_returns_the_expected_error_when_url_has_multiple_pr_id_prefixes() {
        let url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/pull/43")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_url(&url));
        let msg = error.to_string();
        assert!(msg.starts_with("error multiple PR ID prefixes |"), "actual: {msg}");
        assert!(msg.contains("prefix=\"pull\""), "actual: {msg}");
        assert!(msg.contains("url=https://github.com/pull/42/pull/43"), "actual: {msg}");
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

    #[test]
    fn created_issue_new_parses_valid_output() {
        assert2::let_assert!(Ok(actual) = CreatedIssue::new("Test Issue", "https://github.com/owner/repo/issues/123"));
        pretty_assertions::assert_eq!(
            actual,
            CreatedIssue {
                title: "Test Issue".to_string(),
                repo: "https://github.com/owner/repo".to_string(),
                issue_nr: "123".to_string(),
            }
        );
    }

    #[rstest]
    #[case("", "error building CreateIssueOutput | empty=\"repo\" output=\"\"")]
    #[case("issues", "error building CreateIssueOutput | empty=\"repo\" output=\"issues\"")]
    #[case(
        "https://github.com/owner/repo/123",
        "error building CreateIssueOutput | missing=\"issue_nr\" output=\"https://github.com/owner/repo/123\""
    )]
    #[case(
        "repo/issues",
        "error building CreateIssueOutput | empty=\"issue_nr\" output=\"repo/issues\""
    )]
    fn created_issue_new_errors_on_invalid_output(#[case] output: &str, #[case] expected_error: &str) {
        assert2::let_assert!(Err(error) = CreatedIssue::new("title", output));
        pretty_assertions::assert_eq!(error.to_string(), expected_error);
    }

    #[test]
    fn created_issue_pr_title_formats_correctly() {
        let issue = CreatedIssue {
            title: "Fix bug".to_string(),
            issue_nr: "42".to_string(),
            repo: "https://github.com/owner/repo/".to_string(),
        };
        pretty_assertions::assert_eq!(issue.pr_title(), "[42]: Fix bug");
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

    #[test]
    fn handle_create_pr_output_when_stderr_empty_and_command_succeeds_returns_pr_url() {
        let output = std::process::Output {
            status: ExitStatus::from_raw(0),
            stdout: b"https://github.com/owner/repo/pull/123\n".to_vec(),
            stderr: vec![],
        };
        assert2::let_assert!(Ok(url) = handle_create_pr_output(&output));
        pretty_assertions::assert_eq!(url, "https://github.com/owner/repo/pull/123");
    }

    #[test]
    fn handle_create_pr_output_when_stderr_empty_and_command_fails_returns_generic_error() {
        let output = std::process::Output {
            status: ExitStatus::from_raw(1),
            stdout: vec![],
            stderr: vec![],
        };
        assert2::let_assert!(Err(CreatePrError::Other(error)) = handle_create_pr_output(&output));
        assert!(error.to_string().contains("command exited with non-zero status"));
    }

    #[test]
    fn handle_create_pr_output_when_stderr_contains_already_exists_returns_already_exist_error() {
        let output = std::process::Output {
            status: ExitStatus::from_raw(1),
            stdout: vec![],
            stderr: b"already exists: https://github.com/owner/repo/pull/123\n".to_vec(),
        };
        assert2::let_assert!(Err(CreatePrError::AlreadyExist { pr_url }) = handle_create_pr_output(&output));
        pretty_assertions::assert_eq!(pr_url, "https://github.com/owner/repo/pull/123");
    }

    #[test]
    fn handle_create_pr_output_when_stderr_contains_unexpected_error_returns_generic_error() {
        let output = std::process::Output {
            status: ExitStatus::from_raw(1),
            stdout: vec![],
            stderr: b"some error message\n".to_vec(),
        };
        assert2::let_assert!(Err(CreatePrError::Other(error)) = handle_create_pr_output(&output));
        let error_string = error.to_string();
        assert!(error_string.contains("error extracting pr_url from cmd stderr"));
        assert!(error_string.contains("stderr=\"some error message\""));
    }

    #[test]
    fn handle_create_pr_output_when_stderr_not_utf8_returns_generic_error() {
        let output = std::process::Output {
            status: ExitStatus::from_raw(1),
            stdout: vec![],
            stderr: vec![0xff],
        };
        assert2::let_assert!(Err(CreatePrError::Other(error)) = handle_create_pr_output(&output));
        assert!(error.to_string().contains("error decoding stderr"));
    }
}
