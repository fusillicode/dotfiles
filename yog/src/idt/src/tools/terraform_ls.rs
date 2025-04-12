use std::process::Command;

use crate::installers::curl_install::OutputOption;
use crate::Installer;

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

        crate::installers::curl_install::run(
            &format!("https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_darwin_arm64.zip", self.bin_name()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
