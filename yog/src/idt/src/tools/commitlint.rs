use crate::Installer;

pub struct Commitlint {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
