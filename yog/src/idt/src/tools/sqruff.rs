use std::process::Command;

use crate::ToolInstaller;
use crate::installers::curl_install::OutputOption;

pub struct Sqruff {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Sqruff {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-darwin-aarch64.tar.gz",
                self.bin_name()
            ),
            OutputOption::PipeInto(
                Command::new("tar").args(["-xz", "-C"]),
                self.bin_dest_dir.clone(),
            ),
        )?;

        Ok(())
    }
}
