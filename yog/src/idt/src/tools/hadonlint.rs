use crate::installers::curl_install::OutputOption;
use crate::Installer;

pub struct Hadolint {
    pub bin_dir: String,
}

impl Installer for Hadolint {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-Darwin-x86_64",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin_name())),
        )
    }
}
