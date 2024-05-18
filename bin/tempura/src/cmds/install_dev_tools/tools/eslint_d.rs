use crate::cmds::install_dev_tools::tools::Installer;

pub struct EslintDInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for EslintDInstaller {
    fn tool(&self) -> &'static str {
        "eslint_d"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "eslint_d",
            &["eslint_d"],
            &self.bin_dir,
            "eslint_d",
        )
    }
}
