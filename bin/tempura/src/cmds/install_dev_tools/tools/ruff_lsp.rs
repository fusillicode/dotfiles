use crate::cmds::install_dev_tools::tools::Installer;

pub struct RuffLspInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for RuffLspInstaller {
    fn tool(&self) -> &'static str {
        "ruff_lsp"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::pip_install::run(
            &self.dev_tools_dir,
            "ruff-lsp",
            &["ruff-lsp"],
            &self.bin_dir,
            "ruff-lsp",
        )
    }
}
