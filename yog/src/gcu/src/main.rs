#![feature(exit_status_error)]

use std::io::Write;
use std::process::Command;

use color_eyre::eyre::bail;
use url::Url;
use utils::cmd::CmdError;
use utils::cmd::CmdExt;
use utils::sk::SkimItem;

mod git;

/// A comprehensive Git branch management tool with intelligent branch creation and switching.
///
/// This tool provides multiple ways to interact with Git branches:
/// - Create new branches from parameterized arguments
/// - Switch between existing branches with interactive selection
/// - Handle GitHub PR URLs to switch to corresponding branches
/// - Checkout specific files from different branches
/// - Smart branch naming with path-safe character conversion
///
/// # Arguments
///
/// The tool accepts various argument patterns:
/// - No arguments: Interactive branch selection from local branches
/// - Single argument: Branch name to create/switch to
/// - `-b <name>`: Force creation of new branch
/// - PR URL: Switch to branch associated with the PR
/// - File paths: Checkout specific files from a branch (last arg is branch name)
///
/// # Branch Naming
///
/// When creating branches, arguments are converted to safe branch names:
/// - Alphanumeric characters and `.`, `/`, `_` are preserved
/// - Other characters are replaced with `-`
/// - Multiple consecutive separators are collapsed
/// - Names are lowercased
///
/// # Examples
///
/// Interactive branch selection:
/// ```bash
/// gcu
/// ```
///
/// Create and switch to new branch:
/// ```bash
/// gcu feature user authentication
/// ```
///
/// Force new branch creation:
/// ```bash
/// gcu -b feature/login-system
/// ```
///
/// Switch to branch from PR URL:
/// ```bash
/// gcu https://github.com/user/repo/pull/123
/// ```
///
/// Checkout files from another branch:
/// ```bash
/// gcu src/main.rs Cargo.toml main
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();
    let args: Vec<_> = args.iter().map(|s| s.as_str()).collect();

    match args.split_first() {
        None => autocomplete_git_branches(),
        // Assumption: cannot create a branch with a name that starts with -
        Some((hd, _)) if *hd == "-" => switch_branch(hd),
        Some((hd, tail)) if *hd == "-b" => create_branch(&build_branch_name(tail)?),
        Some((hd, &[])) => switch_branch_or_create_if_missing(hd),
        _ => checkout_files_or_create_branch_if_missing(&args),
    }?;

    Ok(())
}

/// Presents an interactive selection menu of available Git branches and switches to the selected one.
///
/// This function retrieves all local and remote Git references, filters them to show only
/// relevant branches, and presents them in an interactive fuzzy finder. The user can select
/// a branch to switch to, or cancel the operation.
///
/// # Returns
///
/// Returns `Ok(())` if a branch was successfully selected and switched to, or if the operation
/// was cancelled. Returns an error if there are issues retrieving branches or switching.
fn autocomplete_git_branches() -> color_eyre::Result<()> {
    let mut git_refs = git::get_local_and_remote_refs()?;
    git::keep_local_and_untracked_refs(&mut git_refs);

    match utils::sk::get_item(git_refs, Default::default())? {
        Some(hd) if hd.text() == "-" || hd.text().is_empty() => switch_branch("-"),
        Some(other) => switch_branch(&other.text()),
        None => Ok(()),
    }
}

/// Attempts to switch to a branch or create it if it doesn't exist.
///
/// This function handles different types of input:
/// - If the input is a GitHub PR URL, it extracts the branch name and switches to it
/// - If the input is a branch name, it attempts to create the branch if it doesn't exist
///
/// # Arguments
///
/// * `arg` - Either a branch name or a GitHub PR URL
///
/// # Returns
///
/// Returns `Ok(())` if the branch was successfully switched to or created.
/// Returns an error if branch operations fail.
fn switch_branch_or_create_if_missing(arg: &str) -> color_eyre::Result<()> {
    if let Ok(url) = Url::parse(arg) {
        utils::github::log_into_github()?;
        let branch_name = utils::github::get_branch_name_from_url(&url)?;
        return switch_branch(&branch_name);
    }
    create_branch_if_missing(&build_branch_name(&[arg])?)
}

/// Either checks out specific files from a branch or creates a new branch.
///
/// This function makes intelligent decisions based on the arguments:
/// - If the last argument is an existing local branch, it treats the preceding arguments as files to checkout from that
///   branch
/// - If the last argument is not an existing branch, it treats all arguments as parts of a new branch name to create
///
/// # Arguments
///
/// * `args` - Array of string arguments representing either files+branch or branch name parts
///
/// # Returns
///
/// Returns `Ok(())` if files were checked out or a branch was created successfully.
fn checkout_files_or_create_branch_if_missing(args: &[&str]) -> color_eyre::Result<()> {
    if let Some((branch, files)) = get_branch_and_files_to_checkout(args)? {
        return checkout_files(files, branch);
    }
    create_branch_if_missing(&build_branch_name(args)?)
}

/// Attempts to identify a branch and files from the argument list.
///
/// This function checks if the last argument is an existing local branch.
/// If so, it returns the branch name and the preceding arguments as files to checkout.
/// If not, it returns `None`.
///
/// # Arguments
///
/// * `args` - Array of string arguments
///
/// # Returns
///
/// Returns `Some((branch, files))` if the last argument is an existing branch,
/// otherwise returns `None`.
fn get_branch_and_files_to_checkout<'a>(args: &'a [&'a str]) -> color_eyre::Result<Option<(&'a str, &'a [&'a str])>> {
    if let Some((branch, files)) = args.split_last()
        && local_branch_exists(branch)?
    {
        return Ok(Some((branch, files)));
    }
    Ok(None)
}

/// Checks if a local Git branch exists.
///
/// This function uses `git rev-parse --verify` to check if the specified branch exists.
/// It distinguishes between branches that don't exist (returning `false`) and actual
/// command errors (returning an error).
///
/// # Arguments
///
/// * `branch` - The name of the branch to check
///
/// # Returns
///
/// Returns `true` if the branch exists, `false` if it doesn't exist,
/// or an error if the Git command fails.
fn local_branch_exists(branch: &str) -> color_eyre::Result<bool> {
    match Command::new("git").args(["rev-parse", "--verify", branch]).exec() {
        Ok(_) => Ok(true),
        Err(CmdError::Stderr { .. }) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Checks out specific files from a given branch.
///
/// This function uses `git checkout <branch> <files...>` to restore specific files
/// from the specified branch to the current working directory.
///
/// # Arguments
///
/// * `files` - Array of file paths to checkout
/// * `branch` - The branch to checkout files from
///
/// # Returns
///
/// Returns `Ok(())` if all files were successfully checked out.
/// Prints the name of each checked out file.
fn checkout_files(files: &[&str], branch: &str) -> color_eyre::Result<()> {
    let mut args = vec!["checkout", branch];
    args.extend_from_slice(files);
    Command::new("git").args(args).exec()?;
    files.iter().for_each(|f| println!("🍁 {f} from {branch}"));
    Ok(())
}

/// Switches to the specified Git branch.
///
/// This function uses `git switch` to change the current branch to the specified one.
/// It prints the branch name after successfully switching.
///
/// # Arguments
///
/// * `branch` - The name of the branch to switch to
///
/// # Returns
///
/// Returns `Ok(())` if the branch switch was successful.
fn switch_branch(branch: &str) -> color_eyre::Result<()> {
    Command::new("git").args(["switch", branch]).exec()?;
    println!("🪵 {branch}");
    Ok(())
}

/// Creates a new Git branch and switches to it.
///
/// This function first checks if the branch should be created using safety logic,
/// then creates the branch with `git checkout -b` and switches to it.
///
/// # Arguments
///
/// * `branch` - The name of the branch to create
///
/// # Returns
///
/// Returns `Ok(())` if the branch was created successfully or if creation was cancelled.
fn create_branch(branch: &str) -> color_eyre::Result<()> {
    if !should_create_new_branch(branch)? {
        return Ok(());
    }
    Command::new("git").args(["checkout", "-b", branch]).exec()?;
    println!("🌱 {branch}");
    Ok(())
}

/// Determines whether a new branch should be created based on safety logic.
///
/// This function implements safety checks to prevent accidental branching:
/// - Always allows creation if the target branch is the default branch
/// - Always allows creation if the current branch is the default branch
/// - For other cases, prompts the user for confirmation to avoid branching from feature branches
///
/// This helps prevent inadvertently creating branches from other feature branches,
/// which is generally not desired in most workflows.
///
/// # Arguments
///
/// * `branch` - The name of the branch to potentially create
///
/// # Returns
///
/// Returns `true` if the branch should be created, `false` if creation should be cancelled.
fn should_create_new_branch(branch: &str) -> color_eyre::Result<bool> {
    if is_default_branch(branch) {
        return Ok(true);
    }
    let curr_branch = utils::git::get_current_branch()?;
    if is_default_branch(&curr_branch) {
        return Ok(true);
    }
    print!("🪚 {curr_branch} -> {branch} ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().is_empty() {
        print!("🪨 {branch} not created");
        return Ok(false);
    }
    Ok(true)
}

/// Checks if the given branch name is a default branch (main or master).
///
/// This function identifies the commonly used default branch names in Git repositories.
/// It's used to determine if special safety logic should be applied when creating branches.
///
/// # Arguments
///
/// * `branch` - The branch name to check
///
/// # Returns
///
/// Returns `true` if the branch is "main" or "master", otherwise `false`.
fn is_default_branch(branch: &str) -> bool {
    branch == "main" || branch == "master"
}

/// Creates a branch if it doesn't already exist, otherwise switches to it.
///
/// This function attempts to create a new branch. If the branch already exists,
/// it switches to the existing branch instead. This provides a seamless experience
/// when the user might not know whether a branch already exists.
///
/// # Arguments
///
/// * `branch` - The name of the branch to create or switch to
///
/// # Returns
///
/// Returns `Ok(())` if the branch was created or switched to successfully.
fn create_branch_if_missing(branch: &str) -> color_eyre::Result<()> {
    if let Err(error) = create_branch(branch) {
        if error.to_string().contains("already exists") {
            println!("🌳 {branch}");
            return switch_branch(branch);
        }
        return Err(error);
    }
    Ok(())
}

/// Builds a safe branch name from the provided arguments.
///
/// This function converts arbitrary strings into Git-safe branch names by:
/// - Preserving alphanumeric characters, dots, slashes, and underscores
/// - Replacing other characters with spaces, then converting spaces to hyphens
/// - Converting to lowercase
/// - Joining multiple arguments with hyphens
/// - Collapsing multiple consecutive separators
///
/// # Arguments
///
/// * `args` - Array of strings to convert into a branch name
///
/// # Returns
///
/// Returns a Git-safe branch name string, or an error if the result would be empty.
///
/// # Examples
///
/// ```rust
/// let name = build_branch_name(&["Feature", "User Login"])?;
/// assert_eq!(name, "feature-user-login");
/// ```
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
    fn test_build_branch_name_fails_as_expected(#[case] input: &str, #[case] expected_content: &str) {
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
    fn test_build_branch_name_succeeds_as_expected(#[case] input: &[&str], #[case] expected_output: &str) {
        assert_eq!(expected_output, build_branch_name(input).unwrap());
    }
}
