use crate::cmds::idt::tools::Installer;

pub struct PrettierDInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PrettierDInstaller {
    fn bin(&self) -> &'static str {
        "prettierd"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("@fsouza/{}", self.bin())],
            &self.bin_dir,
            self.bin(),
        )
    }
}
