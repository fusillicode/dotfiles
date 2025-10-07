//! List and optionally merge GitHub pull requests interactively.
//!
//! # Arguments
//! - `search_filter` Optional free-form search string forwarded to `gh pr list --search`.
//! - `merge_state` Optional merge state filter (`Behind|Blocked|Clean|Dirty|Draft|HasHooks|Unknown|Unmergeable`).
//!
//! # Usage
//! ```bash
//! ghl # list all open PRs interactively
//! ghl "fix ci" # filter via search terms
//! ghl "lint" Clean # search + restrict to Clean mergeable PRs
//! ```
//!
//! # Flow
//! 1. Resolve current repo + optional filters.
//! 2. Fetch PR list (client-side merge state filtering when provided).
//! 3. Multi-select PRs.
//! 4. Attempt merge each; failures reported inline without aborting.
//!
//! # Errors
//! - GitHub CLI invocations fail.
//! - Merge state string fails to parse.
//! - TUI interaction fails.
//!
//! # Rationale
//! Provide a focused alternative to opening a browser or chaining multiple `gh pr` commands when triaging batches of
//! routine PRs while keeping the implementation lean and synchronous.
#![feature(exit_status_error)]

use std::ops::Deref;
use std::str::FromStr;

use color_eyre::owo_colors::OwoColorize;
use ytil_github::pr::PullRequest;
use ytil_github::pr::PullRequestMergeState;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ytil_github::log_into_github()?;

    let repo = ytil_github::get_current_repo()?;

    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let search_filter = args.first().copied();
    let merge_state = args.get(1).copied().map(PullRequestMergeState::from_str).transpose()?;

    let params = format!(
        "search_filter={search_filter:?}{}",
        merge_state
            .map(|ms| format!("\nmerge_state={ms:?}"))
            .unwrap_or_default()
    );
    println!("\n{}\n{}", "Search PRs by".cyan().bold(), params.white().bold());

    let pull_requests = ytil_github::pr::get(&repo, search_filter, &|pr: &PullRequest| {
        if let Some(merge_state) = merge_state {
            return pr.merge_state == merge_state;
        }
        true
    })?;

    let renderable_prs: Vec<_> = pull_requests.into_iter().map(RenderablePullRequest).collect();
    if renderable_prs.is_empty() {
        println!("\n{}", "No PRs matching search criteria".yellow().bold());
    }

    let Some(selected_prs) = ytil_tui::minimal_multi_select::<RenderablePullRequest>(renderable_prs)? else {
        return Ok(());
    };

    for pr in selected_prs.iter().map(Deref::deref) {
        merge_pr(pr);
    }

    Ok(())
}

/// Newtype wrapper implementing colored [`core::fmt::Display`] for a [`PullRequest`].
///
/// Renders: `<number> <author.login> <colored-merge-state> <title>`.
/// Merge state receives a color to aid quick scanning.
pub struct RenderablePullRequest(pub PullRequest);

impl Deref for RenderablePullRequest {
    type Target = PullRequest;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::fmt::Display for RenderablePullRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self.merge_state {
            PullRequestMergeState::Behind => "Behind".yellow().to_string(),
            PullRequestMergeState::Blocked => "Blocked".red().bold().to_string(),
            PullRequestMergeState::Clean => "Clean".green().to_string(),
            PullRequestMergeState::Dirty => "Dirty".red().bold().to_string(),
            PullRequestMergeState::Draft => "Draft".blue().bold().to_string(),
            PullRequestMergeState::HasHooks => "HasHooks".magenta().bold().to_string(),
            PullRequestMergeState::Unknown => "Unknown".bold().to_string(),
            PullRequestMergeState::Unmergeable => "Unmergeable".red().bold().to_string(),
        };
        write!(
            f,
            "{} {} {state} {}",
            self.number.white().bold(),
            self.author.login.blue().bold(),
            self.title.white().bold()
        )
    }
}

/// Attempt to merge the provided pull request and print a colored status line.
///
/// On success prints: `Merged pr=<N> title=<TITLE>` (green).
/// On failure prints: `Error merging ... error=<E>` (red) but does not abort.
fn merge_pr(pr: &PullRequest) {
    let msg = ytil_github::pr::merge(pr.number).map_or_else(
        |error| {
            format!(
                "{} pr={} title={} error={}",
                "Error merging".red().bold(),
                pr.number,
                pr.title,
                format!("{error:?}").red().bold()
            )
        },
        |()| format!("{} pr={} title={}", "Merged".green().bold(), pr.number, pr.title),
    );
    println!("{msg}");
}
