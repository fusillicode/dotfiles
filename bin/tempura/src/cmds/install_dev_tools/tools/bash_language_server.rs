use crate::cmds::install_dev_tools::tools::Installer;

pub struct BashLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for BashLanguageServerInstaller {
    fn tool(&self) -> &'static str {
        "bash_language_server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "bash-language-server",
            &["bash-language-server"],
            &self.bin_dir,
            "bash-language-server",
        )
    }
}
