use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct TyposLspInstaller {
    pub bin_dir: String,
}

impl Installer for TyposLspInstaller {
    fn tool(&self) -> &'static str {
        "typos_lsp"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = "tekumara/typos-vscode";
        let latest_release = crate::utils::github::get_latest_release(repo)?;
        crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/typos-lsp-{latest_release}-aarch64-apple-darwin.tar.gz"),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
