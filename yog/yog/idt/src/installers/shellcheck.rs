use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Shellcheck<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Shellcheck<'_> {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;
        let dest_dir = Path::new("/tmp");

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir,
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
        ytil_sys::chmod_x(target)?;

        Ok(())
    }
}
