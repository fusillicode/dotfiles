use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct DenoInstaller {
    pub bin_dir: String,
}

impl Installer for DenoInstaller {
    fn tool(&self) -> &'static str {
        "deno"
    }

    fn install(&self) -> anyhow::Result<()> {
        // For Markdown preview with peek.nvim
        let repo = "denoland/deno";
        let latest_release = crate::utils::github::get_latest_release(repo)?;
        crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/deno-aarch64-apple-darwin.zip"),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
