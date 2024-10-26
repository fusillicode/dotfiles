use crate::Installer;

pub struct SqlFluff {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlFluff {
    fn bin_name(&self) -> &'static str {
        "sqlfluff"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::pip_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
