#![feature(exit_status_error)]

use std::process::Command;

use anyhow::bail;
use url::Url;

/// Concat the supplied args in param case and switch to the related GitHub branch or
/// create it with if not existing locally or remotely.
/// If a PR URL is supplied switches to the related branch.
/// With no args default to switching to "-".
/// If "-b foo" is supplied it defaults to "git checkout -b foo"
fn main() -> anyhow::Result<()> {
    let args = utils::system::get_args();

    match args.split_first() {
        None => switch_branch("-"),
        Some((hd, _)) if hd == "-" => switch_branch(hd),
        Some((hd, tail)) if hd == "-b" => {
            Command::new("git")
                .args(["checkout", "-b", &build_branch_name(tail)?])
                .output()?;
            Ok(())
        }
        Some((hd, &[])) => {
            if let Ok(url) = Url::parse(hd) {
                utils::github::log_into_github()?;
                let branch_name = utils::github::get_branch_name_from_pr_url(&url)?;
                return switch_branch(&branch_name);
            }
            // upsert_branch(&[hd])?;
            Ok(())
        }
        _ => {
            upsert_branch(&build_branch_name(&args))?;
            Ok(())
        }
    }
}

fn upsert_branch(branch_name: &str) -> anyhow::Result<&str> {
    let output = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .output()?;

    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }

    Ok(branch_name)
}

fn switch_branch(branch: &str) -> anyhow::Result<()> {
    let output = Command::new("git").args(["switch", &branch]).output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }
    Ok(())
}

fn build_branch_name(s: &str) -> anyhow::Result<String> {
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
    fn test_build_branch_name_works_as_expected() {
        assert_eq!(
            "Err(empty string cannot be used as git branch)",
            format!("{:?}", build_branch_name(""))
        );
        assert_eq!(
            "Err(parameterizing str ❌ resulted in empty string)",
            format!("{:?}", build_branch_name("❌"))
        );

        assert_eq!("helloworld", build_branch_name("HelloWorld").unwrap());
        assert_eq!("hello-world", build_branch_name("Hello World").unwrap());
        assert_eq!(
            "feature-implement-user-login",
            build_branch_name("Feature: Implement User Login!").unwrap()
        );
        assert_eq!("version-2-0", build_branch_name("Version 2.0").unwrap());
        assert_eq!(
            "this-is-a-test",
            build_branch_name("This---is...a_test").unwrap()
        );
        assert_eq!(
            "leading-and-trailing",
            build_branch_name("  Leading and trailing   ").unwrap()
        );
        assert_eq!("hello-world", build_branch_name("Hello 🌎 World").unwrap());
        assert_eq!("launch-day", build_branch_name("🚀Launch🚀Day").unwrap());
        assert_eq!(
            "smile-and-code",
            build_branch_name("Smile 😊 and 🤖 code").unwrap()
        );
    }
}
