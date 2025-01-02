#![feature(exit_status_error)]

use std::process::Command;

use anyhow::bail;
use url::Url;

/// Create or switch to the GitHub branch built by parameterizing the
/// supplied args.
/// Existence of branch is checked only against local ones (to avoid
/// fetching them remotely).
/// If a PR URL is supplied as arg, switches to the related branch.
/// With no args, defaults to switching to "-".
/// If "-b" is supplied it defaults to "git checkout -b".
/// If the first arg is a valid path it tries to checkout it and all
/// the other supplied path from the branch supplied as last arg.
fn main() -> anyhow::Result<()> {
    let args = utils::system::get_args();

    match args.split_first() {
        None => switch_branch("-"),
        // Assumption: cannot create a branch with a name that starts with -
        Some((hd, _)) if hd == "-" => switch_branch(hd),
        Some((hd, tail)) if hd == "-b" => create_branch(&build_branch_name(tail)?),
        Some((hd, &[])) => {
            if let Ok(url) = Url::parse(hd) {
                utils::github::log_into_github()?;
                let branch_name = utils::github::get_branch_name_from_pr_url(&url)?;
                return switch_branch(&branch_name);
            }
            upsert_branch(&build_branch_name(&[hd.to_string()])?)
        }
        _ => {
            // Assumption: if the last arg is an existent local branch try to reset the files
            // represented by the previous args
            if let Some((branch, files)) = get_branch_and_files_to_checkout(&args)? {
                return checkout_files(
                    &files.iter().map(|x| x.as_str()).collect::<Vec<_>>(),
                    branch,
                );
            }
            // Assumption: if the last arg is NOT an existent local branch try to create a branch
            upsert_branch(&build_branch_name(&args)?)
        }
    }?;

    Ok(())
}

// NOTE: just drafting some ideas
// enum WhatToDo {
//     SwitchToBranch { branch: String },
//     CreateBranch { branch: String },
//     CheckoutBranch { branch: String },
//     CheckoutFiles { branch: String, files: Vec<String> },
// }

fn get_branch_and_files_to_checkout(
    args: &[String],
) -> anyhow::Result<Option<(&String, &[String])>> {
    if let Some((branch, files)) = args.split_last() {
        if local_branch_exists(branch)? {
            return Ok(Some((branch, files)));
        }
    }
    Ok(None)
}

fn local_branch_exists(branch: &str) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    Ok(false)
}

fn checkout_files(files: &[&str], branch: &str) -> anyhow::Result<()> {
    let mut args = vec!["checkout", branch];
    args.extend_from_slice(files);
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }
    files.iter().for_each(|f| println!("⭕️ get {branch} {f}"));
    Ok(())
}

fn switch_branch(branch: &str) -> anyhow::Result<()> {
    let output = Command::new("git").args(["switch", branch]).output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }
    println!("🔁 to: {branch}");
    Ok(())
}

fn create_branch(branch: &str) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(["checkout", "-b", branch])
        .output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }
    println!("✅ new: {branch}");
    Ok(())
}

fn upsert_branch(branch: &str) -> anyhow::Result<()> {
    if let Err(error) = create_branch(branch) {
        if error.to_string().contains("already exists") {
            println!("⚪️ exist: {branch}");
            return switch_branch(branch);
        }
        return Err(error);
    }
    Ok(())
}

fn build_branch_name(args: &[String]) -> anyhow::Result<String> {
    let branch_name = args
        .iter()
        .flat_map(|x| {
            x.split_whitespace().filter_map(|y| {
                let z = y
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '.' || c == '/' {
                            c
                        } else {
                            ' '
                        }
                    })
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
        bail!("parameterizing {args:?} resulted in empty String")
    }

    Ok(branch_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_branch_name_works_as_expected() {
        assert_eq!(
            r#"Err(parameterizing [""] resulted in empty String)"#,
            format!("{:?}", build_branch_name(&["".into()]))
        );
        assert_eq!(
            r#"Err(parameterizing ["❌"] resulted in empty String)"#,
            format!("{:?}", build_branch_name(&["❌".into()]))
        );
        assert_eq!(
            "helloworld",
            build_branch_name(&["HelloWorld".into()]).unwrap()
        );
        assert_eq!(
            "hello-world",
            build_branch_name(&["Hello World".into()]).unwrap()
        );
        assert_eq!(
            "feature-implement-user-login",
            build_branch_name(&["Feature: Implement User Login!".into()]).unwrap()
        );
        assert_eq!(
            "version-2.0",
            build_branch_name(&["Version 2.0".into()]).unwrap()
        );
        assert_eq!(
            "this-is...a-test",
            build_branch_name(&["This---is...a_test".into()]).unwrap()
        );
        assert_eq!(
            "leading-and-trailing",
            build_branch_name(&["  Leading and trailing   ".into()]).unwrap()
        );
        assert_eq!(
            "hello-world",
            build_branch_name(&["Hello 🌎 World".into()]).unwrap()
        );
        assert_eq!(
            "launch-day",
            build_branch_name(&["🚀Launch🚀Day".into()]).unwrap()
        );
        assert_eq!(
            "smile-and-code",
            build_branch_name(&["Smile 😊 and 🤖 code".into()]).unwrap()
        );
        assert_eq!(
            "hello-world",
            build_branch_name(&["Hello".into(), "World".into()]).unwrap()
        );
        assert_eq!(
            "hello-world-world",
            build_branch_name(&["Hello World".into(), "World".into()]).unwrap()
        );
        assert_eq!(
            "hello-world-42",
            build_branch_name(&["Hello World".into(), "🌎".into(), "42".into()]).unwrap()
        );
        assert_eq!(
            "this-is.-..a-test",
            build_branch_name(&["This".into(), "---is.".into(), "..a_test".into()]).unwrap()
        );
        assert_eq!(
            "dependabot/cargo/opentelemetry-0.27.1",
            build_branch_name(&["dependabot/cargo/opentelemetry-0.27.1".into()]).unwrap()
        );
    }
}
