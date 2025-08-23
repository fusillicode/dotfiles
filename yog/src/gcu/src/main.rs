#![feature(exit_status_error)]

use std::io::Write;
use std::process::Command;

use color_eyre::eyre::bail;
use url::Url;
use utils::cmd::CmdError;
use utils::cmd::CmdExt;
use utils::sk::SkimItem;

mod git;

/// Create or switch to the GitHub branch built by parameterizing the supplied args.
/// Existence of branch is checked only against local ones (to avoid fetching them remotely).
/// If a PR URL is supplied as arg, switches to the related branch.
/// If no args are supplied, fetches local branches and presents a TUI to select one.
/// If "-b" is supplied it defaults to "git checkout -b".
/// If the first arg is a valid path it tries to checkout it and all the other supplied path
/// from the branch supplied as last arg.
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

fn autocomplete_git_branches() -> color_eyre::Result<()> {
    let mut git_refs = git::get_local_and_remote_refs()?;
    git::keep_local_and_untracked_refs(&mut git_refs);

    match utils::sk::get_item(git_refs, Default::default())? {
        Some(hd) if hd.text() == "-" || hd.text().is_empty() => switch_branch("-"),
        Some(other) => switch_branch(&other.text()),
        None => Ok(()),
    }
}

fn switch_branch_or_create_if_missing(arg: &str) -> color_eyre::Result<()> {
    if let Ok(url) = Url::parse(arg) {
        utils::github::log_into_github()?;
        let branch_name = utils::github::get_branch_name_from_url(&url)?;
        return switch_branch(&branch_name);
    }
    create_branch_if_missing(&build_branch_name(&[arg])?)
}

// Assumptions:
// - if the last arg is an existent local branch try to reset the files represented by the previous args
// - if the last arg is NOT an existing local branch try to create a branch
fn checkout_files_or_create_branch_if_missing(args: &[&str]) -> color_eyre::Result<()> {
    if let Some((branch, files)) = get_branch_and_files_to_checkout(args)? {
        return checkout_files(&files, branch);
    }
    create_branch_if_missing(&build_branch_name(args)?)
}

fn get_branch_and_files_to_checkout<'a>(args: &'a [&'a str]) -> color_eyre::Result<Option<(&'a str, &'a [&'a str])>> {
    if let Some((branch, files)) = args.split_last()
        && local_branch_exists(branch)?
    {
        return Ok(Some((branch, files)));
    }
    Ok(None)
}

fn local_branch_exists(branch: &str) -> color_eyre::Result<bool> {
    match Command::new("git").args(["rev-parse", "--verify", branch]).exec() {
        Ok(_) => Ok(true),
        Err(CmdError::Stderr { .. }) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn checkout_files(files: &[&str], branch: &str) -> color_eyre::Result<()> {
    let mut args = vec!["checkout", branch];
    args.extend_from_slice(files);
    Command::new("git").args(args).exec()?;
    files.iter().for_each(|f| println!("🍁 {f} from {branch}"));
    Ok(())
}

fn switch_branch(branch: &str) -> color_eyre::Result<()> {
    Command::new("git").args(["switch", branch]).exec()?;
    println!("🪵 {branch}");
    Ok(())
}

fn create_branch(branch: &str) -> color_eyre::Result<()> {
    if !should_create_new_branch(branch)? {
        return Ok(());
    }
    Command::new("git").args(["checkout", "-b", branch]).exec()?;
    println!("🌱 {branch}");
    Ok(())
}

// Create the supplied branch without asking only if:
//
// - the passed branch is the default one (it will not be created because already there and I'll be switched to it)
// - the current branch is the default one
//
// This logic helps me to avoid inadvertently branching from branches different from the default
// one as it doesn't happen often.
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

fn is_default_branch(branch: &str) -> bool {
    branch == "main" || branch == "master"
}

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
        let res = format!("{:#?}", build_branch_name(&[input.into()]));
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
    #[case(&["Hello".into(), "World".into()], "hello-world")]
    #[case(&["Hello World".into(), "World".into()], "hello-world-world")]
    #[case(&["Hello World".into(), "🌎".into(), "42".into()], "hello-world-42")]
    #[case(&["This".into(), "---is.".into(), "..a_test".into()], "this-is.-..a_test")]
    #[case(&["dependabot/cargo/opentelemetry-0.27.1".into()], "dependabot/cargo/opentelemetry-0.27.1")]
    fn test_build_branch_name_succeeds_as_expected(#[case] input: &[&str], #[case] expected_output: &str) {
        assert_eq!(expected_output, build_branch_name(input).unwrap());
    }
}
