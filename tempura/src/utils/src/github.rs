use std::process::Command;
use std::process::Output;

use anyhow::anyhow;
use anyhow::bail;
use url::Url;

pub fn log_into_github() -> anyhow::Result<()> {
    if crate::system::silent_cmd("gh")
        .args(["auth", "status"])
        .status()?
        .success()
    {
        return Ok(());
    }

    Ok(crate::system::silent_cmd("sh")
        .args(["-c", "gh auth login"])
        .status()?
        .exit_ok()?)
}

pub fn get_latest_release(repo: &str) -> anyhow::Result<String> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/releases/latest"),
            "--jq=.tag_name",
        ])
        .output()?;

    output_as_utf8_string(output)
}

pub fn get_branch_name_from_pr_url(pr_url: &Url) -> anyhow::Result<String> {
    let pr_id = extract_pr_id_form_pr_url(&pr_url)?;

    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_id,
            "--json",
            "headRefName",
            "--jq",
            "'.headRefName'",
        ])
        .output()?;

    output_as_utf8_string(output)
}

fn output_as_utf8_string(output: Output) -> anyhow::Result<String> {
    output.status.exit_ok()?;

    Ok(std::str::from_utf8(&output.stdout)?.trim().into())
}

fn extract_pr_id_form_pr_url(pr_url: &Url) -> anyhow::Result<String> {
    const GITHUB_HOST: &str = "github.com";
    if !(pr_url
        .host_str()
        .ok_or_else(|| anyhow!("missing host in {pr_url}"))?
        == GITHUB_HOST)
    {
        bail!("host in {pr_url} not matching {GITHUB_HOST}")
    }

    let mut path_segments: Vec<&str> = pr_url
        .path_segments()
        .ok_or_else(|| anyhow!("{pr_url} cannot be base"))?
        .collect();
    path_segments.reverse();

    let pr_id = path_segments
        .first()
        .ok_or_else(|| anyhow!("missing PR id in {path_segments:?} of {pr_url}"))?;

    const PR_ID_PREFIX: &str = "pull";
    if path_segments
        .get(1)
        .ok_or_else(|| anyhow!("missing PR id prefix {path_segments:?} of {pr_url}"))?
        == &PR_ID_PREFIX
    {
        bail!("PR prefix in {pr_url} not matching {PR_ID_PREFIX}");
    }

    Ok((*pr_id).into())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_is_not_from_github() {}
//
//     #[test]
//     fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_not_base() {}
//
//     #[test]
//     fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_does_not_have_path_segments(
//     ) {
//     }
//
//     #[test]
//     fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_does_not_have_the_expected_pr_id_prefix(
//     ) {
//     }
//
//     #[test]
//     fn test_extract_pr_id_form_pr_url_returns_the_expected_pr_id_from_an_expected_github_pr_url() {}
// }
