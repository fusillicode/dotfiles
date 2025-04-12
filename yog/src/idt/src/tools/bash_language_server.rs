use crate::Installer;

pub struct BashLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for BashLanguageServer {
    fn bin_name(&self) -> &'static str {
        "bash-language-server"
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
