use crate::cmds::install_dev_tools::tools::Installer;

pub struct PsalmInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PsalmInstaller {
    fn tool(&self) -> &'static str {
        "nvim"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::composer_install::run(
            &self.dev_tools_dir,
            "psalm",
            &["vimeo/psalm"],
            &self.bin_dir,
            "*",
        )
    }
}
