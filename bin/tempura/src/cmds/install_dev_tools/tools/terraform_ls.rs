use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    let repo = "hashicorp/terraform-ls";
    let latest_release = &crate::utils::github::get_latest_release(repo)?[1..];

    crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://releases.hashicorp.com/terraform-ls/{latest_release}/terraform-ls_{latest_release}_darwin_arm64.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )
}
