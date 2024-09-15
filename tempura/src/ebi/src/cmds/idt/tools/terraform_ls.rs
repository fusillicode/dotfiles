use std::process::Command;

use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;

pub struct TerraformLsInstaller {
    pub bin_dir: String,
}

impl Installer for TerraformLsInstaller {
    fn bin(&self) -> &'static str {
        "terraform-ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = format!("hashicorp/{}", self.bin());
        let latest_release = &crate::utils::github::get_latest_release(&repo)?[1..];

        crate::cmds::idt::curl_install::run(
            &format!("https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_darwin_arm64.zip", self.bin()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
