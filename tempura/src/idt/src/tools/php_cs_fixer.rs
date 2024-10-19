use crate::Installer;

pub struct PhpFixer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpFixer {
    fn bin_name(&self) -> &'static str {
        "php-cs-fixer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::composer_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("friendsofphp/{}", self.bin_name())],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
