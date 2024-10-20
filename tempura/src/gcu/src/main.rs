#![feature(exit_status_error)]

use std::process::Command;

use anyhow::bail;
use url::Url;

/// Switch to the GitHub branch that is supplied or get it if it's a PR URL and then switch to it.
fn main() -> anyhow::Result<()> {
    let args = utils::system::get_args();
    let Some(branch_or_url) = args.first() else {
        bail!("no git branch or GitHub PR specified {:?}", args);
    };

    let branch_or_url = if let Ok(url) = Url::parse(branch_or_url) {
        utils::github::log_into_github()?;
        utils::github::get_branch_name_from_pr_url(&url)?
    } else {
        branch_or_url.into()
    };

    let output = Command::new("git")
        .args(["switch", &branch_or_url])
        .output()?;

    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }

    Ok(())
}
