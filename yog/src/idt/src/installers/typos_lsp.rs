use std::path::PathBuf;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TyposLsp {
    pub bin_dir: PathBuf,
}

impl Installer for TyposLsp {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = "tekumara/typos-vscode";
        let latest_release = utils::github::get_latest_release(repo)?;

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-aarch64-apple-darwin.tar.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        utils::system::chmod_x(target)?;

        Ok(())
    }
}
