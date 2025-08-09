use crate::ToolInstaller;

pub struct RuffLsp {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for RuffLsp {
    fn bin_name(&self) -> &'static str {
        "ruff-lsp"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::pip_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dest_dir,
            self.bin_name(),
        )?;

        Ok(())
    }
}
