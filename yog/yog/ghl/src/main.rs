//! List and optionally batch‑merge GitHub pull requests interactively
//!
//! Supports explicit flags instead of positional parameters:
//!
//! # Flags
//! - `--search <FILTER>` or `--search=<FILTER>`: forwarded to `gh pr list --search`. Optional.
//! - `--merge-state <STATE>` or `--merge-state=<STATE>`: client‑side filter over fetched PRs. Accepted
//!   (case‑insensitive): `Behind|Blocked|Clean|Dirty|Draft|HasHooks|Unknown|Unmergeable`.
//!
//! Use `--` to terminate flag parsing (subsequent arguments ignored by this tool).
//!
//! # Usage
//! ```bash
//! ghl # list all open PRs interactively
//! ghl --search "fix ci" # filter by search terms
//! ghl --merge-state Clean # filter by merge state only
//! ghl --search="lint" --merge-state Dirty # combine search + state (supports = or space)
//! ```
//!
//! # Flow
//! 1. Parse flags (`--search`, `--merge-state`) and detect current repository.
//! 2. Fetch PR list via GitHub CLI (`gh pr list`) forwarding the search filter.
//! 3. Apply optional in‑process merge state filter.
//! 4. Present multi‑select TUI.
//! 5. Attempt merge for each selected PR; report per‑PR success / failure; continue on errors.
//!
//! # Errors
//! - Flag parsing fails (unknown flag, missing value, invalid merge state).
//! - GitHub CLI invocation fails.
//! - TUI interaction fails.
#![feature(exit_status_error)]

use std::ops::Deref;
use std::str::FromStr;

use color_eyre::Section;
use color_eyre::owo_colors::OwoColorize;
use ytil_github::pr::IntoEnumIterator;
use ytil_github::pr::PullRequest;
use ytil_github::pr::PullRequestMergeState;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ytil_github::log_into_github()?;

    let repo = ytil_github::get_current_repo()?;

    let mut pargs = pico_args::Arguments::from_env();

    let search_filter: Option<String> = pargs.opt_value_from_str("--search")?;
    let merge_state = pargs
        .opt_value_from_fn("--merge-state", PullRequestMergeState::from_str)
        .with_section(|| {
            format!(
                "accepted values are: {:#?}",
                PullRequestMergeState::iter().collect::<Vec<_>>()
            )
            .red()
            .bold()
            .to_string()
        })?;

    let params = format!(
        "search_filter={search_filter:?}{}",
        merge_state
            .map(|ms| format!("\nmerge_state={ms:?}"))
            .unwrap_or_default()
    );
    println!("\n{}\n{}\n", "Search PRs by".cyan().bold(), params.white().bold());

    let pull_requests = ytil_github::pr::get(&repo, search_filter.as_deref(), &|pr: &PullRequest| {
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
                "{} {} {} {}",
                "Error merging pr".red().bold(),
                format!("number={}", pr.number).white().bold(),
                format!("title={}", pr.title).white().bold(),
                format!("error={error}").red().bold()
            )
        },
        |()| format!("{} pr={} title={}", "Merged".green().bold(), pr.number, pr.title),
    );
    println!("{msg}");
}
