use crate::cmds::idt::tools::Installer;

pub struct TypescriptLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for TypescriptLanguageServerInstaller {
    fn bin(&self) -> &'static str {
        "typescript-language-server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin(), "typescript"],
            &self.bin_dir,
            self.bin(),
        )
    }
}
