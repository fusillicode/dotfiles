use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;
use crate::utils::system::silent_cmd;

pub struct ShellcheckInstaller {
    pub bin_dir: String,
}

impl Installer for ShellcheckInstaller {
    fn tool(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> anyhow::Result<()> {
        let tool = "shellcheck";
        let repo = format!("koalaman/{tool}");
        let latest_release = crate::utils::github::get_latest_release(&repo)?;
        crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.darwin.x86_64.tar.xz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", "/tmp"])),
    )?;
        silent_cmd("mv")
            .args([
                &format!("/tmp/{tool}-{latest_release}/{tool}"),
                &self.bin_dir,
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
