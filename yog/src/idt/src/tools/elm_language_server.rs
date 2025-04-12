use crate::Installer;

pub struct ElmLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElmLanguageServer {
    fn bin_name(&self) -> &'static str {
        "elm-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@elm-tooling/{}", self.bin_name())],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
