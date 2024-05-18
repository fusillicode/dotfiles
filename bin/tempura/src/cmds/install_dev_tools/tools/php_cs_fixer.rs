use crate::cmds::install_dev_tools::tools::Installer;

pub struct PhpFixerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpFixerInstaller {
    fn bin(&self) -> &'static str {
        "php-cs-fixer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::composer_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("friendsofphp/{}", self.bin())],
            &self.bin_dir,
            self.bin(),
        )
    }
}
