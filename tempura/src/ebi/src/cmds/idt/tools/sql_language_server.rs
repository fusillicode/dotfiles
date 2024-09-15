use crate::cmds::idt::tools::Installer;

pub struct SqlLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlLanguageServerInstaller {
    fn bin(&self) -> &'static str {
        "sql-language-server"
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
