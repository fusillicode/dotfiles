use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    let repo = "errata-ai/vale";
    let latest_release = crate::utils::github::get_latest_release(repo)?;
    crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/vale_{}_macOS_arm64.tar.gz", latest_release[1..].to_owned()),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )
}
