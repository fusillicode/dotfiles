use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Deno<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Deno<'_> {
    fn bin_name(&self) -> &'static str {
        "deno"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("{0}land/{0}", self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-aarch64-apple-darwin.zip",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }
}
