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

    extract_success_output(output)
}

pub fn get_branch_name_from_pr_url(pr_url: &Url) -> anyhow::Result<String> {
    let pr_id = extract_pr_id_form_pr_url(pr_url)?;

    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_id,
            "--json",
            "headRefName",
            "--jq",
            ".headRefName",
        ])
        .output()?;

    extract_success_output(output)
}

fn extract_success_output(output: Output) -> anyhow::Result<String> {
    output.status.exit_ok()?;
    Ok(std::str::from_utf8(&output.stdout)?.trim().into())
}

fn extract_pr_id_form_pr_url(pr_url: &Url) -> anyhow::Result<String> {
    let host = pr_url
        .host_str()
        .ok_or_else(|| anyhow!("cannot extract host from {pr_url}"))?;
    if host != GITHUB_HOST {
        bail!("host {host:?} in {pr_url} doesn't match {GITHUB_HOST:?}")
    }

    let path_segments = pr_url
        .path_segments()
        .ok_or_else(|| anyhow!("{pr_url} cannot-be-a-base"))?
        .enumerate()
        .collect::<Vec<_>>();

    match path_segments
        .iter()
        .filter(|(_, ps)| ps == &GITHUB_PR_ID_PREFIX)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [(idx, _)] => Ok(path_segments
            .get(idx + 1)
            .ok_or_else(|| anyhow!("missing PR id in {pr_url} path segments {path_segments:?}"))
            .and_then(|(_, pr_id)| {
                if pr_id.is_empty() {
                    return Err(anyhow!("empty PR id in {pr_url} path segments {path_segments:?}"));
                }
                Ok(pr_id.to_string())
            })?),
        [] => Err(anyhow!(
            "missing PR id prefix {GITHUB_PR_ID_PREFIX:?} in {pr_url} path segments {path_segments:?}"
        )),
        _ => Err(anyhow!(
            "multiple {GITHUB_PR_ID_PREFIX:?} found in {pr_url} path segments {path_segments:?}"
        )),
    }
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
            r#"host "foo.bar" in https://foo.bar/ doesn't match "github.com""#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doesnt_have_path_segments(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"missing PR id prefix "pull" in https://github.com/ path segments [(0, "")]"#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doesnt_have_pr_id() {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"missing PR id in https://github.com/pull path segments [(0, "pull")]"#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_doenst_have_the_expected_pr_id_prefix(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/foo")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"missing PR id prefix "pull" in https://github.com/foo path segments [(0, "foo")]"#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_error_when_url_has_multiple_pr_id_prefixes(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/pull/43")).unwrap();
        assert2::let_assert!(Err(error) = extract_pr_id_form_pr_url(&pr_url));
        assert_eq!(
            r#"multiple "pull" found in https://github.com/pull/42/pull/43 path segments [(0, "pull"), (1, "42"), (2, "pull"), (3, "43")]"#,
            error.to_string()
        )
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_pr_id_from_a_github_pr_url_that_ends_with_the_pr_id(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42")).unwrap();
        assert_eq!("42", extract_pr_id_form_pr_url(&pr_url).unwrap())
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_pr_id_from_a_github_pr_url_that_does_not_end_with_the_pr_id(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/pull/42/foo")).unwrap();
        assert_eq!("42", extract_pr_id_form_pr_url(&pr_url).unwrap())
    }

    #[test]
    fn test_extract_pr_id_form_pr_url_returns_the_expected_pr_id_from_a_github_pr_url_if_pr_id_prefix_is_not_1st_path_segment(
    ) {
        let pr_url = Url::parse(&format!("https://{GITHUB_HOST}/foo/pull/42/foo")).unwrap();
        assert_eq!("42", extract_pr_id_form_pr_url(&pr_url).unwrap())
    }
}
