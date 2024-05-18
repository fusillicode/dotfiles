use crate::cmds::install_dev_tools::tools::Installer;

pub struct PrettierDInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PrettierDInstaller {
    fn tool(&self) -> &'static str {
        "prettierd"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "prettierd",
            &["@fsouza/prettierd"],
            &self.bin_dir,
            "prettierd",
        )
    }
}
