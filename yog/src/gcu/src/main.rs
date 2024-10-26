#![feature(exit_status_error)]

use std::process::Command;

use anyhow::bail;
use url::Url;

/// Switch to the GitHub branch that is supplied or get it if it's a PR URL and then switch to it.
fn main() -> anyhow::Result<()> {
    let args = utils::system::get_args();

    let Some(arg) = args.first() else {
        bail!("no args supplied {:?}", args);
    };

    match arg.as_str() {
        "-b" => create_new_branch(&args),
        arg => switch_branch(arg),
    }
}

fn create_new_branch(args: &[String]) -> anyhow::Result<()> {
    let collected_args = args[1..]
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(" ");

    let output = Command::new("git")
        .args(["checkout", "-b", &to_git_branch(&collected_args)?])
        .output()?;

    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }

    Ok(())
}

fn switch_branch(arg: &str) -> anyhow::Result<()> {
    let branch_or_url = if let Ok(url) = Url::parse(arg) {
        utils::github::log_into_github()?;
        utils::github::get_branch_name_from_pr_url(&url)?
    } else {
        arg.into()
    };

    let output = Command::new("git")
        .args(["switch", &branch_or_url])
        .output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }

    Ok(())
}

fn to_git_branch(s: &str) -> anyhow::Result<String> {
    if s.is_empty() {
        bail!("empty string cannot be used as git branch")
    }

    let out = s
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("-");

    if out.is_empty() {
        bail!("parameterizing str {s} resulted in empty string")
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_git_branch_works_as_expected() {
        assert_eq!(
            "Err(empty string cannot be used as git branch)",
            format!("{:?}", to_git_branch(""))
        );
        assert_eq!(
            "Err(parameterizing str ❌ resulted in empty string)",
            format!("{:?}", to_git_branch("❌"))
        );

        assert_eq!("helloworld", to_git_branch("HelloWorld").unwrap());
        assert_eq!("hello-world", to_git_branch("Hello World").unwrap());
        assert_eq!(
            "feature-implement-user-login",
            to_git_branch("Feature: Implement User Login!").unwrap()
        );
        assert_eq!("version-2-0", to_git_branch("Version 2.0").unwrap());
        assert_eq!(
            "this-is-a-test",
            to_git_branch("This---is...a_test").unwrap()
        );
        assert_eq!(
            "leading-and-trailing",
            to_git_branch("  Leading and trailing   ").unwrap()
        );
        assert_eq!("hello-world", to_git_branch("Hello 🌎 World").unwrap());
        assert_eq!("launch-day", to_git_branch("🚀Launch🚀Day").unwrap());
        assert_eq!(
            "smile-and-code",
            to_git_branch("Smile 😊 and 🤖 code").unwrap()
        );
    }
}
