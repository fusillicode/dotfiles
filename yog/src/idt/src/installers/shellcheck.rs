use std::path::PathBuf;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Shellcheck {
    pub bin_dir: PathBuf,
}

impl Installer for Shellcheck {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        let dest_dir = PathBuf::from("/tmp");

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &dest_dir,
                dest_name: None,
            },
        )?;

        let target = self.bin_dir.join(self.bin_name());
        std::fs::rename(
            dest_dir
                .join(format!("{0}-{latest_release}", self.bin_name()))
                .join(self.bin_name()),
            &target,
        )?;
        utils::system::chmod_x(target)?;

        Ok(())
    }
}
