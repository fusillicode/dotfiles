use std::process::Command;

use crate::Installer;
use crate::installers::curl_install::OutputOption;

pub struct Sqruff {
    pub bin_target_dir: String
}

impl Installer for Sqruff {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-darwin-aarch64.tar.gz",
                self.bin_name()
            ),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &self.bin_target_dir])),
        )
    }
}
