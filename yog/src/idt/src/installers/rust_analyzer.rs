use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct RustAnalyzer {
    pub bin_dir: String,
}

impl Installer for RustAnalyzer {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoZcat {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        let symlink = utils::system::symlink::build(&target, Some(self.bin_name()))?;

        Ok(symlink)
    }
}
