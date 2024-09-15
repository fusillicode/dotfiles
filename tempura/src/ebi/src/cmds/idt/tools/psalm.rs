use crate::cmds::idt::tools::Installer;

pub struct PsalmInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PsalmInstaller {
    fn bin(&self) -> &'static str {
        "psalm"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::composer_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[&format!("vimeo/{}", self.bin())],
            &self.bin_dir,
            "*",
        )
    }
}
