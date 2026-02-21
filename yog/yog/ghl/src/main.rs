//! List and batch-merge GitHub pull requests interactively, or create issues with branches.
//!
//! # Errors
//! - Flag parsing, GitHub CLI invocation, or TUI interaction fails.
#![feature(exit_status_error)]

use core::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use strum::EnumIter;
use ytil_gh::RepoViewField;
use ytil_gh::issue::ListedIssue;
use ytil_gh::pr::IntoEnumIterator;
use ytil_gh::pr::PullRequest;
use ytil_gh::pr::PullRequestMergeState;
use ytil_sys::cli::Args as _;
use ytil_sys::pico_args::Arguments;

/// Newtype wrapper implementing colored [`Display`] for a [`PullRequest`].
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

impl Display for RenderablePullRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Write directly to the formatter, avoiding intermediate String allocations from .to_string()
        write!(
            f,
            "{} {} ",
            self.author.login.blue().bold(),
            self.updated_at.format("%d-%m-%Y %H:%M UTC")
        )?;
        match self.merge_state {
            PullRequestMergeState::Behind => write!(f, "{} ", "Behind".yellow().bold())?,
            PullRequestMergeState::Blocked => write!(f, "{} ", "Blocked".red())?,
            PullRequestMergeState::Clean => write!(f, "{} ", "Clean".green())?,
            PullRequestMergeState::Dirty => write!(f, "{} ", "Dirty".red().bold())?,
            PullRequestMergeState::Draft => write!(f, "{} ", "Draft".blue().bold())?,
            PullRequestMergeState::HasHooks => write!(f, "{} ", "HasHooks".magenta())?,
            PullRequestMergeState::Unknown => write!(f, "Unknown")?,
            PullRequestMergeState::Unmergeable => write!(f, "{} ", "Unmergeable".red().bold())?,
            PullRequestMergeState::Unstable => write!(f, "{} ", "Unstable".magenta().bold())?,
        }
        write!(f, "{}", self.title)
    }
}

struct RenderableListedIssue(pub ListedIssue);

impl Deref for RenderableListedIssue {
    type Target = ListedIssue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableListedIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            // The spacing before the title is required to align it with the first line.
            "{} {} {}",
            self.author.login.blue().bold(),
            self.updated_at.format("%d-%m-%Y %H:%M UTC"),
            self.title
        )
    }
}

/// User-selectable high-level operations to apply to chosen PRs.
///
/// Encapsulates composite actions presented in the TUI. Separate from [`Op`]
/// which models the underlying atomic steps and reporting. Expanding this enum
/// only affects menu construction / selection logic.
#[derive(EnumIter)]
enum SelectableOp {
    Approve,
    ApproveAndMerge,
    DependabotRebase,
    EnableAutoMerge,
}

impl Display for SelectableOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approve => write!(f, "{}", "Approve".green().bold()),
            Self::ApproveAndMerge => write!(f, "{}", "Approve & Merge".green().bold()),
            Self::DependabotRebase => write!(f, "{}", "Dependabot Rebase".blue().bold()),
            Self::EnableAutoMerge => write!(f, "{}", "Enable auto-merge".magenta().bold()),
        }
    }
}

impl SelectableOp {
    pub fn run(&self) -> Box<dyn Fn(&PullRequest)> {
        match self {
            Self::Approve => Box::new(|pr| {
                let _ = Op::Approve.report(pr, ytil_gh::pr::approve(pr.number));
            }),
            Self::ApproveAndMerge => Box::new(|pr| {
                let _ = Op::Approve
                    .report(pr, ytil_gh::pr::approve(pr.number))
                    .and_then(|()| Op::Merge.report(pr, ytil_gh::pr::merge(pr.number)));
            }),
            Self::DependabotRebase => Box::new(|pr| {
                let _ = Op::DependabotRebase.report(pr, ytil_gh::pr::dependabot_rebase(pr.number));
            }),
            Self::EnableAutoMerge => Box::new(|pr| {
                let _ = Op::EnableAutoMerge.report(pr, ytil_gh::pr::enable_auto_merge(pr.number));
            }),
        }
    }
}

/// Atomic pull request operations executed by `ghl`.
///
/// Represents each discrete action the tool can perform against a selected
/// pull request. Higher‑level composite choices in the TUI (see [`SelectableOp`])
/// sequence these as needed. Centralizing variants here keeps reporting logic
/// (`report`, `report_ok`, `report_error`) uniform and extensible.
///
/// # Variants
/// - `Approve` Submit an approving review via [`ytil_gh::pr::approve`] (`gh pr review --approve`).
/// - `Merge` Perform the administrative squash merge via [`ytil_gh::pr::merge`] (`gh pr merge --admin --squash`).
/// - `DependabotRebase` Post the `@dependabot rebase` comment via [`ytil_gh::pr::dependabot_rebase`] to request an
///   updated rebase for a Dependabot PR.
/// - `EnableAutoMerge` Schedule automatic merge via [`ytil_gh::pr::enable_auto_merge`] (rebase) once requirements
///   satisfied.
enum Op {
    Approve,
    Merge,
    DependabotRebase,
    EnableAutoMerge,
}

impl Op {
    /// Report the result of executing an operation on a pull request.
    ///
    /// Delegates to success / error helpers that emit colorized, structured
    /// terminal output. Keeps call‑site chaining terse while centralizing the
    /// formatting logic.
    ///
    /// # Errors
    /// Returns the same error contained in `res` (no transformation) so callers
    /// can continue combinators (`and_then`, etc.) if desired.
    pub fn report(&self, pr: &PullRequest, res: rootcause::Result<()>) -> rootcause::Result<()> {
        res.inspect(|()| self.report_ok(pr)).inspect_err(|err| {
            self.report_error(pr, err);
        })
    }

    /// Emit a success line for the completed operation.
    fn report_ok(&self, pr: &PullRequest) {
        let msg = match self {
            Self::Approve => "Approved",
            Self::Merge => "Merged",
            Self::DependabotRebase => "Dependabot rebased",
            Self::EnableAutoMerge => "Auto-merge enabled",
        };
        println!("{} {}", format!("{msg} PR").green().bold(), format_pr(pr));
    }

    /// Emit a structured error report for a failed operation.
    fn report_error(&self, pr: &PullRequest, error: &rootcause::Report) {
        let msg = match self {
            Self::Approve => "approving",
            Self::Merge => "merging",
            Self::DependabotRebase => "triggering dependabot rebase",
            Self::EnableAutoMerge => "enabling auto-merge",
        };
        eprintln!(
            "{} {} error=\n{}",
            format!("Error {msg} PR").red(),
            format_pr(pr),
            format!("{error:#?}").red()
        );
    }
}

/// Format concise identifying PR fields for log / status lines.
fn format_pr(pr: &PullRequest) -> String {
    format!(
        "{}{:?} {}{:?} {}{:?}",
        "number=".white().bold(),
        pr.number,
        "title=".white().bold(),
        pr.title,
        "author=",
        pr.author,
    )
}

/// Create a GitHub issue and develop it with an associated branch.
///
/// # Errors
/// - User interaction or GitHub CLI operations fail.
fn create_issue_and_branch_from_default_branch() -> Result<(), rootcause::Report> {
    let Some(issue_title) = ytil_tui::text_prompt("Issue title:")?.map(|x| x.trim().to_string()) else {
        return Ok(());
    };

    let Some(checkout_branch) = ytil_tui::yes_no_select("Checkout branch?")? else {
        return Ok(());
    };

    let created_issue = ytil_gh::issue::create(&issue_title)?;
    println!(
        "\n{} number={} title={issue_title:?}",
        "Issue created".green().bold(),
        created_issue.issue_nr
    );

    let develop_output = ytil_gh::issue::develop(&created_issue.issue_nr, checkout_branch)?;
    println!(
        "{} with name={:?}",
        "Branch created".green().bold(),
        develop_output.branch_name
    );

    Ok(())
}

/// Prompts the selection of a branch and creates a pull request for the selected one.
///
/// # Errors
/// - If [`ytil_tui::git_branch::select`] fails.
/// - If [`pr_title_from_branch_name`] fails.
/// - If [`ytil_gh::pr::create`] fails.
fn create_pr() -> Result<(), rootcause::Report> {
    let Some(branch) = ytil_tui::git_branch::select()? else {
        return Ok(());
    };

    let title = pr_title_from_branch_name(branch.name_no_origin())?;
    let pr_url = ytil_gh::pr::create(&title)?;
    println!("{} title={title:?} pr_url={pr_url:?}", "PR created".green().bold());

    Ok(())
}

/// Interactively creates a GitHub branch from a selected issue.
///
/// # Errors
/// - Issue listing, user selection, or branch development fails.
fn create_branch_from_issue() -> Result<(), rootcause::Report> {
    let issues = ytil_gh::issue::list()?;

    let Some(issue) = ytil_tui::minimal_select(issues.into_iter().map(RenderableListedIssue).collect())? else {
        return Ok(());
    };

    let Some(checkout_branch) = ytil_tui::yes_no_select("Checkout branch?")? else {
        return Ok(());
    };

    let develop_output = ytil_gh::issue::develop(&issue.number.to_string(), checkout_branch)?;
    println!(
        "{} with name={:?}",
        "Branch created".green().bold(),
        develop_output.branch_name
    );

    Ok(())
}

/// Parses a branch name to generate a pull request title.
///
/// # Errors
/// - Branch name has no parts separated by `-`.
/// - The first part is not a valid usize for issue number.
/// - The title parts result in an empty title.
fn pr_title_from_branch_name(branch_name: &str) -> rootcause::Result<String> {
    let mut parts = branch_name.split('-');

    let x = parts
        .next()
        .ok_or_else(|| report!("error malformed branch_name"))
        .attach_with(|| format!("branch_name={branch_name:?}"))?;
    let issue_number: usize = x
        .parse()
        .context("error parsing issue number")
        .attach_with(|| format!("branch_name={branch_name:?} issue_number={x:?}"))?;

    let mut title = String::with_capacity(branch_name.len());
    for (i, word) in parts.enumerate() {
        if i > 0 {
            title.push(' ');
        }
        if i == 0 {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                for c in first.to_uppercase() {
                    title.push(c);
                }
                title.push_str(chars.as_str());
            }
        } else {
            title.push_str(word);
        }
    }

    if title.is_empty() {
        Err(report!("error empty title")).attach_with(|| format!("branch_name={branch_name:?}"))?;
    }

    Ok(format!("[{issue_number}]: {title}"))
}

/// List and optionally batch‑merge GitHub pull requests interactively or create issues with associated branches.
///
/// # Errors
/// - Flag parsing fails (unknown flag, missing value, invalid [`PullRequestMergeState`]).
/// - GitHub CLI invocation fails (listing PRs via [`ytil_gh::pr::get`], approving via [`ytil_gh::pr::approve`], merging
///   via [`ytil_gh::pr::merge`], commenting via [`ytil_gh::pr::dependabot_rebase`], creating issue via
///   [`ytil_gh::issue::create`]).
/// - TUI interaction fails (selection UI errors via [`ytil_tui::minimal_multi_select`] and
///   [`ytil_tui::minimal_select`], issue title prompt via [`ytil_tui::text_prompt`], branch checkout prompt via
///   [`ytil_tui::yes_no_select`]).
/// - GitHub CLI invocation fails (issue and branch creation via [`ytil_gh::issue::create`] and
///   [`ytil_gh::issue::develop`]).
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let mut pargs = Arguments::from_env();
    if pargs.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    ytil_gh::log_into_github()?;

    if pargs.contains("issue") {
        create_issue_and_branch_from_default_branch()?;
        return Ok(());
    }

    if pargs.contains("pr") {
        create_pr()?;
        return Ok(());
    }

    if pargs.contains("branch") {
        create_branch_from_issue()?;
        return Ok(());
    }

    let repo_name_with_owner = ytil_gh::get_repo_view_field(&RepoViewField::NameWithOwner)?;

    let search_filter: Option<String> = pargs.opt_value_from_str("--search")?;
    let merge_state = pargs
        .opt_value_from_fn("--merge-state", PullRequestMergeState::from_str)
        .attach_with(|| {
            format!(
                "accepted values are {:#?}",
                PullRequestMergeState::iter().collect::<Vec<_>>()
            )
        })?;

    let params = format!(
        "search_filter={search_filter:?}{}",
        merge_state
            .map(|ms| format!("\nmerge_state={ms:?}"))
            .unwrap_or_default()
    );
    println!("\n{}\n{}\n", "Search PRs by".cyan().bold(), params.white().bold());

    let pull_requests = ytil_gh::pr::get(&repo_name_with_owner, search_filter.as_deref(), &|pr: &PullRequest| {
        if let Some(merge_state) = merge_state {
            return pr.merge_state == merge_state;
        }
        true
    })?;

    let renderable_prs: Vec<_> = pull_requests.into_iter().map(RenderablePullRequest).collect();
    if renderable_prs.is_empty() {
        println!("{}\n{}", "No matching PRs found".yellow().bold(), params.white().bold());
        return Ok(());
    }

    let Some(selected_prs) = ytil_tui::minimal_multi_select::<RenderablePullRequest>(renderable_prs)? else {
        println!("No PRs selected");
        return Ok(());
    };

    let Some(selected_op) = ytil_tui::minimal_select::<SelectableOp>(SelectableOp::iter().collect())? else {
        println!("No operation selected");
        return Ok(());
    };

    println!(); // Cosmetic spacing.

    let selected_op_run = selected_op.run();
    for pr in selected_prs.iter().map(Deref::deref) {
        selected_op_run(pr);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("43-foo-bar-baz", "[43]: Foo bar baz")]
    #[case("1-hello", "[1]: Hello")]
    #[case("123-long-branch-name-here", "[123]: Long branch name here")]
    fn pr_title_from_branch_name_when_valid_input_formats_correctly(#[case] input: &str, #[case] expected: &str) {
        pretty_assertions::assert_eq!(pr_title_from_branch_name(input).unwrap(), expected);
    }

    #[rstest]
    #[case("abc-foo", "error parsing issue number")]
    #[case("42", "error empty title")]
    #[case("", "error parsing issue number")]
    fn pr_title_from_branch_name_when_invalid_input_returns_error(#[case] input: &str, #[case] expected_ctx: &str) {
        assert2::assert!(let Err(err) = pr_title_from_branch_name(input));
        assert_eq!(err.format_current_context().to_string(), expected_ctx);
    }
}
