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
//! gcu https://github.com/org/repo/pull/123 # derive PR branch name & switch
//! ```
//!
//! # Errors
//! - GitHub authentication via [`ytil_gh::log_into_github`] or PR branch name derivation via
//!   [`ytil_gh::get_branch_name_from_url`] fails.
//! - Branch name construction via [`build_branch_name`] yields empty string.
//! - Branch listing via [`ytil_git::branch::get_all`] / switching via [`ytil_git::branch::switch`] / creation via
//!   [`ytil_git::branch::create_from_default_branch`] fails.
//! - Interactive selection via [`ytil_tui::minimal_select`] or user confirmation input fails.
//! - Current branch lookup via [`ytil_git::branch::get_current`] fails.

#![feature(exit_status_error)]

use std::io::Write;

use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize as _;
use url::Url;
use ytil_git::CmdError;
use ytil_sys::cli_args::CliArgs;

/// Interactive selection and switching of Git branches.
///
/// Presents a minimal TUI listing the provided branches (or fetches recent local / remote branches
/// if none provided), with redundant remotes removed.
///
/// Selecting an empty line or "-" triggers previous-branch switching.
///
/// # Errors
/// - Branch enumeration via [`ytil_git::branch::get_all_no_redundant`] fails (if `branches` is empty).
/// - UI rendering via [`ytil_tui::minimal_select`] fails.
/// - Branch switching via [`ytil_git::branch::switch`] fails.
fn autocomplete_git_branches_and_switch() -> color_eyre::Result<()> {
    let Some(branch) = ytil_tui::git_branch::select()? else {
        return Ok(());
    };

    let branch_name_no_origin = branch.name_no_origin();
    ytil_git::branch::switch(branch_name_no_origin).inspect(|()| report_branch_switch(branch_name_no_origin))?;

    Ok(())
}

/// Handles a single input argument, either a GitHub PR URL or a branch name, and switches to the corresponding branch.
///
/// Behaviour:
/// - If `arg` parses as a GitHub PR URL, authenticate then derive the branch name and switch to it.
/// - Otherwise, use `arg` as the branch name and switch to it.
///
/// # Arguments
/// - `arg` Either a GitHub PR URL or a branch name.
///
/// # Errors
/// - GitHub authentication via [`ytil_gh::log_into_github`] fails (if URL).
/// - Pull request branch name derivation via [`ytil_gh::get_branch_name_from_url`] fails (if URL).
/// - Branch switching via [`ytil_git::branch::switch`] fails.
fn handle_single_input_argument(arg: &str) -> color_eyre::Result<()> {
    let branch_name = if let Ok(url) = Url::parse(arg) {
        ytil_gh::log_into_github()?;
        ytil_gh::get_branch_name_from_url(&url)?
    } else {
        arg.to_string()
    };

    match ytil_git::branch::switch(&branch_name).map_err(|e| *e) {
        Err(CmdError::CmdFailure { stderr, .. }) if stderr.contains("invalid reference: ") => {
            create_branch_and_switch(&branch_name)
        }
        other => Ok(other?),
    }
}

/// Creates a new local branch (if desired) and switches to it.
///
/// Behaviour:
/// - if both the current branch and the target branch are non‑default (not `main` / `master`) user confirmation is
///   required.
///
/// # Errors
/// - Current branch discovery via [`ytil_git::branch::get_current`] fails.
/// - Branch creation via [`ytil_git::branch::create_from_default_branch`] or subsequent switching via
///   [`ytil_git::branch::switch`] fails.
/// - Reading user confirmation input fails.
fn create_branch_and_switch(branch_name: &str) -> color_eyre::Result<()> {
    if !should_create_new_branch(branch_name)? {
        return Ok(());
    }
    if let Err(err) = ytil_git::branch::create_from_default_branch(branch_name, None) {
        if err.to_string().contains("already exists") {
            ytil_git::branch::switch(branch_name).inspect(|()| report_branch_exists(branch_name))?;
            return Ok(());
        }
        return Err(err);
    }
    ytil_git::branch::switch(branch_name).inspect(|()| report_branch_new(branch_name))?;
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
/// - Current branch discovery via [`ytil_git::branch::get_current`] fails.
/// - Reading user confirmation input fails.
fn should_create_new_branch(branch_name: &str) -> color_eyre::Result<bool> {
    let default_branch = ytil_git::branch::get_default()?;
    if default_branch == branch_name {
        return Ok(true);
    }
    let curr_branch = ytil_git::branch::get_current()?;
    if default_branch == curr_branch {
        return Ok(true);
    }
    ask_branching_from_not_default(branch_name, &curr_branch);
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().is_empty() {
        report_branch_not_created(branch_name);
        return Ok(false);
    }
    Ok(true)
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
fn report_branch_switch(branch_name: &str) {
    println!("{} {}", ">".magenta().bold(), branch_name.bold());
}

/// Prints a styled indication that a new branch was created.
///
/// # Arguments
/// - `branch` Newly created branch name.
fn report_branch_new(branch_name: &str) {
    println!("{} {}", "+".green().bold(), branch_name.bold());
}

/// Prints a styled indication that the branch already exists; then indicates switch.
///
/// # Arguments
/// - `branch` Existing branch name that is being switched to.
fn report_branch_exists(branch_name: &str) {
    println!("{}{} {}", "!".blue().bold(), ">".magenta().bold(), branch_name.bold());
}

/// Prints a styled indication that branch creation was aborted (no newline).
///
/// # Arguments
/// - `branch` Branch name whose creation was declined.
fn report_branch_not_created(branch_name: &str) {
    print!("{} {} not created", "x".red().bold(), branch_name.bold());
}

/// Prints a styled notice that a new branch is being created from a non-default branch.
///
/// # Arguments
/// - `branch` Target branch being created.
/// - `default_branch` Current (non-default) branch acting as the base.
fn ask_branching_from_not_default(branch_name: &str, default_branch_name: &str) {
    print!(
        "{} {} from {}",
        "*".cyan().bold(),
        branch_name.bold(),
        default_branch_name.bold()
    );
}

/// Switch, create, and derive Git branches (including from GitHub PR URLs).
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli_args::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    match args.split_first() {
        None => autocomplete_git_branches_and_switch(),
        // Assumption: cannot create a branch with a name that starts with -
        Some((hd, _)) if *hd == "-" => ytil_git::branch::switch(hd)
            .inspect(|()| report_branch_switch(hd))
            .map_err(From::from),
        Some((hd, tail)) if *hd == "-b" => create_branch_and_switch(&build_branch_name(tail)?),
        Some((hd, &[])) => handle_single_input_argument(hd),
        _ => create_branch_and_switch(&build_branch_name(&args)?),
    }?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::empty_input("", "branch name construction produced empty string | args=[\n    \"\",\n]")]
    #[case::invalid_characters_only("❌", "branch name construction produced empty string | args=[\n    \"❌\",\n]")]
    fn build_branch_name_fails_as_expected(#[case] input: &str, #[case] expected_output: &str) {
        assert2::let_assert!(Err(actual_error) = build_branch_name(&[input]));
        assert_eq!(format!("{actual_error}"), expected_output);
    }

    #[rstest]
    #[case::single_word(&["HelloWorld"], "helloworld")]
    #[case::space_separated(&["Hello World"], "hello-world")]
    #[case::special_characters(&["Feature: Implement User Login!"], "feature-implement-user-login")]
    #[case::version_number(&["Version 2.0"], "version-2.0")]
    #[case::multiple_separators(&["This---is...a_test"], "this-is...a_test")]
    #[case::leading_trailing_spaces(&["  Leading and trailing   "], "leading-and-trailing")]
    #[case::emoji(&["Hello 🌎 World"], "hello-world")]
    #[case::emoji_at_start_end(&["🚀Launch🚀Day"], "launch-day")]
    #[case::multiple_emojis(&["Smile 😊 and 🤖 code"], "smile-and-code")]
    #[case::multiple_args(&["Hello", "World"], "hello-world")]
    #[case::args_with_spaces(&["Hello World", "World"], "hello-world-world")]
    #[case::mixed_args(&["Hello World", "🌎", "42"], "hello-world-42")]
    #[case::special_chars_in_args(&["This", "---is.", "..a_test"], "this-is.-..a_test")]
    #[case::dependabot_path(&["dependabot/cargo/opentelemetry-0.27.1"], "dependabot/cargo/opentelemetry-0.27.1")]
    fn build_branch_name_succeeds_as_expected(#[case] input: &[&str], #[case] expected_output: &str) {
        assert2::let_assert!(Ok(actual_output) = build_branch_name(input));
        assert_eq!(actual_output, expected_output);
    }
}
