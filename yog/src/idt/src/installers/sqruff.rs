use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Sqruff {
    pub bin_dir: String,
}

impl Installer for Sqruff {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-darwin-aarch64.tar.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        let symlink = utils::system::symlink::build(&target, None)?;

        Ok(symlink)
    }
}
