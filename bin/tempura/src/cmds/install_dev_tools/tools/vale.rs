use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct ValeInstaller {
    pub bin_dir: String,
}

impl Installer for ValeInstaller {
    fn bin(&self) -> &'static str {
        "vale"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = format!("errata-ai/{}", self.bin());
        let latest_release = crate::utils::github::get_latest_release(&repo)?;

        crate::cmds::install_dev_tools::curl_install::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{}_{}_macOS_arm64.tar.gz", self.bin(), latest_release[1..].to_owned()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
