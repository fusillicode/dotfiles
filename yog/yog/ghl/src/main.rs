//! List and optionally batch‑merge GitHub pull requests interactively.
//!
//! Provides a colorized TUI to select multiple PRs then apply a composite
//! operation (approve & merge, Dependabot rebase, enable auto-merge). Mirrors the `run()` pattern
//! used by `gch` so the binary `main` stays trivial.
//!
//! # Flow
//! - Parse flags (`--search`, `--merge-state`).
//! - Detect current repository via [`ytil_github::get_repo_view_field`].
//! - Fetch PR list via [`ytil_github::pr::get`] (GitHub CLI `gh pr list`) forwarding the search filter.
//! - Apply optional in‑process merge state filter.
//! - Present multi‑select TUI via [`ytil_tui::minimal_multi_select`].
//! - Execute chosen high‑level operation over selected PRs, reporting per‑PR result.
//!
//! # Flags
//! - `--search <FILTER>` or `--search=<FILTER>`: forwarded to `gh pr list --search`. Optional.
//! - `--merge-state <STATE>` or `--merge-state=<STATE>`: client‑side filter over fetched PRs. Accepted
//!   (case‑insensitive) values for [`PullRequestMergeState`]:
//!   `Behind|Blocked|Clean|Dirty|Draft|HasHooks|Unknown|Unmergeable`.
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
//! # Errors
//! - Flag parsing fails (unknown flag, missing value, invalid [`PullRequestMergeState`]).
//! - GitHub CLI invocation fails (listing PRs via [`ytil_github::pr::get`], approving via [`ytil_github::pr::approve`],
//!   merging via [`ytil_github::pr::merge`], commenting via [`ytil_github::pr::dependabot_rebase`]).
//! - TUI interaction fails (selection UI errors via [`ytil_tui::minimal_multi_select`] and
//!   [`ytil_tui::minimal_select`]).
//!
//! # Future Work
//! - Add dry‑run mode printing planned operations without executing.
//! - Provide additional bulk actions (labeling, commenting).
//! - Introduce structured logging (JSON) for automated auditing.

#![feature(exit_status_error)]

use core::fmt::Display;
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;

use color_eyre::Section;
use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use ytil_github::RepoViewField;
use ytil_github::pr::IntoEnumIterator;
use ytil_github::pr::PullRequest;
use ytil_github::pr::PullRequestMergeState;
use ytil_system::CliArgs as _;
use ytil_system::pico_args::Arguments;

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
                let _ = Op::Approve.report(pr, ytil_github::pr::approve(pr.number));
            }),
            Self::ApproveAndMerge => Box::new(|pr| {
                let _ = Op::Approve
                    .report(pr, ytil_github::pr::approve(pr.number))
                    .and_then(|()| Op::Merge.report(pr, ytil_github::pr::merge(pr.number)));
            }),
            Self::DependabotRebase => Box::new(|pr| {
                let _ = Op::DependabotRebase.report(pr, ytil_github::pr::dependabot_rebase(pr.number));
            }),
            Self::EnableAutoMerge => Box::new(|pr| {
                let _ = Op::EnableAutoMerge.report(pr, ytil_github::pr::enable_auto_merge(pr.number));
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
/// - `Approve` Submit an approving review via [`ytil_github::pr::approve`] (`gh pr review --approve`).
/// - `Merge` Perform the administrative squash merge via [`ytil_github::pr::merge`] (`gh pr merge --admin --squash`).
/// - `DependabotRebase` Post the `@dependabot rebase` comment via [`ytil_github::pr::dependabot_rebase`] to request an
///   updated rebase for a Dependabot PR.
/// - `EnableAutoMerge` Schedule automatic merge via [`ytil_github::pr::enable_auto_merge`] (rebase) once requirements
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

/// List and optionally batch‑merge GitHub pull requests interactively.
///
/// Mirrors the design of `gch::run` so the binary `main` remains a thin wrapper.
/// Performs GitHub authentication via [`ytil_github::log_into_github`], flag parsing, PR fetching via
/// [`ytil_github::pr::get`], selection via [`ytil_tui::minimal_multi_select`] and [`ytil_tui::minimal_select`], and
/// application of user‑chosen operations.
///
/// # Returns
/// `Ok(())` if all operations complete (individual PR action failures are reported but do not abort processing).
///
/// # Errors
/// - Flag parsing fails (unknown flag, missing value, invalid [`PullRequestMergeState`]).
/// - GitHub CLI invocation fails (listing PRs via [`ytil_github::pr::get`], approving via [`ytil_github::pr::approve`],
///   merging via [`ytil_github::pr::merge`], commenting via [`ytil_github::pr::dependabot_rebase`]).
/// - TUI interaction fails (selection UI errors via [`ytil_tui::minimal_multi_select`] and
///   [`ytil_tui::minimal_select`]).
///
/// # Rationale
/// Uniform `run()` entrypoint across tools (`gch`, `ghl`) simplifies integration (e.g. shared launcher invoking
/// `<tool>::run()`).
///
/// # Future Work
/// - Surface aggregated failure summary at end of run.
/// - Inject CLI dependencies for isolated testing.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut pargs = Arguments::from_env();
    if pargs.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    ytil_github::log_into_github()?;

    if pargs.contains("issue") {
        create_issue_and_branch_from_default_branch()?;
        return Ok(());
    }

    let repo_name_with_owner = ytil_github::get_repo_view_field(&RepoViewField::NameWithOwner)?;

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

    let pull_requests = ytil_github::pr::get(&repo_name_with_owner, search_filter.as_deref(), &|pr: &PullRequest| {
        if let Some(merge_state) = merge_state {
            return pr.merge_state == merge_state;
        }
        true
    })?;

    let renderable_prs: Vec<_> = pull_requests.into_iter().map(RenderablePullRequest).collect();
    if renderable_prs.is_empty() {
        println!(
            "{}\n{}",
            "No PRs matching supplied".yellow().bold(),
            params.white().bold()
        );
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

/// Creates a GitHub issue and a corresponding branch from the default branch.
///
/// Prompts the user for an issue title, creates the issue, creates and pushes a branch named after the issue.
///
/// # Rationale
/// It's not possible to create a GitHub PR from a branch that has no diff with the head branch.
/// For this reason this function just creates and pushes a branch without creating a GitHub PR.
///
/// # Returns
/// Returns `Ok(())` on successful creation.
/// Returns an error if prompting fails, issue creation fails, branch creation fails, or pushing fails.
///
/// # Errors
/// - [`ytil_tui::Text::prompt`] failure if user input cannot be obtained.
/// - [`ytil_github::create_issue`] failure if the issue cannot be created.
/// - [`ytil_git::branch::create_from_default_branch`] failure if the branch cannot be created.
/// - [`ytil_git::branch::push`] failure if pushing the branch fails.
fn create_issue_and_branch_from_default_branch() -> Result<(), color_eyre::eyre::Error> {
    let issue_title = ytil_tui::Text::new("Issue title:").prompt()?;

    let created_issue = ytil_github::create_issue(&issue_title)?;
    println!("\n{} with title={issue_title:?}", "Issue created".green().bold());

    let branch_name = created_issue.branch_name();

    let current_repo = ytil_git::discover_repo(Path::new("."))?;

    ytil_git::branch::create_from_default_branch(&branch_name, Some(&current_repo))?;
    println!("{} with name={branch_name:?}", "Branch created".green().bold());

    ytil_git::branch::push(&branch_name, Some(&current_repo))?;
    println!("{} name={branch_name:?}", "Branch pushed".green().bold());

    Ok(())
}
