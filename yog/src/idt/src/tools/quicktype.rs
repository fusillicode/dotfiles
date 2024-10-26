use crate::Installer;

pub struct Quicktype {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Quicktype {
    fn bin_name(&self) -> &'static str {
        "quicktype"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
