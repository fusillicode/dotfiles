use std::process::Command;

use serde::Deserialize;
use strum::EnumIter;
use strum::EnumString;
pub use strum::IntoEnumIterator;
use ytil_cmd::CmdExt;

/// Pull request summary fetched via the `gh pr list` command.
///
/// Captures only the fields the current workspace needs for listing, filtering,
/// display, and merge decisions. Additional fields can be appended later without
/// breaking callers.
///
/// # Fields
/// - `number` Numeric PR number (unique per repository).
/// - `title` Current PR title.
/// - `author` Author login + bot flag (see [`PullRequestAuthor`]).
/// - `merge_state` High‑level mergeability classification returned by GitHub (see [`PullRequestMergeState`]).
///
/// # Future Work
/// - Add labels and draft status if/when used for filtering.
/// - Include head / base branch names for richer displays.
#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: usize,
    pub title: String,
    pub author: PullRequestAuthor,
    #[serde(rename = "mergeStateStatus")]
    pub merge_state: PullRequestMergeState,
}

/// Author metadata for a pull request.
///
/// Minimal surface: login + bot flag; extended profile fields are intentionally
/// omitted to keep JSON payloads small.
#[derive(Debug, Deserialize)]
pub struct PullRequestAuthor {
    pub login: String,
    pub is_bot: bool,
}

/// Merge state classification returned by GitHub's GraphQL / REST surfaces.
///
/// (Sourced via `mergeStateStatus` field.) Used to colorize and optionally
/// filter PRs prior to attempting a merge.
///
/// Variants map 1:1 to upstream values (`SCREAMING_SNAKE_CASE`) to simplify
/// deserialization and future additions.
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
/// # Arguments
/// - `repo` - `owner/name` repository spec.
/// - `search` - Optional search expression (without the `--search` flag) using GitHub search qualifiers.
/// - `retain_fn` - Predicate applied post‑fetch; only PRs for which it returns true are kept.
///
/// # Returns
/// Vector of deserialized pull requests (may be empty).
///
/// # Errors
/// - Spawning or executing `gh pr list` fails.
/// - Command exits non‑zero (handled inside [`ytil_cmd::CmdExt`]).
/// - Output JSON cannot be deserialized.
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

/// Merge a pull request using administrative squash semantics.
///
/// Invokes: `gh pr merge --admin --squash --delete-branch <PR_NUMBER>`.
///
/// # Arguments
/// - `pr_number` - Numeric pull request number.
///
/// # Returns
/// `Ok(())` if the merge command succeeds.
///
/// # Errors
/// - Spawning or executing the `gh pr merge` command fails.
/// - Command exits with non‑zero status (propagated by [`ytil_cmd::CmdExt`]).
///
/// # Rationale
/// Squash + delete keeps history linear and prunes merged topic branches automatically.
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
/// Issues an approval review for the specified pull request using the GitHub
/// CLI. Mirrors the minimalist style of [`merge`] for consistency and keeps
/// policy / flag decisions localized here.
///
/// # Arguments
/// - `pr_number` - Numeric pull request number.
///
/// # Returns
/// `Ok(())` if the review command succeeds.
///
/// # Errors
/// - Spawning or executing `gh pr review` fails.
/// - Command exits with non‑zero status (propagated by [`ytil_cmd::CmdExt`]).
pub fn approve(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args(["pr", "review", &format!("{pr_number}"), "--approve"])
        .exec()?;
    Ok(())
}

/// Trigger Dependabot to rebase a pull request.
///
/// Sends the special `@dependabot rebase` comment recognized by Dependabot to
/// request an up‑to‑date rebase of its generated pull request. Useful when the
/// PR is out-of-date with the base branch or conflicting after merges.
///
/// # Arguments
/// - `pr_number` Numeric pull request number to rebase.
///
/// # Returns
/// `Ok(())` if the comment command succeeds.
///
/// # Errors
/// - Spawning or executing `gh pr comment` fails.
/// - Command exits with non‑zero status (propagated by [`ytil_cmd::CmdExt`]).
pub fn dependabot_rebase(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args(["pr", "comment", &format!("{pr_number}"), "--body", "@dependabot rebase"])
        .exec()?;
    Ok(())
}

/// Enable GitHub auto-merge for a pull request (squash strategy).
///
/// Invokes: `gh pr merge <PR_NUMBER> --auto --squash --delete-branch`.
/// Schedules a squash merge to occur automatically once required status checks
/// and reviews pass. If all requirements are already satisfied, merge occurs immediately.
///
/// # Arguments
/// - `pr_number` Numeric pull request number to enable auto-merge on.
///
/// # Returns
/// `Ok(())` if the GitHub CLI command succeeds (either scheduling or performing the merge).
///
/// # Errors
/// - Spawning or executing `gh pr merge` fails.
/// - Command exits non-zero (propagated by [`ytil_cmd::CmdExt`]).
pub fn enable_auto_merge(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args([
            "pr",
            "merge",
            &format!("{pr_number}"),
            "--auto",
            "--squas",
            "--delete-branch",
        ])
        .exec()?;
    Ok(())
}
