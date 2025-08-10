use crate::Installer;

pub struct YamlLanguageServer {
    pub dev_tools_dir: String,
    pub bins_dir: String,
}

impl Installer for YamlLanguageServer {
    fn bin_name(&self) -> &'static str {
        "yaml-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bins_dir,
            self.bin_name(),
        )
    }
}
