use crate::cmds::idt::tools::Installer;

pub struct QuicktypeInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for QuicktypeInstaller {
    fn bin(&self) -> &'static str {
        "quicktype"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            self.bin(),
        )
    }
}
