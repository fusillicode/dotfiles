use crate::Installer;

pub struct PhpActor {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PhpActor {
    fn bin_name(&self) -> &'static str {
        "phpactor"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::composer_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("{0}/{0}", self.bin_name())],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
