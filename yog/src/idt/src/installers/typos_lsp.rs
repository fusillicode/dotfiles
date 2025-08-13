use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TyposLsp {
    pub bin_dir: String,
}

impl Installer for TyposLsp {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
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

        let symlink = utils::system::symlink::build(&target, None)?;

        Ok(symlink)
    }
}
