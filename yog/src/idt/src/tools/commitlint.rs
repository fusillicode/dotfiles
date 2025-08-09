use crate::ToolInstaller;

pub struct Commitlint {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
            &self.bin_dest_dir,
            self.bin_name(),
        )?;

        Ok(())
    }
}
