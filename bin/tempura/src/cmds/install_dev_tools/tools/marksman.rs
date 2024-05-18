use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct MarksmanInstaller {
    pub bin_dir: String,
}

impl Installer for MarksmanInstaller {
    fn tool(&self) -> &'static str {
        "marksman"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
            "https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos",
            OutputOption::WriteTo(&format!("{}/marksman", self.bin_dir)),
        )
    }
}
