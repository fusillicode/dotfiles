#![feature(exit_status_error)]

use std::io::Write;
use std::ops::Deref;

use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize as _;
use git::Branch;
use url::Url;

/// CLI for Git branch management: interactive selection, safe creation, switching,
/// and deriving branch names from GitHub PR URLs.
///
/// # Errors
///
/// Returns an error if:
/// - Arguments cannot be read.
/// - Interactive selection fails.
/// - A GitHub PR URL cannot be parsed or mapped to a branch.
/// - Branch creation or switch fails.
/// - An underlying I/O or Git operation fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    match args.split_first() {
        None => autocomplete_git_branches(),
        // Assumption: cannot create a branch with a name that starts with -
        Some((hd, _)) if *hd == "-" => switch_branch(hd),
        Some((hd, tail)) if *hd == "-b" => create_branch_and_switch(&build_branch_name(tail)?),
        Some((hd, &[])) => switch_branch_or_create_if_missing(hd),
        _ => create_branch_and_switch(&build_branch_name(&args)?),
    }?;

    Ok(())
}

/// Interactive selection of Git branches to switch to.
///
/// # Errors
///
/// Returns an error if:
/// - An underlying operation fails.
fn autocomplete_git_branches() -> color_eyre::Result<()> {
    let mut branches = git::get_branches()?;
    git::remove_redundant_remotes(&mut branches);

    match tui::minimal_select(branches.into_iter().map(RenderableBranch).collect())? {
        Some(hd) if hd.name() == "-" || hd.name().is_empty() => switch_branch("-"),
        Some(other) => switch_branch(other.name()),
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

impl core::fmt::Display for RenderableBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let styled_date_time = format!("({})", self.committer_date_time());
        write!(f, "{} {}", self.name(), styled_date_time.blue())
    }
}

/// Switches to an existing branch, or creates & switches if missing; also accepts GitHub PR URLs.
///
/// Behavior:
/// - If `arg` is a GitHub PR URL: logs into GitHub, derives the branch name, then switches to it.
/// - Otherwise: builds a sanitized branch name from `arg` and creates (and switches to) it if allowed.
///
/// # Errors
///
/// Returns an error if:
/// - GitHub authentication fails.
/// - The PR URL cannot be parsed or mapped to a branch.
/// - A branch name cannot be built from the input (sanitization yields empty).
/// - Branch creation fails.
/// - Switching to the branch fails.
/// - An underlying Git or I/O operation fails.
fn switch_branch_or_create_if_missing(arg: &str) -> color_eyre::Result<()> {
    if let Ok(url) = Url::parse(arg) {
        github::log_into_github()?;
        let branch_name = github::get_branch_name_from_url(&url)?;
        return switch_branch(&branch_name);
    }
    create_branch_and_switch(&build_branch_name(&[arg])?)
}

/// Switches to the specified Git branch.
///
/// # Errors
///
/// Returns an error if:
/// - Branch lookup, checkout, or underlying repository operations fail.
fn switch_branch(branch: &str) -> color_eyre::Result<()> {
    git::switch_branch(branch)?;
    println!("{} {}", ">".magenta().bold(), branch.bold());
    Ok(())
}

/// Creates a new local branch from the current `HEAD` and switches to it.
///
/// Safety logic: if both the target branch and the current branch are non‑default (not `main`/`master`)
/// the user must confirm via an empty line on stdin; any non‑empty input aborts creation.
///
/// # Errors
///
/// Returns an error if:
/// - Determining whether creation is allowed fails (current branch lookup / I/O).
/// - The branch name already exists and switching to it fails.
/// - Branch creation fails.
/// - Switching to the (new or existing) branch fails.
/// - An underlying Git or I/O operation fails.
fn create_branch_and_switch(branch: &str) -> color_eyre::Result<()> {
    if !should_create_new_branch(branch)? {
        return Ok(());
    }
    if let Err(error) = git::create_branch(branch) {
        if error.to_string().contains("already exists") {
            println!("{} {}", "@".blue().bold(), branch.bold());
            return switch_branch(branch);
        }
        return Err(error);
    }
    git::switch_branch(branch)?;
    println!("{} {}", "+".green().bold(), branch.bold());
    Ok(())
}

/// Determines if a new branch should be created based on safety logic.
///
/// # Errors
///
/// Returns an error if:
/// - An underlying operation fails.
fn should_create_new_branch(branch: &str) -> color_eyre::Result<bool> {
    if is_default_branch(branch) {
        return Ok(true);
    }
    let curr_branch = git::get_current_branch()?;
    if is_default_branch(&curr_branch) {
        return Ok(true);
    }
    print!("{} {} from {}", "*".cyan().bold(), branch.bold(), curr_branch.bold());
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().is_empty() {
        print!("{} {} not created", "x".red().bold(), branch.bold());
        return Ok(false);
    }
    Ok(true)
}

/// Checks if branch is a default branch (main or master).
fn is_default_branch(branch: &str) -> bool {
    branch == "main" || branch == "master"
}

/// Builds a safe branch name from arguments.
///
/// # Errors
///
/// Returns an error if:
/// - An underlying operation fails.
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
        bail!("parameterizing {args:#?} resulted in empty String")
    }

    Ok(branch_name)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "",
        "Err(\n    \"parameterizing [\\n    \\\"\\\",\\n] resulted in empty String\",\n)"
    )]
    #[case(
        "❌",
        "Err(\n    \"parameterizing [\\n    \\\"❌\\\",\\n] resulted in empty String\",\n)"
    )]
    fn build_branch_name_fails_as_expected(#[case] input: &str, #[case] expected_content: &str) {
        let res = format!("{:#?}", build_branch_name(&[input]));
        assert!(res.contains(expected_content), "unexpected {res}");
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
        assert_eq!(expected_output, build_branch_name(input).unwrap());
    }
}
