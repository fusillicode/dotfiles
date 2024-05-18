use crate::cmds::install_dev_tools::tools::Installer;

pub struct PhpActorInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpActorInstaller {
    fn tool(&self) -> &'static str {
        "phpactor"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::composer_install::run(
            &self.dev_tools_dir,
            "phpactor",
            &["phpactor/phpactor"],
            &self.bin_dir,
            "phpactor",
        )
    }
}
