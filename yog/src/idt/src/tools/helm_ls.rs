use crate::Installer;
use crate::installers::curl_install::OutputOption;

pub struct HelmLs {
    pub bin_dir: String,
}

impl Installer for HelmLs {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin_name())),
        )
    }
}
