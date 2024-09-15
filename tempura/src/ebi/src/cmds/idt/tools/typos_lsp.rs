use std::process::Command;

use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;

pub struct TyposLspInstaller {
    pub bin_dir: String,
}

impl Installer for TyposLspInstaller {
    fn bin(&self) -> &'static str {
        "typos-lsp"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = "tekumara/typos-vscode";
        let latest_release = crate::utils::github::get_latest_release(repo)?;

        crate::cmds::idt::curl_install::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-aarch64-apple-darwin.tar.gz", self.bin()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
