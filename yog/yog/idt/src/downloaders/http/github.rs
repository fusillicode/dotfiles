use rootcause::prelude::ResultExt as _;

/// Fetches the latest release tag name for a GitHub repository.
///
/// Calls `GET https://api.github.com/repos/{repo}/releases/latest` and
/// extracts the `tag_name` field from the JSON response.
///
/// If the `GITHUB_TOKEN` environment variable is set, it is sent as a
/// Bearer token to raise the rate limit from 60 to 5 000 requests/hour.
///
/// # Errors
/// - The HTTP request fails or returns a non-success status.
/// - The response body cannot be parsed as JSON.
/// - The `tag_name` field is missing from the response.
pub fn get_latest_release_tag(repo: &str) -> rootcause::Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");

    let mut req = ureq::get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Mozilla/5.0");

    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }

    let resp = req
        .call()
        .context("error fetching latest release")
        .attach_with(|| format!("repo={repo:?}"))?;

    let body_str = resp
        .into_body()
        .read_to_string()
        .context("error reading release response body")
        .attach_with(|| format!("repo={repo:?}"))?;

    let body: serde_json::Value = serde_json::from_str(&body_str)
        .context("error parsing release JSON")
        .attach_with(|| format!("repo={repo:?}"))?;

    body.get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .ok_or_else(|| rootcause::report!("missing tag_name in release response"))
        .attach_with(|| format!("repo={repo:?}"))
}
