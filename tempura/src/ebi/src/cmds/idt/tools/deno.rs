use std::process::Command;

use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;

// For Markdown preview with peek.nvim
pub struct DenoInstaller {
    pub bin_dir: String,
}

impl Installer for DenoInstaller {
    fn bin(&self) -> &'static str {
        "deno"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = format!("{0}land/{0}", self.bin());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::cmds::idt::curl_install::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-aarch64-apple-darwin.zip",
                self.bin()
            ),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
