use crate::Installer;

pub struct PhpFixerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpFixerInstaller {
    fn bin(&self) -> &'static str {
        "php-cs-fixer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::composer_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("friendsofphp/{}", self.bin())],
            &self.bin_dir,
            self.bin(),
        )
    }
}
