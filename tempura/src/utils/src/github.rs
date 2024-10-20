use std::process::Command;
use std::process::Output;

use anyhow::anyhow;
use anyhow::bail;
use url::Url;

const GITHUB_HOST: &str = "github.com";
const GITHUB_PR_ID_PREFIX: &str = "pull";

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

    cmd_output_as_utf8_string(output)
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

    cmd_output_as_utf8_string(output)
}

fn cmd_output_as_utf8_string(output: Output) -> anyhow::Result<String> {
    output.status.exit_ok()?;

    Ok(std::str::from_utf8(&output.stdout)?.trim().into())
}

fn extract_pr_id_form_pr_url(pr_url: &Url) -> anyhow::Result<String> {
    if !(pr_url
        .host_str()
        .ok_or_else(|| anyhow!("cannot extract host from {pr_url}"))?
        == GITHUB_HOST)
    {
        bail!("host in {pr_url} doesn't match {GITHUB_HOST:?}")
    }

    let path_segments: Vec<&str> = pr_url
        .path_segments()
        .ok_or_else(|| anyhow!("{pr_url} cannot-be-a-base"))?
        .collect();

    let pr_prefix = path_segments.first().ok_or_else(|| {
        anyhow!(
            "missing PR id prefix {GITHUB_PR_ID_PREFIX:?} {pr_url} path segments {path_segments:?}"
        )
    })?;
    if !(pr_prefix == &GITHUB_PR_ID_PREFIX) {
        bail!("PR prefix {pr_prefix:?} in {pr_url} doesn't match {GITHUB_PR_ID_PREFIX:?}");
    }

    let pr_id = path_segments
        .get(1)
        .ok_or_else(|| anyhow!("missing PR id in {pr_url} path segments {path_segments:?}"))?;

    if pr_id.is_empty() {
        bail!("empty PR id in in {pr_url} path segments {path_segments:?}");
    }

    Ok((*pr_id).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_host_cannot_be_extracted() {
        let pr_url = Url::parse("mailto:foo@bar.com").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            "cannot extract host from mailto:foo@bar.com",
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_is_not_from_github() {
        let pr_url = Url::parse("https://foo.bar").unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"host in https://foo.bar/ doesn't match "github.com""#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doesnt_have_path_segments(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"PR prefix "" in https://github.com/ doesn't match "pull""#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doesnt_have_pr_id() {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"missing PR id in https://github.com/pull path segments ["pull"]"#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doenst_have_the_expected_pr_id_prefix(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/foo")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"PR prefix "foo" in https://github.com/foo doesn't match "pull""#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_pr_id_from_an_expected_github_pr_url() {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42")).unwrap();
        assert_eq!("42", extract_pr_id_form_pr_url(&pr_url).unwrap())
    }
}
