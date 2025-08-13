use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct HelmLs {
    pub bin_dir: String,
}

impl Installer for HelmLs {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        let symlink = utils::system::symlink::build(&target, None)?;

        Ok(symlink)
    }
}
