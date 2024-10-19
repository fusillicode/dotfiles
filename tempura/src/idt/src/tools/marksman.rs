use crate::installers::curl_install::OutputOption;
use crate::Installer;

pub struct MarksmanInstaller {
    pub bin_dir: String,
}

impl Installer for MarksmanInstaller {
    fn bin(&self) -> &'static str {
        "marksman"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin())),
        )
    }
}
