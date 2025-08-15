use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct HelmLs {
    pub bin_dir: String,
}

impl Installer for HelmLs {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            CurlDownloaderOption::WriteTo {
                dest_path: &format!("{}/{}", self.bin_dir, self.bin_name()),
            },
        )?;

        utils::system::chmod_x(&target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["version"])
    }
}
