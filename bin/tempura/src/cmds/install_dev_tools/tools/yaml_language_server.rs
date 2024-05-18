use crate::cmds::install_dev_tools::tools::Installer;

pub struct YamlLanguageServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for YamlLanguageServerInstaller {
    fn tool(&self) -> &'static str {
        "yaml_language_serve"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "yaml-language-server",
            &["yaml-language-server"],
            &self.bin_dir,
            "yaml-language-server",
        )
    }
}
