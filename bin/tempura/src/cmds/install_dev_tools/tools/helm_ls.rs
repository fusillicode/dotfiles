use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct HelmLsInstaller {
    pub bin_dir: String,
}

impl Installer for HelmLsInstaller {
    fn tool(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
            "https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64",
            OutputOption::WriteTo(&format!("{}/helm_ls", self.bin_dir)),
        )
    }
}
