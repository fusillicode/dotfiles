use std::process::Command;

use crate::Installer;
use crate::installers::curl_install::OutputOption;

pub struct TyposLsp {
    pub bin_dir: String,
}

impl Installer for TyposLsp {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = "tekumara/typos-vscode";
        let latest_release = utils::github::get_latest_release(repo)?;

        crate::installers::curl_install::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-aarch64-apple-darwin.tar.gz",
                self.bin_name()
            ),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
