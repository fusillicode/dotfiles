use crate::cmds::install_dev_tools::tools::Installer;

pub struct YamlLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for YamlLanguageServerInstaller {
    fn bin(&self) -> &'static str {
        "yaml-language-server"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            self.bin(),
        )
    }
}
