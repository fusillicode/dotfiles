use crate::Installer;

pub struct RuffLsp {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for RuffLsp {
    fn bin_name(&self) -> &'static str {
        "ruff-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::pip::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
