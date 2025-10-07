use std::process::Command;

use serde::Deserialize;
use strum::EnumString;
use ytil_cmd::CmdExt;

#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: usize,
    pub title: String,
    pub author: PullRequestAuthor,
    #[serde(rename = "mergeStateStatus")]
    pub merge_state: PullRequestMergeState,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestAuthor {
    pub login: String,
    pub is_bot: bool,
}

#[derive(Debug, EnumString, Deserialize, Clone, Copy, PartialEq, Eq)]
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
}

/// Fetch pull requests for a repository using `gh pr list`.
///
/// # Arguments
/// * `repo` - `owner/name` repository spec.
/// * `search` - Optional search expression (without the `--search` flag) using GitHub search qualifiers.
/// * `retain_fn` - Optional predicate; if provided only PRs for which it returns true are kept.
///
/// # Returns
/// Vector of deserialized pull requests (may be empty).
///
/// # Rationale
/// Accepting `Option<&str>` for search cleanly distinguishes absence vs empty and avoids
/// forcing callers to include flag/quoting. Using a trait object for the predicate avoids
/// generic inference issues when passing `None`.
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
        "number,title,author,mergeStateStatus",
    ];
    if let Some(s) = search
        && !s.is_empty()
    {
        args.extend(["--search", s]);
    }

    let output = Command::new("gh").args(args).exec()?.stdout;

    if output.is_empty() {
        return Ok(Vec::new());
    }

    let mut prs: Vec<PullRequest> = serde_json::from_slice(&output)?;
    prs.retain(|pr| retain_fn(pr));

    Ok(prs)
}

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
