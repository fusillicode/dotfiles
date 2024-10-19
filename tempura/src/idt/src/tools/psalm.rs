use crate::Installer;

pub struct Psalm {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Psalm {
    fn bin_name(&self) -> &'static str {
        "psalm"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::composer_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("vimeo/{}", self.bin_name())],
            &self.bin_dir,
            "*",
        )
    }
}
