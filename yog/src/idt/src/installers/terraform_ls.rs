use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TerraformLs {
    pub bin_dir: String,
}

impl Installer for TerraformLs {
    fn bin_name(&self) -> &'static str {
        "terraform-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("hashicorp/{}", self.bin_name());
        let latest_release = &utils::github::get_latest_release(&repo)?[1..];

        crate::downloaders::curl::run(
            &format!(
                "https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_darwin_arm64.zip",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        Ok(())
    }
}
