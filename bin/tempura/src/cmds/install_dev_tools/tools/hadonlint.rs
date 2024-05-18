use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct HadolintInstaller {
    pub bin_dir: String,
}

impl Installer for HadolintInstaller {
    fn tool(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
            "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64",
            OutputOption::WriteTo(&format!("{}/hadolint", self.bin_dir)),
        )
    }
}
