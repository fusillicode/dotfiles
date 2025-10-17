//! List and optionally batch‑merge GitHub pull requests interactively.
//!
//! Provides a colorized TUI to select multiple PRs then apply a composite
//! operation (approve & merge, dependabot rebase). Mirrors the `run()` pattern
//! used by `gch` so the binary `main` stays trivial.
//!
//! # Flow
//! - Parse flags (`--search`, `--merge-state`).
//! - Detect current repository.
//! - Fetch PR list via GitHub CLI (`gh pr list`) forwarding the search filter.
//! - Apply optional in‑process merge state filter.
//! - Present multi‑select TUI.
//! - Execute chosen high‑level operation over selected PRs, reporting per‑PR result.
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
//! # Errors
//! - Flag parsing fails (unknown flag, missing value, invalid merge state).
//! - GitHub CLI invocation fails.
//! - TUI interaction fails.
//!
//! # Rationale
//! - Extracting [`run`] keeps application initialization (`main`) minimal and parallels `gch` for consistency.
//! - Centralizing logic eases future testability (can introduce dependency injection without touching binary
//!   entrypoint).
//!
//! # Future Work
//! - Add dry‑run mode printing planned operations without executing.
//! - Provide additional bulk actions (labeling, commenting).
//! - Introduce structured logging (JSON) for automated auditing.

#![feature(exit_status_error)]

use std::ops::Deref;
use std::str::FromStr;

use color_eyre::Section;
use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use ytil_github::pr::IntoEnumIterator;
use ytil_github::pr::PullRequest;
use ytil_github::pr::PullRequestMergeState;

/// Execute the interactive pull request listing and batch operation flow.
///
/// Mirrors the design of `gch::run` so the binary `main` remains a thin wrapper.
/// Performs GitHub authentication, flag parsing, PR fetching, selection, and
/// application of user‑chosen operations.
///
/// # Returns
/// `Ok(())` if all operations complete (individual PR action failures are reported but do not abort processing).
///
/// # Errors
/// - Flag parsing fails (unknown flag, missing value, invalid merge state).
/// - GitHub CLI invocation fails (listing PRs, approving, merging, commenting).
/// - TUI interaction fails (selection UI errors).
///
/// # Rationale
/// Uniform `run()` entrypoint across tools (`gch`, `ghl`) simplifies integration (e.g. shared launcher invoking
/// `<tool>::run()`).
///
/// # Future Work
/// - Surface aggregated failure summary at end of run.
/// - Inject CLI dependencies for isolated testing.
pub fn run() -> color_eyre::Result<()> {
    color_eyre::install()?; // Keep install here to match previous behavior and isolate side effects.
    ytil_github::log_into_github()?;

    let repo = ytil_github::get_current_repo()?;

    let mut pargs = pico_args::Arguments::from_env();

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

    let pull_requests = ytil_github::pr::get(&repo, search_filter.as_deref(), &|pr: &PullRequest| {
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
        return Ok(());
    };

    let Some(selected_op) = ytil_tui::minimal_select::<SelectableOp>(SelectableOp::iter().collect())? else {
        return Ok(());
    };

    println!(); // Cosmetic spacing.

    let selected_op_run = selected_op.run();
    for pr in selected_prs.iter().map(Deref::deref) {
        selected_op_run(pr);
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
            PullRequestMergeState::Behind => "Behind".yellow().bold().to_string(),
            PullRequestMergeState::Blocked => "Blocked".red().bold().to_string(),
            PullRequestMergeState::Clean => "Clean".green().to_string(),
            PullRequestMergeState::Dirty => "Dirty".red().bold().to_string(),
            PullRequestMergeState::Draft => "Draft".blue().bold().to_string(),
            PullRequestMergeState::HasHooks => "HasHooks".magenta().bold().to_string(),
            PullRequestMergeState::Unknown => "Unknown".bold().to_string(),
            PullRequestMergeState::Unmergeable => "Unmergeable".red().bold().to_string(),
            PullRequestMergeState::Unstable => "Unstable".red().bold().to_string(),
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

/// User-selectable high-level operations to apply to chosen PRs.
///
/// Encapsulates composite actions presented in the TUI. Separate from [`Op`]
/// which models the underlying atomic steps and reporting. Expanding this enum
/// only affects menu construction / selection logic.
///
/// # Variants
/// - `ApproveAndMerge` Perform approval review then merge if approval succeeds.
/// - `DependabotRebase` Post the `@dependabot rebase` comment to a Dependabot PR.
///
/// # Future Work
/// - Add bulk label operations (e.g. `Label` / `RemoveLabel`).
/// - Introduce `Comment` with arbitrary body once use-cases emerge.
/// - Provide dry-run variants for auditing actions.
#[derive(strum::Display, EnumIter)]
enum SelectableOp {
    #[strum(to_string = "Approve & Merge")]
    ApproveAndMerge,
    #[strum(to_string = "Dependabot Rebase")]
    DependabotRebase,
}

impl SelectableOp {
    pub fn run(&self) -> Box<dyn Fn(&PullRequest)> {
        match self {
            Self::ApproveAndMerge => Box::new(|pr| {
                let _ = Op::Approve
                    .report(pr, ytil_github::pr::approve(pr.number))
                    .and_then(|()| Op::Merge.report(pr, ytil_github::pr::merge(pr.number)));
            }),
            Self::DependabotRebase => Box::new(|pr| {
                let _ = Op::DependabotRebase.report(pr, ytil_github::pr::dependabot_rebase(pr.number));
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
/// - `Approve` Submit an approving review (`gh pr review --approve`).
/// - `Merge` Perform the administrative squash merge (`gh pr merge --admin --squash`).
/// - `DependabotRebase` Post the `@dependabot rebase` comment to request an updated rebase for a Dependabot PR.
enum Op {
    Approve,
    Merge,
    DependabotRebase,
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
    /// - `res` Result returned by the underlying GitHub CLI wrapper.
    ///
    /// # Returns
    /// Propagates `res` unchanged after side‑effect logging.
    ///
    /// # Errors
    /// Returns the same error contained in `res` (no transformation) so callers
    /// can continue combinators (`and_then`, etc.) if desired.
    pub fn report(&self, pr: &PullRequest, res: color_eyre::Result<()>) -> color_eyre::Result<()> {
        res.inspect(|()| self.report_ok(pr)).inspect_err(|error| {
            self.report_error(pr, error);
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
        };
        println!("{} {}", format!("{msg} PR").green().bold(), format_pr(pr));
    }

    /// Emit a structured error report for a failed operation.
    ///
    /// # Arguments
    /// - `pr` Pull request that failed to process.
    /// - `error` Error returned by the CLI wrapper.
    ///
    /// # Rationale
    /// Keeps multi‑line error payload visually grouped with the PR metadata.
    fn report_error(&self, pr: &PullRequest, error: &color_eyre::Report) {
        let msg = match self {
            Self::Approve => "approving",
            Self::Merge => "merging",
            Self::DependabotRebase => "triggering dependabot rebase",
        };
        eprintln!(
            "{} {} {}",
            format!("Error {msg} PR").red().bold(),
            format_pr(pr),
            format!("error=\n{error}").red().bold()
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
        "{} {} {}",
        format!("number={}", pr.number).white().bold(),
        format!("title={:?}", pr.title).white().bold(),
        format!("author={:?}", pr.author).white().bold(),
    )
}
