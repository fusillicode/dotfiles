use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Hadolint {
    pub bin_dir: String,
}

impl Installer for Hadolint {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-Darwin-x86_64",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        Ok(())
    }
}
