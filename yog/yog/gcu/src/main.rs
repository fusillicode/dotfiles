//! Switch, create, and derive Git branches (including from GitHub PR URLs).
//!
//! # Errors
//! - Git operations, GitHub API calls, or user interaction fails.
#![feature(exit_status_error)]

use std::io::Write;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use url::Url;
use ytil_git::CmdError;
use ytil_sys::cli::Args;

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
fn autocomplete_git_branches_and_switch() -> rootcause::Result<()> {
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
/// # Errors
/// - GitHub authentication via [`ytil_gh::log_into_github`] fails (if URL).
/// - Pull request branch name derivation via [`ytil_gh::get_branch_name_from_url`] fails (if URL).
/// - Branch switching via [`ytil_git::branch::switch`] fails.
fn handle_single_input_argument(arg: &str) -> rootcause::Result<()> {
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
fn create_branch_and_switch(branch_name: &str) -> rootcause::Result<()> {
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
fn should_create_new_branch(branch_name: &str) -> rootcause::Result<bool> {
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
fn build_branch_name(args: &[&str]) -> rootcause::Result<String> {
    fn is_permitted(c: char) -> bool {
        const PERMITTED_CHARS: [char; 3] = ['.', '/', '_'];
        c.is_alphanumeric() || PERMITTED_CHARS.contains(&c)
    }

    // Single-pass approach: walk every char once, push permitted chars lowercased directly into the
    // output buffer, and collapse runs of non-permitted chars / whitespace boundaries into a single
    // '-' separator. This avoids the previous 5+ intermediate allocations per token.
    let mut branch_name = String::new();
    let mut need_separator = false;

    for arg in args {
        for token in arg.split_whitespace() {
            for c in token.chars() {
                if is_permitted(c) {
                    if need_separator && !branch_name.is_empty() {
                        branch_name.push('-');
                    }
                    need_separator = false;
                    for lc in c.to_lowercase() {
                        branch_name.push(lc);
                    }
                } else {
                    // Non-permitted chars collapse into a pending separator.
                    need_separator = true;
                }
            }
            // Boundary between whitespace-separated tokens is also a separator.
            need_separator = true;
        }
    }

    if branch_name.is_empty() {
        Err(report!("branch name construction produced empty string")).attach_with(|| format!("args={args:#?}"))?;
    }

    Ok(branch_name)
}

/// Prints a styled indication of a successful branch switch.
fn report_branch_switch(branch_name: &str) {
    println!("{} {}", ">".magenta().bold(), branch_name.bold());
}

/// Prints a styled indication that a new branch was created.
fn report_branch_new(branch_name: &str) {
    println!("{} {}", "+".green().bold(), branch_name.bold());
}

/// Prints a styled indication that the branch already exists; then indicates switch.
fn report_branch_exists(branch_name: &str) {
    println!("{}{} {}", "!".blue().bold(), ">".magenta().bold(), branch_name.bold());
}

/// Prints a styled indication that branch creation was aborted (no newline).
fn report_branch_not_created(branch_name: &str) {
    print!("{} {} not created", "x".red().bold(), branch_name.bold());
}

/// Prints a styled notice that a new branch is being created from a non-default branch.
fn ask_branching_from_not_default(branch_name: &str, default_branch_name: &str) {
    print!(
        "{} {} from {}",
        "*".cyan().bold(),
        branch_name.bold(),
        default_branch_name.bold()
    );
}

/// Switch, create, and derive Git branches (including from GitHub PR URLs).
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
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
    #[case::empty_input("", "branch name construction produced empty string")]
    #[case::invalid_characters_only("❌", "branch name construction produced empty string")]
    fn build_branch_name_fails_as_expected(#[case] input: &str, #[case] expected_ctx: &str) {
        assert2::assert!(let Err(err) = build_branch_name(&[input]));
        assert_eq!(err.format_current_context().to_string(), expected_ctx);
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
        assert2::assert!(let Ok(actual_output) = build_branch_name(input));
        assert_eq!(actual_output, expected_output);
    }
}
