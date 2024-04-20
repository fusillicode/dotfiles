use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    // For Markdown preview with peek.nvim
    let repo = "denoland/deno";
    let latest_release = crate::utils::github::get_latest_release(repo)?;
    crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/deno-aarch64-apple-darwin.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", bin_dir])),
    )
}
