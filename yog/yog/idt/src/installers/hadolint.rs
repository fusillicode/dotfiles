use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Hadolint<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Hadolint<'_> {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-Darwin-x86_64",
                self.bin_name()
            ),
            &CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    // NOTE: skip because hadolint started to segfault...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
