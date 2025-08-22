use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Marksman<'a> {
    pub bin_dir: &'a Path,
}

impl<'a> Installer for Marksman<'a> {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        utils::system::chmod_x(&target)?;

        Ok(())
    }
}
