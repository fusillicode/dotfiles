use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Marksman {
    pub bin_dir: String,
}

impl Installer for Marksman {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        let ln_sf_op = utils::system::symlink::build(&target, None)?;

        Ok(ln_sf_op)
    }
}
