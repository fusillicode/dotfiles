use crate::Installer;

pub struct PrettierD {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PrettierD {
    fn bin_name(&self) -> &'static str {
        "prettierd"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@fsouza/{}", self.bin_name())],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
