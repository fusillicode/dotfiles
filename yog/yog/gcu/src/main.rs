//! Switch, create, and derive Git branches (including from GitHub PR URLs).
//!
//! # Arguments
//! - `-` Switch to previous branch.
//! - `-b <args...>` Create new branch from sanitized `<args...>` then switch.
//! - `<single>` Switch if exists, else confirm create & switch.
//! - `<multiple args>` Sanitize & join into branch name (create if missing).
//! - `<github pull request url>` Derive branch name from PR & switch (fetch if needed).
//! - (none) Launch interactive selector.
//!
//! # Usage
//! ```bash
//! gcu # interactive branch picker
//! gcu - # previous branch
//! gcu -b Feature Add Foo # create: feature-add-foo
//! gcu feature add foo # sanitized join -> feature-add-foo (create if missing)
//! gcu https://github.com/org/repo/pull/123 # derive & fetch PR branch
//! ```
//!
//! # Errors
//! - GitHub authentication via [`ytil_github::log_into_github`] or PR branch name derivation via
//!   [`ytil_github::get_branch_name_from_url`] fails.
//! - Branch name construction via [`build_branch_name`] yields empty string.
//! - Branch listing via [`ytil_git::get_branches`] / fetching via [`ytil_git::fetch_branches`] / switching via
//!   [`ytil_git::switch_branch`] / creation via [`ytil_git::create_branch`] fails.
//! - Interactive selection via [`ytil_tui::minimal_select`] or user confirmation input fails.
//! - Current branch lookup via [`ytil_git::get_current_branch`] fails.

#![feature(exit_status_error)]

use core::fmt::Display;
use std::io::Write;
use std::ops::Deref;

use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize as _;
use url::Url;
use ytil_git::Branch;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    match args.split_first() {
        None => autocomplete_git_branches(),
        // Assumption: cannot create a branch with a name that starts with -
        Some((hd, _)) if *hd == "-" => ytil_git::switch_branch(hd).inspect(|()| report_branch_switch(hd)),
        Some((hd, tail)) if *hd == "-b" => create_branch_and_switch(&build_branch_name(tail)?),
        Some((hd, &[])) => switch_branch_or_create_if_missing(hd),
        _ => create_branch_and_switch(&build_branch_name(&args)?),
    }?;

    Ok(())
}

/// Interactive selection and switching of Git branches.
///
/// Presents a minimal TUI listing recent local / remote branches (with redundant
/// remotes removed via [`ytil_git::remove_redundant_remotes`]). Selecting an empty
/// line or "-" triggers previous-branch switching.
///
/// # Errors
/// - Branch enumeration via [`ytil_git::get_branches`] fails.
/// - UI rendering via [`ytil_tui::minimal_select`] fails.
/// - Branch switching via [`ytil_git::switch_branch`] fails.
fn autocomplete_git_branches() -> color_eyre::Result<()> {
    let mut branches = ytil_git::get_branches()?;
    ytil_git::remove_redundant_remotes(&mut branches);

    match ytil_tui::minimal_select(branches.into_iter().map(RenderableBranch).collect())? {
        Some(hd) if hd.name() == "-" || hd.name().is_empty() => {
            ytil_git::switch_branch("-").inspect(|()| report_branch_switch("-"))
        }
        Some(other) => ytil_git::switch_branch(other.name()).inspect(|()| report_branch_switch(other.name())),
        None => Ok(()),
    }
}

struct RenderableBranch(Branch);

impl Deref for RenderableBranch {
    type Target = Branch;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let styled_date_time = format!("({})", self.committer_date_time());
        write!(f, "{} {}", self.name(), styled_date_time.blue())
    }
}

/// Switches to an existing branch or creates-and-switches if it does not exist.
///
/// Also accepts a single GitHub PR URL and derives the associated branch name.
///
/// Behaviour:
/// - If `arg` parses as a GitHub PR URL, authenticate then derive the branch name, fetch it via
///   [`ytil_git::fetch_branches`] and switch to it.
/// - Otherwise, sanitize `arg` into a branch name ([`build_branch_name`]) and create, if missing (after confirmation),
///   then switch to it.
///
/// # Errors
/// - GitHub authentication via [`ytil_github::log_into_github`] fails.
/// - Pull request branch name derivation via [`ytil_github::get_branch_name_from_url`] fails.
/// - Fetching the remote branch via [`ytil_git::fetch_branches`] (git fetch) fails.
/// - Branch name construction via [`build_branch_name`] fails or produces an empty string.
/// - Branch creation via [`ytil_git::create_branch`] fails.
/// - Branch switching via [`ytil_git::switch_branch`] fails.
/// - Current branch discovery via [`ytil_git::get_current_branch`] (during creation decision) fails.
/// - Reading user confirmation input (stdin) fails.
fn switch_branch_or_create_if_missing(arg: &str) -> color_eyre::Result<()> {
    if let Ok(url) = Url::parse(arg) {
        ytil_github::log_into_github()?;
        let branch_name = ytil_github::get_branch_name_from_url(&url)?;
        ytil_git::fetch_branches(&[&branch_name])?;
        return ytil_git::switch_branch(&branch_name).inspect(|()| report_branch_switch(&branch_name));
    }
    create_branch_and_switch(&build_branch_name(&[arg])?)
}

/// Creates a new local branch (if desired) and switches to it.
///
/// Behaviour:
/// - if both the current branch and the target branch are non‑default (not `main` / `master`) user confirmation is
///   required.
///
/// # Errors
/// - Current branch discovery via [`ytil_git::get_current_branch`] fails.
/// - Branch creation via [`ytil_git::create_branch`] or subsequent switching via [`ytil_git::switch_branch`] fails.
/// - Reading user confirmation input fails.
fn create_branch_and_switch(branch: &str) -> color_eyre::Result<()> {
    if !should_create_new_branch(branch)? {
        return Ok(());
    }
    if let Err(error) = ytil_git::create_branch(branch) {
        if error.to_string().contains("already exists") {
            ytil_git::switch_branch(branch).inspect(|()| report_branch_exists(branch))?;
            return Ok(());
        }
        return Err(error);
    }
    ytil_git::switch_branch(branch).inspect(|()| report_branch_new(branch))?;
    Ok(())
}

/// Returns `true` if a new branch may be created following the desired behavior.
///
/// Behaviour:
/// - Always allowed when target is a default branch (`main`/`master`).
/// - Always allowed when current branch is a default branch.
/// - Otherwise, requires user confirmation via empty line input (non‑empty aborts).
///
/// # Errors
/// - Current branch discovery via [`ytil_git::get_current_branch`] fails.
/// - Reading user confirmation input fails.
fn should_create_new_branch(branch: &str) -> color_eyre::Result<bool> {
    if is_default_branch(branch) {
        return Ok(true);
    }
    let curr_branch = ytil_git::get_current_branch()?;
    if is_default_branch(&curr_branch) {
        return Ok(true);
    }
    ask_branching_from_not_default(branch, &curr_branch);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().is_empty() {
        report_branch_not_created(branch);
        return Ok(false);
    }
    Ok(true)
}

/// Returns `true` if `branch` is a default branch (`main` or `master`).
fn is_default_branch(branch: &str) -> bool {
    branch == "main" || branch == "master"
}

/// Builds a sanitized, lowercased Git branch name from raw arguments.
///
/// Transformation:
/// - Split each argument by ASCII whitespace into tokens.
/// - Replace unsupported characters with spaces (only alphanumeric plus '.', '/', '_').
/// - Collapse contiguous spaces inside each token into `-` separators.
/// - Discard empty tokens.
/// - Join resulting tokens with `-`.
///
/// # Errors
/// - sanitization produces an empty string.
fn build_branch_name(args: &[&str]) -> color_eyre::Result<String> {
    fn is_permitted(c: char) -> bool {
        const PERMITTED_CHARS: [char; 3] = ['.', '/', '_'];
        c.is_alphanumeric() || PERMITTED_CHARS.contains(&c)
    }

    let branch_name = args
        .iter()
        .flat_map(|x| {
            x.split_whitespace().filter_map(|y| {
                let z = y
                    .chars()
                    .map(|c| if is_permitted(c) { c } else { ' ' })
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
                    .to_lowercase();
                if z.is_empty() {
                    return None;
                }
                Some(z)
            })
        })
        .collect::<Vec<_>>()
        .join("-");

    if branch_name.is_empty() {
        bail!("branch name construction produced empty string | args={args:#?}")
    }

    Ok(branch_name)
}

/// Prints a styled indication of a successful branch switch.
///
/// # Arguments
/// - `branch` Branch name just switched to (displayed in bold magenta/normal styles).
fn report_branch_switch(branch: &str) {
    println!("{} {}", ">".magenta().bold(), branch.bold());
}

/// Prints a styled indication that a new branch was created.
///
/// # Arguments
/// - `branch` Newly created branch name.
fn report_branch_new(branch: &str) {
    println!("{} {}", "+".green().bold(), branch.bold());
}

/// Prints a styled indication that the branch already exists; then indicates switch.
///
/// # Arguments
/// - `branch` Existing branch name that is being switched to.
fn report_branch_exists(branch: &str) {
    println!("{}{} {}", "@".blue().bold(), ">".magenta().bold(), branch.bold());
}

/// Prints a styled indication that branch creation was aborted (no newline).
///
/// # Arguments
/// - `branch` Branch name whose creation was declined.
fn report_branch_not_created(branch: &str) {
    print!("{} {} not created", "x".red().bold(), branch.bold());
}

/// Prints a styled notice that a new branch is being created from a non-default branch.
///
/// # Arguments
/// - `branch` Target branch being created.
/// - `default_branch` Current (non-default) branch acting as the base.
fn ask_branching_from_not_default(branch: &str, default_branch: &str) {
    print!("{} {} from {}", "*".cyan().bold(), branch.bold(), default_branch.bold());
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("", "branch name construction produced empty string | args=[\n    \"\",\n]")]
    #[case("❌", "branch name construction produced empty string | args=[\n    \"❌\",\n]")]
    fn build_branch_name_fails_as_expected(#[case] input: &str, #[case] expected_output: &str) {
        assert2::let_assert!(Err(actual_error) = build_branch_name(&[input]));
        assert_eq!(format!("{actual_error}"), expected_output);
    }

    #[rstest]
    #[case(&["HelloWorld"], "helloworld")]
    #[case(&["Hello World"], "hello-world")]
    #[case(&["Feature: Implement User Login!"], "feature-implement-user-login")]
    #[case(&["Version 2.0"], "version-2.0")]
    #[case(&["This---is...a_test"], "this-is...a_test")]
    #[case(&["  Leading and trailing   "], "leading-and-trailing")]
    #[case(&["Hello 🌎 World"], "hello-world")]
    #[case(&["🚀Launch🚀Day"], "launch-day")]
    #[case(&["Smile 😊 and 🤖 code"], "smile-and-code")]
    #[case(&["Hello", "World"], "hello-world")]
    #[case(&["Hello World", "World"], "hello-world-world")]
    #[case(&["Hello World", "🌎", "42"], "hello-world-42")]
    #[case(&["This", "---is.", "..a_test"], "this-is.-..a_test")]
    #[case(&["dependabot/cargo/opentelemetry-0.27.1"], "dependabot/cargo/opentelemetry-0.27.1")]
    fn build_branch_name_succeeds_as_expected(#[case] input: &[&str], #[case] expected_output: &str) {
        assert2::let_assert!(Ok(actual_output) = build_branch_name(input));
        assert_eq!(actual_output, expected_output);
    }
}
