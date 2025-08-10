use crate::Installer;
use crate::downloaders::curl::InstallOption;

pub struct HelmLs {
    pub bins_dir: String,
}

impl Installer for HelmLs {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            InstallOption::WriteTo {
                dest_path: &format!("{}/{}", self.bins_dir, self.bin_name()),
            },
        )
    }
}
