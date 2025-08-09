use crate::Installer;

pub struct YamlLanguageServer {
    pub dev_tools_dir: String,
    pub bin_target_dir: String,
}

impl Installer for YamlLanguageServer {
    fn bin_name(&self) -> &'static str {
        "yaml-language-server"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_target_dir,
            self.bin_name(),
        )
    }
}
