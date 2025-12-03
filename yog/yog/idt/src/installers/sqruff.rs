use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Sqruff<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Sqruff<'_> {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-darwin-aarch64.tar.gz",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        ytil_sys::chmod_x(target)?;

        Ok(())
    }
}
