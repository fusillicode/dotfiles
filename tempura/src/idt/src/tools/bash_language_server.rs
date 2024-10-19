use crate::Installer;

pub struct BashLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for BashLanguageServerInstaller {
    fn bin(&self) -> &'static str {
        "bash-language-server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            self.bin(),
        )
    }
}
