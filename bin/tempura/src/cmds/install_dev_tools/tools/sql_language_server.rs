use crate::cmds::install_dev_tools::tools::Installer;

pub struct SqlLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlLanguageServerInstaller {
    fn tool(&self) -> &'static str {
        "sql_language_server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "sql-language-server",
            &["sql-language-server"],
            &self.bin_dir,
            "sql-language-server",
        )
    }
}
