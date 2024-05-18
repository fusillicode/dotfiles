use crate::cmds::install_dev_tools::tools::Installer;

pub struct TypescriptLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for TypescriptLanguageServerInstaller {
    fn tool(&self) -> &'static str {
        "typescript_language_server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "typescript-language-server",
            &["typescript-language-server", "typescript"],
            &self.bin_dir,
            "typescript-language-server",
        )
    }
}
