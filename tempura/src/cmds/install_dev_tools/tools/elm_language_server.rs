use crate::cmds::install_dev_tools::tools::Installer;

pub struct ElmLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElmLanguageServerInstaller {
    fn bin(&self) -> &'static str {
        "elm-language-server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("@elm-tooling/{}", self.bin())],
            &self.bin_dir,
            self.bin(),
        )
    }
}
