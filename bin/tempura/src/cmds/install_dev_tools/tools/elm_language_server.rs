use crate::cmds::install_dev_tools::tools::Installer;

pub struct ElmLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElmLanguageServerInstaller {
    fn tool(&self) -> &'static str {
        "elm_language_server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "elm-language-server",
            &["@elm-tooling/elm-language-server"],
            &self.bin_dir,
            "elm-language-server",
        )
    }
}
