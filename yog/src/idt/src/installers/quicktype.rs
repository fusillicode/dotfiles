use crate::Installer;

pub struct Quicktype {
    pub dev_tools_dir: String,
    pub bins_dir: String,
}

impl Installer for Quicktype {
    fn bin_name(&self) -> &'static str {
        "quicktype"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bins_dir,
            self.bin_name(),
        )
    }
}
