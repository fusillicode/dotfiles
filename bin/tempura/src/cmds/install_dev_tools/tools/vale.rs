use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct ValeInstaller {
    pub bin_dir: String,
}

impl Installer for ValeInstaller {
    fn tool(&self) -> &'static str {
        "vale"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = "errata-ai/vale";
        let latest_release = crate::utils::github::get_latest_release(repo)?;
        crate::cmds::install_dev_tools::curl_install::run(
       &format!("https://github.com/{repo}/releases/download/{latest_release}/vale_{}_macOS_arm64.tar.gz", latest_release[1..].to_owned()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
