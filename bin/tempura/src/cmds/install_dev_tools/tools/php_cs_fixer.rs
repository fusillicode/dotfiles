use crate::cmds::install_dev_tools::tools::Installer;

pub struct PhpFixerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpFixerInstaller {
    fn tool(&self) -> &'static str {
        "php_cs_fixer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::composer_install::run(
            &self.dev_tools_dir,
            "php-cs-fixer",
            &["friendsofphp/php-cs-fixer"],
            &self.bin_dir,
            "php-cs-fixer",
        )
    }
}
