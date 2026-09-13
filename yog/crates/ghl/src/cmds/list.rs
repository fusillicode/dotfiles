//! Interactive pull request listing and operations.

use std::fmt::Display;
use std::fmt::Formatter;
use std::ops::Deref;
use std::str::FromStr;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use strum::EnumIter;
use ytil_gh::RepoViewField;
use ytil_gh::pr::IntoEnumIterator;
use ytil_gh::pr::PullRequest;
use ytil_gh::pr::PullRequestMergeState;
use ytil_sys::pico_args::Arguments;

/// List and optionally batch‑merge GitHub pull requests interactively.
///
/// # Errors
/// - Flag parsing fails (unknown flag, missing value, invalid [`PullRequestMergeState`]).
/// - GitHub CLI invocation fails (listing PRs via [`ytil_gh::pr::get`], approving via [`ytil_gh::pr::approve`], merging
///   via [`ytil_gh::pr::merge`], commenting via [`ytil_gh::pr::dependabot_rebase`]).
/// - TUI interaction fails (PR selection or operation selection).
pub fn run(mut pargs: Arguments) -> rootcause::Result<()> {
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

    let Some(selected_prs) = ytil_tui::minimal_multi_select(renderable_prs, ToString::to_string, ToString::to_string)?
    else {
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Write directly to the formatter, avoiding intermediate String allocations from .to_string()
        write!(
            f,
            "{} {} ",
            self.author.login.blue().bold(),
            self.updated_at.strftime("%d-%m-%Y %H:%M UTC")
        )?;
        match self.merge_state {
            PullRequestMergeState::Behind => write!(f, "{} ", "Behind".yellow().bold())?,
            PullRequestMergeState::Blocked => write!(f, "{} ", "Blocked".red())?,
            PullRequestMergeState::Clean => write!(f, "{} ", "Clean".green())?,
            PullRequestMergeState::Dirty => write!(f, "{} ", "Dirty".red().bold())?,
            PullRequestMergeState::Draft => write!(f, "{} ", "Draft".blue().bold())?,
            PullRequestMergeState::HasHooks => write!(f, "{} ", "HasHooks".magenta())?,
            PullRequestMergeState::Unknown => write!(f, "Unknown ")?,
            PullRequestMergeState::Unmergeable => write!(f, "{} ", "Unmergeable".red().bold())?,
            PullRequestMergeState::Unstable => write!(f, "{} ", "Unstable".magenta().bold())?,
        }
        write!(f, "{}", self.title)
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
                drop(Op::Approve.report(pr, ytil_gh::pr::approve(pr.number)));
            }),
            Self::ApproveAndMerge => Box::new(|pr| {
                drop(
                    Op::Approve
                        .report(pr, ytil_gh::pr::approve(pr.number))
                        .and_then(|()| Op::Merge.report(pr, ytil_gh::pr::merge(pr.number))),
                );
            }),
            Self::DependabotRebase => Box::new(|pr| {
                drop(Op::DependabotRebase.report(pr, ytil_gh::pr::dependabot_rebase(pr.number)));
            }),
            Self::EnableAutoMerge => Box::new(|pr| {
                drop(Op::EnableAutoMerge.report(pr, ytil_gh::pr::enable_auto_merge(pr.number)));
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
