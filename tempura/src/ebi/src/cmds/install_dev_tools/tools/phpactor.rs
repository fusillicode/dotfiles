use crate::cmds::install_dev_tools::tools::Installer;

pub struct PhpActorInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpActorInstaller {
    fn bin(&self) -> &'static str {
        "phpactor"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::composer_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("{0}/{0}", self.bin())],
            &self.bin_dir,
            self.bin(),
        )
    }
}
