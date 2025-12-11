//! List and optionally batch‑merge GitHub pull requests interactively, or create issues with associated branches.
//!
//! Provides a colorized TUI to select multiple PRs then apply a composite
//! operation (approve & merge, Dependabot rebase, enable auto-merge). Alternatively, create a GitHub issue
//! and an associated branch from the default branch. Mirrors the `run()` pattern
//! used by `gch` so the binary `main` stays trivial.
//!
//! # Flow
//! - Parse flags (`--search`, `--merge-state`, `issue`).
//! - If `issue` is present:
//!   - Prompt for issue title via [`ytil_tui::text_prompt`].
//!   - Prompt for whether to checkout the branch via [`ytil_tui::yes_no_select`].
//!   - Create issue via [`ytil_gh::issue::create`].
//!   - Develop the issue via [`ytil_gh::issue::develop`] (creates branch and optionally checks it out).
//! - Otherwise:
//!   - Detect current repository via [`ytil_gh::get_repo_view_field`].
//!   - Fetch PR list via [`ytil_gh::pr::get`] (GitHub CLI `gh pr list`) forwarding the search filter.
//!   - Apply optional in‑process merge state filter.
//!   - Present multi‑select TUI via [`ytil_tui::minimal_multi_select`].
//!   - Execute chosen high‑level operation over selected PRs, reporting per‑PR result.
//!
//! # Flags
//! - `--search <FILTER>` or `--search=<FILTER>`: forwarded to `gh pr list --search`. Optional.
//! - `--merge-state <STATE>` or `--merge-state=<STATE>`: client‑side filter over fetched PRs. Accepted
//!   (case‑insensitive) values for [`PullRequestMergeState`]:
//!   `Behind|Blocked|Clean|Dirty|Draft|HasHooks|Unknown|Unmergeable|Unstable`.
//! - `issue`: switch to issue creation mode (prompts for title, creates issue and branch).
//!
//! Use `--` to terminate flag parsing (subsequent arguments ignored by this tool).
//!
//! # Usage
//! ```bash
//! ghl # list all open PRs interactively
//! ghl --search "fix ci" # filter by search terms
//! ghl --merge-state Clean # filter by merge state only
//! ghl --search="lint" --merge-state Dirty # combine search + state (supports = or space)
//! ghl issue # create issue and branch interactively
//! ```
//!
//! # Errors
//! - Flag parsing fails (unknown flag, missing value, invalid [`PullRequestMergeState`]).
//! - GitHub CLI invocation fails (listing PRs via [`ytil_gh::pr::get`], approving via [`ytil_gh::pr::approve`], merging
//!   via [`ytil_gh::pr::merge`], commenting via [`ytil_gh::pr::dependabot_rebase`], creating issue via
//!   [`ytil_gh::issue::create`]).
//! - TUI interaction fails (selection UI errors via [`ytil_tui::minimal_multi_select`] and
//!   [`ytil_tui::minimal_select`], issue title prompt via [`ytil_tui::text_prompt`], branch checkout prompt via
//!   [`ytil_tui::yes_no_select`]).
//! - GitHub CLI invocation fails (issue and branch creation via [`ytil_gh::issue::create`] and
//!   [`ytil_gh::issue::develop`]).
//!
//! # Future Work
//! - Add dry‑run mode printing planned operations without executing.
//! - Provide additional bulk actions (labeling, commenting).
//! - Introduce structured logging (JSON) for automated auditing.

#![feature(exit_status_error)]

use core::fmt::Display;
use std::ops::Deref;
use std::str::FromStr;

use color_eyre::Section;
use color_eyre::eyre::Context as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use ytil_gh::RepoViewField;
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
        let state = match self.merge_state {
            PullRequestMergeState::Behind => "Behind".yellow().bold().to_string(),
            PullRequestMergeState::Blocked => "Blocked".red().to_string(),
            PullRequestMergeState::Clean => "Clean".green().to_string(),
            PullRequestMergeState::Dirty => "Dirty".red().bold().to_string(),
            PullRequestMergeState::Draft => "Draft".blue().bold().to_string(),
            PullRequestMergeState::HasHooks => "HasHooks".magenta().to_string(),
            PullRequestMergeState::Unknown => "Unknown".to_string(),
            PullRequestMergeState::Unmergeable => "Unmergeable".red().bold().to_string(),
            PullRequestMergeState::Unstable => "Unstable".magenta().bold().to_string(),
        };
        write!(
            f,
            // The spacing before the title is required to align it with the first line.
            "{} {} {state}\n      {}",
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
///
/// # Variants
/// - `Approve` Perform [`Op::Approve`] review.
/// - `ApproveAndMerge` Perform [`Op::Approve`] review then [`Op::Merge`] if approval succeeds.
/// - `DependabotRebase` Post the `@dependabot rebase` comment via [`Op::DependabotRebase`] to a Dependabot PR.
/// - `EnableAutoMerge` Enable [`Op::EnableAutoMerge`] (rebase strategy + delete branch) for the PR.
///
/// # Future Work
/// - Add bulk label operations (e.g. `Label` / `RemoveLabel`).
/// - Introduce `Comment` with arbitrary body once use-cases emerge.
/// - Provide dry-run variants for auditing actions.
#[derive(EnumIter)]
enum SelectableOp {
    Approve,
    ApproveAndMerge,
    DependabotRebase,
    EnableAutoMerge,
}

impl Display for SelectableOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let repr = match self {
            Self::Approve => "Approve".green().bold().to_string(),
            Self::ApproveAndMerge => "Approve & Merge".green().bold().to_string(),
            Self::DependabotRebase => "Dependabot Rebase".blue().bold().to_string(),
            Self::EnableAutoMerge => "Enable auto-merge".magenta().bold().to_string(),
        };
        write!(f, "{repr}")
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
    /// # Arguments
    /// - `pr` Subject pull request.
    /// - `res` [`color_eyre::Result`] returned by the underlying GitHub CLI wrapper.
    ///
    /// # Returns
    /// Propagates `res` unchanged after side‑effect logging.
    ///
    /// # Errors
    /// Returns the same error contained in `res` (no transformation) so callers
    /// can continue combinators (`and_then`, etc.) if desired.
    pub fn report(&self, pr: &PullRequest, res: color_eyre::Result<()>) -> color_eyre::Result<()> {
        res.inspect(|()| self.report_ok(pr)).inspect_err(|err| {
            self.report_error(pr, err);
        })
    }

    /// Emit a success line for the completed operation.
    ///
    /// # Arguments
    /// - `pr` Pull request just processed successfully.
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
    ///
    /// # Arguments
    /// - `pr` [`PullRequest`] Pull request that failed to process.
    /// - `error` [`color_eyre::Report`] returned by the CLI wrapper.
    ///
    /// # Rationale
    /// Keeps multi‑line error payload visually grouped with the PR metadata.
    fn report_error(&self, pr: &PullRequest, error: &color_eyre::Report) {
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
///
/// Builds a single colorized string containing number, quoted title, and
/// debug formatting of the author object.
///
/// # Arguments
/// - `pr` Pull request whose identifying fields will be rendered.
///
/// # Returns
/// Colorized composite string suitable for direct printing.
///
/// # Rationale
/// Central helper avoids duplicating formatting order and styling decisions.
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
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

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

    let repo_name_with_owner = ytil_gh::get_repo_view_field(&RepoViewField::NameWithOwner)?;

    let search_filter: Option<String> = pargs.opt_value_from_str("--search")?;
    let merge_state = pargs
        .opt_value_from_fn("--merge-state", PullRequestMergeState::from_str)
        .with_section(|| {
            format!(
                "accepted values are {:#?}",
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

/// Create a GitHub issue and develop it with an associated branch.
///
/// Prompts the user for an issue title, creates the issue via GitHub CLI,
/// then develops it by creating an associated branch from the default branch.
/// Optionally checks out the newly created branch based on user preference.
///
/// # Returns
/// - `()` on successful completion or if the user cancels at any prompt.
///
/// # Errors
/// - If [`ytil_tui::text_prompt`] fails when prompting for issue title.
/// - If [`ytil_tui::yes_no_select`] fails when prompting for branch checkout preference.
/// - If [`ytil_gh::issue::create`] fails when creating the GitHub issue.
/// - If [`ytil_gh::issue::develop`] fails when creating the associated branch.
///
/// # Rationale
/// Separates issue creation flow from PR listing flow, allowing users to quickly
/// bootstrap new work items without leaving the terminal interface.
fn create_issue_and_branch_from_default_branch() -> Result<(), color_eyre::eyre::Error> {
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
/// # Returns
/// - `()` on success or if no branch is selected.
///
/// # Errors
/// - If [`ytil_tui::git_branch::select`] fails.
/// - If [`pr_title_from_branch_name`] fails.
/// - If [`ytil_gh::pr::create`] fails.
fn create_pr() -> Result<(), color_eyre::eyre::Error> {
    let Some(branch) = ytil_tui::git_branch::select()? else {
        return Ok(());
    };

    let title = pr_title_from_branch_name(branch.name_no_origin())?;
    let pr_url = ytil_gh::pr::create(&title)?;
    println!("{} title={title:?} pr_url={pr_url:?}", "PR created".green().bold());

    Ok(())
}

/// Parses a branch name to generate a pull request title.
///
/// # Arguments
/// - `branch_name` The branch name in the format `{issue_number}-{title-words}`.
///
/// # Returns
/// The formatted pull request title as `[{issue_number}]: {Capitalized Title}`.
///
/// # Errors
/// - Branch name has no parts separated by `-`.
/// - The first part is not a valid usize for issue number.
/// - The title parts result in an empty title.
fn pr_title_from_branch_name(branch_name: &str) -> color_eyre::Result<String> {
    let mut parts = branch_name.split('-');

    let issue_number: usize = parts
        .next()
        .ok_or_else(|| eyre!("error malformed branch_name | branch_name={branch_name:?}"))
        .and_then(|x| {
            x.parse().wrap_err_with(|| {
                format!("error parsing issue number | branch_name={branch_name:?} issue_number={x:?}")
            })
        })?;

    let title = parts
        .enumerate()
        .map(|(i, word)| {
            if i == 0 {
                let mut chars = word.chars();
                let Some(first) = chars.next() else {
                    return String::new();
                };
                return first.to_uppercase().chain(chars.as_str().chars()).collect();
            }
            word.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ");

    if title.is_empty() {
        bail!("error empty title | branch_name={branch_name:?}");
    }

    Ok(format!("[{issue_number}]: {title}"))
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
    #[case(
        "abc-foo",
        r#"error parsing issue number | branch_name="abc-foo" issue_number="abc""#
    )]
    #[case("42", r#"error empty title | branch_name="42""#)]
    #[case("", r#"error parsing issue number | branch_name="" issue_number="""#)]
    fn pr_title_from_branch_name_when_invalid_input_returns_error(#[case] input: &str, #[case] expected_error: &str) {
        assert2::let_assert!(Err(err) = pr_title_from_branch_name(input));
        pretty_assertions::assert_eq!(err.to_string(), expected_error);
    }
}
