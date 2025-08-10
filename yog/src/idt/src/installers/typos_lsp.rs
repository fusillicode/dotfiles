use crate::Installer;
use crate::downloaders::curl::InstallOption;

pub struct TyposLsp {
    pub bins_dir: String,
}

impl Installer for TyposLsp {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = "tekumara/typos-vscode";
        let latest_release = utils::github::get_latest_release(repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-aarch64-apple-darwin.tar.gz",
                self.bin_name()
            ),
            InstallOption::PipeIntoTar {
                dest_dir: &self.bins_dir,
                dest_name: self.bin_name(),
            },
        )
    }
}
