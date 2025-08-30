use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct RustAnalyzer<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for RustAnalyzer<'_> {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoZcat {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        utils::system::chmod_x(target)?;

        Ok(())
    }
}
