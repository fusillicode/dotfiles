use crate::Installer;

pub struct SqlLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlLanguageServer {
    fn bin_name(&self) -> &'static str {
        "sql-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
