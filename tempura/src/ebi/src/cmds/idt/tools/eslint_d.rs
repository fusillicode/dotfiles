use crate::cmds::idt::tools::Installer;

pub struct EslintDInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for EslintDInstaller {
    fn bin(&self) -> &'static str {
        "eslint_d"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            self.bin(),
        )
    }
}
