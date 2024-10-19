use std::process::Command;

use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;
use utils::system::silent_cmd;

pub struct ShellcheckInstaller {
    pub bin_dir: String,
}

impl Installer for ShellcheckInstaller {
    fn bin(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = format!("koalaman/{}", self.bin());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::cmds::idt::curl_install::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz", self.bin()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", "/tmp"])),
        )?;

        silent_cmd("mv")
            .args([
                &format!("/tmp/{0}-{latest_release}/{0}", self.bin()),
                &self.bin_dir,
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
