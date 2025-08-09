use crate::ToolInstaller;

pub struct Quicktype {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for Quicktype {
    fn bin_name(&self) -> &'static str {
        "quicktype"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dest_dir,
            self.bin_name(),
        )?;

        Ok(())
    }
}
