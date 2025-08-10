use crate::Installer;
use crate::installers::curl_install::InstallOption;

pub struct Sqruff {
    pub bin_dir: String,
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
            InstallOption::PipeToTar {
                dest_dir: &self.bin_dir,
                dest_name: self.bin_name(),
            },
        )
    }
}
