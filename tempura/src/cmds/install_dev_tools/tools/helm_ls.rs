use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct HelmLsInstaller {
    pub bin_dir: String,
}

impl Installer for HelmLsInstaller {
    fn bin(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin())),
        )
    }
}
