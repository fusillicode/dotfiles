use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre::bail;
use serde::Deserialize;
use strum::EnumIter;
use strum::EnumString;
pub use strum::IntoEnumIterator;
use ytil_cmd::CmdExt;

/// Pull request summary fetched via the `gh pr list` command.
#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: usize,
    pub title: String,
    pub author: PullRequestAuthor,
    #[serde(rename = "mergeStateStatus")]
    pub merge_state: PullRequestMergeState,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

/// Author metadata for a pull request.
#[derive(Debug, Deserialize)]
pub struct PullRequestAuthor {
    pub login: String,
    pub is_bot: bool,
}

/// Merge state classification returned by GitHub's `mergeStateStatus` field.
#[derive(Clone, Copy, Debug, Deserialize, EnumIter, EnumString, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PullRequestMergeState {
    Behind,
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    Unknown,
    Unmergeable,
    Unstable,
}

/// Fetch pull requests for a repository using `gh pr list`.
///
/// # Errors
/// - Spawning or executing `gh pr list` fails.
/// - Command exits non‑zero (handled inside [`ytil_cmd::CmdExt`]).
/// - Output JSON cannot be deserialized.
pub fn get(
    repo: &str,
    search: Option<&str>,
    retain_fn: &dyn Fn(&PullRequest) -> bool,
) -> color_eyre::Result<Vec<PullRequest>> {
    let mut args = vec![
        "pr",
        "list",
        "--repo",
        repo,
        "--json",
        "number,title,author,mergeStateStatus,updatedAt",
    ];
    if let Some(s) = search.filter(|s| !s.is_empty()) {
        args.extend(["--search", s]);
    }

    let output = Command::new("gh").args(args).exec()?.stdout;

    if output.is_empty() {
        return Ok(Vec::new());
    }

    let mut prs: Vec<PullRequest> = serde_json::from_slice(&output)?;
    prs.retain(|pr| retain_fn(pr));
    prs.sort_unstable_by_key(|x| x.updated_at);

    Ok(prs)
}

/// Merge a pull request using administrative squash semantics.
///
/// # Errors
/// - Spawning or executing the `gh pr merge` command fails.
/// - Command exits with non‑zero status.
pub fn merge(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args([
            "pr",
            "merge",
            "--admin",
            "--squash",
            "--delete-branch",
            &format!("{pr_number}"),
        ])
        .exec()?;
    Ok(())
}

/// Approve a pull request via `gh pr review --approve`.
///
/// # Errors
/// - Spawning or executing `gh pr review` fails.
/// - Command exits with non‑zero status.
pub fn approve(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args(["pr", "review", &format!("{pr_number}"), "--approve"])
        .exec()?;
    Ok(())
}

/// Trigger Dependabot to rebase a pull request via `@dependabot rebase` comment.
///
/// # Errors
/// - Spawning or executing `gh pr comment` fails.
/// - Command exits with non‑zero status.
pub fn dependabot_rebase(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args(["pr", "comment", &format!("{pr_number}"), "--body", "@dependabot rebase"])
        .exec()?;
    Ok(())
}

/// Enable GitHub auto-merge for a pull request (squash strategy).
///
/// # Errors
/// - Spawning or executing `gh pr merge` fails.
/// - Command exits non-zero.
pub fn enable_auto_merge(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args([
            "pr",
            "merge",
            &format!("{pr_number}"),
            "--auto",
            "--squash",
            "--delete-branch",
        ])
        .exec()?;
    Ok(())
}

/// Creates a GitHub pull request with the specified title.
///
/// # Errors
/// - Title is empty or `gh pr create` fails.
pub fn create(title: &str) -> color_eyre::Result<String> {
    if title.is_empty() {
        bail!("error cannot create GitHub PR with empty title");
    }
    let output = Command::new("gh")
        .args(["pr", "create", "--title", title, "--body", ""])
        .exec()?;
    ytil_cmd::extract_success_output(&output)
}
