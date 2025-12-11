use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TerraformLs<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for TerraformLs<'_> {
    fn bin_name(&self) -> &'static str {
        "terraform-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("hashicorp/{}", self.bin_name());
        let latest_release = &ytil_gh::get_latest_release(&repo)?[1..];

        let target = crate::downloaders::curl::run(
            &format!(
                "https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_darwin_arm64.zip",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
