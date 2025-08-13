use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Marksman {
    pub bin_dir: String,
}

impl Installer for Marksman {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::downloaders::curl::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        Ok(())
    }
}
