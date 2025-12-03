use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct HelmLs<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for HelmLs<'_> {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            &CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::chmod_x(&target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["version"])
    }
}
