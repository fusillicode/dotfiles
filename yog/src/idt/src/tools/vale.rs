use std::process::Command;

use crate::Installer;
use crate::installers::curl_install::OutputOption;

pub struct Vale {
    pub bin_dir: String,
}

impl Installer for Vale {
    fn bin_name(&self) -> &'static str {
        "vale"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("errata-ai/{}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::installers::curl_install::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}_{}_macOS_arm64.tar.gz",
                self.bin_name(),
                latest_release[1..].to_owned()
            ),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_dir])),
        )
    }
}
