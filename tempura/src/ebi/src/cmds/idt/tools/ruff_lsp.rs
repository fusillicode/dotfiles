use crate::cmds::idt::tools::Installer;

pub struct RuffLspInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for RuffLspInstaller {
    fn bin(&self) -> &'static str {
        "ruff-lsp"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::pip_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            self.bin(),
        )
    }
}
